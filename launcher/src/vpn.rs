//! VPN through SteamOS's own NetworkManager: WireGuard (.conf) and OpenVPN
//! (.ovpn) configs become NetworkManager connections, switched on and off with
//! `nmcli`. NetworkManager lets programs in the user's own session (Game Mode,
//! Desktop Mode) do that without a password, and SteamOS ships both the
//! WireGuard module and the OpenVPN plugin. While a VPN is on, the whole
//! device uses it. Secrets (keys, the OpenVPN password) stay with
//! NetworkManager; piShop never returns them.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use axum::extract::Path as UrlPath;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{install, log, tr};

pub fn router() -> Router {
    Router::new()
        .route("/api/vpn", get(|| async { install::reply(status().await) }))
        .route("/api/vpn/candidates", get(|| async { Json(json!({ "downloads": home().join("Downloads"), "files": candidates() })) }))
        .route("/api/vpn/import", post(|Json(r): Json<ImportReq>| async move { install::reply(import(&r.path).await) }))
        .route("/api/vpn/ip", get(|| async { install::reply(public_ip().await) }))
        .route("/api/vpn/{uuid}/up", post(|UrlPath(u): UrlPath<String>| async move { install::reply(up(&u).await) }))
        .route("/api/vpn/{uuid}/down", post(|UrlPath(u): UrlPath<String>| async move { install::reply(down(&u).await) }))
        .route("/api/vpn/{uuid}/remove", post(|UrlPath(u): UrlPath<String>| async move { install::reply(remove(&u).await) }))
        .route(
            "/api/vpn/{uuid}/login",
            post(|UrlPath(u): UrlPath<String>, Json(r): Json<LoginReq>| async move { install::reply(login(&u, &r.username, &r.password).await) }),
        )
}

// ---------- nmcli ----------

async fn nmcli(args: &[&str]) -> anyhow::Result<String> {
    let out = tokio::process::Command::new("nmcli")
        .args(args)
        // Steam's overlay library comes along in a game's environment; nmcli
        // would only complain about it.
        .env_remove("LD_PRELOAD")
        .output()
        .await
        .map_err(|_| anyhow!(tr!("NetworkManager (nmcli) isn't available here", "o NetworkManager (nmcli) não está disponível aqui")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let err: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains("ld.so:")).collect();
        let err = err.join(" ").trim_start_matches("Error: ").to_string();
        if err.contains("Timeout expired") {
            bail!(tr!("the VPN server didn't answer in time", "o servidor da VPN não respondeu a tempo"));
        }
        bail!("{}", if err.is_empty() { stdout.trim().to_string() } else { err });
    }
    Ok(stdout)
}

/// One line of `nmcli -t` output: fields split on ':', with "\:" and "\\" escapes.
fn terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().unwrap().push(n);
                }
            }
            ':' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out
}

/// A value for nmcli's key=value lists (vpn.data, vpn.secrets).
fn escape_kv(s: &str) -> String {
    s.replace('\\', "\\\\").replace(',', "\\,")
}

#[derive(Serialize, Clone, Debug)]
pub struct Conn {
    pub uuid: String,
    pub name: String,
    /// wireguard | openvpn | vpn (another VPN plugin)
    pub kind: String,
    /// on | connecting | off
    pub state: String,
    pub device: Option<String>,
    /// Address on the VPN, while it's on.
    pub ip: Option<String>,
    /// OpenVPN that asks for a username and password not stored yet.
    pub needs_login: bool,
}

async fn connections() -> anyhow::Result<Vec<Conn>> {
    let all = nmcli(&["-t", "-f", "NAME,UUID,TYPE", "connection", "show"]).await?;
    let active = nmcli(&["-t", "-f", "UUID,STATE,DEVICE", "connection", "show", "--active"]).await.unwrap_or_default();
    let mut out = Vec::new();
    for line in all.lines() {
        let f = terse(line);
        let (Some(name), Some(uuid), Some(kind)) = (f.first(), f.get(1), f.get(2)) else { continue };
        if kind != "wireguard" && kind != "vpn" {
            continue;
        }
        let act = active.lines().map(terse).find(|a| a.first() == Some(uuid));
        let state = match act.as_ref().and_then(|a| a.get(1)).map(String::as_str) {
            Some("activated") => "on",
            Some("activating") => "connecting",
            Some(_) => "connecting",
            None => "off",
        };
        let mut kind = kind.clone();
        let mut needs_login = false;
        if kind == "vpn" {
            let service = nmcli(&["-g", "vpn.service-type", "connection", "show", "uuid", uuid]).await.unwrap_or_default();
            if service.contains("openvpn") {
                kind = "openvpn".into();
                let data = nmcli(&["-g", "vpn.data", "connection", "show", "uuid", uuid]).await.unwrap_or_default();
                let wants_password = data.contains("connection-type = password");
                let has_user = data.split(", ").any(|kv| kv.strip_prefix("username = ").is_some_and(|u| !u.trim().is_empty()));
                needs_login = wants_password && !has_user;
            }
        }
        let ip = if state == "on" {
            nmcli(&["-g", "IP4.ADDRESS", "connection", "show", "uuid", uuid])
                .await
                .ok()
                .and_then(|v| v.split(['|', '\n']).map(str::trim).find(|s| !s.is_empty()).map(|s| s.split('/').next().unwrap_or(s).to_string()))
        } else {
            None
        };
        out.push(Conn {
            uuid: uuid.clone(),
            name: name.clone(),
            kind,
            state: state.into(),
            device: act.and_then(|a| a.get(2).cloned()).filter(|d| !d.is_empty()),
            ip,
            needs_login,
        });
    }
    // Connected first, then by name.
    out.sort_by(|a, b| (a.state == "off").cmp(&(b.state == "off")).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

async fn status() -> anyhow::Result<Value> {
    match connections().await {
        Ok(c) => Ok(json!({ "available": true, "connections": c })),
        Err(e) => Ok(json!({ "available": false, "error": format!("{e:#}"), "connections": [] })),
    }
}

// ---------- importing ----------

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Wireguard,
    Openvpn,
}

/// What a config file is, from its name and contents.
fn kind_of(path: &Path, text: &str) -> Option<Kind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "conf" if text.contains("[Interface]") && text.contains("PrivateKey") => Some(Kind::Wireguard),
        "ovpn" => Some(Kind::Openvpn),
        "conf" if text.lines().any(|l| l.trim_start().starts_with("remote ")) => Some(Kind::Openvpn),
        _ => None,
    }
}

