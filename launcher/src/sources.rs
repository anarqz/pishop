//! Game sources (where games are collected from). The first kind is network
//! storage over SMB. Stored in `<data>/sources.json` (0600: it holds passwords).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Smb,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Source {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub host: String,
    pub share: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Folder inside the share the explorer starts in ("" = share root).
    #[serde(default)]
    pub base_path: String,
}

/// What the UI gets back: never the password itself.
#[derive(Serialize)]
pub struct PublicSource {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub host: String,
    pub share: String,
    pub username: String,
    pub has_password: bool,
    pub base_path: String,
}

impl From<&Source> for PublicSource {
    fn from(s: &Source) -> Self {
        PublicSource {
            id: s.id.clone(),
            kind: s.kind.clone(),
            name: s.name.clone(),
            host: s.host.clone(),
            share: s.share.clone(),
            username: s.username.clone(),
            has_password: !s.password.is_empty(),
            base_path: s.base_path.clone(),
        }
    }
}

static SOURCES: Mutex<Vec<Source>> = Mutex::new(Vec::new());

fn file() -> PathBuf {
    crate::data_dir().join("sources.json")
}

pub fn load() {
    let list = std::fs::read(file())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Source>>(&b).ok())
        .unwrap_or_default();
    *SOURCES.lock().unwrap() = list;
}

fn save(list: &[Source]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = file();
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(list)?)?;
    std::fs::rename(tmp, path)
}

pub fn all() -> Vec<Source> {
    SOURCES.lock().unwrap().clone()
}

pub fn get(id: &str) -> Option<Source> {
    SOURCES.lock().unwrap().iter().find(|s| s.id == id).cloned()
}

/// Normalises user input: "\\host\share", "smb://host/share" or "host/share".
pub fn normalize(mut s: Source) -> Source {
    let host = s.host.trim().trim_start_matches("smb://").trim_start_matches(['\\', '/']).replace('\\', "/");
    let mut parts = host.splitn(2, '/');
    s.host = parts.next().unwrap_or_default().trim_end_matches(':').to_string();
    if let Some(rest) = parts.next().filter(|r| !r.is_empty()) {
        let mut rest = rest.trim_matches('/').splitn(2, '/');
        if s.share.trim().is_empty() {
            s.share = rest.next().unwrap_or_default().to_string();
            if s.base_path.trim().is_empty() {
                s.base_path = rest.next().unwrap_or_default().to_string();
            }
        }
    }
    s.share = s.share.trim().trim_matches(['/', '\\']).to_string();
    s.base_path = s.base_path.trim().replace('\\', "/").trim_matches('/').to_string();
    s.name = s.name.trim().to_string();
    if s.name.is_empty() {
        s.name = format!("{} ({})", s.share, s.host);
    }
    s
}

/// Inserts or updates. An empty password on update keeps the stored one.
pub fn upsert(mut s: Source) -> std::io::Result<Source> {
    let mut list = SOURCES.lock().unwrap();
    if s.id.is_empty() {
        s.id = format!("src{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis());
    }
    match list.iter_mut().find(|x| x.id == s.id) {
        Some(existing) => {
            if s.password.is_empty() {
                s.password = existing.password.clone();
            }
            *existing = s.clone();
        }
        None => list.push(s.clone()),
    }
    save(&list)?;
    Ok(s)
}

pub fn remove(id: &str) -> std::io::Result<()> {
    let mut list = SOURCES.lock().unwrap();
    list.retain(|s| s.id != id);
    save(&list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(host: &str, share: &str) -> Source {
        Source {
            id: String::new(),
            kind: Kind::Smb,
            name: String::new(),
            host: host.into(),
            share: share.into(),
            username: String::new(),
            password: String::new(),
            base_path: String::new(),
        }
    }

    #[test]
    fn normalizes_hosts() {
        let s = normalize(src("192.168.68.91:/games", ""));
        assert_eq!((s.host.as_str(), s.share.as_str()), ("192.168.68.91", "games"));
        let s = normalize(src(r"\\nas\games\consoles\roms", ""));
        assert_eq!((s.host.as_str(), s.share.as_str(), s.base_path.as_str()), ("nas", "games", "consoles/roms"));
        let s = normalize(src("smb://nas", "media"));
        assert_eq!((s.host.as_str(), s.share.as_str()), ("nas", "media"));
    }
}
