//! Archives inside downloads: inspect a download for archives (zip, 7z, rar,
//! iso, tar — multi-part sets counted once) and extract them as background
//! jobs with progress, using the tools SteamOS ships (7z, unrar, bsdtar).
//!
//! GET  /api/archive/inspect?path=   archives found (depth ≤ 2), exe count
//! POST /api/archive/extract         { path, dest?, delete_after } → { id }
//! GET  /api/archive/jobs            extraction jobs (kept until restart)
//! POST /api/archive/jobs/{id}/cancel

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use axum::extract::{Path as UrlPath, Query};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::watch;

use crate::tr;

/// How deep `inspect` looks below the given folder.
const SCAN_DEPTH: usize = 2;
/// 7z asks for a password on encrypted archives; a dummy one makes it fail
/// fast ("Wrong password") instead of waiting for input.
const NO_PASSWORD: &str = "-ppishop-no-password";

pub fn router() -> Router {
    Router::new()
        .route("/api/archive/inspect", get(inspect_route))
        .route("/api/archive/extract", post(extract_route))
        .route("/api/archive/jobs", get(|| async { Json(jobs()) }))
        .route("/api/archive/jobs/{id}/cancel", post(cancel_route))
}

fn err(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": msg.to_string() }))).into_response()
}

// ---------- archive sets ----------

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "7z")]
    SevenZ,
    #[serde(rename = "rar")]
    Rar,
    #[serde(rename = "iso")]
    Iso,
    #[serde(rename = "tar")]
    Tar,
}

/// Which naming scheme a part belongs to; parts of one set share it and the stem.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Family {
    /// `x.part1.rar`, `x.part2.rar` …
    RarParts,
    /// `x.rar` + `x.r00`, `x.r01` …
    RarOld,
    /// `x.7z`
    SevenZ,
    /// `x.7z.001`, `x.7z.002` …
    SevenZSplit,
    /// `x.zip` (+ `x.z01` … when spanned)
    Zip,
    /// `x.zip.001`, `x.zip.002` …
    ZipSplit,
    Iso,
    Tar,
}

impl Family {
    fn kind(self) -> Kind {
        match self {
            Family::RarParts | Family::RarOld => Kind::Rar,
            Family::SevenZ | Family::SevenZSplit => Kind::SevenZ,
            Family::Zip | Family::ZipSplit => Kind::Zip,
            Family::Iso => Kind::Iso,
            Family::Tar => Kind::Tar,
        }
    }
}

/// One file's place in a set. `order` sorts parts; `head` marks the file the
/// tool must be given (first volume, or the `.zip`/`.rar` of old-style sets).
#[derive(Debug, PartialEq)]
struct Part {
    family: Family,
    stem: String,
    order: u32,
    head: bool,
}

fn strip_suffix_ci<'a>(name: &'a str, suffix: &str) -> Option<&'a str> {
    let n = name.len().checked_sub(suffix.len())?;
    (name.is_char_boundary(n) && name[n..].eq_ignore_ascii_case(suffix)).then(|| &name[..n])
}

/// `x.ext.007` → ("x", 7) for a numeric-volume suffix after `ext`.
fn numbered<'a>(name: &'a str, ext: &str) -> Option<(&'a str, u32)> {
    let (rest, digits) = name.rsplit_once('.')?;
    if digits.len() < 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((strip_suffix_ci(rest, ext)?, digits.parse().ok()?))
}

fn classify(name: &str) -> Option<Part> {
    let part = |family, stem: &str, order, head| Some(Part { family, stem: stem.to_string(), order, head });
    // x.part01.rar
    if let Some(rest) = strip_suffix_ci(name, ".rar") {
        if let Some((stem, n)) = rest.rsplit_once('.') {
            if let Some(num) = strip_prefix_ci(n, "part").filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())) {
                let order: u32 = num.parse().ok()?;
                return part(Family::RarParts, stem, order, order == 1);
            }
        }
        return part(Family::RarOld, rest, 0, true);
    }
    // x.r00 … x.r999 (old-style rar volumes)
    if let Some((stem, ext)) = name.rsplit_once('.') {
        let e = ext.to_ascii_lowercase();
        if e.len() >= 3 && e.starts_with('r') && e[1..].bytes().all(|b| b.is_ascii_digit()) {
            return part(Family::RarOld, stem, e[1..].parse::<u32>().ok()? + 1, false);
        }
        if e.len() >= 3 && e.starts_with('z') && e[1..].bytes().all(|b| b.is_ascii_digit()) {
            // spanned zip: x.z01 … x.zNN, then x.zip (the part 7z opens)
            return part(Family::Zip, stem, e[1..].parse().ok()?, false);
        }
    }
    if let Some((stem, n)) = numbered(name, ".7z") {
        return part(Family::SevenZSplit, stem, n, n == 1);
    }
    if let Some((stem, n)) = numbered(name, ".zip") {
        return part(Family::ZipSplit, stem, n, n == 1);
    }
    if let Some(stem) = strip_suffix_ci(name, ".7z") {
        return part(Family::SevenZ, stem, 0, true);
    }
    if let Some(stem) = strip_suffix_ci(name, ".zip") {
        return part(Family::Zip, stem, u32::MAX, true);
    }
    if let Some(stem) = strip_suffix_ci(name, ".iso") {
        return part(Family::Iso, stem, 0, true);
    }
    for ext in [".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tbz", ".tar.zst", ".tzst", ".tar.lz4"] {
        if let Some(stem) = strip_suffix_ci(name, ext) {
            return part(Family::Tar, stem, 0, true);
        }
    }
    None
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    (s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix)).then(|| &s[prefix.len()..])
}

