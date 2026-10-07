//! Updates from GitHub releases. A released piShop (its folder carries the
//! VERSION file the release CI writes) looks for a newer release a little
//! after it starts and every few hours, and downloads it in the background:
//! the bundle (sha256-checked) and, when the release pins another Chromium,
//! that browser build from Google — the same steps as docs/install.sh. The
//! result waits in a folder next to piShop's own and takes its place the next
//! time piShop starts, or right away on "Restart now": piShop closes, swaps
//! the folders and execs the new launcher in its own process, so Steam just
//! sees its game still running. Development builds (no VERSION) only check.

use std::cmp::Ordering;
use std::io::{BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use axum::Json;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{log, tr};

const REPO: &str = "anarqz/pishop";
const ASSET: &str = "piShop-linux-x86_64.tar.gz";
const EVERY: Duration = Duration::from_secs(6 * 3600);

// ---------- versions ----------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ident {
    Num(u64),
    Text(String),
}

/// A release tag as semver ("v0.2.0-alpha.1"); build metadata is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: [u64; 3],
    pre: Vec<Ident>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let mut nums = core.split('.').map(|n| n.parse::<u64>().ok());
        let core = [nums.next()??, nums.next().flatten().unwrap_or(0), nums.next().flatten().unwrap_or(0)];
        let pre = pre
            .map(|p| p.split('.').map(|i| i.parse().map(Ident::Num).unwrap_or_else(|_| Ident::Text(i.to_string()))).collect())
            .unwrap_or_default();
        Some(Version { core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            // 1.0.0 comes after 1.0.0-alpha.
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let c = match (a, b) {
                        (Ident::Num(x), Ident::Num(y)) => x.cmp(y),
                        (Ident::Num(_), Ident::Text(_)) => Ordering::Less,
                        (Ident::Text(_), Ident::Num(_)) => Ordering::Greater,
                        (Ident::Text(x), Ident::Text(y)) => x.cmp(y),
                    };
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The installed release ("v0.2.0-alpha.1"); None for development builds.
pub fn installed() -> Option<String> {
    let v = std::fs::read_to_string(crate::base_dir().join("VERSION")).ok()?;
    let v = v.trim().to_string();
    Version::parse(&v).map(|_| v)
}

/// What piShop shows as its version.
pub fn display_version() -> String {
    installed().unwrap_or_else(|| format!("{} (dev)", env!("CARGO_PKG_VERSION")))
}

// ---------- folders ----------

/// A folder next to piShop's own: "piShop.update", "piShop.old"…
fn sibling(app: &Path, suffix: &str) -> PathBuf {
    let name = app.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "piShop".into());
    app.with_file_name(format!("{name}.{suffix}"))
}

fn staged_dir() -> PathBuf {
    sibling(&crate::base_dir(), "update")
}

/// The version waiting in the staged folder, if it's complete.
fn staged_version() -> Option<String> {
    let v = std::fs::read_to_string(staged_dir().join(".complete")).ok()?;
    Some(v.trim().to_string()).filter(|v| Version::parse(v).is_some())
}

static RESTART: AtomicBool = AtomicBool::new(false);

/// "Restart now" was asked: main swaps and execs once piShop has closed.
pub fn restart_requested() -> bool {
    RESTART.load(AtomicOrdering::Relaxed)
}

/// Puts a downloaded release in place of the running one and execs it, when
/// there's one newer than what's installed. Returns only if there's nothing
/// to do or something failed (piShop then just goes on as it is).
pub fn apply_staged() {
    let app = crate::base_dir();
    let staged = staged_dir();
    let Some(new) = staged_version() else { return };
    let current = installed();
    let newer = match (Version::parse(&new), current.as_deref().and_then(Version::parse)) {
        (Some(n), Some(c)) => n > c,
        _ => false,
    };
    if !newer {
        let _ = std::fs::remove_dir_all(&staged);
        return;
    }
    let old = sibling(&app, "old");
    let _ = std::fs::remove_dir_all(&old);
    if let Err(e) = std::fs::rename(&app, &old) {
        log!("atualização: não consegui tirar {} do lugar: {e}", app.display());
        return;
    }
    if let Err(e) = std::fs::rename(&staged, &app) {
        log!("atualização: não consegui pôr {new} no lugar: {e}");
        let _ = std::fs::rename(&old, &app);
        return;
    }
    // What releases don't ship stays: the browser (unless the release brought
    // another version) and the covers cache.
    for keep in ["chromium", ".cache"] {
        if !app.join(keep).exists() && old.join(keep).exists() {
            let _ = std::fs::rename(old.join(keep), app.join(keep));
        }
    }
    let _ = std::fs::remove_file(app.join(".complete"));
    log!("atualização: {} → {new}", current.unwrap_or_default());
    let err = std::process::Command::new(app.join("pishop")).args(std::env::args_os().skip(1)).exec();
    log!("atualização: não consegui iniciar a versão nova: {err}");
}

/// The previous version's folder, once the new one runs.
pub fn cleanup() {
    let old = sibling(&crate::base_dir(), "old");
    if old.exists() {
        std::thread::spawn(move || {
            let _ = std::fs::remove_dir_all(old);
        });
    }
}

// ---------- checking and downloading ----------

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub current: String,
    /// A development build: newer releases are only reported.
    pub dev: bool,
    pub latest: Option<String>,
    /// idle | checking | up_to_date | available | downloading | ready | error
    pub state: &'static str,
    pub progress: f32,
    pub error: Option<String>,
    /// Unix seconds of the last check.
    pub checked: Option<i64>,
    pub notes: Option<String>,
}

