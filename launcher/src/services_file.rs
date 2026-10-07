//! Settings → Services, Import / Export: the services setup (endpoints and
//! API keys) as a file, so it can be shared with others. Exports land in
//! ~/Downloads; imports are picked from the usual places a shared file ends
//! up (Downloads, home, Desktop, SD cards and USB drives), or downloaded from
//! a link: a GitHub gist, a Pastebin paste or any raw file.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::catalog::{self, ConfigUpdate};
use crate::tr;

const MARK: &str = "services";
const FILE_NAME: &str = "piShop-services.json";
/// Anything bigger isn't one of ours.
const MAX_SIZE: u64 = 64 * 1024;

#[derive(Serialize, Deserialize, Default)]
struct Services {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    prowlarr_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    prowlarr_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    tpb_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    tgdb_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    iic_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    iic_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    iic_cdn: String,
}

#[derive(Serialize, Deserialize)]
struct ServicesFile {
    #[serde(rename = "piShop")]
    kind: String,
    version: u32,
    #[serde(default)]
    exported: String,
    services: Services,
}

impl Services {
    /// Service names this file sets, for the UI.
    fn names(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.tpb_url.is_empty() {
            out.push("The Pirate Bay");
        }
        if !self.prowlarr_url.is_empty() || !self.prowlarr_key.is_empty() {
            out.push("Prowlarr");
        }
        if !self.tgdb_key.is_empty() {
            out.push("TheGamesDB");
        }
        if !self.iic_url.is_empty() || !self.iic_key.is_empty() {
            out.push("isitcracked");
        }
        out.into_iter().map(String::from).collect()
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Writes the current services setup, secrets included, to ~/Downloads.
pub fn export() -> anyhow::Result<PathBuf> {
    let c = catalog::config();
    let file = ServicesFile {
        kind: MARK.into(),
        version: 1,
        exported: crate::tpb::iso8601(std::time::SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64),
        services: Services {
            prowlarr_url: c.prowlarr_url,
            prowlarr_key: c.prowlarr_key,
            tpb_url: c.tpb_url,
            tgdb_key: c.tgdb_key,
            iic_url: c.iic_url,
            iic_key: c.iic_key,
            iic_cdn: c.iic_cdn,
        },
    };
    if file.services.names().is_empty() {
        bail!(tr!("there's nothing to export yet: set up a service first", "ainda não há nada para exportar: configure um serviço primeiro"));
    }
    let dir = home().join("Downloads");
    std::fs::create_dir_all(&dir).with_context(|| tr!("couldn't create {}", "não foi possível criar {}", dir.display()))?;
    let path = dir.join(FILE_NAME);
    write_private(&path, &serde_json::to_vec_pretty(&file)?)?;
    crate::log!("serviços: exportados para {}", path.display());
    Ok(path)
}

/// Keys inside: readable by the owner only.
fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}

fn read(path: &Path) -> Option<ServicesFile> {
    load(path).ok()
}

/// A services file on disk, or why it can't be used.
fn load(path: &Path) -> Result<ServicesFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_SIZE {
        return Err(not_ours());
    }
    parse(&std::fs::read(path).map_err(|e| e.to_string())?)
}

fn not_ours() -> String {
    tr!("not a piShop services file", "não é um arquivo de serviços do piShop")
}

/// A services file's contents. A UTF-8 BOM (some editors save one) is fine,
/// and so are closing braces lost at the end — a paste missing its last
/// line, which is how files shared by hand usually break. Anything else that
/// isn't JSON says where it breaks.
fn parse(bytes: &[u8]) -> Result<ServicesFile, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut text = String::from_utf8_lossy(bytes).trim_end().to_string();
    let mut first: Option<serde_json::Error> = None;
    for _ in 0..4 {
        match serde_json::from_str::<ServicesFile>(&text) {
            Ok(file) if file.kind == MARK => return Ok(file),
            Ok(_) => return Err(not_ours()),
            // Ended inside an object: give it the brace it lost and look again.
            Err(e) if e.is_eof() => {
                first.get_or_insert(e);
                text.push('}');
            }
            Err(e) if e.is_data() && first.is_none() => return Err(not_ours()),
            Err(e) => {
                let e = first.unwrap_or(e);
                return Err(tr!(
                    "the file isn't valid JSON (line {}, column {}): was it copied whole?",
                    "o arquivo não é um JSON válido (linha {}, coluna {}): foi copiado inteiro?",
                    e.line(),
                    e.column()
                ));
            }
        }
    }
    let e = first.map(|e| (e.line(), e.column())).unwrap_or((0, 0));
    Err(tr!(
        "the file isn't valid JSON (line {}, column {}): was it copied whole?",
        "o arquivo não é um JSON válido (linha {}, coluna {}): foi copiado inteiro?",
        e.0,
        e.1
    ))
}