#[derive(Serialize, Clone, Debug)]
pub struct ArchiveSet {
    /// The part the extractor is given, absolute.
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub parts: usize,
    /// All parts together, in bytes.
    pub size: u64,
    /// False when the part numbers have a gap (an unfinished download).
    pub complete: bool,
    #[serde(skip)]
    pub all_parts: Vec<PathBuf>,
    /// Set name without part number or extension (the default folder name).
    #[serde(skip)]
    pub stem: String,
}

/// Groups the files of ONE folder into archive sets. Sets whose head is
/// missing (e.g. `x.r00` without `x.rar`) can't be extracted and are left out.
fn group(files: &[(PathBuf, u64)]) -> Vec<ArchiveSet> {
    let mut sets: BTreeMap<(Family, String), Vec<(Part, PathBuf, u64)>> = BTreeMap::new();
    for (path, size) in files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if let Some(p) = classify(name) {
            sets.entry((p.family, p.stem.to_lowercase())).or_default().push((p, path.clone(), *size));
        }
    }
    let mut out = Vec::new();
    for ((family, _), mut parts) in sets {
        parts.sort_by_key(|(p, ..)| p.order);
        // e.g. `x.part2.rar` without its part1: nothing to start from
        let Some(head) = parts.iter().find(|(p, ..)| p.head) else { continue };
        let head_path = &head.1;
        // Volume numbers must run without gaps from the first one. (A missing
        // LAST volume can't be seen here; the extractor reports that one.)
        let numbers: Vec<u32> = parts.iter().map(|(p, ..)| p.order).filter(|&o| o != u32::MAX).collect();
        let complete = numbers.windows(2).all(|w| w[1] == w[0] + 1)
            && match family {
                Family::RarParts | Family::SevenZSplit | Family::ZipSplit => numbers.first() == Some(&1),
                Family::Zip => numbers.is_empty() || numbers.first() == Some(&1),
                _ => true,
            };
        out.push(ArchiveSet {
            path: head_path.to_string_lossy().into_owned(),
            name: head_path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            kind: family.kind(),
            parts: parts.len(),
            size: parts.iter().map(|(.., s)| s).sum(),
            all_parts: parts.iter().map(|(_, p, _)| p.clone()).collect(),
            stem: head.0.stem.clone(),
            complete,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Folders (relative, lowercased) whose installers are prerequisites, not the game's.
fn is_redist_dir(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["redist", "commonredist", "__installer", "directx", "vcredist", "prereq", "_support"].iter().any(|k| n.contains(k))
}

/// setup.exe / install.exe / Setup_Game.exe — not uninstallers or runtimes.
fn is_installer(stem: &str) -> bool {
    let s = stem.to_ascii_lowercase();
    let looks = s.contains("setup") || s.contains("install");
    const NOT: [&str; 16] = [
        "unins", "uninstall", "redist", "dxsetup", "directx", "prereq", "vcredist", "dotnet", "oalinst", "physx",
        "eaapp", "epiconlineservices", "social-club", "socialclub", "anticheat", "battleye",
    ];
    looks && !NOT.iter().any(|n| s.contains(n))
}

#[derive(Serialize, Debug, Default)]
pub struct Inspection {
    pub archives: Vec<ArchiveSet>,
    pub exe_count: usize,
    pub has_installer: bool,
}

/// Files of a folder tree up to `depth`, grouped by folder. No symlinks followed.
fn walk(root: &Path, depth: usize, out: &mut BTreeMap<PathBuf, Vec<(PathBuf, u64)>>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
        if meta.is_dir() {
            if depth > 0 && !e.file_name().to_string_lossy().starts_with('.') {
                walk(&e.path(), depth - 1, out);
            }
        } else if meta.is_file() {
            out.entry(root.to_path_buf()).or_default().push((e.path(), meta.len()));
        }
    }
}

pub fn inspect(path: &Path) -> anyhow::Result<Inspection> {
    let path = &path.canonicalize().map_err(|_| anyhow::anyhow!(tr!("not found: {}", "não encontrado: {}", path.display())))?;
    let meta = std::fs::metadata(path)?;
    let mut by_dir = BTreeMap::new();
    if meta.is_file() {
        // One file: the set it belongs to, among its siblings.
        let dir = path.parent().unwrap_or(Path::new("/"));
        walk(dir, 0, &mut by_dir);
        let sets = by_dir.values().flat_map(|f| group(f)).filter(|s| s.all_parts.iter().any(|p| p == path)).collect();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let exe = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        return Ok(Inspection { archives: sets, exe_count: exe as usize, has_installer: exe && is_installer(&stem) });
    }
    walk(path, SCAN_DEPTH, &mut by_dir);
    let mut out = Inspection::default();
    for (dir, files) in &by_dir {
        out.archives.extend(group(files));
        let redist = dir.strip_prefix(path).ok().is_some_and(|rel| rel.components().any(|c| is_redist_dir(&c.as_os_str().to_string_lossy())));
        for (f, _) in files {
            if f.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
                out.exe_count += 1;
                if !redist && is_installer(&f.file_stem().unwrap_or_default().to_string_lossy()) {
                    out.has_installer = true;
                }
            }
        }
    }
    out.archives.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The set a given part belongs to (any part may be passed).
fn set_of(path: &Path) -> Option<ArchiveSet> {
    let dir = path.parent()?;
    let mut by_dir = BTreeMap::new();
    walk(dir, 0, &mut by_dir);
    by_dir.values().flat_map(|f| group(f)).find(|s| s.all_parts.iter().any(|p| p == path))
}

// ---------- tools ----------

/// First of `names` found on PATH (plus the usual system folders).
fn find_tool(names: &[&str]) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in ["/usr/bin", "/usr/local/bin", "/bin", "/opt/homebrew/bin"] {
        dirs.push(PathBuf::from(d));
    }
    names.iter().find_map(|n| {
        dirs.iter().map(|d| d.join(n)).find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && is_executable(&m)))
    })
}

