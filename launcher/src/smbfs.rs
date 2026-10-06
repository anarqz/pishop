//! SMB2/3 access (pure Rust, `smb` crate). One authenticated connection per
//! source is kept and shared by listings and copy jobs; it is rebuilt
//! transparently when the server drops it.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{Context, anyhow};
use futures_util::StreamExt;
use smb::{
    Client, ClientConfig, DirAccessMask, Directory, File, FileAccessMask, FileCreateArgs, FileDirectoryInformation,
    GetLen, UncPath,
};
use tokio::sync::Mutex;

use crate::localfs::{Entry, sort_entries};
use crate::sources::Source;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const OP_TIMEOUT: Duration = Duration::from_secs(30);
/// Seconds between 1601-01-01 (FILETIME epoch) and 1970-01-01.
const FILETIME_UNIX_OFFSET: u64 = 11_644_473_600;

struct Conn {
    client: Client,
    share: UncPath,
}

static POOL: LazyLock<Mutex<HashMap<String, Arc<Conn>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Credentials and target are part of the key, so editing a source reconnects.
fn key(s: &Source) -> String {
    format!("{}|{}|{}|{}|{}", s.id, s.host, s.share, s.username, s.password)
}

async fn conn(s: &Source) -> anyhow::Result<Arc<Conn>> {
    let mut pool = POOL.lock().await;
    if let Some(c) = pool.get(&key(s)) {
        return Ok(c.clone());
    }
    pool.retain(|k, _| !k.starts_with(&format!("{}|", s.id)));
    let share = UncPath::from_str(&format!(r"\\{}\{}", s.host, s.share)).map_err(|e| anyhow!("endereço inválido: {e}"))?;
    let client = Client::new(ClientConfig::default());
    let user = if s.username.is_empty() { "guest" } else { s.username.as_str() };
    tokio::time::timeout(CONNECT_TIMEOUT, client.share_connect(&share, user, s.password.clone()))
        .await
        .map_err(|_| anyhow!("o servidor {} não respondeu", s.host))?
        .map_err(|e| anyhow!("falha ao conectar em \\\\{}\\{}: {e}", s.host, s.share))?;
    let c = Arc::new(Conn { client, share });
    pool.insert(key(s), c.clone());
    Ok(c)
}

async fn forget(s: &Source) {
    if let Some(c) = POOL.lock().await.remove(&key(s)) {
        let _ = c.client.close().await;
    }
}

/// Runs `op` on the pooled connection, reconnecting once if it fails (servers
/// close idle sessions; the first call after that would otherwise error).
async fn with_conn<T, F, Fut>(s: &Source, op: F) -> anyhow::Result<T>
where
    F: Fn(Arc<Conn>) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<T>>,
{
    let c = conn(s).await?;
    match tokio::time::timeout(OP_TIMEOUT, op(c)).await {
        Ok(Ok(v)) => return Ok(v),
        Ok(Err(e)) => crate::log!("smb: {e:#}; reconectando"),
        Err(_) => crate::log!("smb: operação expirou; reconectando"),
    }
    forget(s).await;
    let c = conn(s).await?;
    tokio::time::timeout(OP_TIMEOUT, op(c)).await.map_err(|_| anyhow!("o servidor demorou demais para responder"))?
}

/// UI paths use "/" relative to the share root.
pub fn unc_rel(path: &str) -> String {
    path.trim_matches('/').replace('/', "\\")
}

pub async fn list(s: &Source, path: &str) -> anyhow::Result<Vec<Entry>> {
    let rel = unc_rel(path);
    with_conn(s, |c| {
        let rel = rel.clone();
        async move {
            let target = c.share.clone().with_path(&rel);
            let args = FileCreateArgs::make_open_existing(DirAccessMask::new().with_list_directory(true).into());
            let dir = Arc::new(
                c.client
                    .create_file(&target, &args)
                    .await
                    .with_context(|| format!("não foi possível abrir \"{}\"", if rel.is_empty() { "/" } else { &rel }))?
                    .unwrap_dir(),
            );
            let mut out = Vec::new();
            {
                let mut stream = Directory::query::<FileDirectoryInformation>(&dir, "*").await?;
                while let Some(item) = stream.next().await {
                    let e = item?;
                    let name = e.file_name.to_string();
                    if name == "." || name == ".." {
                        continue;
                    }
                    let dir = e.file_attributes.directory();
                    let mtime = e.last_write_time.since_epoch().as_secs().saturating_sub(FILETIME_UNIX_OFFSET) as i64;
                    out.push(Entry { name, dir, size: if dir { 0 } else { e.end_of_file }, mtime });
                }
            }
            let _ = dir.close().await;
            sort_entries(&mut out);
            Ok(out)
        }
    })
    .await
}

/// Opens a remote file for reading; returns it with its length.
pub async fn open_read(s: &Source, path: &str) -> anyhow::Result<(Arc<File>, u64)> {
    let rel = unc_rel(path);
    with_conn(s, |c| {
        let rel = rel.clone();
        async move {
            let args = FileCreateArgs::make_open_existing(FileAccessMask::new().with_generic_read(true));
            let file = c
                .client
                .create_file(&c.share.clone().with_path(&rel), &args)
                .await
                .with_context(|| format!("não foi possível abrir \"{rel}\""))?
                .unwrap_file();
            let len = file.get_len().await?;
            Ok((Arc::new(file), len))
        }
    })
    .await
}

/// Connectivity check used by the settings screen: connect and list the start folder.
pub async fn test(s: &Source) -> anyhow::Result<usize> {
    forget(s).await;
    Ok(list(s, &s.base_path).await?.len())
}
