//! Direct downloads: files fetched straight from a web server instead of a
//! swarm — a community game's parts (an itch.io build, a GitHub release, an
//! archive.org file) or a link added in Transfers.
//!
//! A download writes `<file>.part` and renames it when complete. Stopped
//! (paused, piShop closed, the connection dropped), it picks up where the
//! `.part` ends with a Range request; a server that answers 200 instead of
//! 206 starts it over.
//!
//! A big file on a server that takes ranges is fetched over a few
//! connections at once, each its own slice of the file (archive.org serves
//! one connection far slower than several): the slices are saved, so a
//! stopped download resumes each one; a connection that finishes its slice
//! takes over half of the biggest one left. Links that expire (itch.io's signed URLs, which live a
//! minute) are resolved again before every request. Dropped connections are
//! retried with a growing wait; a download only fails after several attempts
//! in a row that got nothing. State lives in `<data>/direct.json`; what was
//! downloading carries on when piShop starts again.
//!
//! GET  /api/direct                    every download
//! POST /api/direct                    { url, dest? } → a download of its own (Transfers)
//! POST /api/direct/{id}/{action}      pause | resume | remove | delete

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use axum::extract::Path as UrlPath;
use axum::routing::post;
use axum::{Json, Router};
use reqwest::header::{CONTENT_DISPOSITION, CONTENT_RANGE, COOKIE, RANGE, SET_COOKIE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

use crate::install::reply;
use crate::{log, tr};

/// Where a download comes from. Resolved to a URL right before each request.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Source {
    /// Plain links, tried in order (mirrors).
    Http { urls: Vec<String> },
    /// An itch.io page's upload; free and pay-what-you-want games only.
    Itch {
        page: String,
        #[serde(default)]
        upload_id: Option<u64>,
        /// The upload's file name, to pick it when the id isn't known.
        #[serde(default)]
        file: Option<String>,
    },
    /// An asset of a GitHub release ("latest" or a tag), by name pattern.
    Github {
        repo: String,
        #[serde(default = "latest")]
        tag: String,
        /// Regular expression the asset's name must match.
        asset: String,
    },
}

fn latest() -> String {
    "latest".into()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    Paused,
    Done,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Download {
    pub id: u64,
    pub source: Source,
    /// The finished file.
    pub path: String,
    /// Bytes expected (0 while unknown).
    pub total: u64,
    pub done: u64,
    pub state: State,
    #[serde(default)]
    pub error: Option<String>,
    /// Bytes per second right now.
    #[serde(default, skip_deserializing)]
    pub speed: f64,
    /// Unix seconds.
    pub added: i64,
    #[serde(default)]
    pub finished: Option<i64>,
    /// Who asked for it: a community install's part ("cm-<game>/<part>"), or
    /// empty for a link added in Transfers.
    #[serde(default)]
    pub owner: String,
    /// The slices still being fetched in parallel (empty: one connection).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    /// Connections open right now.
    #[serde(default, skip_deserializing)]
    pub connections: usize,
}

/// A slice of the file one connection fetches: bytes `start..end`, the first
/// `done` of them written.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

impl Segment {
    fn pos(&self) -> u64 {
        self.start + self.done
    }
    fn left(&self) -> u64 {
        self.end.saturating_sub(self.pos())
    }
}

/// `from..total` in `n` slices of about the same size.
fn slices(from: u64, total: u64, n: usize) -> Vec<Segment> {
    let n = n.max(1) as u64;
    let len = total.saturating_sub(from);
    (0..n)
        .map(|k| Segment { start: from + len * k / n, end: from + len * (k + 1) / n, done: 0 })
        .filter(|s| s.end > s.start)
        .collect()
}

/// Half of the slice with the most left becomes a new slice (for a
/// connection that's free); None when no slice is worth splitting.
fn steal(segs: &mut Vec<Segment>) -> Option<usize> {
    let (i, s) = segs.iter().enumerate().max_by_key(|(_, s)| s.left())?;
    if s.left() < 2 * MIN_SPLIT {
        return None;
    }
    let mid = s.pos() + s.left() / 2;
    let end = s.end;
    segs[i].end = mid;
    segs.push(Segment { start: mid, end, done: 0 });
    Some(segs.len() - 1)
}

static DOWNLOADS: LazyLock<Mutex<BTreeMap<u64, Download>>> = LazyLock::new(|| {
    let map: BTreeMap<u64, Download> = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    Mutex::new(map)
});
/// Stop signals of the downloads running now, with the run they belong to
/// (a run that ends after a quick pause/resume must not drop the next one's).
static RUNNING: LazyLock<Mutex<HashMap<u64, (u64, watch::Sender<bool>)>>> = LazyLock::new(Default::default);
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn file() -> PathBuf {
    crate::data_dir().join("direct.json")
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn persist(map: &BTreeMap<u64, Download>) {
    let tmp = file().with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec_pretty(map)
        && std::fs::write(&tmp, bytes).is_ok()
    {
        let _ = std::fs::rename(&tmp, file());
    }
}

fn update(id: u64, save: bool, f: impl FnOnce(&mut Download)) {
    let mut map = DOWNLOADS.lock().unwrap();
    if let Some(d) = map.get_mut(&id) {
        f(d);
        if save {
            persist(&map);
        }
    }
}

pub fn get(id: u64) -> Option<Download> {
    DOWNLOADS.lock().unwrap().get(&id).cloned()
}

pub fn list() -> Vec<Download> {
    DOWNLOADS.lock().unwrap().values().cloned().collect()
}

pub fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// Starts downloading `source` to `path`. `size`: bytes expected, when known
/// (a download that ends at another size fails). A `.part` already there
/// (an earlier attempt, or a partial file the user brought) is continued.
pub fn add(source: Source, path: &Path, size: Option<u64>, owner: &str) -> anyhow::Result<u64> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| tr!("couldn't create {}", "não foi possível criar {}", dir.display()))?;
    }
    let part = part_path(path);
    let done = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let id = {
        let mut map = DOWNLOADS.lock().unwrap();
        let id = map.keys().next_back().map_or(1, |k| k + 1);
        map.insert(
            id,
            Download {
                id,
                source,
                path: path.display().to_string(),
                total: size.unwrap_or(0),
                done,
                state: State::Running,
                error: None,
                speed: 0.0,
                added: now(),
                finished: None,
                owner: owner.to_string(),
                segments: Vec::new(),
                connections: 0,
            },
        );
        persist(&map);
        id
    };
    spawn(id, size);
    Ok(id)
}

