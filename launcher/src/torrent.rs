//! Native BitTorrent engine (librqbit): TCP + uTP peers, DHT, trackers, UPnP.
//! Its HTTP API (list/add/pause/delete + ranged streaming) listens on a second
//! loopback port that only the PWA's origin may call (CORS).

use std::net::{Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use librqbit::http_api::{HttpApi, HttpApiOptions};
use librqbit::{Api, ListenerMode, ListenerOptions, Session, SessionOptions, SessionPersistenceConfig};
use librqbit_dualstack_sockets::{BindOpts, TcpListener};

use crate::log;

pub const API_PORT: u16 = 47801;
/// Incoming peer connections (TCP and uTP); forwarded via UPnP when possible.
const PEER_PORT: u16 = 47881;

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

/// Starts the session in the background so it never delays the UI.
pub fn spawn(data: &Path) {
    let data = data.to_path_buf();
    tokio::spawn(async move {
        if let Err(e) = start(&data).await {
            log!("torrent: falha ao iniciar: {e:#}");
        }
    });
}

async fn start(data: &Path) -> anyhow::Result<()> {
    let downloads = download_dir();
    std::fs::create_dir_all(&downloads)?;
    let opts = SessionOptions {
        fastresume: true,
        persistence: Some(SessionPersistenceConfig::Json { folder: Some(data.join("torrents")) }),
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpAndUtp,
            listen_addr: (Ipv6Addr::UNSPECIFIED, PEER_PORT).into(),
            enable_upnp_port_forwarding: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let session = Session::new_with_opts(downloads.clone(), opts).await?;
    let _ = SESSION.set(session.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], API_PORT));
    let listener = TcpListener::bind_tcp(addr, BindOpts { request_dualstack: false, ..Default::default() })?;
    let http = HttpApi::new(Api::new(session, None, None), Some(HttpApiOptions::default()));
    log!("torrent: sessão pronta, API em http://{addr}, downloads em {}", downloads.display());
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
