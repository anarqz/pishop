use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rust_embed::RustEmbed;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::log;

#[derive(RustEmbed)]
#[folder = "../web/dist/"]
struct Assets;

/// Changes whenever the embedded UI changes: index.html references every
/// content-hashed asset, so hashing it covers the whole bundle.
pub fn ui_fingerprint() -> String {
    let index = Assets::get("index.html").map(|f| f.data.into_owned()).unwrap_or_default();
    // FNV-1a: stable across builds, unlike std's DefaultHasher.
    let hash = index.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    format!("{hash:016x}")
}

#[derive(Clone)]
struct AppState {
    quit: Arc<Notify>,
    started: Instant,
    addr: SocketAddr,
}

/// Binds the fixed port (the PWA origin must stay stable for its storage and
/// service worker). A leftover instance holding it is asked to quit first.
pub async fn bind(addr: SocketAddr) -> std::io::Result<TcpListener> {
    match TcpListener::bind(addr).await {
        Ok(l) => return Ok(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            log!("porta {} ocupada, pedindo para a instância anterior sair", addr.port());
            ask_previous_to_quit(addr).await;
        }
        Err(e) => return Err(e),
    }
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn ask_previous_to_quit(addr: SocketAddr) {
    let req = "POST /api/quit HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let exchange = async {
        let mut s = TcpStream::connect(addr).await?;
        s.write_all(req.as_bytes()).await?;
        // Wait for the reply: hanging up early makes the server cancel the request.
        let mut buf = [0u8; 256];
        let n = s.read(&mut buf).await?;
        std::io::Result::Ok(String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string())
    };
    match tokio::time::timeout(Duration::from_secs(2), exchange).await {
        Ok(Ok(status)) => log!("instância anterior respondeu: {status}"),
        Ok(Err(e)) => log!("instância anterior não respondeu: {e}"),
        Err(_) => log!("instância anterior não respondeu a tempo"),
    }
}

pub async fn serve(listener: TcpListener, quit: Arc<Notify>, started: Instant) {
    let addr = listener.local_addr().expect("local addr");
    let state = AppState { quit, started, addr };
    let app = Router::new()
        .route("/api/info", get(info))
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/quit", post(quit_handler))
        .route("/api/keyboard", post(keyboard_handler))
        .fallback(static_handler)
        .with_state(state)
        .merge(crate::api::router());
    if let Err(e) = axum::serve(listener, app).await {
        log!("servidor caiu: {e}");
    }
}

async fn info(State(s): State<AppState>) -> impl IntoResponse {
    axum::Json(json!({
        "name": crate::APP_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "addr": s.addr.to_string(),
        "uptime_secs": s.started.elapsed().as_secs(),
        "pid": std::process::id(),
        "torrent_api": format!("http://127.0.0.1:{}", crate::torrent::api_port()),
        "download_dir": crate::torrent::download_dir(),
    }))
}

async fn quit_handler(State(s): State<AppState>) -> impl IntoResponse {
    // Reply first; the main task tears everything down right after.
    let quit = s.quit.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        quit.notify_one();
    });
    StatusCode::ACCEPTED
}

/// Opens Steam's on-screen keyboard (Game Mode). Fire-and-forget: the `steam`
/// helper just forwards the URL to the running client and exits.
async fn keyboard_handler() -> impl IntoResponse {
    let child = tokio::process::Command::new("steam")
        .arg("steam://open/keyboard")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match child {
        Ok(mut c) => {
            tokio::spawn(async move {
                let _ = c.wait().await;
            });
            StatusCode::ACCEPTED
        }
        Err(e) => {
            log!("não foi possível abrir o teclado da Steam: {e}");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (file, path) = match Assets::get(path) {
        Some(f) => (f, path),
        // SPA fallback for client-side routes; real missing files stay 404.
        None if !path.contains('.') => match Assets::get("index.html") {
            Some(f) => (f, "index.html"),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    (
        [(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, cache.to_string())],
        file.data.into_owned(),
    )
        .into_response()
}