/// What was downloading when piShop closed carries on.
pub fn start() {
    let ids: Vec<(u64, u64)> =
        DOWNLOADS.lock().unwrap().values().filter(|d| d.state == State::Running).map(|d| (d.id, d.total)).collect();
    for (id, total) in ids {
        spawn(id, (total > 0).then_some(total));
    }
}

fn spawn(id: u64, size: Option<u64>) {
    let (tx, rx) = watch::channel(false);
    let run_id = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut running = RUNNING.lock().unwrap();
        if running.contains_key(&id) {
            log!("direto: {id} já estava em andamento");
            return;
        }
        running.insert(id, (run_id, tx));
    }
    tokio::spawn(async move {
        run(id, size, rx).await;
        let mut running = RUNNING.lock().unwrap();
        if running.get(&id).is_some_and(|(r, _)| *r == run_id) {
            running.remove(&id);
        }
    });
}

pub fn pause(id: u64) {
    if let Some((_, tx)) = RUNNING.lock().unwrap().get(&id) {
        let _ = tx.send(true);
    }
    update(id, true, |d| {
        if d.state == State::Running {
            d.state = State::Paused;
            d.speed = 0.0;
            d.connections = 0;
        }
    });
}

pub fn resume(id: u64) {
    let Some(d) = get(id) else { return };
    if d.state == State::Done || d.state == State::Running {
        return;
    }
    update(id, true, |d| {
        d.state = State::Running;
        d.error = None;
    });
    let size = (d.total > 0).then_some(d.total);
    // A run that was just paused may still be closing its file: start once it's gone.
    tokio::spawn(async move {
        for _ in 0..100 {
            if !RUNNING.lock().unwrap().contains_key(&id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if get(id).is_some_and(|d| d.state == State::Running) {
            spawn(id, size);
        }
    });
}

/// Off the list; `delete` also erases what was downloaded.
pub fn remove(id: u64, delete: bool) {
    if let Some((_, tx)) = RUNNING.lock().unwrap().remove(&id) {
        let _ = tx.send(true);
    }
    let gone = {
        let mut map = DOWNLOADS.lock().unwrap();
        let gone = map.remove(&id);
        persist(&map);
        gone
    };
    if let (Some(d), true) = (gone, delete) {
        let path = PathBuf::from(&d.path);
        // The writer may still hold the file for a moment.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = std::fs::remove_file(part_path(&path));
            let _ = std::fs::remove_file(&path);
        });
    }
}

// ---------- the transfer ----------

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // Nothing from the server for this long, headers included: the
        // request fails and is made again. Without it a server that took
        // the connection and went quiet (or Wi-Fi that dropped) held a
        // connection forever.
        .read_timeout(Duration::from_secs(30))
        .build()
        .expect("http client")
});

/// itch.io serves its pages to browsers.
const BROWSER: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";
/// No byte for this long: the connection is dead, try again.
const STALL: Duration = Duration::from_secs(60);
/// Attempts in a row that got nothing before the download fails.
const ATTEMPTS: u32 = 6;
/// Connections for a big file on a server that takes ranges.
const CONNECTIONS: usize = 4;
/// At most this many, counting the extra ones on a server much faster than the others.
const MAX_CONNECTIONS: usize = 6;
/// Connections on one server (more than two gained nothing on archive.org).
const PER_SERVER: usize = 2;
/// Smaller files go through one connection.
const MIN_PARALLEL: u64 = 32 << 20;
/// A slice is only split when both halves get at least this much.
const MIN_SPLIT: u64 = 8 << 20;

enum Outcome {
    Done,
    Stopped,
}

async fn run(id: u64, size: Option<u64>, mut stop: watch::Receiver<bool>) {
    let mut fails = 0;
    loop {
        let before = get(id).map(|d| d.done).unwrap_or(0);
        match attempt(id, size, &mut stop).await {
            Ok(Outcome::Stopped) => return,
            Ok(Outcome::Done) => {
                update(id, true, |d| {
                    d.state = State::Done;
                    d.speed = 0.0;
                    d.error = None;
                    d.finished = Some(now());
                });
                log!("direto: {id} concluído");
                return;
            }
            Err(e) => {
                if *stop.borrow() {
                    return;
                }
                let after = get(id).map(|d| d.done).unwrap_or(0);
                // Progress since the last failure: the server works, the link just dropped.
                fails = if after > before { 1 } else { fails + 1 };
                log!("direto: {id} tentativa falhou ({fails}/{ATTEMPTS}): {e:#}");
                if fails >= ATTEMPTS {
                    update(id, true, |d| {
                        d.state = State::Failed;
                        d.speed = 0.0;
                        d.error = Some(format!("{e:#}"));
                    });
                    return;
                }
                update(id, false, |d| {
                    d.speed = 0.0;
                    d.error = Some(format!("{e:#}"));
                });
                let wait = Duration::from_secs((2u64 << fails).min(60));
                tokio::select! {
                    _ = tokio::time::sleep(wait) => {}
                    _ = stop.changed() => return,
                }
            }
        }
    }
}

