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

/// Official library art: the 600×900 capsule (cover) and the wide hero.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LibraryArt {
    pub cover: Option<String>,
    pub hero: Option<String>,
    /// Wide capsule (460×215 header), for the library's horizontal slot.
    #[serde(default)]
    pub wide: Option<String>,
    /// Square community icon.
    #[serde(default)]
    pub icon: Option<String>,
    /// The same art at twice the size (1200×1800, 3840×1240, 920×430), for
    /// the Steam library.
    #[serde(default)]
    pub cover_2x: Option<String>,
    #[serde(default)]
    pub hero_2x: Option<String>,
    #[serde(default)]
    pub wide_2x: Option<String>,
    /// Hash of the app's icon (also names its .ico on Steam's CDN).
    #[serde(default)]
    pub icon_hash: Option<String>,
}

static DETAILS: LazyLock<Mutex<HashMap<String, Option<StoreInfo>>>> = LazyLock::new(|| Mutex::new(load("steam-store.json")));
static ART: LazyLock<Mutex<HashMap<String, Option<LibraryArt>>>> = LazyLock::new(|| Mutex::new(load("steam-art.json")));
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
        .query(&[("term", name.trim()), ("cc", "br"), ("l", "english")])
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

/// Games → Find artwork: the store's matches for a name (id, name, capsule).
pub async fn search(term: &str) -> Vec<Value> {
    let Some(c) = client() else { return Vec::new() };
    let v: Value = match c
        .get("https://store.steampowered.com/api/storesearch/")
        .query(&[("term", term.trim()), ("cc", "br"), ("l", "english")])
        .send()
        .await
    {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(_) => return Vec::new(),
    };
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|it| {
            Some(serde_json::json!({
                "appid": it["id"].as_u64()?.to_string(),
                "name": it["name"].as_str()?,
                "image": it["tiny_image"].as_str(),
            }))
        })
        .collect()
}

pub async fn details(appid: &str) -> Option<StoreInfo> {
    let appid = appid.trim();
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Descriptions come in the UI language; cached per language.
    let lang = crate::settings::lang().steam();
    let key = format!("{appid}:{lang}");
    if let Some(hit) = DETAILS.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    let found = async {
        let v: Value = client()?
            .get("https://store.steampowered.com/api/appdetails")
            .query(&[("appids", appid), ("l", lang), ("cc", "br")])
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
    m.insert(key, found.clone());
    store("steam-store.json", &*m);
    found
}

/// Opens the game's page in the Steam client (Game Mode shows it on top).
/// Library capsule + hero of an app. Newer apps keep these under hashed file
/// names, so the store's item API says where they are (no key needed).
pub async fn library_art(appid: &str) -> Option<LibraryArt> {
    let appid = appid.trim();
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Some games localize their capsules; cached per language.
    let lang = crate::settings::lang().steam();
    let key = format!("{appid}:{lang}:3");
    if let Some(hit) = ART.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    let input = format!(
        r#"{{"ids":[{{"appid":{appid}}}],"context":{{"language":"{lang}","country_code":"BR"}},"data_request":{{"include_assets":true}}}}"#
    );
    let v: Value = async {
        client()?
            .get("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/")
            .query(&[("input_json", input.as_str())])
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()
    }
    .await
    .or_else(|| {
        log!("steam: assets {appid} indisponíveis");
        None
    })?;
    let a = &v["response"]["store_items"][0]["assets"];
    let url = |key: &str| -> Option<String> {
        let file = a[key].as_str()?;
        let fmt = a["asset_url_format"].as_str()?;
        Some(format!("https://shared.akamai.steamstatic.com/store_item_assets/{}", fmt.replace("${FILENAME}", file)))
    };
    let icon_hash = a["community_icon"].as_str().map(String::from);
    let icon = icon_hash.as_ref().map(|h| format!("https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps/{appid}/{h}.jpg"));
    let found = Some(LibraryArt {
        cover: url("library_capsule"),
        hero: url("library_hero"),
        wide: url("header"),
        icon,
        cover_2x: url("library_capsule_2x"),
        hero_2x: url("library_hero_2x"),
        wide_2x: url("header_2x"),
        icon_hash,
    })
    .filter(|a| a.cover.is_some() || a.hero.is_some());
    let mut m = ART.lock().unwrap();
    m.insert(key, found.clone());
    store("steam-art.json", &*m);
    found
}

/// Steam's own placement of a game's logo over its hero.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LogoPosition {
    /// "BottomLeft", "UpperCenter", "CenterCenter"…
    pub pinned: String,
    pub width_pct: f64,
    pub height_pct: f64,
}

