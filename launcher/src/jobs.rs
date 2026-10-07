//! Copy queue: jobs run one at a time (FIFO). Sources are network shares
//! (each file fetched with several concurrent SMB reads) or this device's own
//! storage (`source_id: "local"`, absolute paths), e.g. a finished download.
//! Jobs persist in `<data>/jobs.json`, and an interrupted job resumes on the
//! next launch, skipping files already copied.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};
use smb::ReadAt;
use tokio::sync::Notify;

use crate::{localfs, log, smbfs, sources};
use crate::tr;

const CHUNK: u64 = 1 << 20;
const READERS: usize = 8;
const FILE_ATTEMPTS: usize = 3;
const SCAN_CONCURRENCY: usize = 8;
/// Local copies: read/write buffer.
const LOCAL_CHUNK: usize = 4 << 20;

/// `source_id` of copies from this device's own storage.
pub const LOCAL: &str = "local";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Scanning,
    Running,
    Done,
    Failed,
    Canceled,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub source_id: String,
    pub source_name: String,
    /// Remote path relative to the share root ("/" separated), or an absolute
    /// path for local copies.
    pub src_path: String,
    pub name: String,
    pub dir: bool,
    /// Local folder the item is copied into (the item keeps its name).
    pub dest_dir: String,
    pub status: Status,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub files_total: u64,
    pub files_done: u64,
    pub current: String,
    /// Bytes per second, smoothed.
    pub speed: f64,
    pub error: Option<String>,
    pub created: i64,
    pub finished: Option<i64>,
}

#[derive(Deserialize)]
pub struct NewItem {
    pub path: String,
    pub name: String,
    pub dir: bool,
}

struct State {
    jobs: Vec<Job>,
    cancel: HashMap<u64, Arc<AtomicBool>>,
    next_id: u64,
}

static STATE: LazyLock<Mutex<State>> =
    LazyLock::new(|| Mutex::new(State { jobs: Vec::new(), cancel: HashMap::new(), next_id: 1 }));
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn file() -> PathBuf {
    crate::data_dir().join("jobs.json")
}

fn persist(st: &State) {
    let tmp = file().with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(&st.jobs) {
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, file());
        }
    }
}

/// Loads the saved queue (interrupted jobs go back to queued) and starts the worker.
pub fn start() {
    let mut jobs: Vec<Job> = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    for j in &mut jobs {
        if matches!(j.status, Status::Running | Status::Scanning) {
            j.status = Status::Queued;
            j.speed = 0.0;
        }
    }
    {
        let mut st = STATE.lock().unwrap();
        st.next_id = jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        st.jobs = jobs;
    }
    tokio::spawn(worker());
}

pub fn list() -> Vec<Job> {
    STATE.lock().unwrap().jobs.clone()
}

/// What a job copies, resolved and checked before it's queued.
struct Prepared {
    source_id: String,
    source_name: String,
    src_path: String,
    name: String,
    dir: bool,
}

pub fn enqueue(source_id: &str, dest_dir: &str, items: Vec<NewItem>) -> anyhow::Result<Vec<u64>> {
    if !Path::new(dest_dir).is_dir() {
        bail!(tr!("the destination folder doesn't exist", "a pasta de destino não existe"));
    }
    let prepared: Vec<Prepared> = if source_id == LOCAL {
        items.into_iter().map(|item| prepare_local(&item, Path::new(dest_dir))).collect::<anyhow::Result<_>>()?
    } else {
        let src = sources::get(source_id).ok_or_else(|| anyhow!(tr!("source not found", "fonte não encontrada")))?;
        items
            .into_iter()
            .map(|item| Prepared {
                source_id: src.id.clone(),
                source_name: src.name.clone(),
                src_path: item.path.trim_matches('/').to_string(),
                name: item.name,
                dir: item.dir,
            })
            .collect()
    };
    let mut st = STATE.lock().unwrap();
    let mut ids = Vec::new();
    for item in prepared {
        let id = st.next_id;
        st.next_id += 1;
        st.jobs.push(Job {
            id,
            source_id: item.source_id,
            source_name: item.source_name,
            src_path: item.src_path,
            name: item.name,
            dir: item.dir,
            dest_dir: dest_dir.to_string(),
            status: Status::Queued,
            total_bytes: 0,
            done_bytes: 0,
            files_total: 0,
            files_done: 0,
            current: String::new(),
            speed: 0.0,
            error: None,
            created: now(),
            finished: None,
        });
        ids.push(id);
    }
    persist(&st);
    drop(st);
    WAKE.notify_one();
    Ok(ids)
}

