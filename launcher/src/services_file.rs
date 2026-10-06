//! Settings → Services, Import / Export: the services setup (endpoints and
//! API keys) as a file, so it can be shared with others. Exports land in
//! ~/Downloads; imports are picked from the usual places a shared file ends
//! up (Downloads, home, Desktop, SD cards and USB drives).

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
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SIZE {
        return None;
    }
    let file: ServicesFile = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (file.kind == MARK).then_some(file)
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
    let file = read(Path::new(path)).ok_or_else(|| anyhow::anyhow!(tr!("not a piShop services file", "não é um arquivo de serviços do piShop")))?;
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
    crate::log!("serviços: importados de {path}: {}", names.join(", "));
    Ok(names)
}