/// What the store's item API leaves out: the library logo (newer apps keep
/// it under a hashed name), where Steam places it, and the client icon.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppInfoArt {
    /// Logo files under store_item_assets, best first (2× first).
    pub logos: Vec<String>,
    pub logo_position: Option<LogoPosition>,
    pub clienticon: Option<String>,
}

static APPINFO: LazyLock<Mutex<HashMap<String, Option<AppInfoArt>>>> = LazyLock::new(|| Mutex::new(load("steam-appinfo.json")));

/// The image for the user's language in a library_assets_full entry, else English.
fn asset_file(entry: &Value, size: &str, lang: &str) -> Option<String> {
    let images = &entry[size];
    images[lang].as_str().or_else(|| images["english"].as_str()).map(String::from)
}

/// Pure: the logo set from an app's PICS "common" section.
fn parse_appinfo(common: &Value, lang: &str) -> AppInfoArt {
    let logo = &common["library_assets_full"]["library_logo"];
    let logos = ["image2x", "image"].iter().filter_map(|size| asset_file(logo, size, lang)).collect();
    let pos = &logo["logo_position"];
    let pct = |k: &str| pos[k].as_str().and_then(|v| v.parse().ok()).or_else(|| pos[k].as_f64());
    let logo_position = match (pos["pinned_position"].as_str(), pct("width_pct"), pct("height_pct")) {
        (Some(p), Some(w), Some(h)) => Some(LogoPosition { pinned: p.to_string(), width_pct: w, height_pct: h }),
        _ => None,
    };
    AppInfoArt { logos, logo_position, clienticon: common["clienticon"].as_str().map(String::from) }
}

/// Steam's app info (PICS) for an app — what the Steam client itself reads
/// for the library — through a public PICS mirror, since the store API
/// doesn't carry logos. `fresh` skips the cache.
pub async fn appinfo_art(appid: &str, fresh: bool) -> Option<AppInfoArt> {
    let appid = appid.trim();
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !fresh {
        if let Some(hit) = APPINFO.lock().unwrap().get(appid).cloned() {
            return hit;
        }
    }
    let v: Value = async {
        client()?.get(format!("https://api.steamcmd.net/v1/info/{appid}")).send().await.ok()?.json().await.ok()
    }
    .await?;
    let common = &v["data"][appid]["common"];
    if !common.is_object() {
        return None;
    }
    let found = Some(parse_appinfo(common, crate::settings::lang().steam()));
    let mut m = APPINFO.lock().unwrap();
    m.insert(appid.to_string(), found.clone());
    store("steam-appinfo.json", &*m);
    found
}

/// A file from an app's store assets on Steam's CDN.
pub fn asset_url(appid: &str, file: &str) -> String {
    format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/{file}")
}

pub fn open_store_page(appid: &str) -> std::io::Result<()> {
    if appid.is_empty() || !appid.chars().all(|c| c.is_ascii_digit()) {
        return Err(std::io::Error::other(crate::tr!("invalid appid", "appid inválido")));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appinfo_logo_and_icon() {
        let common = serde_json::json!({
            "clienticon": "0b360a7c",
            "library_assets_full": { "library_logo": {
                "image": { "english": "d963/logo.png" },
                "image2x": { "english": "d963/logo_2x.png", "brazilian": "aa/logo_2x.png" },
                "logo_position": { "height_pct": "65.57", "pinned_position": "BottomLeft", "width_pct": "47.9" }
            }}
        });
        let a = parse_appinfo(&common, "brazilian");
        assert_eq!(a.logos, ["aa/logo_2x.png", "d963/logo.png"]);
        assert_eq!(a.logo_position, Some(LogoPosition { pinned: "BottomLeft".into(), width_pct: 47.9, height_pct: 65.57 }));
        assert_eq!(a.clienticon.as_deref(), Some("0b360a7c"));
        assert!(parse_appinfo(&serde_json::json!({}), "english").logos.is_empty());
    }
}