/// One request: from where the `.part` ends to the end of the file.
async fn attempt(id: u64, size: Option<u64>, stop: &mut watch::Receiver<bool>) -> anyhow::Result<Outcome> {
    let d = get(id).ok_or_else(|| anyhow!("removed"))?;
    if !d.segments.is_empty() {
        return parallel(id, size, stop, None).await;
    }
    let path = PathBuf::from(&d.path);
    let part = part_path(&path);
    let mut offset = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if let Some(s) = size
        && offset >= s
    {
        // Everything is there already (a partial file that was complete).
        return finish(id, &path, &part, size);
    }

    if *stop.borrow() {
        return Ok(Outcome::Stopped);
    }
    let mut resp = None;
    let mut last_err = None;
    let mut used = 0;
    for (ui, url) in resolve(&d.source).await?.into_iter().enumerate() {
        used = ui;
        // Always a range: a server that answers 206 can be fetched in slices.
        let req = HTTP.get(&url).header(USER_AGENT, BROWSER).header(RANGE, format!("bytes={offset}-"));
        match req.send().await {
            Ok(r) if r.status().is_success() => {
                resp = Some(r);
                break;
            }
            // Asked from the very end: nothing more to send.
            Ok(r) if r.status().as_u16() == 416 && offset > 0 => {
                let total = total_of(&r, 0);
                if total.is_none_or(|t| t == offset) {
                    return finish(id, &path, &part, size.or(total));
                }
                last_err = Some(anyhow!(tr!("the server didn't accept the resume ({})", "o servidor não aceitou continuar ({})", r.status())));
            }
            Ok(r) => last_err = Some(anyhow!(tr!("the server answered {}", "o servidor respondeu {}", r.status()))),
            Err(e) => last_err = Some(anyhow!(e)),
        }
    }
    let mut r = resp.ok_or_else(|| last_err.unwrap_or_else(|| anyhow!(tr!("no link to download from", "nenhum link para baixar"))))?;

    if offset > 0 && r.status().as_u16() != 206 {
        log!("direto: {id} o servidor não continua downloads; recomeçando");
        offset = 0;
    }
    if offset > 0 {
        // The range must start where the .part ends.
        let start = r.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("bytes ")).and_then(|v| v.split('-').next()).and_then(|v| v.parse::<u64>().ok());
        if start != Some(offset) {
            bail!(tr!("the server sent another part of the file", "o servidor mandou outra parte do arquivo"));
        }
    }
    let total = total_of(&r, offset).or(size).unwrap_or(0);
    if let (Some(s), true) = (size, total > 0) {
        if total != s {
            bail!(tr!(
                "the server's file has {total} bytes, {s} were expected",
                "o arquivo do servidor tem {total} bytes, eram esperados {s}"
            ));
        }
    }
    // A link with no fixed name (itch's signed URLs): the server names it.
    if d.owner.is_empty() && offset == 0 {
        rename_from_headers(id, &r);
    }
    let d = get(id).ok_or_else(|| anyhow!("removed"))?;
    let path = PathBuf::from(&d.path);
    let part = part_path(&path);
    // Paused (or removed) while the server was answering: nothing is written.
    if *stop.borrow() {
        return Ok(Outcome::Stopped);
    }
    // Big, and the server takes ranges: in slices, this answer being the first.
    if r.status().as_u16() == 206 && total > 0 && total.saturating_sub(offset) >= MIN_PARALLEL {
        let segs = slices(offset, total, CONNECTIONS);
        log!("direto: {id} em {} conexões a partir de {offset} de {total} bytes", segs.len());
        if offset == 0 {
            std::fs::File::create(&part)?;
        }
        update(id, true, |d| {
            d.total = total;
            d.done = offset;
            d.error = None;
            d.segments = segs;
        });
        return parallel(id, size, stop, Some((r, used))).await;
    }

    let mut out = tokio::fs::OpenOptions::new().create(true).write(true).truncate(offset == 0).append(offset > 0).open(&part).await?;
    let mut done = offset;
    update(id, true, |d| {
        d.total = total;
        d.done = done;
        d.error = None;
    });
    let mut window = (Instant::now(), done);
    let mut saved = Instant::now();
    loop {
        let chunk = tokio::select! {
            // A stop wins over data that's also ready.
            biased;
            _ = stop.changed() => {
                out.flush().await?;
                return Ok(Outcome::Stopped);
            }
            c = tokio::time::timeout(STALL, r.chunk()) => match c {
                Ok(c) => c?,
                Err(_) => bail!(tr!("the download stalled", "o download travou")),
            },
        };
        let Some(chunk) = chunk else { break };
        out.write_all(&chunk).await?;
        done += chunk.len() as u64;
        let elapsed = window.0.elapsed();
        if elapsed >= Duration::from_millis(1000) {
            let speed = (done - window.1) as f64 / elapsed.as_secs_f64();
            window = (Instant::now(), done);
            let save = saved.elapsed() > Duration::from_secs(15);
            if save {
                saved = Instant::now();
            }
            update(id, save, |d| {
                d.done = done;
                d.speed = if d.speed > 0.0 { d.speed * 0.4 + speed * 0.6 } else { speed };
            });
        }
    }
    out.flush().await?;
    drop(out);
    update(id, false, |d| d.done = done);
    if total > 0 && done < total {
        bail!(tr!("the connection closed early", "a conexão fechou antes do fim"));
    }
    finish(id, &path, &part, size.or((total > 0).then_some(total)))
}

