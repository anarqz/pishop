//! Game trailers: the first YouTube result for "<name> Trailer", found with
//! the bundled yt-dlp (no JS runtime needed in flat-playlist mode). Cached.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::{data_dir, log};
use crate::tr;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Trailer {
    pub video_id: String,
    pub url: String,
    pub title: String,
}

fn cache_file() -> PathBuf {
    data_dir().join(".cache").join("trailers.json")
}

static CACHE: LazyLock<Mutex<HashMap<String, Option<Trailer>>>> = LazyLock::new(|| {
    Mutex::new(std::fs::read(cache_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
});

fn yt_dlp() -> PathBuf {
    std::env::var_os("PISHOP_YTDLP").map(PathBuf::from).unwrap_or_else(|| crate::base_dir().join("bin").join("yt-dlp"))
}

pub fn video_id(url: &str) -> Option<String> {
    let id = if let Some(rest) = url.split("v=").nth(1) {
        rest.split('&').next()?
    } else if let Some(rest) = url.split("youtu.be/").nth(1) {
        rest.split(['?', '&']).next()?
    } else {
        url.trim()
    };
    (id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| id.to_string())
}

pub async fn find(name: &str) -> anyhow::Result<Option<Trailer>> {
    let key = name.trim().to_lowercase();
    if let Some(hit) = CACHE.lock().unwrap().get(&key).cloned() {
        return Ok(hit);
    }
    let query = format!("ytsearch1:{} Trailer", name.trim());
    let out = tokio::time::timeout(
        Duration::from_secs(25),
        tokio::process::Command::new(yt_dlp())
            .args(["--flat-playlist", "--no-warnings", "--print", "url", "--print", "title", &query])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| anyhow!(tr!("yt-dlp took too long", "yt-dlp demorou demais")))?
    .map_err(|e| anyhow!(tr!("yt-dlp unavailable: {e}", "yt-dlp indisponível: {e}")))?;
    if !out.status.success() {
        bail!(tr!("yt-dlp failed: {}", "yt-dlp falhou: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("")));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let found = match (lines.next(), lines.next()) {
        (Some(url), title) => video_id(url).map(|id| Trailer {
            url: format!("https://www.youtube.com/watch?v={id}"),
            video_id: id,
            title: title.unwrap_or_default().to_string(),
        }),
        _ => None,
    };
    log!("trailer: {name:?} → {:?}", found.as_ref().map(|t| &t.url));
    let mut cache = CACHE.lock().unwrap();
    cache.insert(key, found.clone());
    if let Ok(bytes) = serde_json::to_vec(&*cache) {
        let _ = std::fs::create_dir_all(cache_file().parent().unwrap());
        let _ = std::fs::write(cache_file(), bytes);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids() {
        assert_eq!(super::video_id("https://www.youtube.com/watch?v=jVGUdY0eht0").as_deref(), Some("jVGUdY0eht0"));
        assert_eq!(super::video_id("https://youtu.be/rxJPDQfYBqA").as_deref(), Some("rxJPDQfYBqA"));
        assert_eq!(super::video_id("https://www.youtube.com/watch?v=U9eeGZA_jrY&ab_channel=X").as_deref(), Some("U9eeGZA_jrY"));
        assert_eq!(super::video_id("Jd9LhXu4MKA").as_deref(), Some("Jd9LhXu4MKA"));
    }
}
