//! The install wizard for Windows games downloaded from the Store.
//!
//! A non-Steam shortcut first points at the game's installer and runs it under
//! the chosen Proton; when the installer is done, the same shortcut (same
//! appid, so the same Proton prefix with whatever the installer registered) is
//! repointed at the installed game's main executable. Games that need no
//! installer get their shortcut straight away. Everything goes through Steam's
//! live client API (`steamclient`), so Steam doesn't restart.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use axum::extract::Path as UrlPath;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::proton::{self, norm};
use crate::steamclient::{self, Art};
use crate::{exeguess, library, localfs, log, relocate, torrent, tr, winetricks};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Ready,
    /// The installer is running (or just finished) under Proton.
    Installing,
    /// The shortcut points at the game.
    Installed,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct InstallState {
    pub stage: Stage,
    /// The game's non-Steam shortcut.
    pub appid: Option<u32>,
    /// Compatibility tool (e.g. "proton_experimental").
    pub tool: String,
    /// Install folder (Linux path).
    pub target: String,
    #[serde(default)]
    pub installer: Option<String>,
    /// The game's executable once installed.
    #[serde(default)]
    pub exe: Option<String>,
    /// Unix seconds when the installer was started.
    #[serde(default)]
    pub started: i64,
    /// How the installer was started ("steam": Steam launched the shortcut).
    #[serde(default)]
    pub runner: String,
    /// The installed game's own folder (where its installer put it).
    #[serde(default)]
    pub game_dir: Option<String>,
    /// The shortcut as it was while another installer borrows it (see
    /// `run_in_prefix`); put back when that installer closes.
    #[serde(default)]
    pub restore: Option<steamclient::Shortcut>,
    /// The installer borrowing the shortcut.
    #[serde(default)]
    pub borrowed: Option<String>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn err(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": msg.to_string() }))).into_response()
}

pub fn reply<T: Serialize>(r: anyhow::Result<T>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/api/install/{hash}", get(|UrlPath(h): UrlPath<String>| async move { reply(options(&h).await) }))
        .route("/api/install/{hash}/start", post(start_route))
        .route("/api/install/{hash}/status", get(|UrlPath(h): UrlPath<String>| async move { reply(status(&h).await) }))
        .route("/api/install/{hash}/finish", post(finish_route))
        .route("/api/install/{hash}/portable", post(portable_route))
        .route("/api/install/{hash}/play", post(|UrlPath(h): UrlPath<String>| async move { reply(play(&h).await) }))
        .route("/api/install/{hash}/info", get(|UrlPath(h): UrlPath<String>| async move { reply(info(&h).await) }))
        .route("/api/install/{hash}/tool", post(tool_route))
        .route("/api/install/{hash}/move", get(|UrlPath(h): UrlPath<String>| async move { Json(move_job(&h)) }).post(move_route))
        .route("/api/install/{hash}/move/cancel", post(|UrlPath(h): UrlPath<String>| async move { reply(move_cancel(&h)) }))
        .route("/api/install/{hash}/run", post(run_route))
        .route("/api/install/{hash}/artwork", post(|UrlPath(h): UrlPath<String>| async move { reply(refresh_artwork(&h).await) }))
        .route(
            "/api/library/artwork",
            get(|| async { Json(ART_JOB.lock().unwrap().clone()) }).post(|| async { reply(refresh_all_artwork().await) }),
        )
        .route("/api/install/{hash}/reset", post(|UrlPath(h): UrlPath<String>| async move { reply(reset(&h).await) }))
        .route("/api/steam/client-api", post(|| async { reply(enable_client_api().await) }))
}

// ---------- what's on disk ----------

/// Top-level files/folders of a download, from the torrent engine.
async fn content(hash: &str) -> anyhow::Result<Vec<PathBuf>> {
    let v: Value = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/torrents/{hash}", torrent::api_port()))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context(tr!("the torrent engine didn't respond", "o motor de torrents não respondeu"))?
        .json()
        .await?;
    let out = PathBuf::from(v["output_folder"].as_str().ok_or_else(|| anyhow!(tr!("download not found", "download não encontrado")))?);
    let mut tops = BTreeSet::new();
    for f in v["files"].as_array().into_iter().flatten() {
        let first = f["components"]
            .as_array()
            .and_then(|c| c.first())
            .and_then(|c| c.as_str())
            .map(String::from)
            .or_else(|| f["name"].as_str().map(|n| n.split('/').next().unwrap_or(n).to_string()));
        if let Some(first) = first {
            tops.insert(out.join(first));
        }
    }
    if tops.is_empty() {
        tops.insert(out);
    }
    Ok(tops.into_iter().collect())
}

/// The download's own folders, plus what its archives were extracted to. Never
/// the folder around a single-file download: that's the shared Downloads
/// folder, full of other games.
fn roots(content: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = content.iter().filter(|p| p.is_dir()).cloned().collect();
    for p in content {
        let archives: Vec<PathBuf> = if p.is_dir() {
            crate::archive::inspect(p).map(|i| i.archives.into_iter().map(|a| PathBuf::from(a.path)).collect()).unwrap_or_default()
        } else {
            vec![p.clone()]
        };
        for a in archives {
            if let Some(dest) = crate::archive::default_dest_for(&a).filter(|d| d.is_dir() && !out.contains(d)) {
                out.push(dest);
            }
        }
    }
    out
}

/// Windows executables that are downloads themselves (a lone setup.exe or game.exe).
fn loose_exes(content: &[PathBuf]) -> Vec<PathBuf> {
    content
        .iter()
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
        .cloned()
        .collect()
}

#[derive(Serialize)]
struct Library {
    path: String,
    label: String,
    free: Option<u64>,
    total: Option<u64>,
}

fn steam_root() -> PathBuf {
    proton::steam_root()
}

/// Steam library folders (internal, SD card, other drives).
fn libraries() -> Vec<Library> {
    proton::library_paths()
        .into_iter()
        .map(|p| {
            let space = localfs::disk_space(&p);
            Library { label: disk_label(&p), path: p.display().to_string(), free: space.map(|s| s.0), total: space.map(|s| s.1) }
        })
        .collect()
}

/// "Internal storage", or the label of the card/drive a path is on.
fn disk_label(path: &Path) -> String {
    if path.starts_with("/home") {
        return tr!("Internal storage", "Armazenamento interno");
    }
    // /run/media/<user>/<label>/… (or /run/media/<label>/…)
    if let Ok(rest) = path.strip_prefix("/run/media") {
        let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let user = std::env::var("USER").unwrap_or_default();
        let label = if parts.len() > 1 && parts[0] == user { parts.get(1) } else { parts.first() };
        if let Some(l) = label {
            return l.clone();
        }
    }
    path.display().to_string()
}

#[derive(Serialize)]
struct Tool {
    name: String,
    display: String,
    installed: bool,
}

/// Proton builds Steam offers, marking the ones already on disk; Steam
/// downloads the others the first time they're used.
async fn tools() -> anyhow::Result<Vec<Tool>> {
    let mut on_disk: Vec<String> = Vec::new();
    for lib in libraries() {
        for e in std::fs::read_dir(Path::new(&lib.path).join("steamapps/common")).into_iter().flatten().flatten() {
            on_disk.push(norm(&e.file_name().to_string_lossy()));
        }
    }
    for e in std::fs::read_dir(steam_root().join("compatibilitytools.d")).into_iter().flatten().flatten() {
        on_disk.push(norm(&e.file_name().to_string_lossy()));
    }
    let mut out: Vec<Tool> = steamclient::compat_tools()
        .await?
        .into_iter()
        .filter(|t| t.display.to_lowercase().contains("proton"))
        .map(|t| {
            // "Proton 9.0-4" is installed as "Proton 9.0 (Beta)"; match on "proton90".
            let short = norm(t.display.split('-').next().unwrap_or(&t.display));
            let installed = on_disk.iter().any(|d| *d == norm(&t.display) || *d == short || d.starts_with(&short) && short.len() > 6);
            Tool { name: t.name, display: t.display, installed }
        })
        .collect();
    out.sort_by_key(|t| !t.installed);
    Ok(out)
}

/// Newest installed numbered Proton, else Experimental, else the first one.
fn default_tool(tools: &[Tool]) -> Option<String> {
    let version = |t: &Tool| -> Option<f32> {
        t.display.trim_start_matches("Proton ").split(['-', ' ']).next()?.parse().ok()
    };
    tools
        .iter()
        .filter(|t| t.installed && t.name.starts_with("proton_") && version(t).is_some())
        .max_by(|a, b| version(a).partial_cmp(&version(b)).unwrap_or(std::cmp::Ordering::Equal))
        .or_else(|| tools.iter().find(|t| t.name == "proton_experimental" && t.installed))
        .or_else(|| tools.first())
        .map(|t| t.name.clone())
}

fn folder_name(game: &str) -> String {
    let s: String = game.chars().filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control()).collect();
    let s = s.trim().trim_end_matches('.').trim().to_string();
    if s.is_empty() { "Game".into() } else { s }
}

