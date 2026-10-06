//! "Discover": latest cracked PC games from isitcracked.com's public
//! (anon-key) Supabase API, paginated and searchable.

use std::time::Duration;

use anyhow::bail;
use serde::Serialize;
use serde_json::{Value, json};

/// Endpoint, key and CDN come from Settings → Serviços.
struct Api {
    url: String,
    key: String,
    cdn: String,
}

fn api() -> anyhow::Result<Api> {
    let c = crate::catalog::config();
    if c.iic_url.trim().is_empty() || c.iic_key.trim().is_empty() {
        bail!("configure o isitcracked em Configurações → Serviços");
    }
    Ok(Api { url: c.iic_url.trim().to_string(), key: c.iic_key.trim().to_string(), cdn: c.iic_cdn.trim().trim_end_matches('/').to_string() })
}

#[derive(Serialize, Clone)]
pub struct Game {
    pub id: String,
    pub title: String,
    pub steam_appid: Option<String>,
    pub cover: Option<String>,
    pub header: Option<String>,
    pub crack_date: Option<String>,
    pub release_date: Option<String>,
    pub scene_group: Option<String>,
    pub drm: Option<String>,
}

#[derive(Serialize)]
pub struct Page {
    pub items: Vec<Game>,
    pub total: u64,
    pub offset: u64,
}

fn cdn(base: &str, path: &Value) -> Option<String> {
    let p = path.as_str()?.trim();
    if p.is_empty() || (!p.starts_with("http") && base.is_empty()) {
        return None;
    }
    Some(if p.starts_with("http") { p.to_string() } else { format!("{base}{p}") })
}

pub async fn page(search: Option<&str>, offset: u64, limit: u64) -> anyhow::Result<Page> {
    page_with(&api()?, search, offset, limit).await
}

/// Settings "Testar": one tiny page with the given values.
pub async fn test(url: &str, key: &str, cdn: &str) -> anyhow::Result<u64> {
    let a = Api { url: url.trim().to_string(), key: key.trim().to_string(), cdn: cdn.trim().trim_end_matches('/').to_string() };
    Ok(page_with(&a, None, 0, 1).await?.total)
}

async fn page_with(a: &Api, search: Option<&str>, offset: u64, limit: u64) -> anyhow::Result<Page> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?;
    let r = client
        .post(&a.url)
        .header("apikey", &a.key)
        .header("Authorization", format!("Bearer {}", a.key))
        .json(&json!({
            "p_status": "cracked",
            "p_search": search.filter(|s| !s.trim().is_empty()),
            "p_limit": limit.clamp(1, 60),
            "p_offset": offset,
        }))
        .send()
        .await?;
    if !r.status().is_success() {
        bail!("isitcracked respondeu {}", r.status());
    }
    let rows: Vec<Value> = r.json().await?;
    let total = rows.first().and_then(|g| g["total_count"].as_u64()).unwrap_or(0);
    let s = |v: &Value| v.as_str().filter(|x| !x.is_empty()).map(String::from);
    let items = rows
        .iter()
        .map(|g| Game {
            id: g["id"].as_str().unwrap_or_default().to_string(),
            title: g["title"].as_str().unwrap_or_default().to_string(),
            steam_appid: s(&g["steam_appid"]),
            cover: cdn(&a.cdn, &g["portrait_url"]).or_else(|| cdn(&a.cdn, &g["cover_url"])),
            header: cdn(&a.cdn, &g["header_url"]).or_else(|| cdn(&a.cdn, &g["cover_url"])),
            crack_date: s(&g["crack_date"]),
            release_date: s(&g["release_date"]),
            scene_group: s(&g["scene_group"]),
            drm: s(&g["drm_protection"]),
        })
        .collect();
    Ok(Page { items, total, offset })
}

/// Closest cracked game for a free-text search (Store header), if any.
pub async fn best_match(query: &str) -> Option<Game> {
    let page = page(Some(query), 0, 10).await.ok()?;
    page.items
        .into_iter()
        .map(|g| (crate::titles::similarity(query, &g.title), g))
        .filter(|(s, _)| *s >= 0.6)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, g)| g)
}