fn is_executable(m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o111 != 0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    /// 7zz / 7z: everything, prints overall progress with -bsp1.
    SevenZ,
    /// 7za: 7z/zip/split only (no iso, no rar).
    SevenZa,
    Unrar,
    /// bsdtar / tar: no percentage; progress from bytes read.
    Tar,
}

/// The extractor for an archive set and its command line.
fn plan(set: &ArchiveSet, dest: &Path) -> anyhow::Result<(Tool, PathBuf, Vec<String>)> {
    let first = set.path.clone();
    let volumes = set.parts > 1;
    let seven = || find_tool(&["7zz", "7z"]).map(|p| (Tool::SevenZ, p)).or_else(|| find_tool(&["7za"]).map(|p| (Tool::SevenZa, p)));
    let tar = || find_tool(&["bsdtar", "tar"]).map(|p| (Tool::Tar, p));
    let pick = match set.kind {
        Kind::Rar => find_tool(&["unrar"]).map(|p| (Tool::Unrar, p)).or_else(|| find_tool(&["7zz", "7z"]).map(|p| (Tool::SevenZ, p))).or_else(|| (!volumes).then(tar).flatten()),
        Kind::Iso => find_tool(&["7zz", "7z"]).map(|p| (Tool::SevenZ, p)).or_else(tar),
        Kind::Zip | Kind::SevenZ => seven().or_else(|| (!volumes).then(tar).flatten()),
        Kind::Tar => tar(),
    };
    let Some((tool, exe)) = pick else {
        let needs = match set.kind {
            Kind::Rar => "unrar / 7z",
            Kind::Tar => "bsdtar / tar",
            _ if volumes => "7z",
            _ => "7z / bsdtar",
        };
        anyhow::bail!(tr!(
            "no tool to extract this archive (needs {needs})",
            "nenhuma ferramenta para extrair este arquivo (precisa de {needs})"
        ));
    };
    let d = dest.to_string_lossy().into_owned();
    let args = match tool {
        Tool::SevenZ | Tool::SevenZa => vec!["x".into(), "-y".into(), "-bsp1".into(), "-bso0".into(), NO_PASSWORD.into(), format!("-o{d}"), first],
        // -p-: never ask for a password; the trailing slash makes `dest` a folder
        Tool::Unrar => vec!["x".into(), "-o+".into(), "-y".into(), "-p-".into(), "-c-".into(), first, format!("{}/", d.trim_end_matches('/'))],
        Tool::Tar => vec!["-xf".into(), first, "-C".into(), d],
    };
    Ok((tool, exe, args))
}

