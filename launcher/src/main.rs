//! piShop launcher: serves the embedded PWA from a local Rust server, opens it
//! fullscreen in the bundled Chromium and ties both lifetimes together so the
//! app behaves like a single game process for Steam.

mod api;
mod browser;
mod catalog;
mod discover;
mod jobs;
mod library;
mod localfs;
mod log;
mod server;
mod services_file;
mod settings;
mod smbfs;
mod sources;
mod steam;
mod steam_store;
mod titles;
mod tgdb;
mod torrent;
mod tpb;
mod trailer;
mod vdf;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;

use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;

pub const APP_NAME: &str = "piShop";
pub const PORT: u16 = 47800;

/// UI port; `PISHOP_PORT` overrides it for development only (the PWA origin,
/// and with it the browser storage, is tied to the port).
pub fn port() -> u16 {
    std::env::var("PISHOP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT)
}

struct Args {
    no_browser: bool,
    install_steam: bool,
    uninstall_steam: bool,
}

fn parse_args() -> Args {
    let mut args = Args { no_browser: false, install_steam: false, uninstall_steam: false };
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--no-browser" => args.no_browser = true,
            "--install-steam" => args.install_steam = true,
            "--uninstall-steam" => args.uninstall_steam = true,
            "-h" | "--help" => {
                println!(
                    "{APP_NAME} {}\n\n\
                     USO: pishop [opções]\n\n  \
                     (sem opções)        abre o app em tela cheia\n  \
                     --no-browser        só o servidor (desenvolvimento)\n  \
                     --install-steam     adiciona o piShop como jogo non-Steam\n  \
                     --uninstall-steam   remove o atalho da Steam",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            other => eprintln!("argumento ignorado: {other}"),
        }
    }
    args
}

/// Directory holding the executable and the bundled `chromium/` folder.
pub fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Writable per-user state (browser profile, logs).
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(APP_NAME)
}

fn main() {
    let args = parse_args();
    if args.install_steam || args.uninstall_steam {
        let result = if args.install_steam { steam::install() } else { steam::uninstall() };
        if let Err(e) = result {
            eprintln!("erro: {e}");
            std::process::exit(1);
        }
        return;
    }

    let data = data_dir();
    let _ = std::fs::create_dir_all(&data);
    log::init(&data.join("launcher.log"));

    torrent::allow_origin(port());

    // Become a subreaper: helpers Chromium detaches (crashpad, zygotes) get
    // re-parented to us instead of Steam's reaper, and we reap them all
    // before exiting, so Steam sees exactly one clean process exit.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1);
    }

    // `block_on` drives `run` on the main thread, so the browser is spawned
    // from it and PR_SET_PDEATHSIG stays tied to the launcher's lifetime.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = rt.block_on(run(args));
    drop(rt);
    browser::reap_orphans();
    log!("encerrado (código {code})");
    std::process::exit(code);
}

async fn run(args: Args) -> i32 {
    let started = Instant::now();
    log!("{APP_NAME} {} iniciando (pid {})", env!("CARGO_PKG_VERSION"), std::process::id());

    for (k, v) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if k.starts_with("LD_") || k.starts_with("Steam") || k.starts_with("STEAM_COMPAT")
            || ["DISPLAY", "WAYLAND_DISPLAY", "XDG_SESSION_TYPE", "SteamGameId"].contains(&k.as_ref())
        {
            log!("env {k}={}", v.to_string_lossy());
        }
    }

    let quit = std::sync::Arc::new(Notify::new());
    let listener = match server::bind(SocketAddr::from(([127, 0, 0, 1], port()))).await {
        Ok(l) => l,
        Err(e) => {
            log!("não foi possível abrir a porta {}: {e}", port());
            return 1;
        }
    };
    let addr = listener.local_addr().expect("local addr");
    log!("servidor em http://{addr} ({} ms)", started.elapsed().as_millis());
    tokio::spawn(server::serve(listener, quit.clone(), started));
    torrent::spawn(&data_dir());
    sources::load();
    jobs::start();

    let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("SIGINT handler");
    let mut hup = signal(SignalKind::hangup()).expect("SIGHUP handler");

    if args.no_browser {
        tokio::select! {
            _ = quit.notified() => log!("saída solicitada pela interface"),
            _ = term.recv() => log!("SIGTERM"),
            _ = int.recv() => log!("SIGINT"),
            _ = hup.recv() => log!("SIGHUP"),
        }
        torrent::stop().await;
        return 0;
    }

    let mut child = match browser::launch(&format!("http://{addr}/"), &data_dir()) {
        Ok(c) => c,
        Err(e) => {
            log!("falha ao abrir o navegador: {e}");
            return 1;
        }
    };
    log!("navegador iniciado (pid {:?}, {} ms)", child.id(), started.elapsed().as_millis());

    let pgid = child.id().map(|p| p as i32);
    let browser_exited = tokio::select! {
        status = child.wait() => {
            log!("navegador encerrou: {status:?}");
            true
        }
        _ = quit.notified() => { log!("saída solicitada pela interface"); false }
        _ = term.recv() => { log!("SIGTERM"); false }
        _ = int.recv() => { log!("SIGINT"); false }
        _ = hup.recv() => { log!("SIGHUP"); false }
    };
    // Close the window first so leaving feels instant, then flush torrents.
    if browser_exited {
        if let Some(pgid) = pgid {
            browser::kill_group(pgid);
        }
    } else {
        browser::shutdown(&mut child).await;
    }
    torrent::stop().await;
    0
}
