//! Store catalog: searches Prowlarr for console/PC game releases, matches each
//! release to a game on SteamGridDB (cover, hero, logo — public endpoints, no
//! key), and hands chosen releases to the embedded BitTorrent engine.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{OnceCell, Semaphore};

use crate::titles::{self, Parsed};
use crate::{data_dir, library, log, torrent, tpb};
use crate::tr;

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .user_agent(concat!("piShop/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
});

/// Prowlarr's download proxy answers with a redirect to the magnet link.
static HTTP_NOREDIRECT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("http client")
});

// ---------- configuration ----------

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    #[serde(default)]
    pub prowlarr_url: String,
    #[serde(default)]
    pub prowlarr_key: String,
    /// TheGamesDB API key (game details). Empty disables it.
    #[serde(default)]
    pub tgdb_key: String,
    /// isitcracked.com Supabase RPC endpoint, its key and the covers CDN.
    #[serde(default)]
    pub iic_url: String,
    #[serde(default)]
    pub iic_key: String,
    #[serde(default)]
    pub iic_cdn: String,
    /// The Pirate Bay JSON API (apibay format) for native search. Empty means
    /// apibay when Prowlarr isn't set up (the Store always has a source).
    #[serde(default)]
    pub tpb_url: String,
}

/// The Pirate Bay's own public JSON API: the Store's source when nothing else
/// is set up, so searching never needs configuration first.
pub const DEFAULT_TPB: &str = "https://apibay.org";

fn tpb_base(c: &Config) -> String {
    if c.tpb_url.is_empty() { DEFAULT_TPB.to_string() } else { c.tpb_url.clone() }
}

fn prowlarr_ready(c: &Config) -> bool {
    !c.prowlarr_url.is_empty() && !c.prowlarr_key.is_empty()
}

fn config_path() -> PathBuf {
    data_dir().join("catalog.json")
}