// ---------- progress ----------

/// Finds "NN%" in a tool's progress output (7z -bsp1, unrar), which redraws
/// itself with backspaces and carriage returns. Only a percentage at the
/// start or the end of a redrawn segment counts, so file names containing
/// "100%" don't. Remembers a short tail in case a number is split across reads.
#[derive(Default)]
struct PercentScanner {
    tail: Vec<u8>,
}

impl PercentScanner {
    /// The last percentage in this chunk, if any.
    fn feed(&mut self, chunk: &[u8]) -> Option<u8> {
        let mut buf = std::mem::take(&mut self.tail);
        buf.extend_from_slice(chunk);
        let segments: Vec<&[u8]> = buf.split(|b| matches!(b, b'\r' | b'\n' | 0x08)).collect();
        let (pending, done) = segments.split_last().expect("split yields at least one segment");
        let mut last = done.iter().filter_map(|seg| percent_in(seg)).last();
        // The unfinished segment counts once its percentage is complete
        // ("47% …" or "… 47%"); otherwise it waits for the next read.
        match leading_percent(pending).or_else(|| pending.ends_with(b"%").then(|| percent_in(pending)).flatten()) {
            Some(p) => last = Some(p),
            None => self.tail = pending[pending.len().saturating_sub(512)..].to_vec(),
        }
        last
    }
}

fn leading_percent(seg: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(seg).ok()?.trim_start();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (!digits.is_empty() && digits.len() <= 3 && s[digits.len()..].starts_with('%'))
        .then(|| digits.parse::<u16>().ok())
        .flatten()
        .filter(|p| *p <= 100)
        .map(|p| p as u8)
}

/// "  12% 3 - file" (leading) or "Extracting  file   12%" (trailing).
fn percent_in(seg: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(seg).ok()?.trim();
    let leading = || {
        let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
        (!digits.is_empty() && s[digits.len()..].starts_with('%')).then(|| digits.parse::<u16>().ok()).flatten()
    };
    let trailing = || {
        let body = s.strip_suffix('%')?;
        let digits: String = body.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        let before = &body[..body.len() - digits.len()];
        (!digits.is_empty() && digits.len() <= 3 && (before.is_empty() || before.ends_with(char::is_whitespace))).then(|| digits.parse::<u16>().ok()).flatten()
    };
    leading().or_else(trailing).filter(|p| *p <= 100).map(|p| p as u8)
}

/// Bytes the process has read from `file` (Linux: /proc/<pid>/fdinfo).
fn bytes_read(pid: u32, file: &Path) -> Option<u64> {
    let fds = std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?;
    for fd in fds.flatten() {
        if std::fs::read_link(fd.path()).ok().as_deref() == Some(file) {
            let info = std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{}", fd.file_name().to_string_lossy())).ok()?;
            return info.lines().find_map(|l| l.strip_prefix("pos:")).and_then(|v| v.trim().parse().ok());
        }
    }
    None
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries
        .flatten()
        .map(|e| match std::fs::symlink_metadata(e.path()) {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) if m.is_file() => m.len(),
            _ => 0,
        })
        .sum()
}

// ---------- errors ----------

/// A user-facing reason for a failed extraction, from the tool's exit code
/// and output (tools run with LC_ALL=C.UTF-8, so messages are English).
fn explain(tool: Tool, code: Option<i32>, output: &str) -> String {
    let o = output.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| o.contains(n));
    if has(&["no space left", "not enough space", "disk is full", "disk full", "enospc"]) || (tool == Tool::Unrar && code == Some(5)) {
        return tr!("not enough space on the disk", "sem espaço no disco");
    }
    if has(&["wrong password", "incorrect password", "password is incorrect", "encrypted", "enter password", "passphrase", "bad password"])
        || (tool == Tool::Unrar && code == Some(11))
    {
        return tr!("the archive is password-protected", "o arquivo está protegido por senha");
    }
    if has(&["missing volume", "cannot find volume", "unavailable data", "next volume", "is not found"]) {
        return tr!("a part of the archive is missing", "falta uma parte do arquivo");
    }
    if has(&[
        "crc failed", "checksum error", "data error", "unexpected end", "is corrupt", "headers error", "cannot open the file as archive",
        "can not open the file as archive", "unrecognized archive", "damaged", "corrupt", "truncated", "not rar archive", "is not rar",
    ]) || (tool == Tool::Unrar && code == Some(3))
    {
        return tr!("the archive is damaged or incomplete", "o arquivo está corrompido ou incompleto");
    }
    let last = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && percent_in(l.as_bytes()).is_none())
        .last()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect::<String>();
    match code {
        Some(c) if last.is_empty() => tr!("extraction failed (code {c})", "a extração falhou (código {c})"),
        _ => tr!("extraction failed: {last}", "a extração falhou: {last}"),
    }
}