/// A local item: must exist, can't land inside itself or onto itself. The
/// name comes from the path, so it can't point outside the destination.
fn prepare_local(item: &NewItem, dest: &Path) -> anyhow::Result<Prepared> {
    let path = PathBuf::from(&item.path);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !path.is_absolute() || name.is_empty() {
        bail!(tr!("invalid path", "caminho inválido"));
    }
    let meta = std::fs::metadata(&path).map_err(|_| anyhow!(tr!("not found: {}", "não encontrado: {}", item.path)))?;
    let (src, dst) = (path.canonicalize()?, dest.canonicalize()?);
    if meta.is_dir() && dst.starts_with(&src) {
        bail!(tr!("can't copy a folder into itself", "não é possível copiar uma pasta para dentro dela mesma"));
    }
    if src.parent() == Some(dst.as_path()) {
        bail!(tr!("\"{}\" is already in this folder", "\"{}\" já está nesta pasta", name));
    }
    let home = localfs::home();
    let parent = path.parent().unwrap_or(Path::new("/"));
    let shown = match parent.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => parent.display().to_string(),
    };
    Ok(Prepared { source_id: LOCAL.into(), source_name: shown, src_path: path.display().to_string(), name, dir: meta.is_dir() })
}

pub fn cancel(id: u64) {
    let mut st = STATE.lock().unwrap();
    if let Some(flag) = st.cancel.get(&id) {
        flag.store(true, Ordering::SeqCst);
    } else if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id && j.status == Status::Queued) {
        j.status = Status::Canceled;
        j.finished = Some(now());
        persist(&st);
    }
}

pub fn retry(id: u64) {
    let mut st = STATE.lock().unwrap();
    if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id && matches!(j.status, Status::Failed | Status::Canceled)) {
        j.status = Status::Queued;
        j.error = None;
        j.finished = None;
        j.speed = 0.0;
        persist(&st);
        drop(st);
        WAKE.notify_one();
    }
}

/// Removes one finished job, or every finished job when `id` is None.
pub fn remove(id: Option<u64>) {
    let mut st = STATE.lock().unwrap();
    st.jobs.retain(|j| {
        let finished = matches!(j.status, Status::Done | Status::Failed | Status::Canceled);
        !(finished && id.is_none_or(|id| id == j.id))
    });
    persist(&st);
}

fn update(id: u64, f: impl FnOnce(&mut Job)) {
    let mut st = STATE.lock().unwrap();
    if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
        f(j);
    }
}

fn update_persist(id: u64, f: impl FnOnce(&mut Job)) {
    let mut st = STATE.lock().unwrap();
    if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
        f(j);
    }
    persist(&st);
}

