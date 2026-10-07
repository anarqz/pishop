//! Finding a game's main executable and its installer, for the install wizard.
//!
//! The ranking was tuned on the 52 games installed on the ROG Ally (Steam's
//! own launch executables from appinfo.vdf versus every .exe in their
//! folders). What counts, in order: the file name matching the game (also as
//! initials: SOTTR, NMS, D2R, mgsvtpp…), sensible folders (root, bin/x64,
//! Binaries/Win64…) and size; redistributables, crash handlers, tools and
//! servers are dropped, launchers, trials and 32-bit twins pushed down.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::tr;

#[derive(Serialize, Clone, Debug)]
pub struct Candidate {
    /// Relative to the scanned root, with '/' separators.
    pub path: String,
    pub size: u64,
    pub score: f32,
}

/// Stop walking a folder tree after this many entries (a stray prefix or a
/// whole library passed by mistake shouldn't hang the request).
const MAX_ENTRIES: usize = 100_000;

const STOP_WORDS: &[&str] = &["the", "of", "a", "and"];

/// Never the game, wherever they sit (substrings of the lowercase file stem).
/// The specific crash/report names come first so a game called "Crash …"
/// still loses its crash reporter (see the guard in `excluded_name`).
const NAME_EXCLUDE: &[&str] = &[
    "crashreport", "crashhandler", "crash_handler", "crash_report", "crashpad", "crashsender", "crashdump", "crashuploader",
    "crash", "crs-", "errorreport", "blizzarderror", "bssndrpt",
    "vcredist", "vc_redist", "redist", "dxsetup", "dxwebsetup", "directx", "prereq", "oalinst", "dotnet", "netfx", "ndp4", "physx", "xnafx",
    "vulkanrt", "installer", "setup", "social-club", "rockstar-games-launcher", "activationui", "unrealcefsubprocess", "subprocess",
    "epicwebhelper", "webhelper", "qtwebengine", "cefprocess", "lightweightdebugger", "statecompiler", "d3dconfig", "beservice",
    "easyanticheat", "anticheat", "pbsvc", "dedicated", "bagedit", "datacleaner", "unins", "uninstall", "languageselect",
    "language_select", "languageselector", "addoninstaller", "overlayinjector", "quicksfv",
];

/// Never the game when one of the stem's words (see `tokens`).
const TOKEN_EXCLUDE: &[&str] = &[
    "server", "editor", "helper", "config", "configurator", "settings", "register", "activation", "benchmark", "update", "updater",
    "patcher", "uploader", "cleanup", "touchup", "uninstaller", "reporter",
];

/// Folders whose .exe files are never the game (whole path segments,
/// lowercase; any segment starting with "crash" too).
const DIR_EXCLUDE: &[&str] = &[
    "commonredist", "_commonredist", "redist", "_redist", "redistributables", "redistributable", "__installer", "installer",
    "installers", "support", "directx", "vcredist", "dxredist", "epiconlineservices", "battleye", "easyanticheat", "eac", "tools",
    "tool", "thirdparty", "engine", "scenariocreator", "bageditor", "prerequisites", "prerequisite", "prereqs", "__overlay", "dotnet",
    "physx", "uninstall",
];

/// Redistributables and store/runtime installers bundled with games: never
/// the game's own installer (substrings of the lowercase stem).
const REDIST: &[&str] = &[
    "vcredist", "vc_redist", "redist", "dxsetup", "dxwebsetup", "directx", "prereq", "dotnet", "netfx", "ndp4", "physx", "oalinst",
    "openal", "xnafx", "vulkanrt", "eaappinstaller", "epiconlineservices", "ubisoftconnect", "uplay", "anticheat", "easyanticheat",
    "social-club", "rockstar-games-launcher", "installermessage", "addoninstaller", "steamsetup", "battle.net", "galaxy", "msxml",
    "xinput", "windowsdesktop-runtime", "dotnet-runtime", "aspnetcore", "windowsappruntime", "webview", "punkbuster", "pbsvc",
];

/// Folders an installer for the game itself never sits in.
const INSTALLER_DIR_EXCLUDE: &[&str] = &[
    "commonredist", "_commonredist", "redist", "_redist", "redistributables", "redistributable", "__installer", "directx",
    "vcredist", "dxredist", "prerequisites", "prerequisite", "prereqs", "support", "dotnet", "physx", "tools", "thirdparty",
    "engine", "__overlay", "easyanticheat", "battleye",
];

const ROMAN: &[(&str, &str)] =
    &[("ii", "2"), ("iii", "3"), ("iv", "4"), ("v", "5"), ("vi", "6"), ("vii", "7"), ("viii", "8"), ("ix", "9"), ("x", "10")];

/// Lowercase ASCII letters and digits only ("Shadow of the Tomb Raider™" →
/// "shadowofthetombraider").
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn apostrophe(c: char) -> bool {
    matches!(c, '\'' | '’' | '`')
}

/// Plain words: anything that isn't an ASCII letter or digit separates, but
/// apostrophes join ("No Man's Sky" → no, mans, sky).
fn words(s: &str) -> Vec<String> {
    let joined: String = s.chars().filter(|c| !apostrophe(*c)).collect();
    joined.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_ascii_lowercase()).collect()
}