/// The slices of a download, over several connections at once. `first`: an
/// answer already streaming from the first slice's start, and which of the
/// source's links gave it.
///
/// A source with mirrors (several links) spreads its connections across
/// them: each server throttles on its own, so their speeds add up. A
/// connection that finishes hands its place to a new one on the same server,
/// or on the fastest one when its own was much slower, so the fast servers
/// end up doing most of the work.
async fn parallel(id: u64, size: Option<u64>, stop: &mut watch::Receiver<bool>, first: Option<(reqwest::Response, usize)>) -> anyhow::Result<Outcome> {
    let d = get(id).ok_or_else(|| anyhow!("removed"))?;
    let path = PathBuf::from(&d.path);
    let part = part_path(&path);
    let total = d.total;
    if total == 0 {
        bail!(tr!("the file's size is unknown", "o tamanho do arquivo é desconhecido"));
    }
    // The file at its full size; each connection writes its own slice.
    {
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&part)?;
        if f.metadata()?.len() < total {
            f.set_len(total)?;
        }
    }
    let segs = Arc::new(Mutex::new(d.segments.clone()));
    let source = d.source.clone();
    // Links that stay valid (plain mirrors); itch.io and GitHub resolve each time.
    let mirrors: Vec<String> = match &source {
        Source::Http { urls } => urls.clone(),
        _ => Vec::new(),
    };
    let lanes = mirrors.len().max(1);
    let mut speeds = vec![0f64; lanes];
    let left = |segs: &Arc<Mutex<Vec<Segment>>>| segs.lock().unwrap().iter().map(|s| s.left()).sum::<u64>();
    type Joined = (usize, usize, f64, anyhow::Result<()>);
    let mut workers: tokio::task::JoinSet<Joined> = tokio::task::JoinSet::new();
    let mut owned: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // Which server each running slice is on, and where it was at the last check.
    let mut on_lane: HashMap<usize, usize> = HashMap::new();
    let mut checked: HashMap<usize, u64> = HashMap::new();
    let mut last_check = Instant::now();
    let first_start = segs.lock().unwrap().first().map(|s| s.pos());
    let (mut first, first_lane) = match first {
        Some((r, lane)) => (Some(r), lane.min(lanes - 1)),
        None => (None, 0),
    };
    let worker_stop = stop.clone();
    let spawn = |workers: &mut tokio::task::JoinSet<Joined>, i: usize, lane: usize, resp: Option<reqwest::Response>| {
        let (part, segs, source, stop) = (part.clone(), segs.clone(), source.clone(), worker_stop.clone());
        let link = resp.as_ref().map(|r| r.url().to_string());
        let mirror = mirrors.get(lane).cloned();
        if let Some(m) = &mirror {
            log!("direto: {id} fatia {i} ← {}", m.split('/').nth(2).unwrap_or(m));
        }
        workers.spawn(async move {
            let (speed, r) = slice_worker(i, &part, &segs, link, mirror, &source, resp, stop).await;
            (i, lane, speed, r)
        });
    };
    let pending: Vec<usize> =
        segs.lock().unwrap().iter().enumerate().filter(|(_, s)| s.left() > 0).map(|(i, _)| i).take(CONNECTIONS).collect();
    for (k, i) in pending.into_iter().enumerate() {
        let lane = if i == 0 && first_start.is_some() && first.is_some() { first_lane } else { k % lanes };
        let resp = if lane == first_lane && i == 0 { first.take() } else { None };
        spawn(&mut workers, i, lane, resp);
        owned.insert(i);
        on_lane.insert(i, lane);
    }
    drop(first);

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut window = (Instant::now(), total - left(&segs));
    let mut saved = Instant::now();
    let mut last_err: Option<anyhow::Error> = None;
    let mut done_at_err = total - left(&segs);
    let mut stopped = false;
    loop {
        tokio::select! {
            biased;
            _ = stop.changed(), if !stopped => {
                // Each connection sees it too and closes its slice.
                stopped = true;
            }
            joined = workers.join_next(), if !workers.is_empty() => {
                let Some(joined) = joined else { continue };
                let (i, lane, speed, result) = joined.unwrap_or_else(|e| (usize::MAX, 0, 0.0, Err(anyhow!(e))));
                owned.remove(&i);
                on_lane.remove(&i);
                if speed > 0.0 {
                    speeds[lane] = if speeds[lane] > 0.0 { speeds[lane] * 0.5 + speed * 0.5 } else { speed };
                }
                if let Err(e) = result {
                    log!("direto: {id} conexão da fatia {i}: {e:#}");
                    last_err = Some(e);
                    // Its server counts as the slowest now, and its place is
                    // taken again while the download as a whole still gets
                    // somewhere; when nothing does, the connections run out
                    // and the download retries from the top.
                    speeds[lane] = 0.0;
                    let done = total - left(&segs);
                    let moving = done > done_at_err;
                    done_at_err = done;
                    if !moving {
                        continue;
                    }
                }
                if stopped || *stop.borrow() {
                    continue;
                }
                // The free place goes to the same server, or to the fastest
                // one when this one ran at under a quarter of its speed.
                let (best, best_speed) = speeds.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((lane, 0.0));
                let lane = if speeds[lane] < best_speed / 4.0 { best } else { lane };
                // The free connection takes a slice nobody has, or half of the biggest one.
                while workers.len() < CONNECTIONS {
                    let next = {
                        let mut s = segs.lock().unwrap();
                        let free = (0..s.len()).find(|j| !owned.contains(j) && s[*j].left() > 0);
                        free.or_else(|| steal(&mut s))
                    };
                    let Some(j) = next else { break };
                    spawn(&mut workers, j, lane, None);
                    owned.insert(j);
                    on_lane.insert(j, lane);
                }
            }
            _ = tick.tick() => {
                // Every 30 s: a server much faster per connection than the
                // others gets one more connection, on half of the biggest slice.
                if last_check.elapsed() >= Duration::from_secs(30) && !stopped && lanes > 1 {
                    let elapsed = last_check.elapsed().as_secs_f64();
                    last_check = Instant::now();
                    let pos: HashMap<usize, u64> = { let s = segs.lock().unwrap(); on_lane.keys().map(|i| (*i, s[*i].pos())).collect() };
                    let mut lane_bytes = vec![0u64; lanes];
                    let mut lane_conns = vec![0usize; lanes];
                    for (i, lane) in &on_lane {
                        lane_conns[*lane] += 1;
                        if let (Some(now), Some(before)) = (pos.get(i), checked.get(i)) {
                            lane_bytes[*lane] += now.saturating_sub(*before);
                        }
                    }
                    checked = pos;
                    let per_conn: Vec<f64> = (0..lanes).map(|l| if lane_conns[l] > 0 { lane_bytes[l] as f64 / lane_conns[l] as f64 / elapsed } else { 0.0 }).collect();
                    for l in 0..lanes {
                        if per_conn[l] > 0.0 {
                            speeds[l] = per_conn[l];
                        }
                    }
                    let best = (0..lanes).max_by(|a, b| per_conn[*a].total_cmp(&per_conn[*b])).unwrap_or(0);
                    let others = (0..lanes).filter(|l| *l != best && lane_conns[*l] > 0).map(|l| per_conn[l]).fold(0f64, f64::max);
                    if per_conn[best] > 0.0 && per_conn[best] >= 2.0 * others && lane_conns[best] < PER_SERVER && workers.len() < MAX_CONNECTIONS {
                        let j = steal(&mut segs.lock().unwrap());
                        if let Some(j) = j {
                            log!("direto: {id} mais uma conexão no servidor mais rápido ({} KB/s por conexão)", (per_conn[best] / 1024.0) as u64);
                            spawn(&mut workers, j, best, None);
                            owned.insert(j);
                            on_lane.insert(j, best);
                        }
                    }
                }
                let done = total - left(&segs);
                let elapsed = window.0.elapsed().as_secs_f64().max(0.001);
                let speed = done.saturating_sub(window.1) as f64 / elapsed;
                window = (Instant::now(), done);
                let save = saved.elapsed() > Duration::from_secs(10);
                if save {
                    saved = Instant::now();
                }
                let snapshot = segs.lock().unwrap().clone();
                let n = workers.len();
                update(id, save, |d| {
                    d.done = done;
                    d.segments = snapshot;
                    d.connections = n;
                    d.speed = if d.speed > 0.0 { d.speed * 0.5 + speed * 0.5 } else { speed };
                    d.error = None;
                });
            }
        }
        if workers.is_empty() {
            break;
        }
    }
    let snapshot = segs.lock().unwrap().clone();
    let remaining: u64 = snapshot.iter().map(|s| s.left()).sum();
    update(id, true, |d| {
        d.done = total - remaining;
        d.segments = snapshot;
        d.connections = 0;
    });
    if stopped || *stop.borrow() {
        return Ok(Outcome::Stopped);
    }
    if remaining > 0 {
        return Err(last_err.unwrap_or_else(|| anyhow!(tr!("the connections closed early", "as conexões fecharam antes do fim"))));
    }
    update(id, true, |d| d.segments.clear());
    finish(id, &path, &part, size.or(Some(total)))
}