/// OpenVPN asks for a username and password ("auth-user-pass").
fn wants_login(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with("auth-user-pass"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

#[derive(Serialize)]
struct Candidate {
    path: String,
    name: String,
    kind: Kind,
    modified: i64,
}

/// VPN configs in the usual places a downloaded file ends up: Downloads, the
/// home and Desktop folders, SD cards and USB drives.
fn candidates() -> Vec<Candidate> {
    let home = home();
    let mut dirs = vec![home.join("Downloads"), home.clone(), home.join("Desktop")];
    let user = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for media in [PathBuf::from("/run/media").join(&user), PathBuf::from("/run/media")] {
        for mount in std::fs::read_dir(&media).into_iter().flatten().flatten() {
            dirs.push(mount.path().join("Downloads"));
            dirs.push(mount.path());
        }
    }
    let mut out: Vec<Candidate> = Vec::new();
    for dir in dirs {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if (ext != "conf" && ext != "ovpn") || out.iter().any(|c| Path::new(&c.path) == path) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.len() > 512 * 1024 {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Some(kind) = kind_of(&path, &text) else { continue };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            out.push(Candidate { name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path: path.display().to_string(), kind, modified });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

#[derive(Deserialize)]
struct ImportReq {
    path: String,
}

/// A name for the connection: the file's, unique among the connections.
fn friendly(stem: &str, taken: &[String]) -> String {
    let base = stem.trim().to_string();
    let base = if base.is_empty() { "VPN".to_string() } else { base };
    if !taken.contains(&base) {
        return base;
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken.contains(n)).unwrap()
}

async fn import(path: &str) -> anyhow::Result<Value> {
    let path = PathBuf::from(path);
    let meta = std::fs::metadata(&path).with_context(|| tr!("file not found", "arquivo não encontrado"))?;
    if meta.len() > 512 * 1024 {
        bail!(tr!("that file is too big to be a VPN config", "esse arquivo é grande demais para ser uma configuração de VPN"));
    }
    let text = std::fs::read_to_string(&path).context(tr!("couldn't read the file", "não foi possível ler o arquivo"))?;
    let kind = kind_of(&path, &text).ok_or_else(|| {
        anyhow!(tr!(
            "not a WireGuard (.conf) or OpenVPN (.ovpn) config",
            "não é uma configuração WireGuard (.conf) nem OpenVPN (.ovpn)"
        ))
    })?;
    let existing = connections().await?;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let name = friendly(&stem, &existing.iter().map(|c| c.name.clone()).collect::<Vec<_>>());

    // NetworkManager names a WireGuard interface after the file (15 characters
    // at most), so the file goes in under a short name of piShop's.
    let dir = crate::data_dir().join("vpn");
    std::fs::create_dir_all(&dir)?;
    let file = match kind {
        Kind::Wireguard => {
            let mut used = Vec::new();
            for c in existing.iter().filter(|c| c.kind == "wireguard") {
                if let Ok(i) = nmcli(&["-g", "connection.interface-name", "connection", "show", "uuid", &c.uuid]).await {
                    used.push(i.trim().to_string());
                }
            }
            let iface = (0..100).map(|n| format!("pswg{n}")).find(|i| !used.contains(i) && !Path::new("/sys/class/net").join(i).exists()).unwrap();
            dir.join(format!("{iface}.conf"))
        }
        Kind::Openvpn => dir.join("pishop-vpn.ovpn"),
    };
    std::fs::write(&file, &text)?;
    // Only for NetworkManager to read; the copy goes once it has the connection.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    }
    let kind_arg = match kind {
        Kind::Wireguard => "wireguard",
        Kind::Openvpn => "openvpn",
    };
    let added = nmcli(&["connection", "import", "type", kind_arg, "file", &file.display().to_string()]).await;
    let _ = std::fs::remove_file(&file);
    let added = added?;
    // "Connection 'pswg0' (2a4c…) successfully added."
    let uuid = added
        .split(['(', ')'])
        .nth(1)
        .filter(|u| u.len() >= 32 && u.chars().all(|c| c.is_ascii_hexdigit() || c == '-'))
        .ok_or_else(|| anyhow!("nmcli: {}", added.trim()))?
        .to_string();
    // The VPN is switched on from piShop (or Steam's network settings), not by itself.
    nmcli(&["connection", "modify", "uuid", &uuid, "connection.id", &name, "connection.autoconnect", "no"]).await?;
    log!("vpn: {name} importada ({kind_arg})");
    Ok(json!({ "uuid": uuid, "name": name, "kind": kind, "needs_login": kind == Kind::Openvpn && wants_login(&text) }))
}

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

/// Stores an OpenVPN username and password with the connection (NetworkManager
/// keeps the password; there's no password prompt in Game Mode).
async fn login(uuid: &str, username: &str, password: &str) -> anyhow::Result<Value> {
    if username.trim().is_empty() {
        bail!(tr!("type the username", "digite o usuário"));
    }
    let user = format!("username={}", escape_kv(username.trim()));
    let pass = format!("password={}", escape_kv(password));
    nmcli(&["connection", "modify", "uuid", uuid, "+vpn.data", &user, "+vpn.data", "password-flags=0", "vpn.secrets", &pass]).await?;
    Ok(json!({}))
}

// ---------- on / off ----------

async fn up(uuid: &str) -> anyhow::Result<Value> {
    // One VPN at a time.
    for c in connections().await? {
        if c.uuid != uuid && c.state != "off" {
            let _ = nmcli(&["connection", "down", "uuid", &c.uuid]).await;
        }
    }
    let r = tokio::time::timeout(Duration::from_secs(60), nmcli(&["--wait", "45", "connection", "up", "uuid", uuid])).await;
    match r {
        Ok(Ok(_)) => {
            log!("vpn: {uuid} ligada");
            Ok(json!({}))
        }
        Ok(Err(e)) => {
            // A half-open attempt shouldn't linger.
            let _ = nmcli(&["connection", "down", "uuid", uuid]).await;
            Err(e.context(tr!("the VPN didn't connect", "a VPN não conectou")))
        }
        Err(_) => {
            let _ = nmcli(&["connection", "down", "uuid", uuid]).await;
            bail!(tr!("the VPN took too long to connect", "a VPN demorou demais para conectar"))
        }
    }
}

async fn down(uuid: &str) -> anyhow::Result<Value> {
    nmcli(&["connection", "down", "uuid", uuid]).await?;
    log!("vpn: {uuid} desligada");
    Ok(json!({}))
}

async fn remove(uuid: &str) -> anyhow::Result<Value> {
    let _ = nmcli(&["connection", "down", "uuid", uuid]).await;
    nmcli(&["connection", "delete", "uuid", uuid]).await?;
    log!("vpn: {uuid} removida");
    Ok(json!({}))
}

/// Where the internet sees this device coming from (asked on demand).
async fn public_ip() -> anyhow::Result<Value> {
    let v: Value = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .get("https://ipinfo.io/json")
        .send()
        .await
        .context(tr!("no answer: is the internet working?", "sem resposta: a internet está funcionando?"))?
        .json()
        .await?;
    let place = [v["city"].as_str(), v["country"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(", ");
    Ok(json!({ "ip": v["ip"], "place": place, "org": v["org"] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terse_lines_keep_escaped_colons() {
        assert_eq!(terse(r"Home\: VPN:1b2c:wireguard"), ["Home: VPN", "1b2c", "wireguard"]);
        assert_eq!(terse(r"a\\b::vpn"), [r"a\b", "", "vpn"]);
    }

    #[test]
    fn configs_are_recognised() {
        let wg = "[Interface]\nPrivateKey = abc\nAddress = 10.0.0.2/32\n[Peer]\nPublicKey = x\n";
        assert_eq!(kind_of(Path::new("br-12.conf"), wg), Some(Kind::Wireguard));
        assert_eq!(kind_of(Path::new("br-12.ovpn"), "client\nremote 1.2.3.4 1194\n"), Some(Kind::Openvpn));
        assert_eq!(kind_of(Path::new("nginx.conf"), "server { listen 80; }"), None);
        assert!(wants_login("client\nauth-user-pass\n"));
        assert!(!wants_login("client\n"));
    }

    #[test]
    fn names_stay_unique() {
        assert_eq!(friendly("Mullvad BR", &[]), "Mullvad BR");
        assert_eq!(friendly("Mullvad BR", &["Mullvad BR".into()]), "Mullvad BR 2");
        assert_eq!(escape_kv(r"a,b\c"), r"a\,b\\c");
    }
}