fn target_for(library: &str, game: &str) -> PathBuf {
    Path::new(library).join(crate::APP_NAME).join(folder_name(game))
}

fn entry(hash: &str) -> anyhow::Result<library::Entry> {
    library::get(hash).ok_or_else(|| anyhow!(tr!("this download has no game data", "este download não tem dados do jogo")))
}

/// Downloads added by magnet link have no game data yet: start with the
/// torrent's own (cleaned-up) name so the wizard can keep its state.
async fn ensure_entry(hash: &str) -> anyhow::Result<library::Entry> {
    if let Some(e) = library::get(hash) {
        return Ok(e);
    }
    let v: Value = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/torrents/{hash}", torrent::api_port()))
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .json()
        .await?;
    let release = v["name"].as_str().unwrap_or(hash).to_string();
    let parsed = crate::titles::parse(&release);
    library::add(library::Entry {
        info_hash: hash.to_string(),
        game: library::Game { name: parsed.name.clone(), platform: parsed.platform_label.map(String::from), ..Default::default() },
        release,
        indexer: String::new(),
        size: 0,
        dest: None,
        added: now(),
        resolved: true,
        install: None,
    });
    entry(hash)
}

async fn options(hash: &str) -> anyhow::Result<Value> {
    let e = ensure_entry(hash).await?;
    let content = content(hash).await?;
    let roots = roots(&content);
    let mut installers = Vec::new();
    for root in &roots {
        for rel in exeguess::installers(root) {
            let path = root.join(&rel);
            installers.push(json!({ "path": path, "name": rel, "kind": exeguess::installer_kind(&path) }));
        }
    }
    let mut portable = Vec::new();
    for root in &roots {
        for c in exeguess::guess(root, &e.game.name).into_iter().take(8) {
            portable.push(json!({ "path": root.join(&c.path), "name": c.path, "size": c.size, "score": c.score }));
        }
    }
    // A download that is just an .exe: an installer if it looks like one, else the game.
    for exe in loose_exes(&content) {
        let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let kind = exeguess::installer_kind(&exe);
        let installer_like = kind.is_some() || name.to_lowercase().contains("setup") || name.to_lowercase().contains("install");
        let size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
        if installer_like {
            installers.push(json!({ "path": exe, "name": name, "kind": kind }));
        } else {
            portable.push(json!({ "path": exe, "name": name, "size": size, "score": 0 }));
        }
    }
    let api = steamclient::available().await;
    let tools = if api { tools().await.unwrap_or_default() } else { Vec::new() };
    let libs = libraries();
    // The download's own folder (single-file downloads: the folder around it).
    let download_dir = match content.as_slice() {
        [one] if one.is_dir() => one.clone(),
        [first, ..] => first.parent().map(PathBuf::from).unwrap_or_default(),
        [] => PathBuf::new(),
    };
    let default_library = libs.iter().max_by_key(|l| l.free.unwrap_or(0)).map(|l| l.path.clone());
    Ok(json!({
        "game": e.game,
        "content": content,
        "download_dir": download_dir,
        "installers": installers,
        "portable": portable,
        "libraries": libs,
        "default_library": default_library,
        "tools": tools,
        "default_tool": default_tool(&tools),
        "steam_api": api,
        "debugging_enabled": steamclient::debugging_enabled(),
        "state": e.install,
    }))
}

// ---------- shortcut ----------

/// An image for a Steam artwork slot: PNG or JPEG (icons may also be .ico).
async fn fetch(url: &str, icon: bool) -> Option<(Vec<u8>, String)> {
    let (bytes, ctype) = crate::catalog::image(url).await.ok()?;
    let ext = if ctype.contains("png") {
        "png"
    } else if ctype.contains("jpeg") || ctype.contains("jpg") {
        "jpg"
    } else if icon && ctype.contains("icon") {
        "ico"
    } else {
        return None;
    };
    Some((bytes, ext.into()))
}