async fn worker() {
    loop {
        let next = {
            let mut st = STATE.lock().unwrap();
            let job = st.jobs.iter().find(|j| j.status == Status::Queued).cloned();
            if let Some(j) = &job {
                st.cancel.insert(j.id, Arc::new(AtomicBool::new(false)));
            }
            job.map(|j| (j.clone(), st.cancel[&j.id].clone()))
        };
        let Some((job, cancel)) = next else {
            WAKE.notified().await;
            continue;
        };
        log!("job {}: {} → {}", job.id, job.src_path, job.dest_dir);
        let result = run(&job, &cancel).await;
        let canceled = cancel.load(Ordering::SeqCst);
        STATE.lock().unwrap().cancel.remove(&job.id);
        update_persist(job.id, |j| {
            j.finished = Some(now());
            j.speed = 0.0;
            j.current.clear();
            match (&result, canceled) {
                (_, true) => j.status = Status::Canceled,
                (Ok(()), _) => {
                    j.status = Status::Done;
                    j.done_bytes = j.total_bytes;
                    j.files_done = j.files_total;
                }
                (Err(e), _) => {
                    j.status = Status::Failed;
                    j.error = Some(format!("{e:#}"));
                }
            }
        });
        match (&result, canceled) {
            (_, true) => log!("job {}: cancelado", job.id),
            (Ok(()), _) => log!("job {}: concluído", job.id),
            (Err(e), _) => log!("job {}: falhou: {e:#}", job.id),
        }
    }
}

struct FileTask {
    /// Share path, or absolute path for local copies.
    remote: String,
    local: PathBuf,
    size: u64,
}

/// Where a job reads from.
#[derive(Clone)]
enum Src {
    Smb(sources::Source),
    Local,
}

async fn run(job: &Job, cancel: &Arc<AtomicBool>) -> anyhow::Result<()> {
    let src = if job.source_id == LOCAL {
        Src::Local
    } else {
        Src::Smb(sources::get(&job.source_id).ok_or_else(|| anyhow!(tr!("the source \"{}\" was removed", "a fonte \"{}\" foi removida", job.source_name)))?)
    };
    let root = Path::new(&job.dest_dir).join(&job.name);
    update(job.id, |j| {
        j.status = Status::Scanning;
        j.error = None;
    });

    // 1. Scan.
    let (mut files, total) = match &src {
        Src::Smb(s) => scan_smb(job, s, &root, cancel).await?,
        Src::Local => {
            let (job, root, cancel) = (job.clone(), root.clone(), cancel.clone());
            tokio::task::spawn_blocking(move || scan_local(&job, &root, &cancel)).await??
        }
    };
    if cancel.load(Ordering::SeqCst) {
        return Ok(());
    }

    // 2. Resume: files already present with the right size count as done.
    let mut done_bytes = 0u64;
    let mut done_files = 0u64;
    files.retain(|f| {
        let present = std::fs::metadata(&f.local).map(|m| m.len() == f.size).unwrap_or(false);
        if present {
            done_bytes += f.size;
            done_files += 1;
        }
        !present
    });
    let needed = total - done_bytes;
    if let Some((free, _)) = localfs::disk_space(Path::new(&job.dest_dir)) {
        if needed > free {
            bail!(tr!("not enough space: needs {}, {} free", "espaço insuficiente: precisa de {}, livre {}", human(needed), human(free)));
        }
    }
    let (n, t) = (files.len() as u64 + done_files, total);
    update_persist(job.id, |j| {
        j.status = Status::Running;
        j.files_total = n;
        j.total_bytes = t;
        j.done_bytes = done_bytes;
        j.files_done = done_files;
    });

    // 3. Copy, with a ticker publishing progress and smoothed speed.
    let progress = Arc::new(AtomicU64::new(done_bytes));
    let ticker = {
        let (progress, id) = (progress.clone(), job.id);
        tokio::spawn(async move {
            let mut last = (Instant::now(), progress.load(Ordering::Relaxed));
            let mut speed = 0.0f64;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let now_bytes = progress.load(Ordering::Relaxed);
                let dt = last.0.elapsed().as_secs_f64();
                let inst = (now_bytes.saturating_sub(last.1)) as f64 / dt.max(0.001);
                speed = if speed == 0.0 { inst } else { speed * 0.7 + inst * 0.3 };
                last = (Instant::now(), now_bytes);
                update(id, |j| {
                    j.done_bytes = now_bytes;
                    j.speed = speed;
                });
            }
        })
    };
    let result = copy_all(job, &src, files, &progress, cancel, done_files).await;
    ticker.abort();
    result
}

