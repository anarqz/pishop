//! Games: every non-Steam shortcut in Steam's library, in two groups — the
//! games installed through piShop (their download's library entry holds
//! their state) and the other shortcuts, which get an entry of their own the
//! first time they're opened here (`library::shortcut_key`). Either way the
//! game is then managed through the install routes by that key: Proton,
//! components, installers in the prefix, moving, artwork, removing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{anyhow, bail};
use axum::extract::{Path as UrlPath, Query};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::install::{self, Stage, reply};
use crate::steamclient::{self, ShortcutInfo};
use crate::{library, localfs, log, proton, relocate, steam, tr, winetricks};

pub fn router() -> Router {
    Router::new()
        .route("/api/games", get(|| async { reply(list().await) }))
        .route("/api/games/{appid}/manage", post(|UrlPath(a): UrlPath<u32>| async move { reply(manage(a).await) }))
        .route("/api/games/{appid}/art/{kind}", get(art_route))
        .route("/api/artwork/search", get(search_route))
        .route("/api/install/{key}/artwork/apply", post(apply_route))
        .route("/api/install/{key}/folder", post(folder_route))
        .route("/api/install/{key}/remove", post(remove_route))
        .route("/api/install/{key}/readd", post(|UrlPath(k): UrlPath<String>| async move { reply(readd(&k).await) }))
        .route("/api/install/{key}/forget", post(|UrlPath(k): UrlPath<String>| async move { reply(forget(&k).await) }))
}

// ---------- the list ----------

#[derive(Serialize, Default)]
struct ArtUrls {
    cover: Option<String>,
    wide: Option<String>,
    hero: Option<String>,
    logo: Option<String>,
}

#[derive(Serialize)]
struct GameItem {
    appid: u32,
    /// The library key the install routes take; other shortcuts have none
    /// until they're opened (`manage`).
    key: Option<String>,
    name: String,
    /// "pishop" (installed through piShop) or "shortcut".
    origin: &'static str,
    /// piShop's install stage (installing / installed).
    stage: Option<Stage>,
    exe: String,
    tool: String,
    /// A Windows game (or one set to run with Proton).
    windows: bool,
    /// Its Proton prefix exists.
    prefix: bool,
    running: bool,
    last_played: i64,
    /// Installed through piShop, but the shortcut is gone from Steam.
    missing: bool,
    /// The artwork Steam shows for it (files in Steam's grid folder).
    art: ArtUrls,
    /// piShop's game data (Store downloads).
    game: Option<library::Game>,
}

/// piShop's own shortcut never shows up as a game.
fn is_pishop(s: &ShortcutInfo) -> bool {
    let me = std::env::var("SteamAppId").ok().and_then(|v| v.parse::<u32>().ok());
    Some(s.appid) == me || (s.name == crate::APP_NAME && unquote(&s.exe).ends_with("/pishop"))
}

/// Steam keeps shortcut paths quoted.
pub(crate) fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn is_windows(exe: &str) -> bool {
    let low = exe.to_lowercase();
    [".exe", ".bat", ".cmd", ".msi", ".lnk"].iter().any(|x| low.ends_with(x))
}

/// The shortcuts as Steam has them now, or as `shortcuts.vdf` has them when
/// Steam's client API is off (then without their Proton). The bool: live.
async fn shortcuts() -> (Vec<ShortcutInfo>, bool) {
    if steamclient::available().await
        && let Ok(list) = steamclient::shortcuts().await
    {
        return (list.into_iter().filter(|s| !is_pishop(s)).collect(), true);
    }
    let list = steam::read_shortcuts()
        .into_iter()
        .map(|s| ShortcutInfo {
            appid: s.appid,
            name: s.name,
            exe: s.exe,
            start_dir: s.start_dir,
            launch_options: s.launch_options,
            tool: String::new(),
            last_played: 0,
        })
        .filter(|s| !is_pishop(s))
        .collect();
    (list, false)
}

