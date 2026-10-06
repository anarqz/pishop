//! Native The Pirate Bay search through its public JSON API (the apibay
//! format: `q.php` search, `t.php` details). The endpoint is set in
//! Settings → Serviços; nothing is built in. Magnets are assembled locally.

use std::time::Duration;

use anyhow::{anyhow, bail};
use serde_json::Value;

/// Shown as the release's indexer.
pub const LABEL: &str = "TPB nativo";

/// Public trackers TPB itself appends to its magnet links.
const TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://tracker.bittor.pw:1337/announce",
    "udp://public.popcorn-tracker.org:6969/announce",
    "udp://tracker.dler.org:6969/announce",
    "udp://exodus.desync.com:6969",
    "udp://open.demonii.com:1337/announce",
];

pub struct Hit {
    pub id: String,
    pub name: String,
    pub info_hash: String,
    pub seeders: u32,
    pub leechers: u32,
    pub size: u64,
    pub files: Option<u32>,
    /// Unix seconds.
    pub added: i64,
    pub category: u32,
}

/// TPB game categories per Store tab.
pub fn categories(kind: &str) -> &'static str {
    match kind {
        "pc" => "401",
        _ => "403,404,405,406,499",
    }
}

pub fn category_name(c: u32) -> &'static str {
    match c {
        401 => "PC/Games",
        402 => "Mac/Games",
        403 => "Console/PlayStation",
        404 => "Console/Xbox 360",
        405 => "Console/Wii",
        406 => "Console/Handheld",
        499 => "Console/Other",
        _ => "Games",
    }
}

fn num(v: &Value) -> u64 {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0)
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("piShop/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

/// GET with one patient retry on 429: the API rate-limits bursts.
async fn get_json(url: &str, query: &[(&str, &str)]) -> anyhow::Result<Value> {
    let c = client()?;
    for attempt in 0..2 {
        let r = c.get(url).query(query).send().await.map_err(|e| anyhow!("não respondeu: {e}"))?;
        match r.status().as_u16() {
            200 => return Ok(r.json().await?),
            429 if attempt == 0 => tokio::time::sleep(Duration::from_millis(1500)).await,
            429 => bail!("limite de requisições atingido; tente de novo em instantes"),
            s => bail!("respondeu {s}"),
        }
    }
    unreachable!()
}

pub async fn search(base: &str, query: &str, cats: &str) -> anyhow::Result<Vec<Hit>> {
    let base = base.trim().trim_end_matches('/');
    let v = get_json(&format!("{base}/q.php"), &[("q", query), ("cat", cats)]).await?;
    let rows = v.as_array().ok_or_else(|| anyhow!("resposta inesperada"))?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let id = r["id"].as_str().map(String::from).or_else(|| r["id"].as_u64().map(|n| n.to_string()))?;
            let hash = r["info_hash"].as_str()?.trim().to_lowercase();
            // "No results returned" placeholder: id 0 / all-zero hash.
            if id == "0" || hash.chars().all(|c| c == '0') {
                return None;
            }
            Some(Hit {
                id,
                name: r["name"].as_str()?.to_string(),
                info_hash: hash,
                seeders: num(&r["seeders"]) as u32,
                leechers: num(&r["leechers"]) as u32,
                size: num(&r["size"]),
                files: Some(num(&r["num_files"]) as u32).filter(|n| *n > 0),
                added: num(&r["added"]) as i64,
                category: num(&r["category"]) as u32,
            })
        })
        .collect())
}

/// Full description text of one torrent (`t.php`), if any.
pub async fn description(base: &str, id: &str) -> Option<String> {
    let base = base.trim().trim_end_matches('/');
    let v = get_json(&format!("{base}/t.php"), &[("id", id)]).await.ok()?;
    v["descr"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn magnet(info_hash: &str, name: &str) -> String {
    let mut m = format!("magnet:?xt=urn:btih:{info_hash}&dn={}", urlencode(name));
    for t in TRACKERS {
        m.push_str("&tr=");
        m.push_str(&urlencode(t));
    }
    m
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Unix seconds → "YYYY-MM-DDTHH:MM:SSZ" (UTC), the format the UI parses.
pub fn iso8601(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_273_516_061), "2010-05-10T18:27:41Z");
    }

    #[test]
    fn magnet_link() {
        let m = magnet("abc123", "Some Game (PC)");
        assert!(m.starts_with("magnet:?xt=urn:btih:abc123&dn=Some%20Game%20%28PC%29&tr=udp%3A%2F%2F"));
    }
}