/// Share listing, folders breadth-first with several listings in flight at
/// once (big collections have thousands of sub-folders).
async fn scan_smb(job: &Job, src: &sources::Source, root: &Path, cancel: &Arc<AtomicBool>) -> anyhow::Result<(Vec<FileTask>, u64)> {
    let mut files = Vec::new();
    let mut total = 0u64;
    if job.dir {
        let mut queue = VecDeque::from([(job.src_path.clone(), root.to_path_buf())]);
        let mut inflight = tokio::task::JoinSet::new();
        loop {
            while inflight.len() < SCAN_CONCURRENCY {
                let Some((remote, local)) = queue.pop_front() else { break };
                let src = src.clone();
                inflight.spawn(async move { (smbfs::list(&src, &remote).await, remote, local) });
            }
            let Some(done) = inflight.join_next().await else { break };
            if cancel.load(Ordering::SeqCst) {
                inflight.abort_all();
                return Ok((files, total));
            }
            let (listing, remote, local) = done.map_err(|e| anyhow!(tr!("scan failed: {e}", "varredura falhou: {e}")))?;
            for e in listing? {
                let r = format!("{remote}/{}", e.name);
                let l = local.join(&e.name);
                if e.dir {
                    queue.push_back((r, l));
                } else {
                    total += e.size;
                    files.push(FileTask { remote: r, local: l, size: e.size });
                }
            }
            let (n, t) = (files.len() as u64, total);
            update(job.id, |j| {
                j.files_total = n;
                j.total_bytes = t;
                j.current = remote;
            });
        }
    } else {
        let (_, size) = smbfs::open_read(src, &job.src_path).await?;
        total = size;
        files.push(FileTask { remote: job.src_path.clone(), local: root.to_path_buf(), size });
    }
    Ok((files, total))
}