/// One connection: fetches its slice (retrying on its own while it gets
/// somewhere), writing at the slice's place in the file. Stops at the slice's
/// end even when another connection took the slice's second half meanwhile.
/// Returns how fast it went (bytes per second).
#[allow(clippy::too_many_arguments)]
async fn slice_worker(
    i: usize,
    part: &Path,
    segs: &Arc<Mutex<Vec<Segment>>>,
    mut link: Option<String>,
    mirror: Option<String>,
    source: &Source,
    mut first: Option<reqwest::Response>,
    mut stop: watch::Receiver<bool>,
) -> (f64, anyhow::Result<()>) {
    let started = Instant::now();
    let from = segs.lock().unwrap()[i].pos();
    let speed = |segs: &Arc<Mutex<Vec<Segment>>>| {
        let got = segs.lock().unwrap()[i].pos().saturating_sub(from);
        got as f64 / started.elapsed().as_secs_f64().max(0.001)
    };
    let mut fails = 0u32;
    loop {
        let (pos, end) = {
            let s = segs.lock().unwrap();
            (s[i].pos(), s[i].end)
        };
        if pos >= end || *stop.borrow() {
            return (speed(segs), Ok(()));
        }
        match fetch_slice(i, pos, end, part, segs, &mut link, mirror.as_deref(), source, first.take(), &mut stop).await {
            Ok(()) => fails = 0,
            Err(e) => {
                let moved = segs.lock().unwrap()[i].pos() > pos;
                fails = if moved { 1 } else { fails + 1 };
                if fails >= 4 {
                    return (speed(segs), Err(e));
                }
                // The link may have expired or moved: ask again.
                link = None;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2u64 << fails)) => {}
                    _ = stop.changed() => return (speed(segs), Ok(())),
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_slice(
    i: usize,
    from: u64,
    end: u64,
    part: &Path,
    segs: &Arc<Mutex<Vec<Segment>>>,
    link: &mut Option<String>,
    mirror: Option<&str>,
    source: &Source,
    first: Option<reqwest::Response>,
    stop: &mut watch::Receiver<bool>,
) -> anyhow::Result<()> {
    use tokio::io::AsyncSeekExt;
    let mut r = match first {
        Some(r) => r,
        None => tokio::select! {
            biased;
            _ = stop.changed() => return Ok(()),
            r = range_request(from, end, link, mirror, source) => r?,
        },
    };
    if *stop.borrow() {
        return Ok(());
    }
    let mut f = tokio::fs::OpenOptions::new().write(true).open(part).await?;
    f.seek(std::io::SeekFrom::Start(from)).await?;
    let mut pos = from;
    loop {
        let chunk = tokio::select! {
            biased;
            _ = stop.changed() => break,
            c = tokio::time::timeout(STALL, r.chunk()) => match c {
                Ok(c) => c?,
                Err(_) => bail!(tr!("the download stalled", "o download travou")),
            },
        };
        let Some(chunk) = chunk else { break };
        // Another connection may have taken this slice's second half.
        let end = segs.lock().unwrap()[i].end;
        let take = (end.saturating_sub(pos) as usize).min(chunk.len());
        f.write_all(&chunk[..take]).await?;
        pos += take as u64;
        {
            let mut s = segs.lock().unwrap();
            let seg = &mut s[i];
            seg.done = (pos - seg.start).min(seg.end.saturating_sub(seg.start));
        }
        if pos >= end {
            break;
        }
    }
    f.flush().await?;
    Ok(())
}

/// `from..end` of the file: this connection's last link (redirects already
/// followed), else its mirror, else the source's links in order. Only a 206
/// starting at `from` is taken.
async fn range_request(from: u64, end: u64, link: &mut Option<String>, mirror: Option<&str>, source: &Source) -> anyhow::Result<reqwest::Response> {
    let mut last: Option<anyhow::Error> = None;
    for round in 0..3 {
        let urls = match round {
            0 => link.clone().into_iter().collect(),
            1 => mirror.map(String::from).into_iter().collect(),
            _ => resolve(source).await?,
        };
        for u in urls {
            let r = HTTP.get(&u).header(USER_AGENT, BROWSER).header(RANGE, format!("bytes={from}-{}", end - 1)).send().await;
            match r {
                Ok(r) if r.status().as_u16() == 206 => {
                    let start = r.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("bytes ")).and_then(|v| v.split('-').next()).and_then(|v| v.parse::<u64>().ok());
                    if start != Some(from) {
                        last = Some(anyhow!(tr!("the server sent another part of the file", "o servidor mandou outra parte do arquivo")));
                        continue;
                    }
                    *link = Some(r.url().to_string());
                    return Ok(r);
                }
                Ok(r) => last = Some(anyhow!(tr!("the server answered {}", "o servidor respondeu {}", r.status()))),
                Err(e) => last = Some(anyhow!(e)),
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!(tr!("no link to download from", "nenhum link para baixar"))))
}

fn finish(id: u64, path: &Path, part: &Path, size: Option<u64>) -> anyhow::Result<Outcome> {
    let len = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if let Some(s) = size
        && len != s
    {
        bail!(tr!("the file has {len} bytes, {s} were expected", "o arquivo tem {len} bytes, eram esperados {s}"));
    }
    std::fs::rename(part, path)?;
    update(id, false, |d| {
        d.done = len;
        d.total = len;
    });
    Ok(Outcome::Done)
}

/// "bytes 0-99/1234" → 1234; else the length of what's coming plus the offset.
fn total_of(r: &reqwest::Response, offset: u64) -> Option<u64> {
    if let Some(total) = r.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit('/').next()).and_then(|t| t.parse().ok()) {
        return Some(total);
    }
    r.content_length().filter(|n| *n > 0).map(|n| n + offset)
}

/// A link added by hand gets the name the server gives it.
fn rename_from_headers(id: u64, r: &reqwest::Response) {
    let name = r
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').find_map(|p| p.trim().strip_prefix("filename=")))
        .map(|n| n.trim_matches('"').to_string())
        .map(|n| safe_name(&n))
        .filter(|n| !n.is_empty());
    if let Some(n) = name {
        update(id, true, |d| {
            let p = PathBuf::from(&d.path);
            d.path = p.with_file_name(n).display().to_string();
        });
    }
}

/// A file name with no path in it.
pub fn safe_name(n: &str) -> String {
    let n = n.rsplit(['/', '\\']).next().unwrap_or(n);
    let n: String = n.chars().filter(|c| !c.is_control()).collect();
    let n = n.trim().trim_start_matches('.').to_string();
    n.chars().take(200).collect()
}

/// The URLs to try for a source, in order.
pub async fn resolve(s: &Source) -> anyhow::Result<Vec<String>> {
    match s {
        Source::Http { urls } => Ok(urls.clone()),
        Source::Itch { page, upload_id, file } => Ok(vec![itch_url(page, *upload_id, file.as_deref()).await?]),
        Source::Github { repo, tag, asset } => Ok(vec![github_asset(repo, tag, asset).await?.0]),
    }
}

// ---------- itch.io ----------

/// The `name=value` pairs of a response's cookies, as a Cookie header.
fn cookies(r: &reqwest::Response, jar: &mut Vec<String>) {
    for v in r.headers().get_all(SET_COOKIE) {
        if let Some(pair) = v.to_str().ok().and_then(|v| v.split(';').next()) {
            let name = pair.split('=').next().unwrap_or_default();
            jar.retain(|c| c.split('=').next() != Some(name));
            jar.push(pair.trim().to_string());
        }
    }
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)?;
    Some(&s[i..i + j])
}