pub fn config() -> Config {
    std::fs::read(config_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Partial update from Settings: only fields sent are changed, and a blank
/// secret keeps the stored one (the UI never receives secrets back).
#[derive(Deserialize, Default)]
pub struct ConfigUpdate {
    pub prowlarr_url: Option<String>,
    pub prowlarr_key: Option<String>,
    pub tgdb_key: Option<String>,
    pub iic_url: Option<String>,
    pub iic_key: Option<String>,
    pub iic_cdn: Option<String>,
    pub tpb_url: Option<String>,
}

pub fn update_config(u: ConfigUpdate) -> anyhow::Result<Config> {
    let mut c = config();
    let secret = |new: Option<String>, old: String| new.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or(old);
    if let Some(url) = u.prowlarr_url {
        c.prowlarr_url = url;
    }
    if let Some(url) = u.iic_url {
        c.iic_url = url.trim().to_string();
    }
    if let Some(cdn) = u.iic_cdn {
        c.iic_cdn = cdn.trim().trim_end_matches('/').to_string();
    }
    if let Some(url) = u.tpb_url {
        c.tpb_url = url.trim().trim_end_matches('/').to_string();
    }
    c.prowlarr_key = secret(u.prowlarr_key, c.prowlarr_key);
    c.tgdb_key = secret(u.tgdb_key, c.tgdb_key);
    c.iic_key = secret(u.iic_key, c.iic_key);
    save_config(c)
}

/// Public view for Settings: URLs plus "has key" flags, never the secrets.
pub fn public_config() -> Value {
    let c = config();
    json!({
        "prowlarr_url": c.prowlarr_url, "has_key": !c.prowlarr_key.is_empty(),
        "has_tgdb_key": !c.tgdb_key.is_empty(),
        "iic_url": c.iic_url, "iic_cdn": c.iic_cdn, "has_iic_key": !c.iic_key.is_empty(),
        "tpb_url": c.tpb_url,
        "tpb_default": DEFAULT_TPB,
    })
}

pub fn save_config(mut c: Config) -> anyhow::Result<Config> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    // Other sources (Prowlarr just added, another TPB mirror) answer differently.
    SEARCH_CACHE.lock().unwrap().clear();
    c.prowlarr_url = c.prowlarr_url.trim().trim_end_matches('/').to_string();
    if !c.prowlarr_url.is_empty() && !c.prowlarr_url.starts_with("http") {
        c.prowlarr_url = format!("http://{}", c.prowlarr_url);
    }
    c.prowlarr_key = c.prowlarr_key.trim().to_string();
    let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(config_path())?;
    f.write_all(&serde_json::to_vec_pretty(&c)?)?;
    Ok(c)
}

/// Checks the Prowlarr connection; returns its version and enabled indexers.
pub async fn test(c: &Config) -> anyhow::Result<Value> {
    let status: Value = prowlarr_get(c, "/api/v1/system/status", &[]).await?;
    let indexers: Value = prowlarr_get(c, "/api/v1/indexer", &[]).await?;
    let names: Vec<String> = indexers
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|i| i["enable"].as_bool().unwrap_or(false))
                .filter_map(|i| i["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "version": status["version"], "indexers": names }))
}

async fn prowlarr_get(c: &Config, path: &str, query: &[(&str, &str)]) -> anyhow::Result<Value> {
    if c.prowlarr_url.is_empty() || c.prowlarr_key.is_empty() {
        bail!(tr!("set up Prowlarr in Settings → Services", "configure o Prowlarr em Configurações → Serviços"));
    }
    let r = HTTP
        .get(format!("{}{}", c.prowlarr_url, path))
        .header("X-Api-Key", &c.prowlarr_key)
        .query(query)
        .timeout(Duration::from_secs(90))
        .send()
        .await
        .map_err(|e| anyhow!(tr!("Prowlarr didn't respond: {e}", "Prowlarr não respondeu: {e}")))?;
    match r.status().as_u16() {
        200 => Ok(r.json().await?),
        401 => bail!(tr!("Prowlarr refused the API key", "a chave de API do Prowlarr foi recusada")),
        s => bail!(tr!("Prowlarr answered {s}", "Prowlarr respondeu {s}")),
    }
}

// ---------- search ----------

#[derive(Serialize, Clone)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub parsed: Parsed,
    pub size: u64,
    pub seeders: u32,
    pub leechers: u32,
    pub grabs: Option<u32>,
    pub files: Option<u32>,
    pub indexer: String,
    pub publish_date: String,
    pub info_url: Option<String>,
    pub info_hash: Option<String>,
    pub categories: Vec<String>,
    pub kind: String,
}

struct Stored {
    release: Release,
    guid: String,
    magnet_url: Option<String>,
    download_url: Option<String>,
    /// The Pirate Bay torrent id (native results, or Prowlarr's TPB ones).
    tpb_id: Option<String>,
}

/// Search answer: results from every source that worked, plus a note for
/// each one that didn't (the others still show).
#[derive(Serialize, Clone)]
pub struct SearchOutcome {
    pub results: Vec<Release>,
    pub warnings: Vec<String>,
}

static RELEASES: LazyLock<Mutex<HashMap<String, Arc<Stored>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn short_hash(s: &str) -> String {
    let h = s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
    format!("{h:016x}")
}

/// Indexers count every query against per-day limits (1337x is set to 100/24h
/// on this Prowlarr), so identical searches within this window are reused.
const SEARCH_TTL: Duration = Duration::from_secs(15 * 60);
static SEARCH_CACHE: LazyLock<Mutex<HashMap<String, (std::time::Instant, Vec<Release>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `fresh`: ask the sources again even when the same search is cached (the
/// answer still replaces the cached one).
pub async fn search(query: &str, kind: &str, fresh: bool) -> anyhow::Result<SearchOutcome> {
    let key = format!("{kind}|{}", query.trim().to_lowercase());
    if let Some((at, hit)) = SEARCH_CACHE.lock().unwrap().get(&key).filter(|_| !fresh) {
        // Results also live in RELEASES, unless that map was reset meanwhile.
        if at.elapsed() < SEARCH_TTL && hit.iter().all(|r| RELEASES.lock().unwrap().contains_key(&r.id)) {
            return Ok(SearchOutcome { results: hit.clone(), warnings: vec![] });
        }
    }
    let out = search_uncached(query, kind).await?;
    // A partial answer (some source failed) is not cached: retry next time.
    if out.warnings.is_empty() {
        let mut cache = SEARCH_CACHE.lock().unwrap();
        cache.retain(|_, (at, _)| at.elapsed() < SEARCH_TTL);
        cache.insert(key, (std::time::Instant::now(), out.results.clone()));
    }
    Ok(out)
}

fn pc_default(mut parsed: Parsed, kind: &str) -> Parsed {
    if kind == "pc" && parsed.platform.is_none() {
        parsed.platform = Some("pc");
        parsed.platform_label = Some("PC");
    }
    parsed
}

async fn prowlarr_search(c: &Config, query: &str, kind: &str) -> anyhow::Result<Vec<Stored>> {
    let cats = match kind {
        "pc" => "4050",
        _ => "1000",
    };
    let raw = prowlarr_get(c, "/api/v1/search", &[("query", query), ("categories", cats), ("type", "search"), ("limit", "100")]).await?;
    let mut out = Vec::new();
    for it in raw.as_array().cloned().unwrap_or_default() {
        let Some(title) = it["title"].as_str() else { continue };
        let guid = it["guid"].as_str().unwrap_or(title).to_string();
        let indexer = it["indexer"].as_str().unwrap_or_default().to_string();
        let info_url = it["infoUrl"].as_str().map(String::from);
        let tpb_id = (indexer.eq_ignore_ascii_case("thepiratebay"))
            .then(|| info_url.as_deref()?.split("id=").nth(1)?.split('&').next().map(String::from))
            .flatten();
        out.push(Stored {
            release: Release {
                id: short_hash(&guid),
                title: title.to_string(),
                parsed: pc_default(titles::parse(title), kind),
                size: it["size"].as_u64().unwrap_or(0),
                seeders: it["seeders"].as_u64().unwrap_or(0) as u32,
                leechers: it["leechers"].as_u64().unwrap_or(0) as u32,
                grabs: it["grabs"].as_u64().map(|g| g as u32),
                files: it["files"].as_u64().map(|g| g as u32),
                indexer,
                publish_date: it["publishDate"].as_str().unwrap_or_default().to_string(),
                info_url,
                info_hash: it["infoHash"].as_str().map(|h| h.to_lowercase()),
                categories: it["categories"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|c| c["name"].as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                kind: kind.to_string(),
            },
            guid,
            magnet_url: it["magnetUrl"].as_str().map(String::from),
            download_url: it["downloadUrl"].as_str().map(String::from),
            tpb_id,
        });
    }
    Ok(out)
}

async fn tpb_search(c: &Config, query: &str, kind: &str) -> anyhow::Result<Vec<Stored>> {
    let hits = tpb::search(&tpb_base(c), query, tpb::categories(kind)).await?;
    Ok(hits
        .into_iter()
        .map(|h| {
            let guid = format!("tpb:{}", h.id);
            Stored {
                release: Release {
                    id: short_hash(&guid),
                    title: h.name.clone(),
                    parsed: pc_default(titles::parse(&h.name), kind),
                    size: h.size,
                    seeders: h.seeders,
                    leechers: h.leechers,
                    grabs: None,
                    files: h.files,
                    indexer: tpb::label(),
                    publish_date: tpb::iso8601(h.added),
                    info_url: None,
                    info_hash: Some(h.info_hash.clone()),
                    categories: vec![tpb::category_name(h.category).to_string()],
                    kind: kind.to_string(),
                },
                guid,
                magnet_url: Some(tpb::magnet(&h.info_hash, &h.name)),
                download_url: None,
                tpb_id: Some(h.id),
            }
        })
        .collect())
}

async fn search_uncached(query: &str, kind: &str) -> anyhow::Result<SearchOutcome> {
    let c = config();
    let use_prowlarr = prowlarr_ready(&c);
    // Without Prowlarr, The Pirate Bay (apibay unless set otherwise) is the
    // source: the Store works with no setup at all.
    let use_tpb = !c.tpb_url.is_empty() || !use_prowlarr;
    let (prowlarr, mut native) = tokio::join!(
        async { if use_prowlarr { Some(prowlarr_search(&c, query, kind).await) } else { None } },
        async { if use_tpb { Some(tpb_search(&c, query, kind).await) } else { None } },
    );
    // Prowlarr set up but not answering: The Pirate Bay rather than nothing.
    if native.is_none() && matches!(prowlarr, Some(Err(_))) {
        native = Some(tpb_search(&c, query, kind).await);
    }

    let mut found: Vec<Stored> = Vec::new();
    let mut warnings = Vec::new();
    let mut sources = 0;
    for (name, r) in [("Prowlarr", prowlarr), ("The Pirate Bay", native)] {
        let Some(r) = r else { continue };
        sources += 1;
        match r {
            Ok(v) => found.extend(v),
            Err(e) => warnings.push(format!("{name}: {e:#}")),
        }
    }
    if warnings.len() == sources {
        bail!("{}", warnings.join(" · "));
    }

    // The same torrent can come from several sources: keep one per info hash,
    // the best-seeded, preferring the native result (direct magnet) on ties.
    let mut by_hash: HashMap<String, usize> = HashMap::new();
    let mut unique: Vec<Stored> = Vec::new();
    for s in found {
        let Some(hash) = s.release.info_hash.clone().filter(|h| !h.is_empty()) else {
            unique.push(s);
            continue;
        };
        match by_hash.get(&hash) {
            Some(&i) => {
                let native = |x: &Stored| x.guid.starts_with("tpb:");
                let cur = &unique[i];
                let better = s.release.seeders > cur.release.seeders
                    || (s.release.seeders == cur.release.seeders && native(&s) && !native(cur));
                if better {
                    unique[i] = s;
                }
            }
            None => {
                by_hash.insert(hash, unique.len());
                unique.push(s);
            }
        }
    }

    let mut store = RELEASES.lock().unwrap();
    if store.len() > 5000 {
        store.clear();
    }
    let mut results: Vec<Release> = unique
        .into_iter()
        .map(|s| {
            let r = s.release.clone();
            store.insert(r.id.clone(), Arc::new(s));
            r
        })
        .collect();
    results.sort_by(|a, b| b.seeders.cmp(&a.seeders).then(b.leechers.cmp(&a.leechers)));
    Ok(SearchOutcome { results, warnings })
}

fn stored(id: &str) -> anyhow::Result<Arc<Stored>> {
    RELEASES.lock().unwrap().get(id).cloned().ok_or_else(|| anyhow!(tr!("this result expired; search again", "resultado expirou; faça a busca de novo")))
}

// ---------- artwork (SteamGridDB public endpoints) ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Art {
    pub game_id: u64,
    pub name: String,
    pub year: Option<i32>,
    pub cover: String,
    pub cover_thumb: String,
    pub score: f64,
}

const MIN_SCORE: f64 = 0.6;

type ArtCell = Arc<OnceCell<Option<Art>>>;
static ART_CELLS: LazyLock<Mutex<HashMap<String, ArtCell>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static ART_DISK: LazyLock<Mutex<HashMap<String, Option<Art>>>> = LazyLock::new(|| {
    let m = std::fs::read(data_dir().join("art-cache.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    Mutex::new(m)
});
static SGDB_SLOTS: Semaphore = Semaphore::const_new(4);

fn art_key(name: &str) -> String {
    name.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn persist_art() {
    let snapshot = ART_DISK.lock().unwrap().clone();
    if let Ok(bytes) = serde_json::to_vec(&snapshot) {
        let path = data_dir().join("art-cache.json");
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

/// Best SteamGridDB match for a cleaned game name (cached on disk, deduped).
pub async fn art(name: &str) -> Option<Art> {
    let key = art_key(name);
    if key.is_empty() {
        return None;
    }
    if let Some(hit) = ART_DISK.lock().unwrap().get(&key).cloned() {
        return hit;
    }
    let cell = ART_CELLS.lock().unwrap().entry(key.clone()).or_default().clone();
    cell.get_or_init(|| async {
        let found = match fetch_art(name).await {
            Ok(a) => a,
            Err(e) => {
                // Network trouble: don't cache, a later request retries.
                log!("sgdb: {name:?}: {e:#}");
                ART_CELLS.lock().unwrap().remove(&key);
                return None;
            }
        };
        ART_DISK.lock().unwrap().insert(key.clone(), found.clone());
        persist_art();
        found
    })
    .await
    .clone()
}

async fn sgdb_search(term: &str, asset_type: &str) -> anyhow::Result<Value> {
    let filters = if asset_type == "grid" { json!({ "dimensions": ["600x900", "660x930", "342x482"] }) } else { json!({}) };
    sgdb_search_with(term, asset_type, filters).await
}

/// Games → Find artwork: SteamGridDB's games for a name, each with a cover
/// to recognise it by (best match first, as the site orders them).
pub async fn sgdb_games(term: &str) -> anyhow::Result<Vec<Value>> {
    let v = sgdb_search(term, "grid").await?;
    Ok(v["data"]["games"]
        .as_array()
        .into_iter()
        .flatten()
        .take(12)
        .filter_map(|g| {
            let game = &g["game"];
            let cover = pick_asset(&g["assets"]).map(|(_, thumb)| thumb);
            Some(json!({
                "id": game["id"].as_u64()?,
                "name": game["name"].as_str()?,
                "year": game["release_date"].as_i64().map(|ts| 1970 + ts / 31_556_952),
                "verified": game["verified"].as_bool().unwrap_or(false),
                "cover": cover,
            }))
        })
        .collect())
}

async fn sgdb_search_with(term: &str, asset_type: &str, filters: Value) -> anyhow::Result<Value> {
    let _slot = SGDB_SLOTS.acquire().await?;
    let r = HTTP
        .post("https://www.steamgriddb.com/api/public/search/main/games")
        .json(&json!({ "asset_type": asset_type, "term": term, "offset": 0, "filters": filters }))
        .send()
        .await?;
    if !r.status().is_success() {
        bail!(tr!("SteamGridDB answered {}", "SteamGridDB respondeu {}", r.status()));
    }
    Ok(r.json().await?)
}

fn pick_asset(assets: &Value) -> Option<(String, String)> {
    pick_asset_scored(assets, None, false)
}

/// Best acceptable asset by score: English (the names users search by),
/// the preferred `style` (e.g. "official" over fan-made), and for logos a
/// wide shape so the game's name is actually written out.
fn pick_asset_scored(assets: &Value, style: Option<&str>, wide: bool) -> Option<(String, String)> {
    let mut best: Option<(i32, usize, String, String)> = None;
    for (i, a) in assets.as_array()?.iter().enumerate() {
        let bad = a["nsfw"].as_bool().unwrap_or(false)
            || a["humor"].as_bool().unwrap_or(false)
            || a["epilepsy"].as_bool().unwrap_or(false)
            || a["is_animated"].as_bool().unwrap_or(false);
        let Some(url) = a["url"].as_str() else { continue };
        if bad {
            continue;
        }
        let mut score = 0;
        if a["language"].as_str().is_none_or(|l| l == "en") {
            score += 4;
        }
        if style.is_some() && a["style"].as_str() == style {
            score += 2;
        }
        if wide {
            let (w, h) = (a["width"].as_f64().unwrap_or(0.0), a["height"].as_f64().unwrap_or(1.0));
            if w / h.max(1.0) >= 1.6 {
                score += 3;
            }
        }
        // Keep the site's relevance order among equals.
        if best.as_ref().is_none_or(|b| score > b.0) {
            let thumb = a["thumb"].as_str().filter(|t| !t.ends_with(".webm")).unwrap_or(url);
            best = Some((score, i, url.to_string(), thumb.to_string()));
        }
    }
    best.map(|(_, _, url, thumb)| (url, thumb))
}

async fn fetch_art(name: &str) -> anyhow::Result<Option<Art>> {
    let v = sgdb_search(name, "grid").await?;
    let games = v["data"]["games"].as_array().cloned().unwrap_or_default();
    let mut best: Option<Art> = None;
    for (rank, g) in games.iter().take(8).enumerate() {
        let gname = g["game"]["name"].as_str().unwrap_or_default();
        let Some((cover, thumb)) = pick_asset(&g["assets"]) else { continue };
        // Relevance order breaks ties; verified games get a small boost.
        let mut score = titles::similarity(name, gname) - rank as f64 * 0.01;
        if g["game"]["verified"].as_bool().unwrap_or(false) {
            score += 0.02;
        }
        if best.as_ref().is_none_or(|b| score > b.score) {
            let year = g["game"]["release_date"].as_i64().map(|ts| 1970 + (ts / 31_556_952) as i32);
            best = Some(Art {
                game_id: g["game"]["id"].as_u64().unwrap_or(0),
                name: gname.to_string(),
                year,
                cover,
                cover_thumb: thumb,
                score,
            });
        }
    }
    Ok(best.filter(|b| b.score >= MIN_SCORE))
}

/// Whether two titles name the same game: close names and the same sequel
/// numbers ("Minecraft Dungeons II" is not "Minecraft Dungeons"; "2" = "II").
pub fn same_game(a: &str, b: &str) -> bool {
    const ROMAN: [&str; 9] = ["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];
    let number = |w: &str| -> Option<u32> {
        let w = w.to_ascii_lowercase();
        if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) {
            return w.parse().ok();
        }
        ROMAN.iter().position(|r| *r == w).map(|i| i as u32 + 2)
    };
    // Words, with sequel numbers written one way.
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| number(w).map(|n| n.to_string()).unwrap_or_else(|| w.to_lowercase())).collect()
    };
    let sequels = |s: &str| -> Vec<u32> {
        let mut v: Vec<u32> = s.split(|c: char| !c.is_ascii_alphanumeric()).filter_map(number).collect();
        v.sort_unstable();
        v
    };
    sequels(a) == sequels(b) && titles::similarity(&words(a).join(" "), &words(b).join(" ")) >= 0.8
}

/// What SteamGridDB has for one game, for the Steam library.
#[derive(Default, Debug, Clone)]
pub struct SgdbPack {
    pub cover: Option<String>,
    pub wide: Option<String>,
    pub hero: Option<String>,
    pub logo: Option<String>,
    pub icon: Option<String>,
}

/// The best static asset Steam can show: PNG/JPEG (icons may be .ico),
/// nothing NSFW/joke/animated, English first, the preferred style, and for
/// logos a wide shape.
fn pick_for_steam(assets: &Value, style: Option<&str>, wide: bool, icon: bool) -> Option<String> {
    let mut best: Option<(i32, String)> = None;
    for a in assets.as_array()? {
        let flag = |k: &str| a[k].as_bool().unwrap_or(false);
        if flag("nsfw") || flag("humor") || flag("epilepsy") || flag("is_animated") {
            continue;
        }
        let mime = a["mime"].as_str().unwrap_or("");
        if !(matches!(mime, "image/png" | "image/jpeg") || (icon && mime.contains("icon"))) {
            continue;
        }
        let Some(url) = a["url"].as_str() else { continue };
        let mut score = 0;
        if a["language"].as_str().is_none_or(|l| l == "en") {
            score += 4;
        }
        if style.is_some() && a["style"].as_str() == style {
            score += 2;
        }
        if wide && a["width"].as_f64().unwrap_or(0.0) / a["height"].as_f64().unwrap_or(1.0).max(1.0) >= 1.6 {
            score += 3;
        }
        if icon && mime == "image/png" {
            score += 1;
        }
        // The site's order (votes) breaks ties.
        if best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, url.to_string()));
        }
    }
    best.map(|(_, url)| url)
}

/// SteamGridDB's art for exactly this game — nothing when it only has
/// look-alikes — one search per kind. `id`: the SteamGridDB game picked by
/// hand (searched by its own name), instead of matching on the name.
pub async fn sgdb_pack(name: &str, id: Option<u64>) -> SgdbPack {
    let kinds = [
        ("grid", json!({ "dimensions": ["600x900", "660x930"] }), None, false),
        ("grid", json!({ "dimensions": ["920x430", "460x215"] }), None, false),
        ("hero", json!({}), Some("alternate"), false),
        ("logo", json!({}), Some("official"), true),
        ("icon", json!({}), Some("official"), false),
    ];
    let mut found: Vec<Option<String>> = Vec::new();
    for (kind, filters, style, wide) in kinds {
        let pick = async {
            let v = sgdb_search_with(name, kind, filters).await.ok()?;
            let games = v["data"]["games"].as_array()?;
            let g = games.iter().find(|g| match id {
                Some(id) => g["game"]["id"].as_u64() == Some(id),
                None => same_game(name, g["game"]["name"].as_str().unwrap_or("")),
            })?;
            pick_for_steam(&g["assets"], style, wide, kind == "icon")
        };
        found.push(pick.await);
    }
    let mut it = found.into_iter();
    let mut next = || it.next().flatten();
    SgdbPack { cover: next(), wide: next(), hero: next(), logo: next(), icon: next() }
}

static HEROES: LazyLock<Mutex<HashMap<u64, Option<String>>>> = LazyLock::new(|| {
    Mutex::new(std::fs::read(data_dir().join(".cache").join("heroes.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
});

/// Wide background art for a matched game (cached on disk).
pub async fn hero(game: &Art) -> Option<String> {
    if let Some(hit) = HEROES.lock().unwrap().get(&game.game_id).cloned() {
        return hit;
    }
    let found = extra_art(game).await;
    let mut m = HEROES.lock().unwrap();
    m.insert(game.game_id, found.clone());
    if let Ok(bytes) = serde_json::to_vec(&*m) {
        let path = data_dir().join(".cache").join("heroes.json");
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::write(path, bytes);
    }
    found
}

/// Hero (wide background) for the details page, for a matched game. Logos
/// are not used: too many fan/joke variants are tagged "official".
async fn extra_art(game: &Art) -> Option<String> {
    let pick = |v: anyhow::Result<Value>, style: &str| -> Option<String> {
        let v = v.ok()?;
        let games = v["data"]["games"].as_array()?.clone();
        let g = games.iter().find(|g| g["game"]["id"].as_u64() == Some(game.game_id))?;
        pick_asset_scored(&g["assets"], Some(style), false).map(|(url, _)| url)
    };
    pick(sgdb_search(&game.name, "hero").await, "alternate")
}

// ---------- image proxy ----------

/// Covers live in `<piShop folder>/.cache/covers` (kept across deploys);
/// falls back to the data dir if the app folder is read-only.
fn cover_cache_dir() -> PathBuf {
    static DIR: LazyLock<PathBuf> = LazyLock::new(|| {
        let preferred = crate::base_dir().join(".cache").join("covers");
        if std::fs::create_dir_all(&preferred).is_ok() {
            return preferred;
        }
        let fallback = data_dir().join(".cache").join("covers");
        let _ = std::fs::create_dir_all(&fallback);
        fallback
    });
    DIR.clone()
}

/// Fetches (and caches on disk) artwork from SteamGridDB's CDN.
pub async fn image(url: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let iic_cdn = config().iic_cdn;
    let allowed = url.starts_with("https://cdn2.steamgriddb.com/")
        || url.starts_with("https://cdn.thegamesdb.net/")
        || url.starts_with("https://shared.akamai.steamstatic.com/")
        || url.starts_with("https://shared.fastly.steamstatic.com/")
        || url.starts_with("https://cdn.akamai.steamstatic.com/")
        || url.starts_with("https://store.akamai.steamstatic.com/")
        || (!iic_cdn.is_empty() && url.starts_with(&format!("{}/", iic_cdn)));
    if !allowed {
        bail!(tr!("image source not allowed", "origem de imagem não permitida"));
    }
    let path = url.split('?').next().unwrap_or(url);
    let ext = path.rsplit('.').next().filter(|e| e.len() <= 4).unwrap_or("img").to_string();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
    .to_string();
    let dir = cover_cache_dir();
    let path = dir.join(format!("{}.{ext}", short_hash(url)));
    if let Ok(bytes) = tokio::fs::read(&path).await {
        return Ok((bytes, mime));
    }
    let r = HTTP.get(url).send().await?.error_for_status()?;
    let bytes = r.bytes().await?.to_vec();
    // Write-then-rename so a half-written file is never served from cache.
    let tmp = path.with_extension("part");
    if tokio::fs::write(&tmp, &bytes).await.is_ok() {
        let _ = tokio::fs::rename(&tmp, &path).await;
    }
    Ok((bytes, mime))
}

// ---------- details ----------

static DESCRIPTIONS: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Best-effort description: The Pirate Bay's API has one per torrent.
async fn description(s: &Stored) -> Option<String> {
    let id = s.tpb_id.as_deref()?;
    let base = tpb_base(&config());
    if let Some(hit) = DESCRIPTIONS.lock().unwrap().get(id).cloned() {
        return hit;
    }
    let text = tpb::description(&base, id).await;
    DESCRIPTIONS.lock().unwrap().insert(id.to_string(), text.clone());
    text
}

/// `with_art: false` skips the SteamGridDB lookup by torrent name: the Store
/// already matched the game for the search and passes that along instead.
pub async fn details(id: &str, with_art: bool) -> anyhow::Result<Value> {
    let s = stored(id)?;
    let r = &s.release;
    let game = if with_art { art(&r.parsed.name).await } else { None };
    let hero = match &game {
        Some(g) => hero(g).await,
        None => None,
    };
    let desc = description(&s).await;
    Ok(json!({ "release": r, "art": game, "hero": hero, "description": desc }))
}

// ---------- torrent source & download ----------

enum Source {
    Magnet(String),
    TorrentFile(Vec<u8>),
}

async fn resolve_source(s: &Stored) -> anyhow::Result<Source> {
    if s.guid.starts_with("magnet:") {
        return Ok(Source::Magnet(s.guid.clone()));
    }
    let url = s.magnet_url.as_deref().or(s.download_url.as_deref()).ok_or_else(|| anyhow!(tr!("the indexer didn't provide a link", "o indexador não forneceu link")))?;
    if url.starts_with("magnet:") {
        return Ok(Source::Magnet(url.to_string()));
    }
    let r = HTTP_NOREDIRECT.get(url).send().await.with_context(|| tr!("Prowlarr didn't respond when asked for the torrent", "Prowlarr não respondeu ao pedir o torrent"))?;
    if r.status().is_redirection() {
        let loc = r.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or_default().to_string();
        if loc.starts_with("magnet:") {
            return Ok(Source::Magnet(loc));
        }
        let r = HTTP.get(&loc).send().await?.error_for_status()?;
        return Ok(Source::TorrentFile(r.bytes().await?.to_vec()));
    }
    let r = r.error_for_status()?;
    let bytes = r.bytes().await?.to_vec();
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if text.trim_start().starts_with("magnet:") {
            return Ok(Source::Magnet(text.trim().to_string()));
        }
    }
    Ok(Source::TorrentFile(bytes))
}

async fn rqbit_add(src: Source, query: &[(&str, &str)]) -> anyhow::Result<Value> {
    if !torrent::running() {
        bail!(tr!("the torrent engine isn't ready yet; try again in a few seconds", "o motor de torrents ainda não está pronto; tente de novo em alguns segundos"));
    }
    let body = match src {
        Source::Magnet(m) => m.into_bytes(),
        Source::TorrentFile(b) => b,
    };
    let r = HTTP
        .post(format!("http://127.0.0.1:{}/torrents", torrent::api_port()))
        .query(query)
        .body(body)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .with_context(|| tr!("the torrent engine didn't respond", "o motor de torrents não respondeu"))?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        bail!(v["human_readable"].as_str().or(v["error"].as_str()).map(String::from).unwrap_or_else(|| tr!("couldn't add the torrent", "falha ao adicionar o torrent")));
    }
    Ok(v)
}

/// File list read from the swarm (magnet metadata) without downloading.
pub async fn files(id: &str) -> anyhow::Result<Value> {
    let s = stored(id)?;
    let src = resolve_source(&s).await?;
    let v = rqbit_add(src, &[("list_only", "true"), ("overwrite", "true")]).await?;
    let files: Vec<Value> = v["details"]["files"]
        .as_array()
        .map(|a| a.iter().map(|f| json!({ "name": f["name"], "length": f["length"] })).collect())
        .unwrap_or_default();
    Ok(json!({ "name": v["details"]["name"], "files": files }))
}

/// `hint`: what the Store showed for the search (SteamGridDB + isitcracked).
/// Transfers gets that right away; Steam and TheGamesDB are asked in the
/// background and take precedence (see `library`).
pub async fn download(id: &str, dest: Option<String>, hint: Option<library::Hint>) -> anyhow::Result<Value> {
    let s = stored(id)?;
    let src = resolve_source(&s).await?;
    let mut q: Vec<(&str, &str)> = vec![("overwrite", "true")];
    if let Some(d) = dest.as_deref() {
        std::fs::create_dir_all(d).with_context(|| tr!("couldn't create {d}", "não foi possível criar {d}"))?;
        q.push(("output_folder", d));
    }
    let v = rqbit_add(src, &q).await?;
    log!("catálogo: baixando {:?} → {:?}", s.release.title, dest);
    let info_hash = v["details"]["info_hash"].as_str().unwrap_or_default().to_lowercase();
    let r = &s.release;
    let hint = hint.unwrap_or_default();
    let fallback = r.parsed.name.clone();
    let platform = r.parsed.platform_label.map(String::from);
    // Steam is searched by name for PC releases only (unknown counts as PC).
    let pc = r.parsed.platform_label.is_none_or(|p| p == "PC");
    library::add(library::Entry {
        info_hash: info_hash.clone(),
        game: library::quick(&hint, &fallback, platform.clone()),
        release: r.title.clone(),
        indexer: r.indexer.clone(),
        size: r.size,
        dest,
        added: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
        resolved: false,
        install: None,
    });
    let hash = info_hash.clone();
    tokio::spawn(async move {
        let game = library::resolve(&hint, &fallback, platform, pc).await;
        log!("biblioteca: {:?} ← {}", game.name, game.sources.join(" → "));
        library::update_game(&hash, game);
    });
    Ok(json!({ "torrent_id": v["id"], "name": v["details"]["name"], "info_hash": info_hash }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequels_must_match() {
        assert!(same_game("Minecraft Dungeons II", "Minecraft Dungeons II"));
        assert!(!same_game("Minecraft Dungeons II", "Minecraft Dungeons"));
        assert!(same_game("Dark Souls 2", "Dark Souls II"));
        assert!(!same_game("Cyberpunk 2077", "Cyberpunk"));
        assert!(same_game("The Walking Dead: Streets of Survival", "The Walking Dead Streets of Survival"));
    }

    #[test]
    fn steam_needs_static_pictures() {
        let assets = serde_json::json!([
            { "url": "a.webp", "mime": "image/webp", "language": "en" },
            { "url": "b.png", "mime": "image/png", "language": "ru", "style": "official" },
            { "url": "c.png", "mime": "image/png", "language": "en", "is_animated": true },
            { "url": "d.png", "mime": "image/png", "language": "en", "style": "official", "width": 900, "height": 300 },
        ]);
        assert_eq!(pick_for_steam(&assets, Some("official"), true, false).as_deref(), Some("d.png"));
        let icons = serde_json::json!([{ "url": "i.ico", "mime": "image/vnd.microsoft.icon", "language": "en" }]);
        assert_eq!(pick_for_steam(&icons, None, false, true).as_deref(), Some("i.ico"));
        assert_eq!(pick_for_steam(&icons, None, false, false), None);
    }
}