/// Exit codes that still mean "extracted" (warnings).
fn succeeded(tool: Tool, code: i32) -> bool {
    match tool {
        Tool::SevenZ | Tool::SevenZa | Tool::Unrar => code == 0 || code == 1,
        Tool::Tar => code == 0,
    }
}

// ---------- jobs ----------

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Running,
    Done,
    Failed,
    Canceled,
}

#[derive(Serialize, Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub archive: String,
    pub dest: String,
    pub state: State,
    /// 0..1
    pub progress: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Unix seconds.
    pub started: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished: Option<i64>,
    pub delete_after: bool,
    /// Parts deleted after extracting (delete_after).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

struct Entry {
    job: Job,
    cancel: Option<watch::Sender<bool>>,
}

static JOBS: LazyLock<Mutex<BTreeMap<u64, Entry>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
static NEXT: AtomicU64 = AtomicU64::new(1);

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn jobs() -> Vec<Job> {
    JOBS.lock().unwrap().values().map(|e| e.job.clone()).collect()
}

fn update(id: u64, f: impl FnOnce(&mut Job)) {
    if let Some(e) = JOBS.lock().unwrap().get_mut(&id) {
        f(&mut e.job);
    }
}

/// Progress only moves forward and stays below 1 until the tool exits.
fn set_progress(id: u64, p: f32) {
    update(id, |j| {
        let p = p.clamp(0.0, 0.99);
        if p > j.progress {
            j.progress = p;
        }
    });
}

/// Where extracting the archive at `path` (any part) lands by default. Works
/// from the name alone, so it still answers after the archive was deleted.
pub fn default_dest_for(path: &Path) -> Option<PathBuf> {
    group(&[(path.to_path_buf(), 0)]).into_iter().next().map(|set| default_dest(&set))
}

fn default_dest(set: &ArchiveSet) -> PathBuf {
    let dir = Path::new(&set.path).parent().unwrap_or(Path::new("/"));
    let stem = set.stem.trim().trim_end_matches('.');
    let mut dest = dir.join(if stem.is_empty() { "extracted" } else { stem });
    if dest.exists() && !dest.is_dir() {
        dest = dir.join(format!("{}-extracted", stem));
    }
    dest
}

/// Starts extracting the set `path` belongs to. Returns the job id.
pub fn extract(path: &Path, dest: Option<PathBuf>, delete_after: bool) -> Result<u64, (StatusCode, String)> {
    let bad = |m: String| (StatusCode::BAD_REQUEST, m);
    let path = path.canonicalize().map_err(|_| bad(tr!("not found: {}", "não encontrado: {}", path.display())))?;
    let set = set_of(&path).ok_or_else(|| bad(tr!("not an archive piShop can extract", "não é um arquivo que o piShop consegue extrair")))?;
    if !set.complete {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, tr!("a part of the archive is missing", "falta uma parte do arquivo")));
    }
    let dest = dest.unwrap_or_else(|| default_dest(&set));
    {
        let jobs = JOBS.lock().unwrap();
        if jobs.values().any(|e| e.job.state == State::Running && e.job.archive == set.path) {
            return Err((StatusCode::CONFLICT, tr!("this archive is already being extracted", "este arquivo já está sendo extraído")));
        }
    }
    let (tool, exe, args) = plan(&set, &dest).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("{e:#}")))?;
    let created = !dest.exists();
    std::fs::create_dir_all(&dest).map_err(|e| bad(tr!("couldn't create {}: {e}", "não foi possível criar {}: {e}", dest.display())))?;

    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = watch::channel(false);
    let job = Job {
        id,
        archive: set.path.clone(),
        dest: dest.to_string_lossy().into_owned(),
        state: State::Running,
        progress: 0.0,
        error: None,
        started: now(),
        finished: None,
        delete_after,
        deleted: false,
    };
    JOBS.lock().unwrap().insert(id, Entry { job, cancel: Some(tx) });
    crate::log!("arquivo: extraindo {} ({} partes, {:?}) → {} com {}", set.name, set.parts, set.kind, dest.display(), exe.display());
    tokio::spawn(run(id, set, dest, created, tool, exe, args, rx));
    Ok(id)
}

pub fn cancel(id: u64) -> Result<(), (StatusCode, String)> {
    let mut jobs = JOBS.lock().unwrap();
    let e = jobs.get_mut(&id).ok_or((StatusCode::NOT_FOUND, tr!("unknown job", "tarefa desconhecida")))?;
    if e.job.state != State::Running {
        return Err((StatusCode::CONFLICT, tr!("this job isn't running", "esta tarefa não está em andamento")));
    }
    if let Some(tx) = &e.cancel {
        let _ = tx.send(true);
    }
    Ok(())
}