/// One artwork slot from the first candidate that loads.
async fn put(appid: u32, slot: Art, urls: &[String]) -> Option<String> {
    for url in urls {
        let Some((bytes, ext)) = fetch(url, false).await else { continue };
        match steamclient::set_artwork(appid, slot, &bytes, &ext).await {
            Ok(()) => return Some(url.clone()),
            Err(e) => log!("arte: {slot:?} de {url}: {e:#}"),
        }
    }
    None
}

/// The shortcut's icon from the first candidate that loads (kept in piShop's
/// data folder: Steam points at the file).
async fn set_icon_from(appid: u32, candidates: &[(String, &'static str)]) -> Option<&'static str> {
    let dir = crate::data_dir().join("icons");
    for (url, from) in candidates {
        let Some((bytes, ext)) = fetch(url, true).await else { continue };
        for old in ["png", "jpg", "ico"] {
            let _ = std::fs::remove_file(dir.join(format!("{appid}.{old}")));
        }
        let path = dir.join(format!("{appid}.{ext}"));
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &bytes)).is_ok()
            && steamclient::set_icon(appid, &path.to_string_lossy()).await.is_ok()
        {
            return Some(from);
        }
    }
    None
}

/// Puts a game's whole artwork set on its shortcut, at the best sizes there
/// are: cover, wide capsule (banner), hero, logo and icon. Steam's own
/// library art comes first when the game has an appid (2× sizes, the hashed
/// logo and Steam's placement for it, the client .ico); SteamGridDB fills
/// what Steam lacks, only for that exact game; then the art piShop already
/// had. `fresh` asks Steam's app info again. Returns where each piece came
/// from ("steam", "steamgriddb", "piShop" or null).
pub async fn apply_artwork(appid: u32, game: &library::Game, fresh: bool) -> Value {
    let sid = game.steam_appid.clone().filter(|a| !a.is_empty() && a.chars().all(|c| c.is_ascii_digit()));
    let (store, info) = match sid.as_deref() {
        Some(a) => tokio::join!(crate::steam_store::library_art(a), crate::steam_store::appinfo_art(a, fresh)),
        None => (None, None),
    };
    let store = store.unwrap_or_default();
    let info = info.unwrap_or_default();
    let steam = |files: &[Option<String>]| -> Vec<String> { files.iter().flatten().cloned().collect() };
    let mut logos: Vec<String> = Vec::new();
    if let Some(a) = sid.as_deref() {
        logos.extend(info.logos.iter().map(|f| crate::steam_store::asset_url(a, f)));
        // Older apps keep it under the plain name.
        logos.push(crate::steam_store::asset_url(a, "logo_2x.png"));
        logos.push(crate::steam_store::asset_url(a, "logo.png"));
    }
    let mut icons: Vec<String> = Vec::new();
    if let Some(a) = sid.as_deref() {
        let base = format!("https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/{a}");
        for h in [info.clienticon.as_ref(), store.icon_hash.as_ref()].into_iter().flatten() {
            icons.push(format!("{base}/{h}.ico"));
        }
    }

    let mut sgdb: Option<crate::catalog::SgdbPack> = None;
    let mut sources = serde_json::Map::new();
    let slots: [(Art, &str, Vec<String>); 4] = [
        (Art::Cover, "cover", steam(&[store.cover_2x.clone(), store.cover.clone()])),
        (Art::Wide, "wide", steam(&[store.wide_2x.clone(), store.wide.clone()])),
        (Art::Hero, "hero", steam(&[store.hero_2x.clone(), store.hero.clone()])),
        (Art::Logo, "logo", logos),
    ];
    for (slot, key, urls) in slots {
        let mut source = put(appid, slot, &urls).await.map(|_| "steam");
        if source.is_none() {
            if sgdb.is_none() {
                sgdb = Some(crate::catalog::sgdb_pack(&game.name).await);
            }
            let pack = sgdb.as_ref().unwrap();
            let url = match slot {
                Art::Cover => pack.cover.clone(),
                Art::Wide => pack.wide.clone(),
                Art::Hero => pack.hero.clone(),
                Art::Logo => pack.logo.clone(),
            };
            source = put(appid, slot, &url.into_iter().collect::<Vec<_>>()).await.map(|_| "steamgriddb");
        }
        if source.is_none() {
            let had = match slot {
                Art::Cover => game.cover.clone(),
                Art::Hero => game.hero.clone(),
                _ => None,
            };
            source = put(appid, slot, &had.into_iter().collect::<Vec<_>>()).await.map(|_| "piShop");
        }
        if matches!(slot, Art::Logo) && source.is_some() {
            // Steam's own placement for its logo; else bottom-left, like most games.
            let (pinned, w, h) = match (&info.logo_position, source) {
                (Some(p), Some("steam")) => (p.pinned.clone(), p.width_pct, p.height_pct),
                _ => ("BottomLeft".to_string(), 42.0, 60.0),
            };
            let _ = steamclient::set_logo_position(appid, &pinned, w, h).await;
        }
        sources.insert(key.into(), json!(source));
    }

    // Icon: Steam's .ico, else SteamGridDB's, else Steam's small square.
    let steam_icons: Vec<(String, &'static str)> = icons.into_iter().map(|u| (u, "steam")).collect();
    let mut icon_source = set_icon_from(appid, &steam_icons).await;
    if icon_source.is_none() {
        if sgdb.is_none() {
            sgdb = Some(crate::catalog::sgdb_pack(&game.name).await);
        }
        let rest: Vec<(String, &'static str)> = sgdb
            .as_ref()
            .and_then(|p| p.icon.clone())
            .map(|u| (u, "steamgriddb"))
            .into_iter()
            .chain(store.icon.clone().map(|u| (u, "steam")))
            .collect();
        icon_source = set_icon_from(appid, &rest).await;
    }
    sources.insert("icon".into(), json!(icon_source));
    log!("arte: atalho {appid} ← {}", Value::Object(sources.clone()));
    Value::Object(sources)
}

/// Refreshes one installed game's artwork on its shortcut.
async fn refresh_artwork(hash: &str) -> anyhow::Result<Value> {
    let e = entry(hash)?;
    let appid = e.install.as_ref().and_then(|s| s.appid).ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    if !steamclient::available().await {
        bail!(tr!("Steam's client API isn't reachable", "a API do cliente Steam não está acessível"));
    }
    Ok(apply_artwork(appid, &e.game, true).await)
}

/// Refreshing every installed game's artwork (Transfers), in the background.
#[derive(Serialize, Clone, Default)]
struct ArtJob {
    running: bool,
    done: usize,
    total: usize,
    current: Option<String>,
}

static ART_JOB: LazyLock<Mutex<ArtJob>> = LazyLock::new(Default::default);

async fn refresh_all_artwork() -> anyhow::Result<ArtJob> {
    if ART_JOB.lock().unwrap().running {
        return Ok(ART_JOB.lock().unwrap().clone());
    }
    if !steamclient::available().await {
        bail!(tr!("Steam's client API isn't reachable", "a API do cliente Steam não está acessível"));
    }
    let games: Vec<(u32, library::Game)> =
        library::all().into_values().filter_map(|e| Some((e.install.as_ref()?.appid?, e.game))).collect();
    *ART_JOB.lock().unwrap() = ArtJob { running: true, done: 0, total: games.len(), current: None };
    tokio::spawn(async move {
        for (appid, game) in games {
            ART_JOB.lock().unwrap().current = Some(game.name.clone());
            apply_artwork(appid, &game, true).await;
            ART_JOB.lock().unwrap().done += 1;
        }
        let mut job = ART_JOB.lock().unwrap();
        job.running = false;
        job.current = None;
    });
    Ok(ART_JOB.lock().unwrap().clone())
}

/// Steam stores shortcut paths quoted. AddShortcut quotes the exe itself;
/// SetShortcutExe/StartDir store exactly what they get.
fn quoted(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

async fn create_shortcut(game: &library::Game, exe: &Path, launch_options: &str, tool: &str) -> anyhow::Result<u32> {
    if !steamclient::available().await {
        bail!(tr!(
            "turn on Steam's client API first (Install → Enable)",
            "ative a API do cliente Steam primeiro (Instalar → Ativar)"
        ));
    }
    let dir = exe.parent().unwrap_or(Path::new("/"));
    let appid = steamclient::add_shortcut(&game.name, &exe.display().to_string(), &dir.display().to_string(), launch_options).await?;
    steamclient::set_start_dir(appid, &quoted(dir)).await?;
    // AddShortcut ignores its launch-options argument.
    if !launch_options.is_empty() {
        steamclient::set_launch_options(appid, launch_options).await?;
    }
    if !tool.is_empty() {
        steamclient::set_compat_tool(appid, tool).await?;
    }
    apply_artwork(appid, game, false).await;
    Ok(appid)
}

#[derive(Deserialize)]
struct StartReq {
    installer: String,
    library: String,
    tool: String,
}

async fn start_route(UrlPath(hash): UrlPath<String>, Json(r): Json<StartReq>) -> Response {
    reply(start(&hash, r).await)
}

/// Shortcut → installer under Proton, installing into the chosen library.
async fn start(hash: &str, r: StartReq) -> anyhow::Result<InstallState> {
    let e = ensure_entry(hash).await?;
    let installer = PathBuf::from(&r.installer);
    if !installer.is_file() {
        bail!(tr!("installer not found", "instalador não encontrado"));
    }
    let target = target_for(&r.library, &e.game.name);
    std::fs::create_dir_all(&target).with_context(|| tr!("couldn't create {}", "não foi possível criar {}", target.display()))?;
    // Inno Setup / NSIS take the install folder on the command line; others ask.
    let args = exeguess::installer_kind(&installer)
        .and_then(|k| exeguess::install_dir_args(k, &exeguess::windows_path(&target)))
        .unwrap_or_default();
    let appid = match e.install.as_ref().and_then(|s| s.appid) {
        // A previous attempt: reuse its shortcut (and Proton prefix).
        Some(appid) => {
            steamclient::set_exe(appid, &quoted(&installer)).await?;
            steamclient::set_start_dir(appid, &quoted(installer.parent().unwrap_or(Path::new("/")))).await?;
            steamclient::set_launch_options(appid, &args).await?;
            steamclient::set_compat_tool(appid, &r.tool).await?;
            appid
        }
        None => create_shortcut(&e.game, &installer, &args, &r.tool).await?,
    };
    let state = InstallState {
        stage: Stage::Installing,
        appid: Some(appid),
        tool: r.tool.clone(),
        target: target.display().to_string(),
        installer: Some(installer.display().to_string()),
        exe: None,
        started: now(),
        runner: "steam".into(),
        game_dir: None,
        restore: None,
        borrowed: None,
    };
    library::set_install(hash, Some(state.clone()));
    // Steam launches it as its own game: gamescope only shows windows whose
    // ancestors include a Steam reaper (`SteamLaunch AppId=…`).
    steamclient::run(appid).await?;
    tokio::spawn(return_when_done(appid));
    log!("instalar: {:?} → instalador {:?} (atalho {appid}, args {args:?})", e.game.name, installer);
    Ok(state)
}

/// Waits for what Steam started under the shortcut to come and go (or never
/// to show up within two minutes). False if it's still running after 12 hours.
async fn wait_until_closed(appid: u32) -> bool {
    let mut seen = false;
    let started = std::time::Instant::now();
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if steamclient::running(appid) {
            seen = true;
        } else if seen || started.elapsed() > Duration::from_secs(120) {
            return true;
        }
        if started.elapsed() > Duration::from_secs(12 * 3600) {
            return false;
        }
    }
}

/// Brings piShop back to the front (Steam would otherwise stay on its own screen).
async fn bring_back() {
    if let Ok(me) = std::env::var("SteamGameId") {
        if let Err(e) = steamclient::run_game_id(&me).await {
            log!("instalar: não consegui voltar ao piShop: {e:#}");
        }
    }
}

async fn return_when_done(appid: u32) {
    if wait_until_closed(appid).await {
        bring_back().await;
        log!("instalar: instalador do atalho {appid} terminou");
    }
}

/// Programs the installer registered (Uninstall keys written since it started).
fn registered(state: &InstallState) -> Vec<proton::Program> {
    let Some(appid) = state.appid else { return Vec::new() };
    proton::programs(&proton::compatdata(appid).join("pfx")).into_iter().filter(|p| p.modified >= state.started - 5).collect()
}

/// Where an installer may have put the game: what it registered in the
/// prefix's registry (even on another drive letter, like D:\Games), the
/// target folder, and folders it created inside the prefix since it started.
fn search_roots(state: &InstallState) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = registered(state).into_iter().map(|p| p.path).collect();
    let target = PathBuf::from(&state.target);
    if !out.contains(&target) {
        out.push(target);
    }
    let Some(appid) = state.appid else { return out };
    let drive_c = proton::compatdata(appid).join("pfx/drive_c");
    let parents = [
        "Program Files",
        "Program Files (x86)",
        "Games",
        "GOG Games",
        "users/steamuser/AppData/Local/Programs",
        "users/steamuser/AppData/Roaming",
    ];
    for p in parents {
        for e in std::fs::read_dir(drive_c.join(p)).into_iter().flatten().flatten() {
            let new = e
                .metadata()
                .and_then(|m| m.modified().or_else(|_| m.created()))
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .is_some_and(|d| d.as_secs() as i64 >= state.started - 5);
            if new && e.path().is_dir() && !out.contains(&e.path()) {
                out.push(e.path());
            }
        }
    }
    out
}

async fn status(hash: &str) -> anyhow::Result<Value> {
    let e = entry(hash)?;
    let Some(state) = e.install else { return Ok(json!({ "state": null })) };
    let running = state.appid.is_some_and(steamclient::running);
    let mut candidates = Vec::new();
    // Once the installer is gone, offer the executables it left behind.
    if state.stage == Stage::Installing && !running && now() - state.started > 5 {
        // The executable the installer registered as the game's icon comes first.
        for p in registered(&state) {
            if let Some(icon) = p.icon {
                let size = std::fs::metadata(&icon).map(|m| m.len()).unwrap_or(0);
                candidates.push(json!({ "path": icon, "name": icon.file_name().map(|n| n.to_string_lossy().into_owned()), "root": p.path, "size": size, "score": 1e6, "registered": true }));
            }
        }
        for root in search_roots(&state) {
            for c in exeguess::guess(&root, &e.game.name).into_iter().take(6) {
                candidates.push(json!({ "path": root.join(&c.path), "name": c.path, "root": root, "size": c.size, "score": c.score }));
            }
        }
        candidates.sort_by(|a, b| b["score"].as_f64().partial_cmp(&a["score"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        let mut seen = BTreeSet::new();
        candidates.retain(|c| seen.insert(c["path"].as_str().unwrap_or_default().to_string()));
    }
    Ok(json!({ "state": state, "running": running, "candidates": candidates }))
}

#[derive(Deserialize)]
struct ExeReq {
    exe: String,
    #[serde(default)]
    tool: Option<String>,
}

async fn finish_route(UrlPath(hash): UrlPath<String>, Json(r): Json<ExeReq>) -> Response {
    reply(finish(&hash, &r.exe).await)
}

/// Repoints the installer's shortcut at the game.
async fn finish(hash: &str, exe: &str) -> anyhow::Result<InstallState> {
    let e = entry(hash)?;
    let mut state = e.install.ok_or_else(|| anyhow!(tr!("nothing is being installed", "nada está sendo instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    let exe = PathBuf::from(exe);
    if !exe.is_file() {
        bail!(tr!("executable not found", "executável não encontrado"));
    }
    steamclient::set_exe(appid, &quoted(&exe)).await?;
    steamclient::set_start_dir(appid, &quoted(exe.parent().unwrap_or(Path::new("/")))).await?;
    steamclient::set_launch_options(appid, "").await?;
    state.stage = Stage::Installed;
    state.exe = Some(exe.display().to_string());
    state.game_dir = None;
    state.game_dir = game_dir(&state).map(|d| d.display().to_string());
    library::set_install(hash, Some(state.clone()));
    log!("instalar: {:?} pronto → {}", e.game.name, exe.display());
    Ok(state)
}

async fn portable_route(UrlPath(hash): UrlPath<String>, Json(r): Json<ExeReq>) -> Response {
    reply(portable(&hash, &r.exe, r.tool.as_deref().unwrap_or("")).await)
}

/// No installer needed: the shortcut points at the game right away.
async fn portable(hash: &str, exe: &str, tool: &str) -> anyhow::Result<InstallState> {
    let e = ensure_entry(hash).await?;
    let exe = PathBuf::from(exe);
    if !exe.is_file() {
        bail!(tr!("executable not found", "executável não encontrado"));
    }
    let appid = match e.install.as_ref().and_then(|s| s.appid) {
        Some(appid) => {
            steamclient::set_exe(appid, &quoted(&exe)).await?;
            steamclient::set_start_dir(appid, &quoted(exe.parent().unwrap_or(Path::new("/")))).await?;
            steamclient::set_launch_options(appid, "").await?;
            if !tool.is_empty() {
                steamclient::set_compat_tool(appid, tool).await?;
            }
            appid
        }
        None => create_shortcut(&e.game, &exe, "", tool).await?,
    };
    // The game's folder is the download (or extracted) folder that holds it.
    let holder = roots(&content(hash).await.unwrap_or_default()).into_iter().filter(|r| exe.starts_with(r)).max_by_key(|r| r.components().count());
    let state = InstallState {
        stage: Stage::Installed,
        appid: Some(appid),
        tool: tool.into(),
        target: exe.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        installer: None,
        exe: Some(exe.display().to_string()),
        started: now(),
        runner: String::new(),
        game_dir: holder.or_else(|| exe.parent().map(PathBuf::from)).map(|d| d.display().to_string()),
        restore: None,
        borrowed: None,
    };
    library::set_install(hash, Some(state.clone()));
    Ok(state)
}

async fn play(hash: &str) -> anyhow::Result<Value> {
    let appid = entry(hash)?.install.and_then(|s| s.appid).ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    if moving(hash) || winetricks::active(appid) {
        bail!(tr!("wait for the current task to finish", "espere a tarefa atual terminar"));
    }
    steamclient::run(appid).await?;
    Ok(json!({ "appid": appid }))
}

// ---------- the installed game ----------

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Where the installed game lives: the folder its installer registered (the
/// one holding the executable), else the install folder piShop chose, else
/// the executable's own folder.
fn game_dir(state: &InstallState) -> Option<PathBuf> {
    if let Some(d) = state.game_dir.as_ref().map(PathBuf::from).filter(|d| d.is_dir()) {
        return Some(d);
    }
    let exe = canon(Path::new(state.exe.as_ref()?));
    let pfx = proton::compatdata(state.appid?).join("pfx");
    let target = canon(Path::new(&state.target));
    proton::programs(&pfx)
        .into_iter()
        .map(|p| canon(&p.path))
        .filter(|p| exe.starts_with(p))
        .max_by_key(|p| p.components().count())
        .or_else(|| (!state.target.is_empty() && exe.starts_with(&target)).then_some(target))
        .or_else(|| exe.parent().map(PathBuf::from))
}

/// Everything about the installed game: what its Steam shortcut runs, where
/// the game and its prefix are, its Proton, and whether it can move into the
/// prefix's Program Files.
async fn info(hash: &str) -> anyhow::Result<Value> {
    let e = entry(hash)?;
    let state = e.install.clone().ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    let compat = proton::compatdata(appid);
    let pfx = compat.join("pfx");
    let api = steamclient::available().await;
    let shortcut = if api { steamclient::shortcut(appid).await.ok().flatten() } else { None };
    let dir = game_dir(&state);
    let size = match dir.clone() {
        Some(d) => tokio::task::spawn_blocking(move || relocate::dir_size(&d)).await.unwrap_or(0),
        None => 0,
    };
    let drive_c = pfx.join("drive_c");
    let in_prefix = dir.as_ref().is_some_and(|d| canon(d).starts_with(canon(&drive_c)));
    let prefix_ok = pfx.join("system.reg").is_file();
    let running = steamclient::running(appid);
    let busy = running || proton::in_use(appid) || winetricks::active(appid) || moving(hash);
    let free = relocate::free_space(&compat);
    let content = content(hash).await.unwrap_or_default();
    let is_download = dir.as_ref().is_some_and(|d| content.iter().any(|c| canon(d).starts_with(canon(c))));
    let targets = match &dir {
        Some(d) => targets(d, size, &pfx, prefix_ok, is_download, in_prefix),
        None => Vec::new(),
    };
    let tool = shortcut.as_ref().map(|s| s.tool.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| state.tool.clone());
    let tools = if api { tools().await.unwrap_or_default() } else { Vec::new() };
    Ok(json!({
        "appid": appid,
        "steam_api": api,
        "shortcut": shortcut,
        "tool": tool,
        "tools": tools,
        "exe": state.exe.as_ref().map(|x| json!({ "path": x, "windows": proton::to_windows(&pfx, Path::new(x)) })),
        "game_dir": dir.as_ref().map(|d| json!({ "path": d, "windows": proton::to_windows(&pfx, d), "size": size, "disk": disk_label(d) })),
        "prefix": { "path": pfx, "exists": prefix_ok, "disk": disk_label(&compat), "free": free },
        "in_prefix": in_prefix,
        "targets": targets,
        "moving": move_job(hash),
        "borrowed": state.borrowed,
        "running": running,
        "busy": busy,
    }))
}

/// Where the game can move: into its prefix's Program Files, or into any
/// Steam library (`<library>/piShop/<its folder>`), each with what stands in
/// its way.
fn targets(dir: &Path, size: u64, pfx: &Path, prefix_ok: bool, is_download: bool, in_prefix: bool) -> Vec<Value> {
    let name = dir.file_name().unwrap_or_default();
    let here = canon(dir);
    let mut out = Vec::new();
    let mut add = |kind: &str, id: String, label: String, to: PathBuf, windows: String, is_here: bool| {
        let same_disk = relocate::same_disk(dir, &to);
        let free = relocate::free_space(&to);
        let blocked = if is_here {
            None
        } else if is_download {
            Some(tr!(
                "These are the download's own files: copy them with Explore instead.",
                "Estes são os próprios arquivos do download: copie-os com o Explorar."
            ))
        } else if kind == "prefix" && !prefix_ok {
            Some(tr!("The prefix doesn't exist yet: play the game once first.", "O prefixo ainda não existe: jogue uma vez primeiro."))
        } else if to.exists() {
            Some(tr!("A folder with this name is already there.", "Já existe uma pasta com esse nome lá."))
        } else if !same_disk && free.is_some_and(|f| f < size + (512 << 20)) {
            Some(tr!("Not enough free space there.", "Não há espaço livre suficiente lá."))
        } else {
            None
        };
        out.push(json!({
            "id": id, "kind": kind, "label": label, "to": to, "windows": windows,
            "here": is_here, "same_disk": same_disk, "free": free, "blocked": blocked,
        }));
    };
    let into_prefix = pfx.join("drive_c/Program Files").join(name);
    add(
        "prefix",
        "prefix".into(),
        tr!("Inside its prefix", "Dentro do prefixo"),
        into_prefix,
        format!("C:\\Program Files\\{}", name.to_string_lossy()),
        in_prefix,
    );
    for lib in libraries() {
        let root = PathBuf::from(&lib.path);
        let to = root.join(crate::APP_NAME).join(name);
        let windows = proton::to_windows(pfx, &to);
        let is_here = !in_prefix && here.starts_with(canon(&root));
        add("library", lib.path, lib.label, to, windows, is_here);
    }
    out
}

#[derive(Deserialize)]
struct ToolReq {
    tool: String,
}

async fn tool_route(UrlPath(hash): UrlPath<String>, Json(r): Json<ToolReq>) -> Response {
    reply(set_tool(&hash, &r.tool).await)
}

/// Switches the shortcut's compatibility tool (the prefix stays; Proton
/// upgrades it on the next start).
async fn set_tool(hash: &str, tool: &str) -> anyhow::Result<InstallState> {
    let mut state = entry(hash)?.install.ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    if steamclient::running(appid) || winetricks::active(appid) {
        bail!(tr!("close the game first", "feche o jogo primeiro"));
    }
    steamclient::set_compat_tool(appid, tool).await?;
    state.tool = tool.to_string();
    library::set_install(hash, Some(state.clone()));
    log!("instalar: atalho {appid} agora usa {tool}");
    Ok(state)
}

#[derive(Serialize, Clone)]
struct MoveJob {
    /// running | done | failed | canceled
    state: &'static str,
    done: u64,
    total: u64,
    to: String,
    error: Option<String>,
}

static MOVES: LazyLock<Mutex<HashMap<String, (MoveJob, Arc<relocate::Progress>)>>> = LazyLock::new(Default::default);

fn move_job(hash: &str) -> Option<MoveJob> {
    let moves = MOVES.lock().unwrap();
    let (job, p) = moves.get(hash)?;
    let mut job = job.clone();
    if job.state == "running" {
        job.done = p.done.load(Ordering::Relaxed);
    }
    Some(job)
}

pub fn moving(hash: &str) -> bool {
    move_job(hash).is_some_and(|j| j.state == "running")
}

#[derive(Deserialize)]
struct MoveReq {
    /// "prefix" or a Steam library's path.
    #[serde(default = "into_prefix")]
    to: String,
}

fn into_prefix() -> String {
    "prefix".into()
}

async fn move_route(UrlPath(hash): UrlPath<String>, body: Option<Json<MoveReq>>) -> Response {
    let to = body.map(|Json(r)| r.to).unwrap_or_else(into_prefix);
    reply(move_start(&hash, &to).await)
}

/// Moves the game's files into its prefix's Program Files or into another
/// Steam library, then points the registry and the shortcut (target and
/// start folder) at the new place — every move, without exception.
async fn move_start(hash: &str, to: &str) -> anyhow::Result<Value> {
    if moving(hash) {
        bail!(tr!("already moving", "já está movendo"));
    }
    let info = info(hash).await?;
    let target = info["targets"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["id"] == to))
        .cloned()
        .ok_or_else(|| anyhow!(tr!("unknown destination", "destino desconhecido")))?;
    if target["here"] == true {
        bail!(tr!("the game is already there", "o jogo já está lá"));
    }
    if let Some(b) = target["blocked"].as_str() {
        bail!("{b}");
    }
    if info["busy"] == true {
        bail!(tr!("close the game first", "feche o jogo primeiro"));
    }
    let (Some(from), Some(to)) = (info["game_dir"]["path"].as_str(), target["to"].as_str()) else {
        bail!(tr!("nothing to move", "nada para mover"));
    };
    let (from, to) = (canon(Path::new(from)), PathBuf::from(to));
    let appid = info["appid"].as_u64().unwrap_or_default() as u32;
    let pfx = proton::compatdata(appid).join("pfx");
    // Every way the registry may spell the old folder, taken before it goes.
    let from_win = proton::windows_paths(&pfx, &from);
    let to_win = target["windows"].as_str().unwrap_or_default().to_string();
    let start_dir = info["shortcut"]["start_dir"].as_str().map(|s| s.trim_matches('"').to_string());
    let total = info["game_dir"]["size"].as_u64().unwrap_or(0);
    let progress = Arc::new(relocate::Progress::default());
    progress.total.store(total, Ordering::Relaxed);
    let job = MoveJob { state: "running", done: 0, total, to: to.display().to_string(), error: None };
    MOVES.lock().unwrap().insert(hash.to_string(), (job, progress.clone()));
    log!("mover: {} → {}", from.display(), to.display());

    let hash = hash.to_string();
    tokio::spawn(async move {
        let (f, t, p) = (from.clone(), to.clone(), progress.clone());
        let moved = tokio::task::spawn_blocking(move || relocate::move_dir(&f, &t, &p)).await.map_err(|e| anyhow!("{e}")).and_then(|r| r);
        let result = match moved {
            Ok(()) => after_move(&hash, &pfx, &from, &to, &from_win, &to_win, start_dir).await,
            Err(e) => Err(e),
        };
        if let Err(e) = &result {
            log!("mover: {e:#}");
        }
        let mut moves = MOVES.lock().unwrap();
        if let Some((job, p)) = moves.get_mut(&hash) {
            job.done = p.done.load(Ordering::Relaxed);
            match result {
                Ok(()) => job.state = "done",
                Err(_) if p.cancel.load(Ordering::Relaxed) => job.state = "canceled",
                Err(e) => {
                    job.state = "failed";
                    job.error = Some(format!("{e:#}"));
                }
            }
        }
    });
    Ok(json!({ "started": true }))
}

/// The files are in the prefix now: the registry, the shortcut's target and
/// start folder (quoted, as Steam keeps them) and piShop's state follow.
async fn after_move(
    hash: &str,
    pfx: &Path,
    from: &Path,
    to: &Path,
    from_win: &[String],
    to_win: &str,
    start_dir: Option<String>,
) -> anyhow::Result<()> {
    let mut state = entry(hash)?.install.ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    let relocated = |p: &Path| {
        let rest = canon_old(p).strip_prefix(from).ok()?.to_path_buf();
        // join("") would leave a trailing slash on the folder itself.
        Some(if rest.as_os_str().is_empty() { to.to_path_buf() } else { to.join(rest) })
    };
    match proton::relocate_registry(pfx, from_win, to_win) {
        Ok(n) => log!("mover: {n} caminho(s) atualizados no registro"),
        Err(e) => log!("mover: registro não atualizado: {e:#}"),
    }
    let exe = state.exe.as_ref().map(PathBuf::from).and_then(|x| relocated(&x)).ok_or_else(|| anyhow!(tr!("executable not found", "executável não encontrado")))?;
    let start = start_dir.map(PathBuf::from).and_then(|d| relocated(&d)).unwrap_or_else(|| exe.parent().unwrap_or(to).to_path_buf());
    steamclient::set_exe(appid, &quoted(&exe)).await?;
    steamclient::set_start_dir(appid, &quoted(&start)).await?;
    // piShop's own install folder, if the installer left it empty.
    if !state.target.is_empty() {
        let _ = std::fs::remove_dir(&state.target);
    }
    state.exe = Some(exe.display().to_string());
    state.game_dir = Some(to.display().to_string());
    library::set_install(hash, Some(state));
    log!("mover: atalho {appid} → {}", exe.display());
    Ok(())
}

/// Paths saved before the move can't be canonicalized anymore (they're gone);
/// canonicalize what still exists of them.
fn canon_old(p: &Path) -> PathBuf {
    for a in p.ancestors() {
        if let Ok(c) = a.canonicalize() {
            return p.strip_prefix(a).map(|rest| c.join(rest)).unwrap_or_else(|_| p.to_path_buf());
        }
    }
    p.to_path_buf()
}

#[derive(Deserialize)]
struct RunReq {
    path: String,
}

async fn run_route(UrlPath(hash): UrlPath<String>, Json(r): Json<RunReq>) -> Response {
    reply(run_in_prefix(&hash, &r.path).await)
}

/// Runs an installer from the download — the redistributables a repack ships,
/// a patch — in the game's prefix. It goes through the game's own Steam
/// shortcut (same Proton, same prefix, and its windows show), which points
/// back at the game once the installer closes.
async fn run_in_prefix(hash: &str, path: &str) -> anyhow::Result<Value> {
    let mut state = entry(hash)?.install.ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    if steamclient::running(appid) || winetricks::active(appid) || moving(hash) || state.restore.is_some() {
        bail!(tr!("close the game first", "feche o jogo primeiro"));
    }
    let file = PathBuf::from(path);
    if !file.is_file() {
        bail!(tr!("file not found", "arquivo não encontrado"));
    }
    let system32 = proton::compatdata(appid).join("pfx/drive_c/windows/system32");
    let win = format!("Z:{}", file.display().to_string().replace('/', "\\"));
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    // Packages and scripts run through the prefix's own msiexec / cmd.
    let (exe, args) = match ext.as_str() {
        "msi" => (system32.join("msiexec.exe"), format!("/i \"{win}\"")),
        "bat" | "cmd" => (system32.join("cmd.exe"), format!("/c \"{win}\"")),
        _ => (file.clone(), String::new()),
    };
    if !exe.is_file() {
        bail!(tr!(
            "this game has no Proton prefix yet: play it once first",
            "este jogo ainda não tem prefixo do Proton: jogue uma vez primeiro"
        ));
    }
    let shortcut = steamclient::shortcut(appid).await?.ok_or_else(|| anyhow!(tr!("the Steam shortcut is gone", "o atalho da Steam sumiu")))?;
    // Saved first: if piShop isn't around when the installer closes, the next
    // start puts the shortcut back (`recover`).
    state.restore = Some(shortcut);
    state.borrowed = Some(file.display().to_string());
    library::set_install(hash, Some(state));
    let dir = file.parent().unwrap_or(Path::new("/"));
    steamclient::set_exe(appid, &quoted(&exe)).await?;
    steamclient::set_start_dir(appid, &quoted(dir)).await?;
    steamclient::set_launch_options(appid, &args).await?;
    steamclient::run(appid).await?;
    log!("instalar: {} no prefixo do atalho {appid}", file.display());
    let hash = hash.to_string();
    tokio::spawn(async move {
        if wait_until_closed(appid).await {
            restore_shortcut(&hash).await;
            bring_back().await;
        }
    });
    Ok(json!({ "running": true }))
}

/// Points the shortcut back at the game after an installer borrowed it.
async fn restore_shortcut(hash: &str) {
    let Some(mut state) = library::get(hash).and_then(|e| e.install) else { return };
    let (Some(appid), Some(b)) = (state.appid, state.restore.clone()) else { return };
    if steamclient::running(appid) {
        return;
    }
    let put_back = async {
        steamclient::set_exe(appid, &b.exe).await?;
        steamclient::set_start_dir(appid, &b.start_dir).await?;
        steamclient::set_launch_options(appid, &b.launch_options).await
    };
    match put_back.await {
        Ok(()) => {
            log!("instalar: atalho {appid} de volta ao jogo ({})", b.exe);
            state.restore = None;
            state.borrowed = None;
            library::set_install(hash, Some(state));
        }
        Err(e) => log!("instalar: não consegui restaurar o atalho {appid}: {e:#}"),
    }
}

/// At start: shortcuts left borrowed (piShop closed while an installer ran)
/// go back to their games once nothing runs in them. Steam's API may take a
/// moment to answer after Steam starts, so this retries for a while.
pub async fn recover() {
    for _ in 0..30 {
        let pending: Vec<String> = library::all()
            .into_iter()
            .filter(|(_, e)| e.install.as_ref().is_some_and(|s| s.restore.is_some()))
            .map(|(hash, _)| hash)
            .collect();
        if pending.is_empty() {
            return;
        }
        if steamclient::available().await {
            for hash in pending {
                restore_shortcut(&hash).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

fn move_cancel(hash: &str) -> anyhow::Result<Value> {
    if let Some((_, p)) = MOVES.lock().unwrap().get(hash) {
        p.cancel.store(true, Ordering::Relaxed);
    }
    Ok(json!({}))
}

/// Starts over: removes the shortcut the wizard created and its artwork
/// (Steam keeps grid images of removed shortcuts). Game files stay.
async fn reset(hash: &str) -> anyhow::Result<Value> {
    if let Some(appid) = entry(hash)?.install.and_then(|s| s.appid) {
        steamclient::remove_shortcut(appid).await?;
        remove_artwork(appid);
    }
    library::set_install(hash, None);
    Ok(json!({}))
}

fn remove_artwork(appid: u32) {
    let prefix = appid.to_string();
    for user in std::fs::read_dir(steam_root().join("userdata")).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(user.path().join("config/grid")).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            // 123.jpg, 123p.png, 123_hero.jpg, 123_logo.png, 123.json (logo position)
            let rest = name.strip_prefix(&prefix).unwrap_or("-");
            if rest.starts_with(['.', 'p', '_']) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    for ext in ["png", "jpg"] {
        let _ = std::fs::remove_file(crate::data_dir().join("icons").join(format!("{appid}.{ext}")));
    }
}

/// Turns on Steam's client API: writes Steam's debugging flag and restarts
/// Steam from outside its process tree (piShop closes with it; SteamOS brings
/// Steam right back in Game Mode).
async fn enable_client_api() -> anyhow::Result<Value> {
    steamclient::enable_debugging().context(tr!("couldn't write Steam's settings", "não foi possível gravar a configuração da Steam"))?;
    let restart = std::process::Command::new("systemd-run")
        .args(["--user", "--collect", "--unit", "pishop-steam-restart", "--on-active=3", "systemctl", "--user", "restart", "steam-launcher.service"])
        .status();
    match restart {
        Ok(s) if s.success() => Ok(json!({ "restarting": true })),
        _ => Ok(json!({ "restarting": false })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe_on_windows_drives() {
        assert_eq!(folder_name("Marvel’s Spider-Man: Remastered"), "Marvel’s Spider-Man Remastered");
        assert_eq!(folder_name("What? / Why*"), "What  Why");
        assert_eq!(folder_name("Trailing dots..."), "Trailing dots");
        assert_eq!(folder_name("  :?*  "), "Game");
    }

    #[test]
    fn default_tool_prefers_newest_installed_proton() {
        let t = |name: &str, display: &str, installed: bool| Tool { name: name.into(), display: display.into(), installed };
        let tools = vec![
            t("proton_experimental", "Proton Experimental", true),
            t("proton_9", "Proton 9.0-4", true),
            t("proton_11", "Proton 11.0-2", true),
            t("proton_12", "Proton 12.0-1", false),
            t("GE-Proton11-1", "GE-Proton11-1", true),
        ];
        assert_eq!(default_tool(&tools).as_deref(), Some("proton_11"));
        let only_exp = vec![t("proton_experimental", "Proton Experimental", true), t("proton_9", "Proton 9.0-4", false)];
        assert_eq!(default_tool(&only_exp).as_deref(), Some("proton_experimental"));
    }

    #[test]
    fn tool_folder_names_match_display_names() {
        assert_eq!(norm("Proton - Experimental"), norm("Proton Experimental"));
        assert_eq!(norm("Proton 9.0 (Beta)"), norm("Proton 9.0"));
        assert_eq!(norm("GE-Proton11-1"), norm("GE-Proton11-1"));
    }
}