fn unescape_html(s: &str) -> String {
    s.replace("&amp;", "&").replace("&quot;", "\"").replace("&#039;", "'").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">")
}

/// The uploads listed on an itch.io page: (upload id, file name).
fn uploads(html: &str) -> Vec<(u64, String)> {
    html.split("data-upload_id=\"")
        .skip(1)
        .filter_map(|chunk| {
            let id = chunk.split('"').next()?.parse().ok()?;
            let name = between(chunk, "class=\"name\" title=\"", "\"").map(unescape_html).unwrap_or_default();
            Some((id, name))
        })
        .collect()
}

fn pick_upload(list: &[(u64, String)], file: Option<&str>) -> Option<u64> {
    match file {
        Some(f) => list.iter().find(|(_, n)| n.eq_ignore_ascii_case(f)).map(|(id, _)| *id),
        None => list.first().map(|(id, _)| *id),
    }
}

/// A signed download URL (valid for a minute) for an upload of a free or
/// pay-what-you-want itch.io game, the way the page's own button gets it.
/// A pinned upload id that stopped working (the developer uploaded a new
/// build) falls back to the upload with the same file name.
async fn itch_url(page: &str, upload_id: Option<u64>, file: Option<&str>) -> anyhow::Result<String> {
    let page = page.trim().trim_end_matches('/');
    if !page.starts_with("https://") || !page.contains(".itch.io/") {
        bail!(tr!("not an itch.io game page: {page}", "não é uma página de jogo do itch.io: {page}"));
    }
    let mut jar = Vec::new();
    let r = HTTP.get(page).header(USER_AGENT, BROWSER).send().await?.error_for_status()?;
    cookies(&r, &mut jar);
    let html = r.text().await?;
    let csrf = between(&html, "name=\"csrf_token\" value=\"", "\"").ok_or_else(|| anyhow!(tr!("itch.io changed its page", "o itch.io mudou a página")))?.to_string();
    if let Some(u) = upload_id.or_else(|| pick_upload(&uploads(&html), file)) {
        match itch_file(page, u, &csrf, &jar).await {
            Ok(url) => return Ok(url),
            Err(e) if file.is_none() => return Err(e),
            Err(e) => log!("direto: itch.io upload {u} falhou ({e:#}); procurando {file:?}"),
        }
    }
    // Downloads behind "Download Now": the download page lists them.
    let r = HTTP.post(format!("{page}/download_url")).header(USER_AGENT, BROWSER).header(COOKIE, jar.join("; ")).form(&[("csrf_token", &csrf)]).send().await?;
    cookies(&r, &mut jar);
    let v: Value = r.json().await?;
    let url = v["url"].as_str().ok_or_else(|| anyhow!(tr!("this itch.io game isn't free to download", "este jogo do itch.io não é gratuito para baixar")))?;
    let r = HTTP.get(url).header(USER_AGENT, BROWSER).header(COOKIE, jar.join("; ")).send().await?.error_for_status()?;
    cookies(&r, &mut jar);
    let list = uploads(&r.text().await?);
    let upload = pick_upload(&list, file).ok_or_else(|| anyhow!(tr!("the file isn't on the itch.io page anymore", "o arquivo não está mais na página do itch.io")))?;
    itch_file(page, upload, &csrf, &jar).await
}

