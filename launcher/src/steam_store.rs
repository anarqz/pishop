//! Steam store data (public endpoints, no key): the primary source for game
//! pages whenever an appid is known — Portuguese synopsis, genres, studios,
//! release date and the official wide background. TheGamesDB is the fallback.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{data_dir, log, titles};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoreInfo {
    pub appid: String,
    pub name: String,
    pub overview: Option<String>,
    pub genres: Vec<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub release_date: Option<String>,
    pub background: Option<String>,
    /// First gameplay screenshot (full HD): vivid backdrop for the game page.
    #[serde(default)]
    pub screenshot: Option<String>,
}

fn cache_file(name: &str) -> PathBuf {
    data_dir().join(".cache").join(name)
}

fn load<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read(cache_file(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn store<T: Serialize>(name: &str, v: &T) {
    if let Ok(bytes) = serde_json::to_vec(v) {
        let _ = std::fs::create_dir_all(cache_file(name).parent().unwrap());
        let _ = std::fs::write(cache_file(name), bytes);
    }
}

static DETAILS: LazyLock<Mutex<HashMap<String, Option<StoreInfo>>>> = LazyLock::new(|| Mutex::new(load("steam-store.json")));
static APPIDS: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(load("steam-appids.json")));

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(12)).build().ok()
}

/// Steam descriptions carry a little HTML; keep plain text.
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"").replace("&amp;", "&").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").trim().to_string()
}

/// appid for a game name via the store search (cached), when none is known.
pub async fn find_appid(name: &str) -> Option<String> {
    let key = name.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    if let Some(hit) = APPIDS.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    let v: Value = client()?
        .get("https://store.steampowered.com/api/storesearch/")
        .query(&[("term", name.trim()), ("cc", "br"), ("l", "brazilian")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let found = v["items"].as_array().and_then(|items| {
        items
            .iter()
            .filter_map(|it| Some((titles::similarity(name, it["name"].as_str()?), it["id"].as_u64()?)))
            .filter(|(s, _)| *s >= 0.75)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id.to_string())
    });
    let mut m = APPIDS.lock().unwrap();
    m.insert(key, found.clone());
    store("steam-appids.json", &*m);
    found
}

pub async fn details(appid: &str) -> Option<StoreInfo> {
    let appid = appid.trim();
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some(hit) = DETAILS.lock().unwrap().get(appid).cloned() {
        return hit;
    }
    let found = async {
        let v: Value = client()?
            .get("https://store.steampowered.com/api/appdetails")
            .query(&[("appids", appid), ("l", "brazilian"), ("cc", "br")])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;
        let entry = &v[appid];
        if !entry["success"].as_bool().unwrap_or(false) {
            return Some(None);
        }
        let d = &entry["data"];
        let names = |k: &str| -> Vec<String> {
            d[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
        };
        Some(Some(StoreInfo {
            appid: appid.to_string(),
            name: d["name"].as_str().unwrap_or_default().to_string(),
            overview: d["short_description"].as_str().map(plain).filter(|s| !s.is_empty()),
            genres: d["genres"]
                .as_array()
                .map(|a| a.iter().filter_map(|g| g["description"].as_str().map(String::from)).collect())
                .unwrap_or_default(),
            developers: names("developers"),
            publishers: names("publishers"),
            release_date: d["release_date"]["date"].as_str().map(String::from),
            background: d["background_raw"].as_str().or(d["background"].as_str()).map(String::from),
            screenshot: d["screenshots"][0]["path_full"].as_str().map(String::from),
        }))
    }
    .await;
    let Some(found) = found else {
        log!("steam: appdetails {appid} indisponível");
        return None; // network trouble: not cached
    };
    let mut m = DETAILS.lock().unwrap();
    m.insert(appid.to_string(), found.clone());
    store("steam-store.json", &*m);
    found
}

/// Opens the game's page in the Steam client (Game Mode shows it on top).
pub fn open_store_page(appid: &str) -> std::io::Result<()> {
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return Err(std::io::Error::other("appid inválido"));
    }
    let mut child = std::process::Command::new("steam")
        .arg(format!("steam://store/{appid}"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
