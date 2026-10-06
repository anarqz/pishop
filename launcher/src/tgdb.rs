//! TheGamesDB: synopsis, genres, developers/publishers, rating, players.
//! The key allows ~1000 requests/month, so every lookup is cached on disk for
//! good, and the id → name tables are fetched once.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::bail;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use crate::{data_dir, log, titles};

const API: &str = "https://api.thegamesdb.net";
const PC_PLATFORM: i64 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameInfo {
    pub id: i64,
    pub title: String,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub players: Option<i64>,
    pub rating: Option<String>,
    pub genres: Vec<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub platform: Option<String>,
    pub youtube: Option<String>,
}

/// The API key comes from Settings → Serviços; without it TGDB is skipped.
fn key() -> Option<String> {
    Some(crate::catalog::config().tgdb_key.trim().to_string()).filter(|k| !k.is_empty())
}

pub fn configured() -> bool {
    key().is_some()
}

fn cache_path(name: &str) -> PathBuf {
    data_dir().join(".cache").join(name)
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read(cache_path(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(name: &str, v: &T) {
    let path = cache_path(name);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(bytes) = serde_json::to_vec(v) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct Cached {
    /// Unix seconds of the lookup.
    at: u64,
    info: Option<GameInfo>,
}

/// A miss is re-checked after this long (new games get added to TGDB).
const MISS_TTL_SECS: u64 = 7 * 24 * 3600;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// name (lowercase) → result. Hits are kept for good, misses for a week.
static GAMES: LazyLock<Mutex<HashMap<String, Cached>>> = LazyLock::new(|| Mutex::new(read_json("tgdb-games.json")));

fn cached(key: &str) -> Option<Option<GameInfo>> {
    let m = GAMES.lock().unwrap();
    let c = m.get(key)?;
    (c.info.is_some() || now().saturating_sub(c.at) < MISS_TTL_SECS).then(|| c.info.clone())
}
/// One request per name at a time (several screens may ask concurrently).
static INFLIGHT: AsyncMutex<()> = AsyncMutex::const_new(());

async fn get(path: &str, query: &[(&str, &str)]) -> anyhow::Result<Value> {
    get_with(&key().ok_or_else(|| anyhow::anyhow!("TheGamesDB não configurado"))?, path, query).await
}

async fn get_with(k: &str, path: &str, query: &[(&str, &str)]) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?;
    let mut q: Vec<(&str, &str)> = vec![("apikey", k)];
    q.extend_from_slice(query);
    let r = client.get(format!("{API}{path}")).query(&q).send().await?;
    let v: Value = r.json().await?;
    if v["code"].as_i64() != Some(200) {
        bail!("TGDB: {}", v["status"].as_str().unwrap_or("erro"));
    }
    if let Some(left) = v["remaining_monthly_allowance"].as_i64() {
        if left < 50 {
            log!("tgdb: restam {left} requisições este mês");
        }
    }
    Ok(v)
}

/// id → name table for genres/developers/publishers, fetched once and kept.
async fn names(kind: &str) -> HashMap<String, String> {
    let file = format!("tgdb-{kind}.json");
    let cached: HashMap<String, String> = read_json(&file);
    if !cached.is_empty() {
        return cached;
    }
    let Ok(v) = get(&format!("/v1/{}", capitalize(kind)), &[]).await else { return cached };
    let mut out = HashMap::new();
    if let Some(map) = v["data"][kind].as_object() {
        for (id, item) in map {
            if let Some(n) = item["name"].as_str() {
                out.insert(id.clone(), n.to_string());
            }
        }
    }
    if !out.is_empty() {
        write_json(&file, &out);
    }
    out
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_i64().map(|i| i.to_string())).collect()).unwrap_or_default()
}

/// Best TGDB entry for a game name, preferring the PC release with the most
/// complete metadata.
pub async fn details(name: &str) -> Option<GameInfo> {
    let key = name.trim().to_lowercase();
    if key.is_empty() || !configured() {
        return None;
    }
    if let Some(hit) = cached(&key) {
        return hit;
    }
    let _one = INFLIGHT.lock().await;
    if let Some(hit) = cached(&key) {
        return hit;
    }
    let found = match lookup(name).await {
        Ok(found) => found,
        Err(e) => {
            log!("tgdb: {name:?}: {e:#}");
            return None; // not cached: retry later
        }
    };
    GAMES.lock().unwrap().insert(key, Cached { at: now(), info: found.clone() });
    write_json("tgdb-games.json", &*GAMES.lock().unwrap());
    found
}

async fn lookup(name: &str) -> anyhow::Result<Option<GameInfo>> {
    let v = get(
        "/v1.1/Games/ByGameName",
        &[("name", name), ("fields", "players,publishers,genres,overview,rating,platform,youtube"), ("include", "platform")],
    )
    .await?;
    let games = v["data"]["games"].as_array().cloned().unwrap_or_default();
    let platforms = &v["include"]["platform"];
    let best = games
        .iter()
        .map(|g| {
            let title = g["game_title"].as_str().unwrap_or_default();
            let mut score = titles::similarity(name, title);
            if g["platform"].as_i64() == Some(PC_PLATFORM) {
                score += 0.15;
            }
            if g["overview"].as_str().is_some_and(|o| o.len() > 40) {
                score += 0.05;
            }
            if g["developers"].as_array().is_some_and(|d| !d.is_empty()) {
                score += 0.03;
            }
            (score, g)
        })
        .filter(|(s, _)| *s >= 0.7)
        .max_by(|a, b| a.0.total_cmp(&b.0));
    let Some((_, g)) = best else { return Ok(None) };

    let (genres, developers, publishers) = tokio::join!(names("genres"), names("developers"), names("publishers"));
    let resolve = |list: Vec<String>, table: &HashMap<String, String>| -> Vec<String> {
        list.into_iter().filter_map(|id| table.get(&id).cloned()).collect()
    };
    let platform_id = g["platform"].as_i64().unwrap_or(0).to_string();
    let platform = platforms["data"][platform_id.as_str()]["name"].as_str().map(String::from);
    let youtube = g["youtube"].as_str().filter(|s| !s.is_empty()).map(String::from);
    Ok(Some(GameInfo {
        id: g["id"].as_i64().unwrap_or(0),
        title: g["game_title"].as_str().unwrap_or(name).to_string(),
        overview: g["overview"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        release_date: g["release_date"].as_str().map(String::from),
        players: g["players"].as_i64(),
        rating: g["rating"].as_str().filter(|r| !r.is_empty() && *r != "Not Rated").map(String::from),
        genres: resolve(ids(&g["genres"]), &genres),
        developers: resolve(ids(&g["developers"]), &developers),
        publishers: resolve(ids(&g["publishers"]), &publishers),
        platform,
        youtube,
    }))
}

/// Settings "Testar": validates a key and reports the monthly allowance left.
pub async fn test(key: &str) -> anyhow::Result<Value> {
    let v = get_with(key, "/v1/Genres", &[]).await?;
    Ok(serde_json::json!({ "remaining": v["remaining_monthly_allowance"] }))
}