/// Local tree walk (on a blocking thread). Symlinked folders are skipped so a
/// link can't loop; linked files are copied as files.
fn scan_local(job: &Job, root: &Path, cancel: &AtomicBool) -> anyhow::Result<(Vec<FileTask>, u64)> {
    let src = PathBuf::from(&job.src_path);
    let meta = std::fs::metadata(&src).map_err(|_| anyhow!(tr!("not found: {}", "não encontrado: {}", job.src_path)))?;
    if !meta.is_dir() {
        return Ok((vec![FileTask { remote: job.src_path.clone(), local: root.to_path_buf(), size: meta.len() }], meta.len()));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    let mut queue = VecDeque::from([(src, root.to_path_buf())]);
    while let Some((dir, local)) = queue.pop_front() {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        for e in std::fs::read_dir(&dir)? {
            let e = e?;
            let path = e.path();
            let link = e.file_type()?.is_symlink();
            let Ok(m) = std::fs::metadata(&path) else { continue };
            if m.is_dir() {
                if !link {
                    queue.push_back((path, local.join(e.file_name())));
                }
            } else {
                total += m.len();
                files.push(FileTask { remote: path.display().to_string(), local: local.join(e.file_name()), size: m.len() });
            }
        }
        if files.len() % 256 == 0 {
            let (n, t, cur) = (files.len() as u64, total, dir.display().to_string());
            update(job.id, |j| {
                j.files_total = n;
                j.total_bytes = t;
                j.current = cur;
            });
        }
    }
    Ok((files, total))
}

async fn copy_all(
    job: &Job,
    src: &Src,
    files: Vec<FileTask>,
    progress: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
    mut done_files: u64,
) -> anyhow::Result<()> {
    for f in files {
        if cancel.load(Ordering::SeqCst) {
            return Ok(());
        }
        let shown = f.remote.rsplit('/').next().unwrap_or(&f.remote).to_string();
        update(job.id, |j| j.current = shown);
        let mut attempt = 0;
        loop {
            let before = progress.load(Ordering::SeqCst);
            let copied = match src {
                Src::Smb(s) => copy_file(s, &f, progress, cancel).await,
                Src::Local => {
                    let (f, progress, cancel) = (FileTask { remote: f.remote.clone(), local: f.local.clone(), size: f.size }, progress.clone(), cancel.clone());
                    tokio::task::spawn_blocking(move || copy_local_file(&f, &progress, &cancel))
                        .await
                        .unwrap_or_else(|e| Err(anyhow!(tr!("reader failed: {e}", "leitor falhou: {e}"))))
                }
            };
            match copied {
                Ok(()) => break,
                Err(e) => {
                    // Roll back this file's partial progress before retrying.
                    progress.store(before, Ordering::SeqCst);
                    attempt += 1;
                    if cancel.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    if attempt >= FILE_ATTEMPTS {
                        return Err(e.context(tr!("copying \"{}\"", "ao copiar \"{}\"", f.remote)));
                    }
                    log!("job {}: tentativa {attempt} falhou ({e:#}), repetindo", job.id);
                    tokio::time::sleep(Duration::from_secs(attempt as u64)).await;
                }
            }
        }
        done_files += 1;
        update(job.id, |j| j.files_done = done_files);
    }
    Ok(())
}

async fn copy_file(
    src: &sources::Source,
    f: &FileTask,
    progress: &Arc<AtomicU64>,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    if let Some(parent) = f.local.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = part_path(&f.local);
    let (remote, len) = smbfs::open_read(src, &f.remote).await?;
    let out = std::fs::File::create(&part)?;
    out.set_len(len)?;
    let out = Arc::new(out);
    let next = Arc::new(AtomicU64::new(0));

    let mut readers = Vec::new();
    for _ in 0..READERS {
        let (remote, out, next, progress, cancel) =
            (remote.clone(), out.clone(), next.clone(), progress.clone(), cancel.clone());
        readers.push(tokio::spawn(async move {
            let mut buf = vec![0u8; CHUNK as usize];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(());
                }
                let off = next.fetch_add(CHUNK, Ordering::SeqCst);
                if off >= len {
                    return Ok(());
                }
                let want = CHUNK.min(len - off) as usize;
                let mut got = 0;
                while got < want {
                    let n = remote.read_at(&mut buf[got..want], off + got as u64).await?;
                    if n == 0 {
                        return Err(anyhow!(tr!("the file ended earlier than expected", "o arquivo terminou antes do esperado")));
                    }
                    got += n;
                }
                out.write_all_at(&buf[..got], off)?;
                progress.fetch_add(got as u64, Ordering::Relaxed);
            }
        }));
    }
    let mut result = Ok(());
    for r in readers {
        match r.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => result = Err(e),
            Err(e) => result = Err(anyhow!(tr!("reader failed: {e}", "leitor falhou: {e}"))),
        }
    }
    let _ = remote.close().await;
    if result.is_err() || cancel.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(&part);
        return result;
    }
    std::fs::rename(&part, &f.local)?;
    Ok(())
}

fn part_path(local: &Path) -> PathBuf {
    local.with_file_name(format!("{}.pishop-part", local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()))
}

/// Plain read/write into a `.pishop-part` file, renamed when complete; a
/// cancel drops the partial file.
fn copy_local_file(f: &FileTask, progress: &AtomicU64, cancel: &AtomicBool) -> anyhow::Result<()> {
    if let Some(parent) = f.local.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = part_path(&f.local);
    let mut input = std::fs::File::open(&f.remote)?;
    let mut out = std::fs::File::create(&part)?;
    let mut buf = vec![0u8; LOCAL_CHUNK];
    let mut copied = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Ok(());
        }
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                drop(out);
                let _ = std::fs::remove_file(&part);
                return Err(e.into());
            }
        };
        if let Err(e) = out.write_all(&buf[..n]) {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err(e.into());
        }
        copied += n as u64;
        progress.fetch_add(n as u64, Ordering::Relaxed);
    }
    drop(out);
    if copied != f.size {
        let _ = std::fs::remove_file(&part);
        bail!(tr!("the file changed while it was being copied", "o arquivo mudou durante a cópia"));
    }
    std::fs::rename(&part, &f.local)?;
    Ok(())
}

fn human(n: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", units[i])
}
