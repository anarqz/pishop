//! Compatibility reports for Proton: ProtonDB's summary (the tier) and the
//! game's issue on Valve's Proton tracker (GitHub), whose comments are player
//! reports. Both are cached on disk; GitHub's anonymous API is rate-limited,
//! so saved reports are served when it says no.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex as AsyncMutex;

use crate::{data_dir, log, titles, tr};

const PROTON_REPO: &str = "ValveSoftware/Proton";
const HOUR: u64 = 3600;
/// ProtonDB summaries: hits for a day, misses (no reports yet) for 6 h.
const PDB_HIT_TTL: u64 = 24 * HOUR;
const PDB_MISS_TTL: u64 = 6 * HOUR;
const ISSUES_TTL: u64 = 6 * HOUR;
/// Newest comments shown for the game's issue.
const COMMENTS: usize = 20;
const BODY_MAX: usize = 1200;

pub fn router() -> Router {
    Router::new().route("/api/compat/protondb", get(protondb_route)).route("/api/compat/issues", get(issues_route))
}

fn err(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": msg.to_string() }))).into_response()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!("piShop/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

// ---------- disk cache ----------

#[derive(Serialize, Deserialize, Clone)]
struct Cached<T> {
    at: u64,
    v: T,
}

fn cache_file(name: &str) -> PathBuf {
    data_dir().join(".cache").join(name)
}

fn load<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read(cache_file(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn store<T: Serialize>(name: &str, v: &T) {
    let path = cache_file(name);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if let Ok(bytes) = serde_json::to_vec(v) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

// ---------- ProtonDB ----------

/// ProtonDB's summary, field names as ProtonDB sends them.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub tier: String,
    #[serde(default)]
    pub best_reported_tier: Option<String>,
    #[serde(default)]
    pub trending_tier: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub total: Option<u64>,
}

static PDB: LazyLock<Mutex<HashMap<String, Cached<Option<Summary>>>>> = LazyLock::new(|| Mutex::new(load("protondb.json")));

/// `None` when ProtonDB has no reports for the app.
pub async fn protondb(appid: &str) -> anyhow::Result<Option<Summary>> {
    let cached = PDB.lock().unwrap().get(appid).cloned();
    if let Some(c) = &cached {
        let ttl = if c.v.is_some() { PDB_HIT_TTL } else { PDB_MISS_TTL };
        if now().saturating_sub(c.at) < ttl {
            return Ok(c.v.clone());
        }
    }
    let fetched = async {
        let r = client()?.get(format!("https://www.protondb.com/api/v1/reports/summaries/{appid}.json")).send().await?;
        match r.status().as_u16() {
            200 => Ok(Some(r.json::<Summary>().await?)),
            404 => Ok(None),
            s => Err(anyhow!("ProtonDB {s}")),
        }
    }
    .await;
    match fetched {
        Ok(v) => {
            let mut m = PDB.lock().unwrap();
            m.insert(appid.to_string(), Cached { at: now(), v: v.clone() });
            store("protondb.json", &*m);
            Ok(v)
        }
        // Offline or ProtonDB down: an old answer beats none.
        Err(e) => match cached {
            Some(c) => Ok(c.v),
            None => {
                log!("compat: protondb {appid}: {e:#}");
                bail!(tr!("ProtonDB didn't respond; try again later", "O ProtonDB não respondeu; tente de novo mais tarde"))
            }
        },
    }
}

#[derive(Deserialize)]
struct CompatQuery {
    #[serde(default)]
    appid: String,
    #[serde(default)]
    name: String,
}

fn digits(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).then(|| s.to_string())
}

/// The appid to use: the one given, else Steam's store search for the name.
async fn resolve_appid(q: &CompatQuery) -> Option<String> {
    match digits(&q.appid) {
        Some(a) => Some(a),
        None if !q.name.trim().is_empty() => crate::steam_store::find_appid(q.name.trim()).await,
        None => None,
    }
}

async fn protondb_route(Query(q): Query<CompatQuery>) -> Response {
    let Some(appid) = resolve_appid(&q).await else {
        return Json(Value::Null).into_response();
    };
    match protondb(&appid).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

// ---------- Proton issue tracker (GitHub) ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub comments: u64,
    pub updated_at: String,
    pub labels: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Comment {
    pub author: String,
    pub created_at: String,
    pub body: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Other {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Reports {
    pub issue: Option<Issue>,
    pub comments: Vec<Comment>,
    pub others: Vec<Other>,
    /// The appid the search used (given, or found by name).
    #[serde(default)]
    pub appid: Option<String>,
    /// Served from the cache because GitHub refused or failed.
    #[serde(default)]
    pub stale: bool,
}

static ISSUES: LazyLock<Mutex<HashMap<String, Cached<Reports>>>> = LazyLock::new(|| Mutex::new(load("proton-issues.json")));
/// One GitHub conversation at a time: the anonymous limits are tiny.
static GITHUB: AsyncMutex<()> = AsyncMutex::const_new(());

enum GhError {
    Limited,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for GhError {
    fn from(e: anyhow::Error) -> Self {
        GhError::Other(e)
    }
}

impl From<reqwest::Error> for GhError {
    fn from(e: reqwest::Error) -> Self {
        GhError::Other(e.into())
    }
}

async fn gh(path: &str, query: &[(&str, &str)]) -> Result<Value, GhError> {
    let r = client()?
        .get(format!("https://api.github.com{path}"))
        .query(query)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;
    match r.status().as_u16() {
        200 => Ok(r.json().await?),
        // 403 with no requests left, or the secondary limit: either way, wait.
        403 | 429 => Err(GhError::Limited),
        s => Err(GhError::Other(anyhow!("GitHub {s}"))),
    }
}

fn issue_from(v: &Value) -> Option<Issue> {
    Some(Issue {
        number: v["number"].as_u64()?,
        title: v["title"].as_str()?.to_string(),
        url: v["html_url"].as_str().unwrap_or_default().to_string(),
        state: v["state"].as_str().unwrap_or_default().to_string(),
        comments: v["comments"].as_u64().unwrap_or(0),
        updated_at: v["updated_at"].as_str().unwrap_or_default().to_string(),
        labels: v["labels"]
            .as_array()
            .map(|l| l.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect())
            .unwrap_or_default(),
    })
}

/// "Cyberpunk 2077 (1091500)" → ("Cyberpunk 2077", Some("1091500")).
fn split_title(title: &str) -> (&str, Option<&str>) {
    let t = title.trim();
    if let Some(open) = t.rfind('(') {
        let inner = t[open + 1..].trim_end_matches(')').trim();
        if t.ends_with(')') && !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
            return (t[..open].trim(), Some(inner));
        }
    }
    (t, None)
}

/// How likely an issue is the game's main report thread (higher is better).
fn rank(i: &Issue, appid: Option<&str>, name: &str) -> f64 {
    let (title_name, title_appid) = split_title(&i.title);
    let mut score = match (appid, title_appid) {
        (Some(a), Some(b)) if a == b => 2.0,
        (Some(_), Some(_)) => -2.0,
        _ => 0.0,
    };
    if !name.is_empty() {
        score += titles::similarity(name, title_name);
    }
    // The main thread is titled just "Name (appid)" and gathers the most comments.
    if title_appid.is_some() {
        score += 0.5;
    }
    score + (i.comments as f64 + 1.0).ln() * 0.05
}

async fn search(q: &str) -> Result<Vec<Issue>, GhError> {
    let full = format!("repo:{PROTON_REPO} is:issue in:title {q}");
    let v = gh("/search/issues", &[("q", full.as_str()), ("sort", "comments"), ("order", "desc"), ("per_page", "20")]).await?;
    Ok(v["items"].as_array().map(|a| a.iter().filter_map(issue_from).collect()).unwrap_or_default())
}

async fn newest_comments(issue: &Issue) -> Result<Vec<Comment>, GhError> {
    if issue.comments == 0 {
        return Ok(Vec::new());
    }
    const PER: u64 = 30;
    let last = issue.comments.div_ceil(PER).max(1);
    let path = format!("/repos/{PROTON_REPO}/issues/{}/comments", issue.number);
    let page = |p: u64| {
        let path = path.clone();
        async move {
            let ps = p.to_string();
            let per = PER.to_string();
            gh(&path, &[("per_page", per.as_str()), ("page", ps.as_str())]).await
        }
    };
    let mut raw: Vec<Value> = page(last).await?.as_array().cloned().unwrap_or_default();
    // A nearly empty last page: the one before completes the newest batch.
    if raw.len() < COMMENTS && last > 1 {
        let mut prev = page(last - 1).await?.as_array().cloned().unwrap_or_default();
        prev.append(&mut raw);
        raw = prev;
    }
    let mut out: Vec<Comment> = raw
        .iter()
        .filter_map(|c| {
            let body = clean_markdown(c["body"].as_str().unwrap_or_default());
            (!body.is_empty()).then(|| Comment {
                author: c["user"]["login"].as_str().unwrap_or("?").to_string(),
                created_at: c["created_at"].as_str().unwrap_or_default().to_string(),
                body,
                url: c["html_url"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect();
    out.reverse(); // newest first
    out.truncate(COMMENTS);
    Ok(out)
}

async fn fetch_reports(appid: Option<&str>, name: &str) -> Result<Reports, GhError> {
    let mut found = match appid {
        Some(a) => search(a).await?,
        None => Vec::new(),
    };
    if found.iter().all(|i| split_title(&i.title).1 != appid) && !name.is_empty() {
        let quoted = format!("\"{}\"", name.replace('"', ""));
        for i in search(&quoted).await? {
            if !found.iter().any(|f| f.number == i.number) {
                found.push(i);
            }
        }
    }
    let mut ranked: Vec<(f64, Issue)> = found.into_iter().map(|i| (rank(&i, appid, name), i)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
    // Below this the "best" match is a different game.
    let main = ranked.first().filter(|(s, _)| *s >= 0.75).map(|(_, i)| i.clone());
    let others = ranked
        .iter()
        .filter(|(s, i)| *s >= 0.5 && Some(i.number) != main.as_ref().map(|m| m.number))
        .take(5)
        .map(|(_, i)| Other { number: i.number, title: i.title.clone(), url: i.url.clone(), state: i.state.clone() })
        .collect();
    let comments = match &main {
        Some(i) => newest_comments(i).await?,
        None => Vec::new(),
    };
    Ok(Reports { issue: main, comments, others, appid: appid.map(String::from), stale: false })
}

pub async fn reports(appid: Option<String>, name: &str) -> anyhow::Result<Reports> {
    let key = match &appid {
        Some(a) => format!("appid:{a}"),
        None => format!("name:{}", name.trim().to_lowercase()),
    };
    let fresh = |c: &Cached<Reports>| now().saturating_sub(c.at) < ISSUES_TTL;
    if let Some(c) = ISSUES.lock().unwrap().get(&key).filter(|c| fresh(c)) {
        return Ok(c.v.clone());
    }
    let _one = GITHUB.lock().await;
    let cached = ISSUES.lock().unwrap().get(&key).cloned();
    if let Some(c) = cached.as_ref().filter(|c| fresh(c)) {
        return Ok(c.v.clone());
    }
    match fetch_reports(appid.as_deref(), name.trim()).await {
        Ok(r) => {
            let mut m = ISSUES.lock().unwrap();
            m.insert(key, Cached { at: now(), v: r.clone() });
            store("proton-issues.json", &*m);
            Ok(r)
        }
        Err(e) => {
            if let Some(mut c) = cached {
                c.v.stale = true;
                return Ok(c.v);
            }
            match e {
                GhError::Limited => bail!(tr!(
                    "GitHub is limiting requests right now; try again in a few minutes",
                    "O GitHub está limitando as consultas agora; tente de novo em alguns minutos"
                )),
                GhError::Other(e) => {
                    log!("compat: issues {key}: {e:#}");
                    bail!(tr!("The Proton issue tracker didn't respond; try again later", "O rastreador de problemas do Proton não respondeu; tente de novo mais tarde"))
                }
            }
        }
    }
}

async fn issues_route(Query(q): Query<CompatQuery>) -> Response {
    if digits(&q.appid).is_none() && q.name.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, tr!("tell me the game (appid or name)", "informe o jogo (appid ou nome)"));
    }
    let appid = resolve_appid(&q).await;
    match reports(appid, &q.name).await {
        Ok(r) => Json(r).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

// ---------- comment text ----------

/// GitHub markdown/HTML → readable plain text: logs, system dumps (<details>,
/// code fences), quotes and images go; links keep their text.
fn clean_markdown(src: &str) -> String {
    let mut s = src.replace("\r\n", "\n");
    // Collapsed sections hold system information and logs.
    s = cut_between(&s, "<details", "</details>", "");
    // Fenced code: logs and specs.
    let mut out = String::new();
    let mut in_fence = false;
    for line in s.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        // Fenced logs, quoted replies and horizontal rules ("---", "* * *").
        let rule = t.len() >= 3 && t.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) && t.chars().filter(|c| !c.is_whitespace()).count() >= 3;
        if in_fence || t.starts_with('>') || rule {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    s = out;
    // Images, then links → their text.
    s = replace_md_links(&s);
    // Remaining HTML tags.
    s = strip_tags(&s);
    s = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    // Markdown decoration.
    let lines: Vec<String> = s
        .lines()
        .map(|l| {
            let l = l.trim_end();
            let l = l.trim_start_matches(|c| c == '#').trim_start();
            let l = l.replace("**", "").replace("__", "").replace('`', "");
            if let Some(rest) = l.trim_start().strip_prefix("- [ ] ").or_else(|| l.trim_start().strip_prefix("- [x] ")) {
                format!("• {rest}")
            } else if let Some(rest) = l.trim_start().strip_prefix("- ").or_else(|| l.trim_start().strip_prefix("* ")) {
                format!("• {rest}")
            } else {
                l
            }
        })
        .collect();
    // Collapse blank runs.
    let mut text = String::new();
    let mut blank = 0;
    for l in lines {
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        text.push_str(&l);
        text.push('\n');
    }
    let text = text.trim().to_string();
    truncate_words(&text, BODY_MAX)
}

fn cut_between(s: &str, open: &str, close: &str, with: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::new();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find(open).map(|i| i + pos) {
        out.push_str(&s[pos..start]);
        match lower[start..].find(close).map(|i| i + start + close.len()) {
            Some(end) => {
                out.push_str(with);
                pos = end;
            }
            None => {
                pos = s.len();
                break;
            }
        }
    }
    out.push_str(&s[pos.min(s.len())..]);
    out
}

/// `![alt](url)` → "" and `[text](url)` → "text".
fn replace_md_links(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        let image = b[i] == b'!' && b.get(i + 1) == Some(&b'[');
        if b[i] == b'[' || image {
            let open = if image { i + 1 } else { i };
            if let Some(close) = s[open..].find("](").map(|k| k + open) {
                if let Some(end) = s[close + 2..].find(')').map(|k| k + close + 2) {
                    // Only plain link targets (no line breaks inside).
                    if !s[open..end].contains('\n') {
                        if !image {
                            out.push_str(&s[open + 1..close]);
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag.trim_start_matches('/').to_lowercase();
                if t.starts_with("br") || t.starts_with("p") || t.starts_with("li") {
                    out.push('\n');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    // An unclosed "<" was just text (e.g. "< 30 fps").
    if in_tag {
        out.push('<');
        out.push_str(&tag);
    }
    out
}

fn truncate_words(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    let at = cut.rfind(char::is_whitespace).filter(|&i| i > max / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(split_title("Cyberpunk 2077 (1091500)"), ("Cyberpunk 2077", Some("1091500")));
        assert_eq!(split_title("Cyberpunk 2077 (1091500): crash on launch"), ("Cyberpunk 2077 (1091500): crash on launch", None));
        assert_eq!(split_title("Portal 2"), ("Portal 2", None));
    }

    #[test]
    fn main_thread_wins() {
        let i = |n, t: &str, c| Issue { number: n, title: t.into(), url: String::new(), state: "open".into(), comments: c, updated_at: String::new(), labels: vec![] };
        let main = i(1, "Cyberpunk 2077 (1091500)", 1800);
        let crash = i(2, "Cyberpunk 2077 (1091500): Immediate crash on launch with gamedrive option", 1);
        let other = i(3, "Cyberpunk Red (999)", 3);
        let a = Some("1091500");
        assert!(rank(&main, a, "Cyberpunk 2077") > rank(&crash, a, "Cyberpunk 2077"));
        assert!(rank(&main, a, "Cyberpunk 2077") > rank(&other, a, "Cyberpunk 2077"));
        assert!(rank(&main, None, "Cyberpunk 2077") >= 0.75);
    }

    #[test]
    fn markdown() {
        let src = "### Works great\r\n\r\nProton **9.0-4**, see [log](https://x/y.txt) ![shot](https://i/p.png)\n\n```\nwine: err:module\n```\n> quoted text\n- - -\n<details><summary>System Information</summary>\nGPU: whatever\n</details>\n- runs at 60 fps<br>no crashes\n";
        let out = clean_markdown(src);
        assert_eq!(out, "Works great\n\nProton 9.0-4, see log\n\n• runs at 60 fps\nno crashes");
    }

    #[test]
    fn long_bodies_are_cut_on_a_word() {
        let s = "word ".repeat(400);
        let t = truncate_words(s.trim(), 50);
        assert!(t.ends_with('…') && t.chars().count() <= 51);
    }
}