/// Words of an identifier-like name: separators, camelCase and letter/digit
/// boundaries split ("NewColossus_x64vk" → new, colossus, x, 64, vk;
/// "GTAVLauncher" → gtav, launcher).
fn tokens(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().filter(|c| !apostrophe(*c)).collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if let Some(&p) = i.checked_sub(1).and_then(|j| chars.get(j)) {
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());
            let boundary = p.is_ascii_alphanumeric()
                && ((p.is_ascii_lowercase() && c.is_ascii_uppercase())
                    || (p.is_ascii_uppercase() && c.is_ascii_uppercase() && next_lower)
                    || (p.is_ascii_digit() != c.is_ascii_digit()));
            if boundary && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        }
        cur.push(c.to_ascii_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn has_token(s: &str, t: &str) -> bool {
    tokens(s).iter().any(|w| w == t)
}

/// Initials of the name's words, plain and with roman numerals as digits
/// ("Diablo II Resurrected" → dir / d2r): for all the words, and for every
/// leading run of at least two ("… – Infernal Edition" suffixes).
fn acronyms(game: &str) -> (HashSet<String>, HashSet<String>) {
    let ws = words(game);
    let plain: Vec<String> = ws.iter().map(|w| w[..1].to_string()).collect();
    let digits: Vec<String> = ws
        .iter()
        .map(|w| {
            if w.chars().all(|c| c.is_ascii_digit()) {
                w.clone()
            } else {
                ROMAN.iter().find(|(r, _)| r == w).map(|(_, d)| d.to_string()).unwrap_or_else(|| w[..1].to_string())
            }
        })
        .collect();
    let (mut full, mut prefixes) = (HashSet::new(), HashSet::new());
    for k in 2..=ws.len() {
        for set in [&plain, &digits] {
            let a: String = set[..k].concat();
            if k == ws.len() {
                full.insert(a);
            } else {
                prefixes.insert(a);
            }
        }
    }
    (full, prefixes)
}

/// The stem without a 64-bit marker at the end ("Wreckfest_x64" → "Wreckfest"),
/// and whether it had one.
fn strip_arch(stem: &str) -> (&str, bool) {
    let low = stem.to_ascii_lowercase();
    for suffix in ["_x86_64", "-x86_64", "_x64", "-x64", "_win64", "-win64", "_64bit", "-64bit", "64bit", "_64", "-64", "x64", "win64"] {
        if low.len() > suffix.len() + 1 && low.ends_with(suffix) {
            return (&stem[..stem.len() - suffix.len()], true);
        }
    }
    (stem, false)
}

/// How much a file stem looks like the game's name, 0–1.
fn name_score(stem: &str, game: &str) -> f32 {
    let stem = strip_arch(stem).0;
    let s = norm(stem);
    let g = norm(game);
    if s.is_empty() || g.is_empty() {
        return 0.0;
    }
    if s == g {
        return 1.0;
    }
    if s.len() >= 4 && (g.contains(&s) || s.contains(&g)) {
        return 0.85;
    }
    let (full, prefixes) = acronyms(game);
    if s.len() >= 2 && full.contains(&s) {
        return 0.85;
    }
    if s.len() >= 2 && prefixes.contains(&s) {
        return 0.8;
    }
    let gw: HashSet<String> = tokens(game).into_iter().filter(|w| !STOP_WORDS.contains(&w.as_str())).collect();
    let sw: HashSet<String> = tokens(stem).into_iter().collect();
    if gw.is_empty() || sw.is_empty() {
        return 0.0;
    }
    let shared = gw.intersection(&sw).count();
    0.6 * 2.0 * shared as f32 / (gw.len() + sw.len()) as f32
}

/// Tools, redistributables, crash handlers… — unless the game itself is named
/// after the word ("Crash Bandicoot" keeps its main exe).
fn excluded_name(stem: &str, game_norm: &str) -> bool {
    let low = stem.to_ascii_lowercase();
    let applies = |k: &str| !game_norm.contains(&norm(k));
    if NAME_EXCLUDE.iter().any(|k| low.contains(k) && applies(k)) {
        return true;
    }
    let toks = tokens(stem);
    if TOKEN_EXCLUDE.iter().any(|k| toks.iter().any(|t| t == k) && applies(k)) {
        return true;
    }
    low == "cdb" || low.starts_with("7z") || low.ends_with("_be")
}

/// Folders where game binaries usually live (lowercase relative path).
fn good_dir(d: &str) -> bool {
    const EXACT: &[&str] =
        &["binaries", "bin", "x64", "x86", "bin64", "bin32", "win64", "win32", "engine", "retail", "game", "_retail_", "en_us/client/bin/pc"];
    const ARCH: &[&str] = &["win64", "win32", "x64", "x86", "x64_dx12", "x86_64", "pc", "release", "shipping"];
    if EXACT.contains(&d) {
        return true;
    }
    let segs: Vec<&str> = d.split('/').collect();
    match segs.as_slice() {
        [b, a] => (*b == "bin" || *b == "binaries") && ARCH.contains(a),
        // <Project>/Binaries/Win64 (Unreal), but never Engine/Binaries/…
        [p, b, a] => *p != "engine" && (*b == "bin" || *b == "binaries") && ARCH.contains(a),
        _ => false,
    }
}

fn excluded_dir(d: &str) -> bool {
    !d.is_empty() && d.split('/').any(|s| DIR_EXCLUDE.contains(&s) || s.starts_with("crash")) && !good_dir(d)
}

/// "dir/sub/Game.exe" → ("dir/sub", "Game"); the extension is assumed .exe.
fn split_rel(rel: &str) -> (&str, &str) {
    let (dir, file) = rel.rsplit_once('/').unwrap_or(("", rel));
    let stem = file.len().checked_sub(4).filter(|&n| file.is_char_boundary(n)).map(|n| &file[..n]).unwrap_or(file);
    (dir, stem)
}

fn depth_of(dir: &str) -> usize {
    if dir.is_empty() { 0 } else { dir.matches('/').count() + 1 }
}

fn is_exe(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// Every .exe under `root` (a folder, or a single .exe for one-file
/// downloads), down to `max_depth` folders, plus the Unity `X_Data` folders
/// seen on the way. Symlinks are never followed (Proton prefixes link to /).
pub fn scan(root: &Path, max_depth: usize) -> (Vec<(String, u64)>, HashSet<String>) {
    let mut exes = Vec::new();
    let mut data_dirs = HashSet::new();
    if root.is_file() {
        if let (true, Some(name), Ok(meta)) = (is_exe(root), root.file_name(), root.metadata()) {
            exes.push((name.to_string_lossy().into_owned(), meta.len()));
        }
        return (exes, data_dirs);
    }
    let mut stack = vec![(root.to_path_buf(), String::new(), 0usize)];
    let mut seen = 0usize;
    'walk: while let Some((dir, rel, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                break 'walk;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if kind.is_dir() {
                // Unity's data folder: noted, never descended (no .exe inside, many files).
                if name.to_ascii_lowercase().ends_with("_data") {
                    data_dirs.insert(child);
                } else if depth < max_depth {
                    stack.push((entry.path(), child, depth + 1));
                }
            } else if kind.is_file() && name.to_ascii_lowercase().ends_with(".exe") {
                exes.push((child, entry.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
    }
    exes.sort();
    (exes, data_dirs)
}

/// Candidates for the game's main executable, best first; tools,
/// redistributables and the like are left out. Pure: works on a listing.
pub fn rank(game: &str, exes: &[(String, u64)], data_dirs: &HashSet<String>) -> Vec<Candidate> {
    let game_norm = norm(game);
    let data_dirs: HashSet<String> = data_dirs.iter().map(|d| d.replace('\\', "/").to_ascii_lowercase()).collect();
    let rels: Vec<String> = exes.iter().map(|(r, _)| r.replace('\\', "/")).collect();
    let any_x64 = rels.iter().any(|r| r.to_ascii_lowercase().contains("x64"));
    let stem_norms: Vec<String> = rels.iter().map(|r| norm(split_rel(r).1)).collect();
    // Unreal: a small exe at the root starts <Project>/Binaries/Win64/*-Shipping.exe;
    // Steam launches the former.
    let root_bootstrapper = exes.iter().zip(&rels).any(|((_, size), rel)| {
        !rel.contains('/') && *size < 5_000_000 && !excluded_name(split_rel(rel).1, &game_norm)
    });

    let mut out = Vec::new();
    for (i, (rel, (_, size))) in rels.iter().zip(exes).enumerate() {
        let (dir, stem) = split_rel(rel);
        let dir_low = dir.to_ascii_lowercase();
        let low = stem.to_ascii_lowercase();
        if excluded_name(stem, &game_norm) || excluded_dir(&dir_low) {
            continue;
        }
        let mut score = name_score(stem, game) * 100.0;
        let depth = depth_of(dir) as f32;
        score -= if depth == 0.0 { 0.0 } else if good_dir(&dir_low) { 4.0 * depth } else { 15.0 * depth };
        score += ((*size as f64 / 1e6 + 1.0).log10() * 10.0) as f32;

        // Launchers: Steam often starts them, but the game's own exe is what runs.
        let nlow = norm(stem);
        let play_launcher = low.starts_with("play")
            && low.chars().nth(4).is_some_and(|c| c.is_ascii_alphabetic())
            && (stem_norms.iter().enumerate().any(|(j, n)| j != i && n.len() >= 3 && nlow[4..].contains(n.as_str()))
                || name_score(&stem[4..], game) >= 0.8);
        if low.contains("launcher") || play_launcher {
            score -= 30.0;
        }
        if low.contains("start_protected_game") {
            score -= 25.0;
        }
        for t in ["trial", "demo"] {
            if has_token(stem, t) && !has_token(game, t) {
                score -= 30.0;
            }
        }
        if ((low.contains("x86") && !low.contains("x86_64")) || low.contains("win32")) && any_x64 {
            score -= 8.0;
        }
        if has_token(stem, "vr") && !has_token(game, "vr") {
            score -= 10.0;
        }
        // The 64-bit build over its 32-bit twin.
        if strip_arch(stem).1 {
            score += 5.0;
        }
        if root_bootstrapper && ["-win64-shipping", "-win64-test"].iter().any(|s| low.ends_with(s)) {
            score -= 12.0;
        }
        // Unity: X.exe next to X_Data.
        let data = if dir_low.is_empty() { format!("{low}_data") } else { format!("{dir_low}/{low}_data") };
        if data_dirs.contains(&data) {
            score += 30.0;
        }
        out.push(Candidate { path: rel.clone(), size: *size, score: (score * 10.0).round() / 10.0 });
    }
    // Microsoft GDK builds: the small root exe only hands over to Gaming
    // Services (missing under Wine); the WinGDK binary is the game.
    if let Some(top) = out.iter().map(|c| c.score).reduce(f32::max) {
        for c in out.iter_mut().filter(|c| split_rel(&c.path).1.to_ascii_lowercase().ends_with("-wingdk-shipping")) {
            c.score = top + 1.0;
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.size.cmp(&a.size)).then(a.path.cmp(&b.path)));
    out
}

/// The game's main executable candidates under `root` (folder or single .exe).
pub fn guess(root: &Path, game: &str) -> Vec<Candidate> {
    let (exes, data_dirs) = scan(root, 6);
    rank(game, &exes, &data_dirs)
}

/// How a stem ranks as an installer by name: 0 setup, 1 install(er), 2 other.
fn setup_rank(low: &str) -> Option<u8> {
    match low {
        "setup" => Some(0),
        "install" | "installer" => Some(1),
        _ if low.starts_with("setup")
            || low.ends_with("setup")
            || low.ends_with("installer")
            || low.starts_with("install")
            || ["_setup", "-setup", " setup", ".setup"].iter().any(|s| low.contains(s)) =>
        {
            Some(2)
        }
        _ => None,
    }
}

fn redistributable(low: &str, dir_low: &str) -> bool {
    REDIST.iter().any(|k| low.contains(k))
        || low.starts_with("unins")
        || low.contains("uninstall")
        || dir_low.split('/').any(|s| INSTALLER_DIR_EXCLUDE.contains(&s) || s.starts_with("crash"))
}

/// The game's own installers under `root` (depth ≤ 2), best first: a root
/// "setup.exe" leads. Redistributables (vcredist, DirectX, UE prerequisites,
/// .NET, PhysX…) are left out. When nothing is named like an installer, the
/// root's few .exe files are looked inside (an NSIS "game-1.2-win64.exe").
/// Paths are relative to `root` (for a single .exe: its file name).
pub fn installers(root: &Path) -> Vec<String> {
    let (exes, _) = scan(root, 2);
    let mut found: Vec<(usize, u64, String)> = Vec::new();
    let mut unnamed: Vec<(u64, String)> = Vec::new();
    for (rel, size) in &exes {
        let (dir, stem) = split_rel(rel);
        let low = stem.to_ascii_lowercase();
        if redistributable(&low, &dir.to_ascii_lowercase()) {
            continue;
        }
        let depth = depth_of(dir);
        match setup_rank(&low) {
            Some(r) => found.push((depth * 4 + r as usize, *size, rel.clone())),
            None if depth == 0 => unnamed.push((*size, rel.clone())),
            None => {}
        }
    }
    if found.is_empty() && unnamed.len() <= 4 {
        for (size, rel) in unnamed {
            let path = if root.is_file() { root.to_path_buf() } else { root.join(&rel) };
            if installer_kind(&path).is_some() {
                found.push((3, size, rel));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    found.into_iter().map(|(_, _, rel)| rel).collect()
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    let Some((&first, rest)) = needle.split_first() else { return true };
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        match hay[i..=hay.len() - needle.len()].iter().position(|&b| b == first) {
            Some(p) => {
                let at = i + p;
                if &hay[at + 1..at + needle.len()] == rest {
                    return true;
                }
                i = at + 1;
            }
            None => return false,
        }
    }
    false
}

/// Which installer framework built `exe`, from markers in its first 8 MB:
/// "wix" (Burn bundles), "nsis", "inno", "installshield". None if unknown or
/// not a Windows executable.
pub fn installer_kind(exe: &Path) -> Option<&'static str> {
    let mut buf = Vec::new();
    File::open(exe).ok()?.take(8 << 20).read_to_end(&mut buf).ok()?;
    if !buf.starts_with(b"MZ") {
        return None;
    }
    let has = |ascii: &[&str], wide: &[&str]| {
        ascii.iter().any(|m| contains(&buf, m.as_bytes())) || wide.iter().any(|m| contains(&buf, &utf16le(m)))
    };
    if has(&[".wixburn"], &["WixBundle"]) {
        Some("wix")
    } else if has(&["NullsoftInst", "Nullsoft.NSIS"], &["Nullsoft Install System"]) {
        Some("nsis")
    } else if has(&["Inno Setup"], &["Inno Setup"]) {
        Some("inno")
    } else if has(&["InstallShield"], &["InstallShield"]) {
        Some("installshield")
    } else {
        None
    }
}

/// Command-line arguments that preselect the install folder, when the
/// framework supports it: Inno `/DIR="Z:\x\y"`, NSIS `/D=Z:\x\y` (NSIS wants
/// it last and unquoted, spaces included).
pub fn install_dir_args(kind: &str, windows_dir: &str) -> Option<String> {
    match kind {
        "inno" => Some(format!("/DIR=\"{windows_dir}\"")),
        "nsis" => Some(format!("/D={windows_dir}")),
        _ => None,
    }
}

/// A Linux path as Windows programs see it under Proton (drive Z: is /).
pub fn windows_path(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

pub fn router() -> Router {
    Router::new().route("/api/exe/guess", get(guess_route))
}

#[derive(Deserialize)]
struct GuessQuery {
    root: String,
    #[serde(default)]
    name: String,
}

fn err(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": msg.to_string() }))).into_response()
}

/// `GET /api/exe/guess?root=<abs path>&name=<game>` → `{ candidates: [top 12],
/// installers: [{ path, kind }] }`, paths relative to `root`.
async fn guess_route(Query(q): Query<GuessQuery>) -> Response {
    let root = PathBuf::from(q.root.trim());
    if !root.is_absolute() || root.parent().is_none() || !(root.is_dir() || (root.is_file() && is_exe(&root))) {
        return err(StatusCode::BAD_REQUEST, tr!("folder not found", "pasta não encontrada"));
    }
    let name = q.name;
    let work = tokio::task::spawn_blocking(move || {
        let mut candidates = guess(&root, &name);
        candidates.truncate(12);
        let installers: Vec<_> = installers(&root)
            .into_iter()
            .take(6)
            .map(|rel| {
                let path = if root.is_file() { root.clone() } else { root.join(&rel) };
                json!({ "path": rel, "kind": installer_kind(&path) })
            })
            .collect();
        json!({ "candidates": candidates, "installers": installers })
    });
    match work.await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Exes = &'static [(&'static str, u64)];

    /// Real folders from the Ally (name, expected main exe, every .exe).
    /// Where Steam starts a launcher, the game's own exe is the answer.
    const FIXTURE: &[(&str, &str, Exes)] = &[
        ("Dishonored", "Binaries/Win32/Dishonored.exe", &[("Binaries/Win32/Dishonored.exe", 18041856), ("Binaries/Redist/vcredist_x64.exe", 4961800), ("Binaries/Redist/vcredist_x86_2005sp1.exe", 2723264), ("Binaries/Redist/UE3Redist.exe", 24911744), ("Binaries/Redist/vcredist_x86_2008sp1.exe", 4216840), ("Binaries/Redist/vcredist_x86.exe", 2723264), ("Binaries/Redist/directx_full_redist/DXSETUP.exe", 517976)]),
        ("Wreckfest", "Wreckfest_x64.exe", &[("Wreckfest.exe", 19287040), ("Wreckfest_x64.exe", 23306752), ("DataCleaner.exe", 428032), ("BagEdit/BagEditCommunity.exe", 4834816), ("tools/bgeometry.exe", 8057856), ("tools/DeviceOut.exe", 25600), ("tools/bimage.exe", 7676928), ("tools/workshop/Uploader.exe", 19287040), ("crash/crashpad_handler.exe", 1618944), ("crash/crashpad_handler_64.exe", 2115072)]),
        ("Plague Inc: Evolved", "PlagueIncEvolved.exe", &[("PlagueIncEvolved.exe", 667136), ("UnityCrashHandler64.exe", 1669552), ("ScenarioCreator/PlagueIncSC.exe", 650752), ("ScenarioCreator/UnityCrashHandler64.exe", 1094600)]),
        ("Grand Theft Auto V Legacy", "GTA5.exe", &[("GTA5.exe", 47467128), ("GTAVLanguageSelect.exe", 572536), ("GTA5_BE.exe", 1473144), ("PlayGTAV.exe", 572536), ("GTAVLauncher.exe", 572536), ("BattlEye/BEService_x64.exe", 18663720), ("Redistributables/Social-Club-Setup.exe", 128406648), ("Redistributables/Rockstar-Games-Launcher.exe", 111419512)]),
        ("No Man's Sky", "Binaries/NMS.exe", &[("Binaries/NMS.exe", 88545352)]),
        ("The Witcher 3: Wild Hunt - Complete Edition", "bin/x64_dx12/witcher3.exe", &[("REDprelauncher.exe", 1360848), ("bin/x64_dx12/witcher3.exe", 90674640), ("bin/x64_dx12/crashreporter/CrashReporter.exe", 238544), ("bin/x64_dx12/crashreporter/7za.exe", 1166800), ("bin/x64_dx12/D3D12_0/d3dconfig.exe", 730976), ("bin/x64_dx12/D3D12_0/D3D12StateObjectCompiler.exe", 2481504)]),
        ("Don't Starve Together", "bin64/dontstarve_steam_x64.exe", &[("DXRedist/DXSETUP.exe", 537432), ("VCRedist/vcredist_x86.exe", 4216840), ("bin/dontstarve_dedicated_server_nullrenderer.exe", 5518336), ("bin/dontstarve_steam.exe", 5977088), ("bin64/dontstarve_steam_x64.exe", 7684096), ("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe", 7109632)]),
        ("Minecraft Dungeons II", "Dungeons/Binaries/WinGDK/Dungeons-WinGDK-Shipping.exe", &[("Dungeons.exe", 200704), ("gamelaunchhelper.exe", 100848), ("Dungeons/Binaries/WinGDK/Dungeons-WinGDK-Shipping.exe", 193196032), ("Engine/Binaries/Win64/CrashReportClient.exe", 27935232), ("unins000.exe", 1515889)]),
        ("ASTRONEER", "Astro.exe", &[("Astro.exe", 414208), ("Astro/Binaries/Win64/Astro-Win64-Shipping.exe", 111153224), ("Engine/Extras/Redist/en-us/UE4PrereqSetup_x64.exe", 41033784), ("Engine/Binaries/Win64/CrashReportClient.exe", 19499520)]),
        ("State of Decay 2", "StateOfDecay2/Binaries/Win64/StateOfDecay2-Win64-Shipping.exe|StateOfDecay2.exe", &[("StateOfDecay2.exe", 108544), ("StateOfDecay2/Binaries/Win64/StateOfDecay2-Win64-Shipping.exe", 76998672), ("Engine/Binaries/Win64/UnrealCEFSubProcess.exe", 8287744), ("_CommonRedist/DirectX/Jun2010/DXSETUP.exe", 517976), ("_CommonRedist/vcredist/2019/VC_redist.x64.exe", 14882584), ("_CommonRedist/vcredist/2019/VC_redist.x86.exe", 14328440)]),
        ("Risk of Rain 2", "Risk of Rain 2.exe", &[("Risk of Rain 2.exe", 653824), ("UnityCrashHandler64.exe", 1123864)]),
        ("Shadow of the Tomb Raider", "SOTTR.exe", &[("crashpad_handler.exe", 800256), ("SOTTR.exe", 39870720)]),
        ("Cyberpunk 2077", "bin/x64/Cyberpunk2077.exe", &[("REDprelauncher.exe", 1595984), ("bin/x64/REDEngineErrorReporter.exe", 262280), ("bin/x64/Cyberpunk2077.exe", 59945608), ("bin/x64/CrashReporter/CrashReporter.exe", 89736), ("bin/x64/CrashReporter/7za.exe", 1162376)]),
        ("Red Dead Redemption 2", "RDR2.exe", &[("PlayRDR2.exe", 507888), ("RDR2.exe", 89562608), ("Redistributables/Social-Club-Setup.exe", 128406648), ("Redistributables/VulkanRT-1.1.108.0-Installer.exe", 894272), ("Redistributables/Rockstar-Games-Launcher.exe", 111419512), ("x64/crashpad/release/crashpad_handler.exe", 743424)]),
        ("Need for Speed™ Heat", "NeedForSpeedHeat.exe", &[("NeedForSpeedHeatTrial.exe", 419854336), ("NeedForSpeedHeat.exe", 336545280), ("__overlay/overlayinjector.exe", 238376), ("__Installer/Cleanup.exe", 929576), ("__Installer/Touchup.exe", 929064), ("__Installer/vc/vc2017/redist/vc_redist.x64.exe", 15261400), ("__Installer/vc/vc2015/redist/vc_redist.x86.exe", 13767776), ("__Installer/vc/vc2015/redist/vc_redist.x64.exe", 14572000), ("__Installer/Origin/redist/internal/EAappInstaller.exe", 246792952), ("__Installer/directx/redist/DXSETUP.exe", 517976)]),
        ("Battlefield 4™", "bf4.exe", &[("BFLauncher_x86.exe", 181528), ("BF4X86WebHelper.exe", 624408), ("bf4_x86.exe", 31265048), ("bf4.exe", 40419608), ("BFLauncher.exe", 183576), ("BF4WebHelper.exe", 624408), ("Core/ActivationUI.exe", 2022408), ("__overlay/overlayinjector.exe", 238376), ("__Installer/Cleanup.exe", 935904), ("__Installer/Touchup.exe", 937952), ("__Installer/customcomponent/webplugin/battlelog-web-plugins.exe", 3819328), ("__Installer/Origin/redist/internal/EAappInstaller.exe", 241196464), ("__Installer/vc/vc2012Update3/redist/vcredist_x64.exe", 7185000), ("__Installer/vc/vc2012Update3/redist/vcredist_x86.exe", 6552288), ("__Installer/punkbuster/redist/pbsvc.exe", 3894632), ("__Installer/directx/redist/DXSETUP.exe", 517976), ("__Installer/DLC/Xpack4/__Installer/Cleanup.exe", 852120), ("__Installer/DLC/Xpack4/__Installer/Touchup.exe", 854168)]),
        ("Days Gone", "BendGame/Binaries/Win64/DaysGone.exe", &[("Engine/Binaries/ThirdParty/CRS/crs-handler.exe", 1192104), ("Engine/Binaries/ThirdParty/CRS/crs-video.exe", 997544), ("Engine/Binaries/ThirdParty/CRS/crs-uploader.exe", 830632), ("BendGame/Binaries/Win64/DaysGone.exe", 81800264)]),
        ("Palworld", "Palworld.exe", &[("Palworld.exe", 182784), ("Pal/Binaries/Win64/Palworld-Win64-Shipping.exe", 161802312), ("Engine/Binaries/Win64/CrashReportClient.exe", 22904832), ("Engine/Binaries/Win64/EpicWebHelper.exe", 4088320)]),
        ("HITMAN World of Assassination", "Retail/HITMAN3.exe", &[("Launcher.exe", 930696), ("Retail/HITMAN3.exe", 41743240)]),
        ("METAL GEAR SOLID 3: Snake Eater - Master Collection Version", "METAL GEAR SOLID3.exe", &[("METAL GEAR SOLID3.exe", 12948040), ("launcher.exe", 653824)]),
        ("Diablo II: Resurrected – Infernal Edition", "D2R.exe", &[("D2R.exe", 32107216), ("BlizzardError.exe", 898696)]),
    ];

    fn listing(exes: Exes) -> Vec<(String, u64)> {
        exes.iter().map(|(p, s)| (p.to_string(), *s)).collect()
    }

    fn data(dirs: &[&str]) -> HashSet<String> {
        dirs.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    fn picks_the_main_exe_on_real_games() {
        let unity: &[(&str, &[&str])] =
            &[("Plague Inc: Evolved", &["PlagueIncEvolved_Data", "ScenarioCreator/PlagueIncSC_Data"]), ("Risk of Rain 2", &["Risk of Rain 2_Data"])];
        let mut misses = Vec::new();
        for (game, expected, exes) in FIXTURE {
            let dirs = unity.iter().find(|(g, _)| g == game).map(|(_, d)| data(d)).unwrap_or_default();
            let ranked = rank(game, &listing(exes), &dirs);
            let top = ranked.first().map(|c| c.path.clone()).unwrap_or_default();
            if !expected.split('|').any(|e| e.eq_ignore_ascii_case(&top)) {
                misses.push(format!("{game}: got {top:?}, want {expected:?} — {:?}", &ranked[..ranked.len().min(3)]));
            }
        }
        assert!(misses.is_empty(), "{misses:#?}");
    }

    #[test]
    fn leaves_out_tools_and_redistributables() {
        let exes = listing(&[
            ("Game.exe", 50_000_000),
            ("UnityCrashHandler64.exe", 1_000_000),
            ("unins000.exe", 2_000_000),
            ("_CommonRedist/vcredist/VC_redist.x64.exe", 15_000_000),
            ("Engine/Binaries/Win64/EpicWebHelper.exe", 4_000_000),
            ("Engine/Binaries/Win64/SomethingUnknown.exe", 90_000_000),
            ("__overlay/overlayinjector.exe", 200_000),
            ("dedicated_server.exe", 9_000_000),
        ]);
        let ranked = rank("Some Game", &exes, &HashSet::new());
        let paths: Vec<&str> = ranked.iter().map(|c| c.path.as_str()).collect();
        assert_eq!(paths, ["Game.exe"]);
    }

    #[test]
    fn a_game_named_after_an_excluded_word_keeps_its_exe() {
        let exes = listing(&[("CrashBandicootNSaneTrilogy.exe", 60_000_000), ("CrashReporter.exe", 300_000)]);
        let ranked = rank("Crash Bandicoot N. Sane Trilogy", &exes, &HashSet::new());
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].path, "CrashBandicootNSaneTrilogy.exe");
    }

    #[test]
    fn names_initials_and_words() {
        assert_eq!(name_score("Cyberpunk2077", "Cyberpunk 2077"), 1.0);
        assert!(name_score("SOTTR", "Shadow of the Tomb Raider") >= 0.85);
        assert!(name_score("NMS", "No Man's Sky") >= 0.85);
        assert!(name_score("mgsvtpp", "METAL GEAR SOLID V: THE PHANTOM PAIN") >= 0.85);
        assert!(name_score("re4", "Resident Evil 4") >= 0.85);
        assert!(name_score("D2R", "Diablo II: Resurrected – Infernal Edition") >= 0.8);
        assert!(name_score("GTA5", "Grand Theft Auto V Legacy") >= 0.8);
        assert!(name_score("ShadowOfMordor", "Middle-earth™: Shadow of Mordor™") >= 0.85);
        assert!(name_score("NewColossus_x64vk", "Wolfenstein II: The New Colossus") > 0.2);
        assert_eq!(name_score("BlizzardError", "Diablo II: Resurrected"), 0.0);
        assert_eq!(tokens("GTAVLauncher"), ["gtav", "launcher"]);
        assert_eq!(tokens("NewColossus_x64vk"), ["new", "colossus", "x", "64", "vk"]);
    }

    #[test]
    fn unity_data_folder_wins() {
        let exes = listing(&[("Launcher.exe", 30_000_000), ("Peak.exe", 700_000), ("UnityCrashHandler64.exe", 1_600_000)]);
        let ranked = rank("Something Else", &exes, &data(&["Peak_Data"]));
        assert_eq!(ranked[0].path, "Peak.exe");
    }

    fn tempdir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("pishop-exeguess-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn pe_with(marker: &[u8]) -> Vec<u8> {
        let mut b = b"MZ".to_vec();
        b.resize(4096, 0);
        b.extend_from_slice(marker);
        b.resize(b.len() + 512, 0);
        b
    }

    #[test]
    fn scan_and_guess_on_disk() {
        let root = tempdir("scan");
        touch(&root, "Peak.exe", &[0; 700]);
        touch(&root, "UnityCrashHandler64.exe", &[0; 1600]);
        std::fs::create_dir_all(root.join("Peak_Data/StreamingAssets")).unwrap();
        touch(&root, "Peak_Data/StreamingAssets/inner.exe", &[0; 10]);
        touch(&root, "a/b/c/d/e/f/g/deep.exe", &[0; 10]);
        let (exes, dirs) = scan(&root, 6);
        let paths: Vec<&str> = exes.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["Peak.exe", "UnityCrashHandler64.exe"], "no descent into *_Data, depth capped");
        assert!(dirs.contains("Peak_Data"));
        assert_eq!(guess(&root, "PEAK")[0].path, "Peak.exe");
        // a single downloaded .exe is its own listing
        assert_eq!(scan(&root.join("Peak.exe"), 6).0, vec![("Peak.exe".to_string(), 700)]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn finds_the_installer_not_the_redistributables() {
        let root = tempdir("setup");
        touch(&root, "_CommonRedist/vcredist/VC_redist.x64.exe", b"MZ");
        touch(&root, "Redist/DXSETUP.exe", b"MZ");
        touch(&root, "UE4PrereqSetup_x64.exe", b"MZ");
        touch(&root, "unins000.exe", b"MZ");
        touch(&root, "MyGame/setup_mygame_1.2.exe", b"MZ");
        touch(&root, "setup.exe", b"MZ");
        assert_eq!(installers(&root), ["setup.exe", "MyGame/setup_mygame_1.2.exe"]);
        std::fs::remove_dir_all(&root).unwrap();

        // No installer-like name: the content says so (NSIS), a plain game exe doesn't.
        let root = tempdir("nsis");
        touch(&root, "openttd-15.3-windows-win64.exe", &pe_with(b"NullsoftInst"));
        touch(&root, "readme.txt", b"hi");
        assert_eq!(installers(&root), ["openttd-15.3-windows-win64.exe"]);
        assert_eq!(installers(&root.join("openttd-15.3-windows-win64.exe")), ["openttd-15.3-windows-win64.exe"]);
        std::fs::remove_dir_all(&root).unwrap();

        let root = tempdir("portable");
        touch(&root, "Game.exe", &pe_with(b"just a game"));
        assert!(installers(&root).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tells_installer_frameworks_apart() {
        let root = tempdir("kinds");
        touch(&root, "inno.exe", &pe_with(b"Inno Setup Setup Data (6.2.2)"));
        touch(&root, "inno16.exe", &pe_with(&utf16le("This installation was built with Inno Setup.")));
        touch(&root, "nsis.exe", &pe_with(b"Nullsoft.NSIS.exehead"));
        touch(&root, "wix.exe", &pe_with(b".wixburn"));
        touch(&root, "is.exe", &pe_with(&utf16le("InstallShield")));
        touch(&root, "plain.exe", &pe_with(b"nothing to see"));
        touch(&root, "notpe.exe", b"Inno Setup but not a PE");
        let kind = |f: &str| installer_kind(&root.join(f));
        assert_eq!(kind("inno.exe"), Some("inno"));
        assert_eq!(kind("inno16.exe"), Some("inno"));
        assert_eq!(kind("nsis.exe"), Some("nsis"));
        assert_eq!(kind("wix.exe"), Some("wix"));
        assert_eq!(kind("is.exe"), Some("installshield"));
        assert_eq!(kind("plain.exe"), None);
        assert_eq!(kind("notpe.exe"), None);
        assert_eq!(kind("missing.exe"), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn install_folder_arguments() {
        let dir = windows_path(Path::new("/run/media/deck/SN01T/piShop/My Game"));
        assert_eq!(dir, r"Z:\run\media\deck\SN01T\piShop\My Game");
        assert_eq!(install_dir_args("inno", &dir).unwrap(), r#"/DIR="Z:\run\media\deck\SN01T\piShop\My Game""#);
        assert_eq!(install_dir_args("nsis", &dir).unwrap(), r"/D=Z:\run\media\deck\SN01T\piShop\My Game");
        assert_eq!(install_dir_args("installshield", &dir), None);
    }

    /// The whole Ally dataset: `EXESCAN_JSON=…/exescan.json cargo test --release exeguess -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn dataset_accuracy() {
        let Ok(path) = std::env::var("EXESCAN_JSON") else { return };
        let games: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        // Where Steam starts a launcher, the game's own exe counts too.
        let also: &[(u64, &[&str])] = &[
            (1091500, &["bin/x64/Cyberpunk2077.exe"]),
            (292030, &["bin/x64_dx12/witcher3.exe"]),
            (271590, &["GTA5.exe"]),
            (1174180, &["RDR2.exe"]),
            (1659040, &["Retail/HITMAN3.exe"]),
            (2131650, &["METAL GEAR SOLID3.exe"]),
            (1546990, &["Gameface/Binaries/Win64/ViceCity.exe"]),
            (1547000, &["Gameface/Binaries/Win64/SanAndreas.exe"]),
            (1222680, &["NeedForSpeedHeat.exe"]),
            (1237970, &["Titanfall2.exe"]),
            (1238860, &["bf4.exe"]),
            (1649240, &["Returnal.exe", "Returnal/Binaries/Win64/Returnal-Win64-Shipping.exe"]),
            (218620, &["PAYDAY2.exe"]),
            (495420, &["StateOfDecay2/Binaries/Win64/StateOfDecay2-Win64-Shipping.exe", "StateOfDecay2.exe"]),
            (2073850, &["Discovery/Binaries/Win64/Discovery.exe"]),
            (269170, &["PoolNationVR.exe"]),
        ];
        let (mut ok, mut total, mut misses) = (0, 0, Vec::new());
        for g in games.as_array().unwrap() {
            let exes: Vec<(String, u64)> = g["exes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (e[0].as_str().unwrap().replace('\\', "/"), e[1].as_u64().unwrap()))
                .collect();
            if exes.is_empty() {
                continue;
            }
            let appid = g["appid"].as_u64().unwrap();
            let mut accept: HashSet<String> = g["launch"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|l| l["exe"].as_str())
                .filter(|e| !e.is_empty() && !e.contains("://"))
                .map(|e| e.replace('\\', "/").to_ascii_lowercase())
                .collect();
            for (id, extra) in also {
                if *id == appid {
                    accept.extend(extra.iter().map(|e| e.to_ascii_lowercase()));
                }
            }
            let name = g["name"].as_str().unwrap();
            let ranked = rank(name, &exes, &HashSet::new());
            let top = ranked.first().map(|c| c.path.to_ascii_lowercase()).unwrap_or_default();
            total += 1;
            if accept.contains(&top) {
                ok += 1;
            } else {
                misses.push(format!("{name}: {top} — accept {accept:?}"));
            }
        }
        println!("dataset accuracy: {ok}/{total}");
        for m in &misses {
            println!("  miss: {m}");
        }
        assert!(ok * 100 >= total * 95, "{ok}/{total}");
    }
}
