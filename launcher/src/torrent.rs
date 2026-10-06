//! Native BitTorrent engine (librqbit): TCP + uTP peers, DHT, trackers, UPnP.
//! Its HTTP API (list/add/pause/delete + ranged streaming) listens on a second
//! loopback port that only the PWA's origin may call (CORS).

use std::net::{Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use librqbit::http_api::{HttpApi, HttpApiOptions};
use librqbit::limits::LimitsConfig;
use librqbit::{Api, ListenerMode, ListenerOptions, Session, SessionOptions, SessionPersistenceConfig};
use librqbit_dualstack_sockets::{BindOpts, TcpListener};
use serde::{Deserialize, Serialize};

use crate::log;

/// Engine ports follow the UI port (47800 → API 47801, peers 47881), so a
/// second instance on another UI port (tests) never fights over them.
pub fn api_port() -> u16 {
    crate::port() + 1
}

/// Incoming peer connections (TCP and uTP); forwarded via UPnP when possible.
fn peer_port() -> u16 {
    crate::port() + 81
}

static SESSION: OnceLock<Arc<Session>> = OnceLock::new();

/// Where finished files land: ~/Downloads/piShop.
pub fn download_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Downloads")
        .join(crate::APP_NAME)
}

/// Must run before any thread starts: librqbit reads it when building CORS.
pub fn allow_origin(port: u16) {
    let re = format!(r"^http://(127\.0\.0\.1|localhost):{port}$");
    unsafe { std::env::set_var("CORS_ALLOW_REGEXP", re) };
}

/// Settings → Downloads: overall speed cap (bytes/s, None = unlimited).
/// Stored in `<data>/torrent.json`; applied live and at every start.
#[derive(Serialize, Deserialize, Clone, Copy, Default)]
pub struct Limits {
    #[serde(default)]
    pub download_bps: Option<u32>,
}

fn limits_file() -> PathBuf {
    crate::data_dir().join("torrent.json")
}

pub fn limits() -> Limits {
    std::fs::read(limits_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn set_limits(l: Limits) -> anyhow::Result<Limits> {
    let l = Limits { download_bps: l.download_bps.filter(|b| *b > 0) };
    std::fs::write(limits_file(), serde_json::to_vec_pretty(&l)?)?;
    if let Some(s) = SESSION.get() {
        s.ratelimits.set_download_bps(l.download_bps.and_then(NonZeroU32::new));
    }
    log!("torrent: limite de download {}", l.download_bps.map(|b| format!("{b} B/s")).unwrap_or_else(|| "desligado".into()));
    Ok(l)
}

const START_ATTEMPTS: u32 = 30;

/// Starts the engine in the background so it never delays the UI. If a port
/// is briefly taken (an instance still shutting down), it keeps retrying for
/// about a minute instead of leaving the session without downloads.
pub fn spawn(data: &Path) {
    let data = data.to_path_buf();
    tokio::spawn(async move {
        for attempt in 1..=START_ATTEMPTS {
            match start(&data).await {
                Ok(()) => return,
                Err(e) => {
                    log!("torrent: falha ao iniciar (tentativa {attempt}/{START_ATTEMPTS}): {e:#}");
                    // The session exists but its API server stopped: a second
                    // session would clash with it, so don't retry from scratch.
                    if SESSION.get().is_some() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        log!("torrent: motor indisponível nesta sessão");
    });
}

pub fn running() -> bool {
    SESSION.get().is_some()
}

async fn start(data: &Path) -> anyhow::Result<()> {
    // Claim the API port first: a session is only created once it's ours.
    let addr = SocketAddr::from(([127, 0, 0, 1], api_port()));
    let listener = TcpListener::bind_tcp(addr, BindOpts { request_dualstack: false, ..Default::default() })?;

    let downloads = download_dir();
    std::fs::create_dir_all(&downloads)?;
    let opts = SessionOptions {
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json { folder: Some(data.join("torrents")) }),
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpAndUtp,
            listen_addr: (Ipv6Addr::UNSPECIFIED, peer_port()).into(),
            enable_upnp_port_forwarding: true,
            ..Default::default()
        }),
        ratelimits: LimitsConfig { download_bps: limits().download_bps.and_then(NonZeroU32::new), upload_bps: None },
        ..Default::default()
    };
    let session = Session::new_with_opts(downloads.clone(), opts).await?;
    let _ = SESSION.set(session.clone());

    let http = HttpApi::new(Api::new(session, None, None), Some(HttpApiOptions::default()));
    log!("torrent: sessão pronta, API em http://{addr}, peers na porta {}, downloads em {}", peer_port(), downloads.display());
    http.make_http_api_and_run(listener, None).await
}

/// Flushes session state (resume data) before exit; bounded so quitting stays fast.
pub async fn stop() {
    if let Some(s) = SESSION.get() {
        if tokio::time::timeout(Duration::from_secs(2), s.stop()).await.is_err() {
            log!("torrent: sessão não parou a tempo");
        }
    }
}