static STATUS: LazyLock<Mutex<Status>> = LazyLock::new(|| {
    Mutex::new(Status { current: display_version(), dev: installed().is_none(), state: "idle", ..Default::default() })
});

fn set(f: impl FnOnce(&mut Status)) {
    f(&mut STATUS.lock().unwrap());
}

pub fn status() -> Status {
    STATUS.lock().unwrap().clone()
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("piShop/{}", display_version()))
        .connect_timeout(Duration::from_secs(15))
        .build()?)
}

struct Release {
    version: String,
    url: String,
    notes: Option<String>,
}

/// The newest published release with piShop's bundle (pre-releases count:
/// alpha builds are the current ones).
async fn latest() -> anyhow::Result<Option<Release>> {
    let list: Vec<Value> = client()?
        .get(format!("https://api.github.com/repos/{REPO}/releases?per_page=15"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mut best: Option<(Version, Release)> = None;
    for r in &list {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(tag) = r["tag_name"].as_str() else { continue };
        let Some(v) = Version::parse(tag) else { continue };
        let Some(url) = r["assets"].as_array().into_iter().flatten().find(|a| a["name"] == ASSET).and_then(|a| a["browser_download_url"].as_str())
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| v > *b) {
            let release = Release { version: tag.to_string(), url: url.to_string(), notes: r["html_url"].as_str().map(String::from) };
            best = Some((v, release));
        }
    }
    Ok(best.map(|(_, r)| r))
}

async fn download(url: &str, to: &Path, progress: impl Fn(f32)) -> anyhow::Result<()> {
    let mut r = client()?.get(url).send().await?.error_for_status()?;
    let total = r.content_length();
    let mut f = tokio::fs::File::create(to).await?;
    let mut done = 0u64;
    while let Some(chunk) = r.chunk().await? {
        f.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if let Some(t) = total.filter(|t| *t > 0) {
            progress(done as f32 / t as f32);
        }
    }
    f.flush().await?;
    Ok(())
}

fn sha256(path: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Chrome for Testing's zip into `dest`, without its top folder.
fn unzip_browser(zip: &Path, dest: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut archive = zip::ZipArchive::new(BufReader::new(std::fs::File::open(zip)?))?;
    for i in 0..archive.len() {
        let mut e = archive.by_index(i)?;
        let Some(rel) = e.enclosed_name() else { continue };
        let rel: PathBuf = rel.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(&rel);
        if e.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut e, &mut f)?;
        if let Some(mode) = e.unix_mode() {
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

/// Downloads a release and leaves it complete in the staged folder.
async fn stage(r: &Release) -> anyhow::Result<()> {
    let app = crate::base_dir();
    let work = sibling(&app, "update-work");
    let _ = tokio::fs::remove_dir_all(&work).await;
    tokio::fs::create_dir_all(&work).await?;
    if crate::localfs::disk_space(&work).is_some_and(|(free, _)| free < 1_200_000_000) {
        bail!(tr!("not enough free space (piShop needs about 1.2 GB to update)", "sem espaço livre (o piShop precisa de cerca de 1,2 GB para atualizar)"));
    }
    let tarball = work.join(ASSET);
    // The bundle is a small part of the download when the browser comes too.
    download(&r.url, &tarball, |p| set(|s| s.progress = p * 0.1)).await.context(tr!("download failed", "o download falhou"))?;
    if let Ok(sum) = async {
        client()?.get(format!("{}.sha256", r.url)).send().await?.error_for_status()?.text().await.map_err(anyhow::Error::from)
    }
    .await
    {
        let want = sum.split_whitespace().next().unwrap_or_default().to_lowercase();
        let path = tarball.clone();
        let got = tokio::task::spawn_blocking(move || sha256(&path)).await??;
        if got != want {
            bail!(tr!("the download is corrupted (sha256 doesn't match)", "o download está corrompido (o sha256 não confere)"));
        }
    }
    let x = work.join("x");
    tokio::fs::create_dir_all(&x).await?;
    let status = tokio::process::Command::new("tar").arg("-C").arg(&x).arg("-xzf").arg(&tarball).status().await?;
    if !status.success() {
        bail!(tr!("couldn't unpack the update", "não foi possível descompactar a atualização"));
    }
    let new = x.join("piShop");
    if !new.join("pishop").is_file() {
        bail!(tr!("the update package is invalid", "o pacote da atualização é inválido"));
    }

    // The browser comes from Google, only when the release pins another version.
    let chrome = std::fs::read_to_string(new.join("chromium.version")).unwrap_or_default().trim().to_string();
    let have = app.join("chromium").join(format!(".version-{chrome}")).exists();
    if !chrome.is_empty() && !have {
        let zip = work.join("chrome.zip");
        let url = format!("https://storage.googleapis.com/chrome-for-testing-public/{chrome}/linux64/chrome-linux64.zip");
        download(&url, &zip, |p| set(|s| s.progress = 0.1 + p * 0.85)).await.context("Chromium")?;
        let dest = new.join("chromium");
        let z = zip.clone();
        let d = dest.clone();
        tokio::task::spawn_blocking(move || unzip_browser(&z, &d)).await??;
        tokio::fs::write(dest.join(format!(".version-{chrome}")), b"").await?;
    }

    let staged = staged_dir();
    let _ = tokio::fs::remove_dir_all(&staged).await;
    tokio::fs::rename(&new, &staged).await?;
    tokio::fs::write(staged.join(".complete"), r.version.as_bytes()).await?;
    let _ = tokio::fs::remove_dir_all(&work).await;
    Ok(())
}

static BUSY: AtomicBool = AtomicBool::new(false);

/// Looks for a newer release and, on a released build, downloads it.
pub async fn check() {
    if BUSY.swap(true, AtomicOrdering::SeqCst) {
        return;
    }
    set(|s| {
        s.state = "checking";
        s.error = None;
    });
    let result: anyhow::Result<()> = async {
        let found = latest().await.context(tr!("GitHub didn't answer", "o GitHub não respondeu"))?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        set(|s| s.checked = Some(now));
        let Some(r) = found else {
            set(|s| s.state = "up_to_date");
            return Ok(());
        };
        set(|s| {
            s.latest = Some(r.version.clone());
            s.notes = r.notes.clone();
        });
        let Some(current) = installed() else {
            // Development build: say what's out there, don't replace it.
            let newer = Version::parse(&r.version).zip(Version::parse(env!("CARGO_PKG_VERSION"))).is_some_and(|(n, c)| n > c);
            set(|s| s.state = if newer { "available" } else { "up_to_date" });
            return Ok(());
        };
        let newer = Version::parse(&r.version).zip(Version::parse(&current)).is_some_and(|(n, c)| n > c);
        if !newer {
            set(|s| s.state = "up_to_date");
            return Ok(());
        }
        if staged_version().as_deref() == Some(r.version.as_str()) {
            set(|s| s.state = "ready");
            return Ok(());
        }
        set(|s| {
            s.state = "downloading";
            s.progress = 0.0;
        });
        log!("atualização: baixando {} ({current} instalada)", r.version);
        stage(&r).await?;
        log!("atualização: {} pronta; entra no próximo início", r.version);
        set(|s| {
            s.state = "ready";
            s.progress = 1.0;
        });
        Ok(())
    }
    .await;
    if let Err(e) = result {
        log!("atualização: {e:#}");
        let _ = std::fs::remove_dir_all(sibling(&crate::base_dir(), "update-work"));
        set(|s| {
            s.state = "error";
            s.error = Some(format!("{e:#}"));
        });
    }
    BUSY.store(false, AtomicOrdering::SeqCst);
}

/// Checks a little after start (piShop and the network settle first), then
/// every few hours.
pub fn start() {
    cleanup();
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            check().await;
            tokio::time::sleep(EVERY).await;
        }
    });
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/api/update", get(|| async { Json(status()) }))
        .route(
            "/api/update/check",
            post(|| async {
                tokio::spawn(check());
                Json(status())
            }),
        )
        .route("/api/update/apply", post(apply_route))
}

/// "Restart now": piShop closes as usual, then main swaps and execs.
async fn apply_route() -> Response {
    if staged_version().is_none() {
        return (axum::http::StatusCode::CONFLICT, Json(json!({ "error": tr!("no update is ready", "nenhuma atualização está pronta") })))
            .into_response();
    }
    RESTART.store(true, AtomicOrdering::Relaxed);
    crate::server::request_quit();
    Json(json!({ "restarting": true })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_order_like_semver() {
        assert!(v("v0.2.0-alpha.1") > v("v0.1.0-alpha.2"));
        assert!(v("v0.1.0-alpha.10") > v("v0.1.0-alpha.2"));
        assert!(v("v0.1.0") > v("v0.1.0-alpha.9"));
        assert!(v("v0.1.0-beta") > v("v0.1.0-alpha.9"));
        assert!(v("v0.1.0-alpha.1") > v("v0.1.0-alpha"));
        assert_eq!(v("v1.2.3+build.5"), v("1.2.3"));
        assert_eq!(v("v1.2"), v("1.2.0"));
        assert!(Version::parse("latest").is_none());
    }

    #[test]
    fn folders_sit_next_to_the_app() {
        assert_eq!(sibling(Path::new("/home/deck/Applications/piShop"), "update"), Path::new("/home/deck/Applications/piShop.update"));
        assert_eq!(sibling(Path::new("/opt/pishop-custom"), "old"), Path::new("/opt/pishop-custom.old"));
    }
}