/// Reads a child's output: progress (if `scan`) and the last few KB for errors.
async fn pump(mut r: impl AsyncRead + Unpin, id: u64, scan: bool, log: Arc<Mutex<Vec<u8>>>) {
    let mut scanner = PercentScanner::default();
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let chunk = &buf[..n];
                if scan {
                    if let Some(p) = scanner.feed(chunk) {
                        set_progress(id, p as f32 / 100.0);
                    }
                }
                let mut l = log.lock().unwrap();
                l.extend(chunk.iter().map(|&b| if b == 0x08 { b'\n' } else { b }));
                let excess = l.len().saturating_sub(16 * 1024);
                l.drain(..excess);
            }
        }
    }
}

fn kill_group(pid: u32, signal: i32) {
    // The tool runs in its own session (setsid): its pid is the group id.
    unsafe {
        libc::kill(-(pid as i32), signal);
    }
}

/// Removes the destination folder when this job created it and nothing landed in it.
fn tidy(dest: &Path, created: bool) {
    if created && std::fs::read_dir(dest).is_ok_and(|mut d| d.next().is_none()) {
        let _ = std::fs::remove_dir(dest);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(id: u64, set: ArchiveSet, dest: PathBuf, created: bool, tool: Tool, exe: PathBuf, args: Vec<String>, mut cancel: watch::Receiver<bool>) {
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Steam's runtime libraries and overlay don't belong in system tools.
        .env_remove("LD_PRELOAD")
        .env_remove("LD_LIBRARY_PATH")
        .kill_on_drop(true);
    if cfg!(target_os = "linux") {
        // English messages (for `explain`) while keeping UTF-8 file names.
        cmd.env("LC_ALL", "C.UTF-8");
    }
    // Own session: no terminal to ask a password on, and one group to kill.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tidy(&dest, created);
            return finish(id, &set, Err(tr!("couldn't start {}: {e}", "não foi possível iniciar {}: {e}", exe.display())));
        }
    };
    let pid = child.id().unwrap_or(0);
    let log = Arc::new(Mutex::new(Vec::new()));
    let scan = tool != Tool::Tar;
    let out = tokio::spawn(pump(child.stdout.take().unwrap(), id, scan, log.clone()));
    let errs = tokio::spawn(pump(child.stderr.take().unwrap(), id, scan, log.clone()));

    // bsdtar prints no percentage: follow how far it has read the archive
    // (Linux), or how much it has written so far.
    let ticker = (tool == Tool::Tar).then(|| {
        let first = PathBuf::from(&set.path);
        let (total, dest) = (set.size.max(1), dest.clone());
        tokio::spawn(async move {
            let canonical = first.canonicalize().unwrap_or(first);
            loop {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let done = bytes_read(pid, &canonical).unwrap_or_else(|| dir_size(&dest));
                set_progress(id, done as f32 / total as f32);
            }
        })
    });

    let outcome = tokio::select! {
        status = child.wait() => Ok(status),
        _ = cancel.wait_for(|c| *c) => Err(()),
    };
    if let Some(t) = &ticker {
        t.abort();
    }
    let result = match outcome {
        Err(()) => {
            kill_group(pid, libc::SIGTERM);
            if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
                kill_group(pid, libc::SIGKILL);
                let _ = child.wait().await;
            }
            let _ = tokio::join!(out, errs);
            tidy(&dest, created);
            update(id, |j| {
                j.state = State::Canceled;
                j.finished = Some(now());
            });
            crate::log!("arquivo: extração de {} cancelada", set.name);
            return;
        }
        Ok(status) => {
            let _ = tokio::join!(out, errs);
            let output = String::from_utf8_lossy(&log.lock().unwrap()).into_owned();
            match status {
                Ok(s) if s.code().is_some_and(|c| succeeded(tool, c)) => Ok(()),
                Ok(s) => Err(explain(tool, s.code(), &output)),
                Err(e) => Err(tr!("extraction failed: {e}", "a extração falhou: {e}")),
            }
        }
    };
    // A failed extraction leaves nothing usable (and a full disk wants its
    // space back): drop the folder this job created. Canceling keeps it.
    if result.is_err() && created {
        let _ = std::fs::remove_dir_all(&dest);
    }
    finish(id, &set, result);
}