async fn itch_file(page: &str, upload: u64, csrf: &str, jar: &[String]) -> anyhow::Result<String> {
    let r = HTTP
        .post(format!("{page}/file/{upload}?source=view_game&as_props=1&after_download_lightbox=true"))
        .header(USER_AGENT, BROWSER)
        .header(COOKIE, jar.join("; "))
        .form(&[("csrf_token", csrf)])
        .send()
        .await?;
    let v: Value = r.json().await.context(tr!("itch.io didn't answer", "o itch.io não respondeu"))?;
    match v["url"].as_str() {
        Some(u) => Ok(u.to_string()),
        None => bail!(tr!("itch.io refused the download: {}", "o itch.io recusou o download: {}", v["errors"])),
    }
}

// ---------- GitHub releases ----------

/// A GitHub release's asset, and what the release is called.
pub struct Release {
    pub tag: String,
    /// What people call it: the tag when it reads like a version ("1.2.0"),
    /// else the release's name ("v0.2.34" for a rolling "Release" tag).
    pub version: String,
    pub url: String,
    pub size: u64,
    pub name: String,
    /// GitHub's SHA-256 of the asset, when it has one.
    pub sha256: Option<String>,
}

fn asset_pattern(pattern: &str) -> anyhow::Result<regex::Regex> {
    regex::RegexBuilder::new(pattern).case_insensitive(true).build().with_context(|| tr!("bad file pattern {pattern}", "padrão de arquivo inválido {pattern}"))
}

async fn github(path: &str) -> anyhow::Result<Value> {
    Ok(HTTP
        .get(format!("https://api.github.com/repos/{path}"))
        .header(USER_AGENT, "piShop")
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// What a release is called: a tag with digits, else the release's name,
/// else a version in the asset's name, else the tag.
pub fn version_label(tag: &str, name: &str, asset: &str) -> String {
    let digits = |s: &str| s.chars().any(|c| c.is_ascii_digit());
    if digits(tag) {
        return tag.trim().to_string();
    }
    if digits(name) {
        return name.trim().to_string();
    }
    static VERSION: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\d+(?:\.\d+)+").unwrap());
    VERSION.find(asset).map(|m| m.as_str().to_string()).unwrap_or_else(|| tag.trim().to_string())
}

fn release_of(r: &Value, re: &regex::Regex) -> Option<Release> {
    let a = r["assets"].as_array()?.iter().find(|a| a["name"].as_str().is_some_and(|n| re.is_match(n)))?;
    let tag = r["tag_name"].as_str().unwrap_or_default().to_string();
    let name = a["name"].as_str()?.to_string();
    Some(Release {
        version: version_label(&tag, r["name"].as_str().unwrap_or_default(), &name),
        tag,
        url: a["browser_download_url"].as_str()?.to_string(),
        size: a["size"].as_u64().unwrap_or(0),
        sha256: a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase),
        name,
    })
}

/// The asset matching `pattern` in the release tagged `tag` ("latest": the newest).
pub async fn github_release(repo: &str, tag: &str, pattern: &str) -> anyhow::Result<Release> {
    let re = asset_pattern(pattern)?;
    let path = if tag.is_empty() || tag == "latest" { format!("{repo}/releases/latest") } else { format!("{repo}/releases/tags/{tag}") };
    let v = github(&path).await.with_context(|| tr!("GitHub: release {tag} of {repo} not found", "GitHub: versão {tag} de {repo} não encontrada"))?;
    release_of(&v, &re).ok_or_else(|| anyhow!(tr!("no file matching {pattern} in {repo}'s release", "nenhum arquivo como {pattern} na versão de {repo}")))
}

/// The newest stable release that has an asset matching `pattern`: what a
/// community part marked updatable can move to.
pub async fn github_latest(repo: &str, pattern: &str) -> anyhow::Result<Release> {
    let re = asset_pattern(pattern)?;
    let v = github(&format!("{repo}/releases?per_page=15")).await.with_context(|| tr!("GitHub: {repo}'s releases", "GitHub: versões de {repo}"))?;
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false) && !r["prerelease"].as_bool().unwrap_or(false))
        .find_map(|r| release_of(r, &re))
        .ok_or_else(|| anyhow!(tr!("no release of {repo} has a file matching {pattern}", "nenhuma versão de {repo} tem um arquivo como {pattern}")))
}

/// The download URL and size of a release asset whose name matches `pattern`.
pub async fn github_asset(repo: &str, tag: &str, pattern: &str) -> anyhow::Result<(String, u64, String)> {
    let r = github_release(repo, tag, pattern).await?;
    Ok((r.url, r.size, r.name))
}

// ---------- itch.io versions ----------

/// The version itch.io shows next to an upload on the game's page ("Version
/// 0.9.7", for uploads pushed with butler), found by the file's name.
pub async fn itch_version(page: &str, file: &str) -> anyhow::Result<Option<String>> {
    let html = HTTP.get(page.trim()).header(USER_AGENT, BROWSER).timeout(Duration::from_secs(20)).send().await?.error_for_status()?.text().await?;
    Ok(itch_version_in(&html, file))
}