#[derive(Serialize)]
pub struct Candidate {
    path: String,
    name: String,
    modified: i64,
    services: Vec<String>,
}

/// piShop services files in Downloads, home, Desktop and on removable media.
pub fn candidates() -> Vec<Candidate> {
    let home = home();
    let mut dirs = vec![home.join("Downloads"), home.join("Downloads").join(crate::APP_NAME), home.clone(), home.join("Desktop")];
    let user = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for media in [PathBuf::from("/run/media").join(&user), PathBuf::from("/run/media"), PathBuf::from("/media").join(&user)] {
        for mount in std::fs::read_dir(&media).into_iter().flatten().flatten() {
            let p = mount.path();
            dirs.push(p.join("Downloads"));
            dirs.push(p);
        }
    }
    let mut out: Vec<Candidate> = Vec::new();
    for dir in dirs {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") || out.iter().any(|c| Path::new(&c.path) == path) {
                continue;
            }
            let Some(file) = read(&path) else { continue };
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.push(Candidate {
                name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                modified,
                services: file.services.names(),
            });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// Applies a services file: only what it sets, so importing someone's keys
/// never wipes a service they didn't share. Returns the services imported.
pub fn import(path: &str) -> anyhow::Result<Vec<String>> {
    let file = load(Path::new(path)).map_err(|m| anyhow::anyhow!(m))?;
    apply(file, path)
}

fn apply(file: ServicesFile, from: &str) -> anyhow::Result<Vec<String>> {
    let s = file.services;
    let names = s.names();
    let some = |v: String| Some(v.trim().to_string()).filter(|v| !v.is_empty());
    catalog::update_config(ConfigUpdate {
        prowlarr_url: some(s.prowlarr_url),
        prowlarr_key: some(s.prowlarr_key),
        tgdb_key: some(s.tgdb_key),
        iic_url: some(s.iic_url),
        iic_key: some(s.iic_key),
        iic_cdn: some(s.iic_cdn),
        tpb_url: some(s.tpb_url),
    })?;
    crate::log!("serviços: importados de {from}: {}", names.join(", "));
    Ok(names)
}

// ---------- from a link ----------

/// Imports a services file shared as a link: a GitHub gist (its page or raw
/// file), a Pastebin paste (its page or raw), a file on GitHub, or any raw URL.
pub async fn import_url(url: &str) -> anyhow::Result<Vec<String>> {
    let url = url.trim();
    let parsed = reqwest::Url::parse(url).map_err(|_| anyhow::anyhow!(tr!("that's not a link", "isso não é um link")))?;
    if !matches!(parsed.scheme(), "https" | "http") {
        bail!(tr!("only http(s) links", "só links http(s)"));
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("piShop/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let bytes = match gist_id(&parsed) {
        Some(id) => gist_file(&client, &id).await?,
        None => fetch(&client, &raw_url(&parsed)).await?,
    };
    let file = parse(&bytes).map_err(|m| anyhow::anyhow!(m))?;
    apply(file, url)
}

/// The gist a gist.github.com page link is about (its id: the hex segment).
fn gist_id(u: &reqwest::Url) -> Option<String> {
    if u.host_str()? != "gist.github.com" {
        return None;
    }
    u.path_segments()?.find(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit())).map(String::from)
}

/// Where a link's file is as plain text: Pastebin pages → /raw/, GitHub
/// file pages (…/blob/…) → raw.githubusercontent.com; the rest as is.
fn raw_url(u: &reqwest::Url) -> String {
    let segs: Vec<&str> = u.path_segments().map(|s| s.filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    match (u.host_str().unwrap_or(""), segs.as_slice()) {
        ("pastebin.com" | "www.pastebin.com", ["raw", ..]) => u.to_string(),
        ("pastebin.com" | "www.pastebin.com", [id]) | ("pastebin.com" | "www.pastebin.com", ["dl", id]) => format!("https://pastebin.com/raw/{id}"),
        ("github.com" | "www.github.com", [user, repo, "blob", rest @ ..]) if !rest.is_empty() => {
            format!("https://raw.githubusercontent.com/{user}/{repo}/{}", rest.join("/"))
        }
        _ => u.to_string(),
    }
}

/// A gist's services file: its .json file if it has one, else its first.
async fn gist_file(client: &reqwest::Client, id: &str) -> anyhow::Result<Vec<u8>> {
    let v: serde_json::Value = client
        .get(format!("https://api.github.com/gists/{id}"))
        .header("accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!(tr!("couldn't open the gist: {}", "não foi possível abrir o gist: {}", e.status().map(|s| s.to_string()).unwrap_or_default())))?
        .json()
        .await?;
    let files: Vec<&serde_json::Value> = v["files"].as_object().map(|m| m.values().collect()).unwrap_or_default();
    let f = files
        .iter()
        .find(|f| f["filename"].as_str().is_some_and(|n| n.to_lowercase().ends_with(".json")))
        .or(files.first())
        .ok_or_else(|| anyhow::anyhow!(tr!("the gist has no files", "o gist não tem arquivos")))?;
    match (f["truncated"].as_bool(), f["content"].as_str()) {
        (Some(false), Some(text)) => Ok(text.as_bytes().to_vec()),
        _ => fetch(client, f["raw_url"].as_str().unwrap_or_default()).await,
    }
}

/// A small file from the web (a services file is a few hundred bytes).
async fn fetch(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let r = client.get(url).send().await.map_err(|e| anyhow::anyhow!(tr!("couldn't download it: {}", "não foi possível baixar: {}", e)))?;
    if !r.status().is_success() {
        bail!(tr!("couldn't download it: {}", "não foi possível baixar: {}", r.status()));
    }
    if r.content_length().is_some_and(|n| n > MAX_SIZE) {
        bail!(tr!("that file is too big to be a services file", "esse arquivo é grande demais para ser de serviços"));
    }
    let bytes = r.bytes().await?;
    if bytes.len() as u64 > MAX_SIZE {
        bail!(tr!("that file is too big to be a services file", "esse arquivo é grande demais para ser de serviços"));
    }
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> reqwest::Url {
        reqwest::Url::parse(s).unwrap()
    }

    #[test]
    fn shared_links_lead_to_the_raw_file() {
        assert_eq!(gist_id(&url("https://gist.github.com/anarqz/0123456789abcdef0123456789abcdef")).as_deref(), Some("0123456789abcdef0123456789abcdef"));
        assert_eq!(gist_id(&url("https://gist.github.com/0123456789abcdef0123456789abcdef#file-pishop-services-json")).as_deref(), Some("0123456789abcdef0123456789abcdef"));
        assert_eq!(gist_id(&url("https://pastebin.com/AbCdEf12")), None);
        assert_eq!(raw_url(&url("https://pastebin.com/AbCdEf12")), "https://pastebin.com/raw/AbCdEf12");
        assert_eq!(raw_url(&url("https://pastebin.com/raw/AbCdEf12")), "https://pastebin.com/raw/AbCdEf12");
        assert_eq!(raw_url(&url("https://github.com/me/cfg/blob/main/piShop-services.json")), "https://raw.githubusercontent.com/me/cfg/main/piShop-services.json");
        let raw = "https://gist.githubusercontent.com/me/0123456789abcdef/raw/piShop-services.json";
        assert_eq!(raw_url(&url(raw)), raw);
    }

    #[test]
    fn services_files_are_recognised() {
        let ok = br#"{"piShop":"services","version":1,"services":{"tpb_url":"https://apibay.org"}}"#;
        assert!(parse(ok).is_ok());
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(ok);
        assert!(parse(&bom).is_ok());
        assert!(parse(br#"{"piShop":"something-else","version":1,"services":{}}"#).is_err());
        assert!(parse(br#"{"prowlarr_url":"http://x"}"#).is_err());
        assert!(parse(b"<html>not json</html>").is_err());
    }

    #[test]
    fn a_paste_that_lost_its_last_braces_still_imports() {
        // An export pasted by hand (Windows line endings) without the line
        // closing "services" — or without both closing lines.
        let lost_one = "{\r\n  \"piShop\": \"services\",\r\n  \"version\": 1,\r\n  \"services\": {\r\n    \"tpb_url\": \"https://apibay.org\"\r\n}";
        let f = parse(lost_one.as_bytes()).expect("one brace short");
        assert_eq!(f.services.tpb_url, "https://apibay.org");
        let lost_two = "{\n  \"piShop\": \"services\",\n  \"version\": 1,\n  \"services\": {\n    \"tpb_url\": \"https://apibay.org\"\n";
        assert!(parse(lost_two.as_bytes()).is_ok());
        // Broken elsewhere: says where.
        let broken = "{\n  \"piShop\": \"services\"\n  \"version\": 1\n}";
        // (No Debug on the file type: it holds API keys.)
        let Err(e) = parse(broken.as_bytes()) else { panic!("a broken file was accepted") };
        assert!(e.contains("line 3") || e.contains("linha 3"), "{e}");
    }
}