fn finish(id: u64, set: &ArchiveSet, result: Result<(), String>) {
    let delete = JOBS.lock().unwrap().get(&id).is_some_and(|e| e.job.delete_after) && result.is_ok();
    let mut deleted = false;
    if delete {
        deleted = set.all_parts.iter().all(|p| std::fs::remove_file(p).is_ok());
        crate::log!("arquivo: {} partes de {} apagadas ({})", set.parts, set.name, if deleted { "ok" } else { "com falhas" });
    }
    match &result {
        Ok(()) => crate::log!("arquivo: {} extraído", set.name),
        Err(e) => crate::log!("arquivo: extração de {} falhou: {e}", set.name),
    }
    update(id, |j| {
        j.finished = Some(now());
        match result {
            Ok(()) => {
                j.state = State::Done;
                j.progress = 1.0;
                j.deleted = deleted;
            }
            Err(e) => {
                j.state = State::Failed;
                j.error = Some(e);
            }
        }
    });
}

// ---------- routes ----------

#[derive(Deserialize)]
struct InspectQuery {
    path: String,
}

async fn inspect_route(Query(q): Query<InspectQuery>) -> Response {
    let path = PathBuf::from(q.path.trim());
    if !path.is_absolute() {
        return err(StatusCode::BAD_REQUEST, tr!("invalid path", "caminho inválido"));
    }
    match tokio::task::spawn_blocking(move || inspect(&path)).await {
        Ok(Ok(i)) => Json(i).into_response(),
        Ok(Err(e)) => err(StatusCode::NOT_FOUND, format!("{e:#}")),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct ExtractReq {
    path: String,
    #[serde(default)]
    dest: Option<String>,
    #[serde(default)]
    delete_after: bool,
}

async fn extract_route(Json(r): Json<ExtractReq>) -> Response {
    let path = PathBuf::from(r.path.trim());
    let dest = r.dest.map(|d| PathBuf::from(d.trim())).filter(|d| !d.as_os_str().is_empty());
    if !path.is_absolute() || dest.as_ref().is_some_and(|d| !d.is_absolute()) {
        return err(StatusCode::BAD_REQUEST, tr!("invalid path", "caminho inválido"));
    }
    match extract(&path, dest, r.delete_after) {
        Ok(id) => Json(json!({ "id": id })).into_response(),
        Err((status, msg)) => err(status, msg),
    }
}

async fn cancel_route(UrlPath(id): UrlPath<u64>) -> Response {
    match cancel(id) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err((status, msg)) => err(status, msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<(PathBuf, u64)> {
        names.iter().map(|n| (PathBuf::from(format!("/d/{n}")), 10)).collect()
    }

    fn summary(names: &[&str]) -> Vec<(String, Kind, usize)> {
        group(&files(names)).into_iter().map(|s| (s.name, s.kind, s.parts)).collect()
    }

    #[test]
    fn rar_part_sets() {
        assert_eq!(
            summary(&["Game-RUNE.part1.rar", "Game-RUNE.part2.rar", "Game-RUNE.part3.rar", "readme.nfo"]),
            [("Game-RUNE.part1.rar".into(), Kind::Rar, 3)]
        );
        assert_eq!(summary(&["g.part02.rar", "g.part01.rar"]), [("g.part01.rar".into(), Kind::Rar, 2)]);
        assert_eq!(summary(&["G.PART001.RAR", "G.PART002.RAR"]), [("G.PART001.RAR".into(), Kind::Rar, 2)]);
        // part1 missing: nothing extractable
        assert!(summary(&["g.part2.rar", "g.part3.rar"]).is_empty());
    }

    #[test]
    fn old_style_rar() {
        assert_eq!(summary(&["setup.r00", "setup.rar", "setup.r01", "setup.sfv"]), [("setup.rar".into(), Kind::Rar, 3)]);
        assert!(summary(&["orphan.r00", "orphan.r01"]).is_empty());
        assert_eq!(summary(&["single.rar"]), [("single.rar".into(), Kind::Rar, 1)]);
    }

    #[test]
    fn split_next_to_single() {
        // x.7z and x.7z.001… are two different sets
        let s = summary(&["x.7z", "x.7z.001", "x.7z.002"]);
        assert_eq!(s, [("x.7z".into(), Kind::SevenZ, 1), ("x.7z.001".into(), Kind::SevenZ, 2)]);
    }

    #[test]
    fn split_and_spanned() {
        assert_eq!(summary(&["x.7z.001", "x.7z.002", "x.7z.003"]), [("x.7z.001".into(), Kind::SevenZ, 3)]);
        assert_eq!(summary(&["x.zip.001", "x.zip.002"]), [("x.zip.001".into(), Kind::Zip, 2)]);
        assert_eq!(summary(&["x.z01", "x.z02", "x.zip"]), [("x.zip".into(), Kind::Zip, 3)]);
        assert_eq!(summary(&["a.7z", "b.zip", "c.iso"]), [("a.7z".into(), Kind::SevenZ, 1), ("b.zip".into(), Kind::Zip, 1), ("c.iso".into(), Kind::Iso, 1)]);
    }

    #[test]
    fn gaps_mean_incomplete() {
        let complete = |names: &[&str]| group(&files(names)).iter().map(|s| s.complete).collect::<Vec<_>>();
        assert_eq!(complete(&["x.7z.001", "x.7z.003"]), [false]);
        assert_eq!(complete(&["g.part1.rar", "g.part3.rar"]), [false]);
        assert_eq!(complete(&["o.rar", "o.r01"]), [false]); // r00 missing
        assert_eq!(complete(&["z.zip", "z.z02"]), [false]); // z01 missing
        assert_eq!(complete(&["x.7z.001", "x.7z.002", "g.part1.rar", "g.part2.rar", "o.rar", "o.r00", "z.zip", "z.z01", "a.iso"]), [true; 5]);
    }

    #[test]
    fn tar_variants() {
        let s = summary(&["a.tar", "b.tar.gz", "c.tgz", "d.tar.xz", "e.tar.zst", "notes.txt", "game.exe"]);
        assert_eq!(s.len(), 5);
        assert!(s.iter().all(|(_, k, n)| *k == Kind::Tar && *n == 1));
        let sets = group(&files(&["My Game v1.2.tar.xz"]));
        assert_eq!(sets[0].stem, "My Game v1.2");
    }

    #[test]
    fn stems_for_default_folder() {
        assert_eq!(group(&files(&["Game.Name-RUNE.part01.rar", "Game.Name-RUNE.part02.rar"]))[0].stem, "Game.Name-RUNE");
        assert_eq!(group(&files(&["game.7z.001"]))[0].stem, "game");
        assert_eq!(group(&files(&["setup.zip"]))[0].stem, "setup");
    }

    #[test]
    fn installers() {
        assert!(is_installer("setup"));
        assert!(is_installer("Setup_Game"));
        assert!(is_installer("install"));
        assert!(!is_installer("unins000"));
        assert!(!is_installer("DXSETUP"));
        assert!(!is_installer("UE4PrereqSetup_x64"));
        assert!(!is_installer("vc_redist.x64"));
        assert!(!is_installer("Game"));
    }

    #[test]
    fn seven_zip_progress() {
        let mut s = PercentScanner::default();
        assert_eq!(s.feed(b"\x08\x08\x08\x08  5% 3 - Data/file.pak\x08\x08\x08\x08"), Some(5));
        assert_eq!(s.feed(b"  12% 5 - Data/100% real.txt\x08\x08"), Some(12));
        // "4" | "7%" split across reads
        assert_eq!(s.feed(b"\x08\x08  4"), None);
        assert_eq!(s.feed(b"7% 9 - x"), Some(47));
        assert_eq!(s.feed(b"\r100%\n"), Some(100));
    }

    #[test]
    fn unrar_progress() {
        let mut s = PercentScanner::default();
        let out = b"Extracting from g.part1.rar\n\nExtracting  g/data.bin                                    1%\x08\x08\x08\x08  2%\x08\x08\x08\x08 37%";
        assert_eq!(s.feed(out), Some(37));
        assert_eq!(s.feed(b"\x08\x08\x08\x08\x08 OK \nAll OK\n"), None);
        assert_eq!(percent_in(b"Extracting  g/data.bin       12%"), Some(12));
        assert_eq!(percent_in(b"Extracting  weird100%"), None);
        assert_eq!(percent_in(b"250%"), None);
    }

    #[test]
    fn explanations() {
        assert_eq!(explain(Tool::SevenZ, Some(2), "ERROR: Wrong password : data.bin"), tr!("the archive is password-protected", "o arquivo está protegido por senha"));
        assert_eq!(explain(Tool::Unrar, Some(3), "CRC failed in the encrypted file"), tr!("the archive is password-protected", "o arquivo está protegido por senha"));
        assert_eq!(explain(Tool::Unrar, Some(3), "g/data.bin - CRC failed"), tr!("the archive is damaged or incomplete", "o arquivo está corrompido ou incompleto"));
        assert_eq!(explain(Tool::Tar, Some(1), "bsdtar: Write failed: No space left on device"), tr!("not enough space on the disk", "sem espaço no disco"));
        assert_eq!(explain(Tool::SevenZ, Some(2), "ERROR: x.7z.002\nMissing volume : x.7z.002"), tr!("a part of the archive is missing", "falta uma parte do arquivo"));
        assert_eq!(explain(Tool::SevenZ, Some(2), "ERROR: x.zip\nCannot open the file as archive"), tr!("the archive is damaged or incomplete", "o arquivo está corrompido ou incompleto"));
        assert!(explain(Tool::Tar, Some(7), "something odd happened").ends_with("something odd happened"));
    }
}