fn itch_version_in(html: &str, file: &str) -> Option<String> {
    let at = html.find(&format!("title=\"{file}\""))?;
    let rest = &html[at..];
    // Only inside this upload's block.
    let block = &rest[..rest.find("class=\"upload\"").unwrap_or(rest.len())];
    let text = between(block, "class=\"version_name\">", "<")?;
    Some(text.trim().trim_start_matches("Version").trim().to_string()).filter(|v| !v.is_empty())
}

/// The exact size of an itch.io upload now (what tells a new build apart
/// when the page shows no version): the first byte of it, asked for.
pub async fn itch_size(page: &str, upload_id: Option<u64>, file: &str) -> anyhow::Result<u64> {
    let url = itch_url(page, upload_id, Some(file)).await?;
    let r = HTTP.get(&url).header(USER_AGENT, BROWSER).header(RANGE, "bytes=0-0").timeout(Duration::from_secs(20)).send().await?.error_for_status()?;
    total_of(&r, 0).ok_or_else(|| anyhow!(tr!("itch.io didn't say the file's size", "o itch.io não informou o tamanho do arquivo")))
}

// ---------- routes ----------

pub fn router() -> Router {
    Router::new()
        .route("/api/direct", axum::routing::get(|| async { Json(list()) }).post(add_route))
        .route("/api/direct/{id}/{action}", post(action_route))
}

#[derive(Deserialize)]
struct AddReq {
    url: String,
    #[serde(default)]
    dest: Option<String>,
}

async fn add_route(Json(r): Json<AddReq>) -> axum::response::Response {
    reply(add_link(&r.url, r.dest.as_deref()))
}

/// Transfers → Add link: a web link of its own, into Downloads (or `dest`).
fn add_link(url: &str, dest: Option<&str>) -> anyhow::Result<Value> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        bail!(tr!("a link starts with https://", "um link começa com https://"));
    }
    let dir = dest.filter(|d| !d.trim().is_empty()).map(PathBuf::from).unwrap_or_else(crate::torrent::download_dir);
    let guess = url.split(['?', '#']).next().unwrap_or(url).rsplit('/').next().unwrap_or_default();
    let guess = percent_decode(guess);
    let name = safe_name(&guess);
    let name = if name.is_empty() { "download".to_string() } else { name };
    let id = add(Source::Http { urls: vec![url.to_string()] }, &dir.join(name), None, "")?;
    log!("direto: link adicionado ({id}) {url}");
    Ok(json!({ "id": id }))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn action_route(UrlPath((id, action)): UrlPath<(u64, String)>) -> axum::response::Response {
    let r = match action.as_str() {
        "pause" => Ok(pause(id)),
        "resume" => Ok(resume(id)),
        "remove" => Ok(remove(id, false)),
        "delete" => Ok(remove(id, true)),
        _ => Err(anyhow!(tr!("unknown action", "ação desconhecida"))),
    };
    reply(r.map(|_| json!({})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_from_releases_and_itch_pages() {
        assert_eq!(version_label("1.0.0", "SpaghettiKart Alfa", "spaghetti-linux.zip"), "1.0.0");
        assert_eq!(version_label("Release", "v0.2.34", "FH2_Extra_Options_0.2.34-release.zip"), "v0.2.34");
        assert_eq!(version_label("Release", "", "FH2_Extra_Options_0.2.34-release.zip"), "0.2.34");
        let page = r#"<div class="upload"><div class="info_column"><div class="upload_name"><strong title="azimuth-windows.zip" class="name">azimuth-windows.zip</strong> <span class="file_size"><span>2.6 MB</span></span></div><div class="build_row"><span class="version_name">Version 0.9.7</span></div></div></div><div class="upload"><strong title="other.zip" class="name">other.zip</strong></div>"#;
        assert_eq!(itch_version_in(page, "azimuth-windows.zip").as_deref(), Some("0.9.7"));
        assert_eq!(itch_version_in(page, "other.zip"), None);
        assert_eq!(itch_version_in(page, "missing.zip"), None);
    }


    #[test]
    fn itch_uploads_from_a_download_page() {
        let html = r#"<div class="upload"><a data-upload_id="19662858" class="button download_btn">Download</a>
            <div class="upload_name"><strong class="name" title="azimuth-windows.zip">azimuth-windows.zip</strong></div></div>
            <div class="upload"><a data-upload_id="42" class="button">Download</a><strong class="name" title="Tom &amp; Jerry.zip">x</strong></div>"#;
        let list = uploads(html);
        assert_eq!(list, vec![(19662858, "azimuth-windows.zip".to_string()), (42, "Tom & Jerry.zip".to_string())]);
        assert_eq!(pick_upload(&list, Some("AZIMUTH-windows.zip")), Some(19662858));
        assert_eq!(pick_upload(&list, None), Some(19662858));
        assert_eq!(pick_upload(&list, Some("other.zip")), None);
    }

    #[test]
    fn slices_and_stealing() {
        let s = slices(100, 1100, 4);
        assert_eq!(s.iter().map(|x| (x.start, x.end)).collect::<Vec<_>>(), [(100, 350), (350, 600), (600, 850), (850, 1100)]);
        assert_eq!(s.iter().map(|x| x.left()).sum::<u64>(), 1000);
        // Small slices aren't split.
        let mut small = s.clone();
        assert_eq!(steal(&mut small), None);
        // The one with most left gives its second half away.
        let mut big = slices(0, 400 << 20, 2);
        big[0].done = 150 << 20;
        let j = steal(&mut big).unwrap();
        assert_eq!(j, 2);
        assert_eq!(big[1].end, big[2].start);
        assert_eq!(big[2].end, 400 << 20);
        assert_eq!(big.iter().map(|x| x.left()).sum::<u64>(), (400 << 20) - (150 << 20));
    }

    #[test]
    fn names_from_links() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name(".hidden"), "hidden");
        assert_eq!(percent_decode("%28Forza%20Horizon%202%29%20Anthem.rar"), "(Forza Horizon 2) Anthem.rar");
        assert_eq!(part_path(Path::new("/a/b.rar")), PathBuf::from("/a/b.rar.part"));
    }
}