async fn list() -> anyhow::Result<Value> {
    let (shortcuts, live) = shortcuts().await;
    let lib = library::all();
    // appid → its entry; a download's entry wins over a shortcut's own one.
    let mut by_appid: HashMap<u32, (String, library::Entry)> = HashMap::new();
    for (k, e) in &lib {
        let Some(a) = e.install.as_ref().and_then(|s| s.appid) else { continue };
        if !library::is_shortcut_key(k) || !by_appid.contains_key(&a) {
            by_appid.insert(a, (k.clone(), e.clone()));
        }
    }
    let running = steamclient::running_appids();
    let grid = steam::grid_dirs();
    let mut games: Vec<GameItem> = Vec::new();
    for s in &shortcuts {
        let found = by_appid.get(&s.appid);
        let pishop = found.is_some_and(|(k, _)| !library::is_shortcut_key(k));
        let exe = unquote(&s.exe);
        games.push(GameItem {
            appid: s.appid,
            key: found.map(|(k, _)| k.clone()),
            name: s.name.clone(),
            origin: if pishop { "pishop" } else { "shortcut" },
            stage: found.filter(|_| pishop).and_then(|(_, e)| e.install.as_ref().map(|i| i.stage)),
            windows: is_windows(&exe) || !s.tool.is_empty(),
            exe,
            tool: s.tool.clone(),
            prefix: proton::compatdata(s.appid).join("pfx/system.reg").is_file(),
            running: running.contains(&s.appid),
            last_played: s.last_played,
            missing: false,
            art: art_urls(&grid, s.appid),
            game: found.filter(|_| pishop).map(|(_, e)| e.game.clone()),
        });
    }
    if live {
        let present: HashSet<u32> = shortcuts.iter().map(|s| s.appid).collect();
        for (appid, (k, e)) in &by_appid {
            if present.contains(appid) {
                continue;
            }
            if library::is_shortcut_key(k) {
                // A shortcut deleted in Steam: its own entry goes too.
                library::remove(k);
                continue;
            }
            let install = e.install.as_ref();
            games.push(GameItem {
                appid: *appid,
                key: Some(k.clone()),
                name: e.game.name.clone(),
                origin: "pishop",
                stage: install.map(|i| i.stage),
                exe: install.and_then(|i| i.exe.clone()).unwrap_or_default(),
                tool: install.map(|i| i.tool.clone()).unwrap_or_default(),
                windows: true,
                prefix: false,
                running: false,
                last_played: 0,
                missing: true,
                art: ArtUrls::default(),
                game: Some(e.game.clone()),
            });
        }
    }
    // Newest piShop installs first; other shortcuts by name.
    let started = |g: &GameItem| g.key.as_ref().and_then(|k| lib.get(k)).and_then(|e| e.install.as_ref()).map(|i| i.started).unwrap_or(0);
    games.sort_by(|a, b| match (a.origin, b.origin) {
        ("pishop", "pishop") => started(b).cmp(&started(a)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        ("pishop", _) => std::cmp::Ordering::Less,
        (_, "pishop") => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    let tools: Vec<Value> = if live {
        steamclient::compat_tools().await.unwrap_or_default().into_iter().map(|t| json!({ "name": t.name, "display": t.display })).collect()
    } else {
        Vec::new()
    };
    Ok(json!({ "steam_api": live, "games": games, "tools": tools }))
}

// ---------- artwork on disk ----------

/// File names Steam gives each artwork slot of an appid in its grid folder.
fn grid_names(appid: u32, kind: &str) -> Vec<String> {
    let stem = match kind {
        "cover" => format!("{appid}p"),
        "wide" => format!("{appid}"),
        "hero" => format!("{appid}_hero"),
        "logo" => format!("{appid}_logo"),
        _ => return Vec::new(),
    };
    ["png", "jpg", "jpeg", "webp"].iter().map(|x| format!("{stem}.{x}")).collect()
}

/// The newest file Steam has for a slot, across accounts.
fn grid_file(grid: &[PathBuf], appid: u32, kind: &str) -> Option<(PathBuf, i64)> {
    let names = grid_names(appid, kind);
    grid.iter()
        .flat_map(|g| names.iter().map(move |n| g.join(n)))
        .filter_map(|p| {
            let t = std::fs::metadata(&p).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
            Some((p, t))
        })
        .max_by_key(|(_, t)| *t)
}

fn art_urls(grid: &[PathBuf], appid: u32) -> ArtUrls {
    let url = |kind: &str| grid_file(grid, appid, kind).map(|(_, t)| format!("/api/games/{appid}/art/{kind}?v={t}"));
    ArtUrls { cover: url("cover"), wide: url("wide"), hero: url("hero"), logo: url("logo") }
}

async fn art_route(UrlPath((appid, kind)): UrlPath<(u32, String)>) -> Response {
    let Some((path, _)) = grid_file(&steam::grid_dirs(), appid, &kind) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    match tokio::fs::read(&path).await {
        // The URL carries the file's time: a new picture gets a new URL.
        Ok(bytes) => ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "max-age=604800")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------- opening a game ----------

/// The key to manage a game by: its download's entry for piShop's installs;
/// for other shortcuts their own entry, created or brought up to date with
/// what Steam has now (executable, Proton) and a game folder found safe.
async fn manage(appid: u32) -> anyhow::Result<Value> {
    let lib = library::all();
    if let Some((k, _)) = lib.iter().find(|(k, e)| !library::is_shortcut_key(k) && e.install.as_ref().and_then(|s| s.appid) == Some(appid)) {
        return Ok(json!({ "key": k }));
    }
    let (all, live) = shortcuts().await;
    if !live {
        bail!(tr!("Steam's client API isn't reachable", "a API do cliente Steam não está acessível"));
    }
    let s = all.iter().find(|s| s.appid == appid).ok_or_else(|| anyhow!(tr!("shortcut not found", "atalho não encontrado")))?;
    let key = library::shortcut_key(appid);
    let exe = unquote(&s.exe);
    let existing = library::get(&key);
    let mut state = existing.as_ref().and_then(|e| e.install.clone()).unwrap_or_default();
    state.stage = Stage::Installed;
    state.appid = Some(appid);
    state.external = true;
    // While an installer borrows the shortcut, Steam shows the installer.
    if state.restore.is_none() {
        state.exe = Some(exe.clone());
        state.tool = s.tool.clone();
    }
    let others = exes_except(&all, appid);
    let exe_path = PathBuf::from(state.exe.clone().unwrap_or_default());
    let keep = state
        .game_dir
        .as_ref()
        .map(PathBuf::from)
        .is_some_and(|d| d.is_dir() && canon(&exe_path).starts_with(canon(&d)) && safe_game_dir(&d, &others).is_ok());
    if !keep {
        state.game_dir = guess_game_dir(&exe_path, appid, &others).map(|d| d.display().to_string());
    }
    let mut game = existing.as_ref().map(|e| e.game.clone()).unwrap_or_default();
    game.name = s.name.clone();
    library::add(library::Entry {
        info_hash: key.clone(),
        game,
        release: String::new(),
        indexer: String::new(),
        size: 0,
        dest: None,
        added: existing.as_ref().map(|e| e.added).unwrap_or_else(now),
        resolved: true,
        install: Some(state),
    });
    Ok(json!({ "key": key }))
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn exes_except(all: &[ShortcutInfo], appid: u32) -> Vec<PathBuf> {
    all.iter().filter(|o| o.appid != appid).map(|o| PathBuf::from(unquote(&o.exe))).filter(|p| p.is_absolute()).collect()
}

/// Folders a game's binaries sit in, under the game's own folder.
const BIN_DIRS: &[&str] = &[
    "binaries", "bin", "bin64", "bin32", "x64", "x86", "x86_64", "win64", "win32", "x64_dx12", "retail", "_retail_", "shipping",
    "release", "pc", "system", "exe",
];

/// Folders whose children are one game each.
fn anchors(pfx: &Path) -> Vec<PathBuf> {
    let home = localfs::home();
    let mut v = Vec::new();
    for lib in proton::library_paths() {
        v.push(lib.join("steamapps/common"));
        v.push(lib.join(crate::APP_NAME));
    }
    let c = pfx.join("drive_c");
    for d in ["Program Files", "Program Files (x86)", "Games", "GOG Games", "users/steamuser/AppData/Local/Programs"] {
        v.push(c.join(d));
    }
    v.push(home.join("Games/Heroic"));
    v.push(home.join("Games/Lutris"));
    v.push(home.join("Games"));
    v.push(home.join("Downloads").join(crate::APP_NAME));
    v.push(home.join("Downloads"));
    for media in std::fs::read_dir("/run/media").into_iter().flatten().flatten() {
        v.push(media.path().join("Games"));
        for inner in std::fs::read_dir(media.path()).into_iter().flatten().flatten() {
            v.push(inner.path().join("Games"));
        }
    }
    v
}

/// Where another shortcut's game lives, when that's clear and safe to move or
/// delete: only for a Windows executable, the folder its installer registered,
/// else the one right under a games folder (steamapps/common, Program Files,
/// ~/Games…), else the executable's folder minus the usual binary subfolders
/// (Binaries/Win64…). Never a folder other shortcuts run from.
fn guess_game_dir(exe: &Path, appid: u32, others: &[PathBuf]) -> Option<PathBuf> {
    if !is_windows(&exe.to_string_lossy()) || !exe.is_file() {
        return None;
    }
    let pfx = proton::compatdata(appid).join("pfx");
    let registered: Vec<PathBuf> = proton::programs(&pfx).into_iter().map(|p| p.path).collect();
    let home = localfs::home();
    let protected = protected();
    guess_from(exe, &registered, &anchors(&pfx), |d| safe_in(d, others, &home, &protected).is_ok())
}

/// `guess_game_dir` without the machine around it (registered programs,
/// games folders and the safety check given).
fn guess_from(exe: &Path, registered: &[PathBuf], anchors: &[PathBuf], safe: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let exe = canon(exe);
    let registered = registered.iter().map(|p| canon(p)).filter(|p| exe.starts_with(p)).max_by_key(|p| p.components().count());
    let anchored = anchors.iter().find_map(|a| {
        let a = canon(a);
        let first = exe.strip_prefix(&a).ok()?.components().next()?;
        let d = a.join(first);
        (d != exe).then_some(d)
    });
    let walked = (|| {
        let mut d = exe.parent()?.to_path_buf();
        while d.file_name().is_some_and(|n| BIN_DIRS.contains(&n.to_string_lossy().to_lowercase().as_str())) {
            d = d.parent()?.to_path_buf();
        }
        // Unreal: <Game>/<Project>/Binaries/Win64 next to <Game>/Engine.
        if d.parent().is_some_and(|p| p.join("Engine").is_dir()) {
            d = d.parent()?.to_path_buf();
        }
        Some(d)
    })();
    [registered, anchored, walked].into_iter().flatten().find(|d| safe(d))
}

/// Folders a game folder must never be, nor hold.
fn protected() -> Vec<PathBuf> {
    let home = localfs::home();
    let mut v: Vec<PathBuf> = [
        "", "Downloads", "Desktop", "Documents", "Music", "Pictures", "Videos", "Applications", ".local", ".local/share", ".config",
        "Emulation", "Emulation/roms", "Games", ".steam",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect();
    v.push(proton::steam_root());
    v.push(home.join("Downloads").join(crate::APP_NAME));
    v.push(crate::base_dir());
    v.push(crate::data_dir());
    for lib in proton::library_paths() {
        for d in ["", "steamapps", "steamapps/common", "steamapps/compatdata", "steamapps/shadercache", crate::APP_NAME] {
            v.push(lib.join(d));
        }
    }
    for media in std::fs::read_dir("/run/media").into_iter().flatten().flatten() {
        v.push(media.path());
        for inner in std::fs::read_dir(media.path()).into_iter().flatten().flatten() {
            v.push(inner.path());
        }
    }
    v
}

/// Whether a folder can be treated as one game's own: inside home or a card,
/// not one of the shared folders above (nor holding one), not a prefix's
/// drive_c or Program Files, and no other shortcut runs from inside it.
fn safe_game_dir(d: &Path, others: &[PathBuf]) -> Result<(), String> {
    safe_in(d, others, &localfs::home(), &protected())
}

/// `safe_game_dir` with the home folder and the protected folders given.
fn safe_in(d: &Path, others: &[PathBuf], home: &Path, protected: &[PathBuf]) -> Result<(), String> {
    let d = canon(d);
    if !d.is_dir() {
        return Err(tr!("the folder doesn't exist", "a pasta não existe"));
    }
    let home = canon(home);
    if !(d.starts_with(&home) || d.starts_with("/run/media")) {
        return Err(tr!("only folders in your home or on a card or drive", "só pastas da sua pasta pessoal ou de um cartão ou disco"));
    }
    if protected.iter().any(|p| canon(p).starts_with(&d)) {
        return Err(tr!("this folder holds more than one game", "esta pasta guarda mais do que um jogo"));
    }
    let name = d.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let parent = d.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ["drive_c", "pfx", "program files", "program files (x86)", "windows", "users"].contains(&name.as_str()) || (name == "steamuser" && parent == "users") {
        return Err(tr!("this is part of a Proton prefix, not a game", "isto é parte de um prefixo do Proton, não um jogo"));
    }
    if others.iter().any(|o| canon(o).starts_with(&d)) {
        return Err(tr!("another shortcut runs from this folder", "outro atalho roda desta pasta"));
    }
    Ok(())
}

async fn other_exes(appid: u32) -> Vec<PathBuf> {
    exes_except(&shortcuts().await.0, appid)
}

// ---------- game folder ----------

#[derive(Deserialize)]
struct FolderReq {
    path: String,
}

async fn folder_route(UrlPath(key): UrlPath<String>, Json(r): Json<FolderReq>) -> Response {
    reply(set_folder(&key, &r.path).await)
}

/// Games → "Game folder": the folder that is the game (what moves and what
/// "Delete the game's files" deletes), set by hand.
async fn set_folder(key: &str, path: &str) -> anyhow::Result<Value> {
    let mut state = install::entry(key)?.install.ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    let dir = canon(Path::new(path));
    if let Some(exe) = state.exe.as_ref().map(|x| canon(Path::new(x)))
        && !exe.starts_with(&dir)
    {
        bail!(tr!("the game's executable isn't inside this folder", "o executável do jogo não está dentro desta pasta"));
    }
    safe_game_dir(&dir, &other_exes(appid).await).map_err(|m| anyhow!(m))?;
    state.game_dir = Some(dir.display().to_string());
    library::set_install(key, Some(state));
    log!("jogos: pasta do atalho {appid} → {}", dir.display());
    Ok(json!({ "game_dir": dir }))
}

// ---------- removing ----------

#[derive(Deserialize)]
struct RemoveReq {
    #[serde(default)]
    shortcut: bool,
    #[serde(default)]
    prefix: bool,
    #[serde(default)]
    files: bool,
}

async fn remove_route(UrlPath(key): UrlPath<String>, Json(r): Json<RemoveReq>) -> Response {
    reply(remove(&key, r).await)
}

/// Games → Remove: any of the game's files (its own folder), its Proton
/// prefix (saves and settings kept there go with it) and its Steam shortcut.
async fn remove(key: &str, r: RemoveReq) -> anyhow::Result<Value> {
    let e = install::entry(key)?;
    let state = e.install.clone().ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let appid = state.appid.ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    if !(r.shortcut || r.prefix || r.files) {
        bail!(tr!("pick what to remove", "escolha o que remover"));
    }
    if steamclient::running(appid) || proton::in_use(appid) || winetricks::active(appid) || install::moving(key) || state.restore.is_some() {
        bail!(tr!("close the game first", "feche o jogo primeiro"));
    }
    let mut removed: Vec<&str> = Vec::new();
    let mut freed = 0u64;
    let wipe = |p: PathBuf| async move {
        tokio::task::spawn_blocking(move || {
            let size = relocate::dir_size(&p);
            std::fs::remove_dir_all(&p).map(|_| size)
        })
        .await?
        .map_err(anyhow::Error::from)
    };
    if r.files {
        let dir = install::game_dir(&state).ok_or_else(|| anyhow!(tr!("piShop doesn't know this game's folder", "o piShop não sabe a pasta deste jogo")))?;
        safe_game_dir(&dir, &other_exes(appid).await).map_err(|m| anyhow!(m))?;
        freed += wipe(canon(&dir)).await?;
        removed.push("files");
        log!("jogos: arquivos de {:?} apagados ({})", e.game.name, dir.display());
    }
    if r.prefix {
        let compat = proton::compatdata(appid);
        let ours = compat.file_name().is_some_and(|n| n == appid.to_string().as_str()) && compat.parent().is_some_and(|p| p.ends_with("compatdata"));
        if ours && compat.is_dir() {
            freed += wipe(compat.clone()).await?;
            log!("jogos: prefixo de {:?} apagado ({})", e.game.name, compat.display());
        }
        removed.push("prefix");
    }
    if r.shortcut {
        install::reset(key).await?;
        removed.push("shortcut");
        log!("jogos: atalho {appid} ({:?}) removido da Steam", e.game.name);
    }
    Ok(json!({ "removed": removed, "freed": freed }))
}

// ---------- installs whose shortcut was deleted in Steam ----------

/// A new shortcut for a piShop install deleted in Steam: same executable,
/// Proton and artwork; its old prefix (where the game may live) follows.
async fn readd(key: &str) -> anyhow::Result<Value> {
    let e = install::entry(key)?;
    let mut state = e.install.clone().ok_or_else(|| anyhow!(tr!("not installed yet", "ainda não instalado")))?;
    let exe = state
        .exe
        .clone()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .ok_or_else(|| anyhow!(tr!("the game's executable is gone", "o executável do jogo sumiu")))?;
    let appid = install::create_shortcut(&e.game, &exe, "", &state.tool).await?;
    let mut exe_now = exe.clone();
    if let Some(old) = state.appid.filter(|old| *old != appid) {
        let from = proton::compatdata(old);
        let to = from.with_file_name(appid.to_string());
        if from.is_dir() && !to.exists() && std::fs::rename(&from, &to).is_ok() {
            log!("jogos: prefixo {old} agora é do atalho {appid}");
            // A game inside the prefix moved with it.
            let moved = |p: &Path| p.strip_prefix(&from).ok().map(|rest| to.join(rest));
            if let Some(x) = moved(&exe) {
                exe_now = x;
                steamclient::set_exe(appid, &install::quoted(&exe_now)).await?;
                steamclient::set_start_dir(appid, &install::quoted(exe_now.parent().unwrap_or(Path::new("/")))).await?;
            }
            state.game_dir = state.game_dir.as_ref().and_then(|d| moved(Path::new(d))).map(|d| d.display().to_string()).or(state.game_dir);
        }
    }
    state.appid = Some(appid);
    state.exe = Some(exe_now.display().to_string());
    library::set_install(key, Some(state));
    log!("jogos: {:?} de volta à Steam (atalho {appid})", e.game.name);
    Ok(json!({ "appid": appid }))
}

/// Takes a deleted shortcut's game off Games (files stay where they are).
async fn forget(key: &str) -> anyhow::Result<Value> {
    library::set_install(key, None);
    if library::is_shortcut_key(key) || install::content(key).await.is_err() {
        library::remove(key);
    }
    Ok(json!({}))
}

// ---------- artwork by hand ----------

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search_route(Query(q): Query<SearchQuery>) -> Response {
    let term = q.q.trim().to_string();
    if term.is_empty() {
        return reply::<Value>(Err(anyhow!(tr!("type a name", "digite um nome"))));
    }
    let (steam, sgdb) = tokio::join!(crate::steam_store::search(&term), crate::catalog::sgdb_games(&term));
    let sgdb_error = sgdb.as_ref().err().map(|e| format!("{e:#}"));
    Json(json!({ "steam": steam, "sgdb": sgdb.unwrap_or_default(), "sgdb_error": sgdb_error })).into_response()
}

#[derive(Deserialize)]
struct ApplyReq {
    /// "steam" (official art of a Steam app) or "sgdb" (a SteamGridDB game).
    source: String,
    id: String,
    #[serde(default)]
    name: String,
}

async fn apply_route(UrlPath(key): UrlPath<String>, Json(r): Json<ApplyReq>) -> Response {
    reply(apply(&key, r).await)
}

/// Games → Find artwork: the game the user picked becomes the shortcut's
/// artwork source from now on (refreshes use it too), and its whole set goes on.
async fn apply(key: &str, r: ApplyReq) -> anyhow::Result<Value> {
    let id = r.id.trim().to_string();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        bail!(tr!("pick a game", "escolha um jogo"));
    }
    let ok = library::update(key, |e| match r.source.as_str() {
        "steam" => {
            e.game.steam_appid = Some(id.clone());
            e.game.art_source = Some("steam".into());
        }
        _ => {
            e.game.sgdb_id = id.parse().ok();
            e.game.sgdb_name = Some(r.name.trim().to_string()).filter(|n| !n.is_empty());
            e.game.art_source = Some("sgdb".into());
        }
    });
    if !ok {
        bail!(tr!("this game has no data in piShop", "este jogo não tem dados no piShop"));
    }
    install::refresh_artwork(key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake home with games laid out the usual ways.
    fn home(tag: &str) -> PathBuf {
        let h = std::env::temp_dir().join(format!("pishop-games-{tag}-{}", std::process::id())).join("home");
        let _ = std::fs::remove_dir_all(&h);
        for d in [
            "Games/Hades",
            "Games/Ue/Engine/Binaries",
            "Games/Ue/Proj/Binaries/Win64",
            "Downloads",
            "Emulation/tools/launchers",
            "lib/steamapps/common/Celeste",
            "pfx/drive_c/Program Files/Foo/bin",
        ] {
            std::fs::create_dir_all(h.join(d)).unwrap();
        }
        for f in [
            "Games/Hades/Hades.exe",
            "Games/Ue/Proj/Binaries/Win64/Proj-Win64-Shipping.exe",
            "Emulation/tools/launchers/ryujinx.sh",
            "lib/steamapps/common/Celeste/Celeste.exe",
            "pfx/drive_c/Program Files/Foo/bin/foo.exe",
        ] {
            std::fs::write(h.join(f), b"MZ").unwrap();
        }
        h
    }

    fn protected_of(h: &Path) -> Vec<PathBuf> {
        ["", "Downloads", "Games", "Emulation", "lib", "lib/steamapps", "lib/steamapps/common"].iter().map(|d| h.join(d)).collect()
    }

    #[test]
    fn game_folders_are_found_and_kept_safe() {
        let h = home("guess");
        let prot = protected_of(&h);
        let anchors = vec![h.join("lib/steamapps/common"), h.join("pfx/drive_c/Program Files"), h.join("Games"), h.join("Downloads")];
        let safe = |d: &Path| safe_in(d, &[], &h, &prot).is_ok();
        let guess = |exe: &str| guess_from(&h.join(exe), &[], &anchors, safe).map(|d| d.strip_prefix(canon(&h)).unwrap().to_path_buf());
        assert_eq!(guess("Games/Hades/Hades.exe"), Some(PathBuf::from("Games/Hades")));
        assert_eq!(guess("lib/steamapps/common/Celeste/Celeste.exe"), Some(PathBuf::from("lib/steamapps/common/Celeste")));
        assert_eq!(guess("pfx/drive_c/Program Files/Foo/bin/foo.exe"), Some(PathBuf::from("pfx/drive_c/Program Files/Foo")));
        assert_eq!(guess("Games/Ue/Proj/Binaries/Win64/Proj-Win64-Shipping.exe"), Some(PathBuf::from("Games/Ue")));
    }

    #[test]
    fn shared_and_system_folders_are_never_a_game() {
        let h = home("safe");
        let prot = protected_of(&h);
        let launcher = h.join("Emulation/tools/launchers/ryujinx.sh");
        // Shared folders, the home itself, a prefix's insides.
        assert!(safe_in(&h, &[], &h, &prot).is_err());
        assert!(safe_in(&h.join("Games"), &[], &h, &prot).is_err());
        assert!(safe_in(&h.join("Emulation/tools/launchers"), &[], &h, &prot).is_err() || safe_in(&h.join("Emulation/tools/launchers"), &[launcher.clone()], &h, &prot).is_err());
        assert!(safe_in(&h.join("pfx/drive_c"), &[], &h, &prot).is_err());
        assert!(safe_in(&h.join("pfx/drive_c/Program Files"), &[], &h, &prot).is_err());
        // Another shortcut runs from inside: not this game's own folder.
        assert!(safe_in(&h.join("Games/Hades"), &[h.join("Games/Hades/Hades.exe")], &h, &prot).is_err());
        // Outside home and cards.
        assert!(safe_in(Path::new("/usr"), &[], &h, &prot).is_err());
        assert!(safe_in(&h.join("Games/Hades"), &[launcher], &h, &prot).is_ok());
    }
}
