//! Steam's own client API — the functions its Big Picture UI calls — reached
//! over the CEF remote-debugging port Steam opens when
//! `~/.local/share/Steam/.cef-enable-remote-debugging` exists (Decky Loader
//! relies on the same switch). It lets piShop add a shortcut, pick its Proton
//! and set its artwork live, without restarting Steam.
//!
//! The port only listens on 127.0.0.1, but any local program can drive Steam
//! through it while it's on: that's the trade-off Decky users already make.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::tr;

const PORT: u16 = 8080;
const TIMEOUT: Duration = Duration::from_secs(15);

fn steam_root() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/share/Steam")
}

fn flag_file() -> PathBuf {
    steam_root().join(".cef-enable-remote-debugging")
}

/// Whether Steam was told to open the debugging port (takes effect on its next start).
pub fn debugging_enabled() -> bool {
    flag_file().exists()
}

/// Turns the debugging port on for Steam's next start.
pub fn enable_debugging() -> std::io::Result<()> {
    std::fs::write(flag_file(), b"")
}

async fn shared_context() -> anyhow::Result<String> {
    let targets: Vec<Value> = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{PORT}/json"))
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .context("steam debugging port closed")?
        .json()
        .await?;
    targets
        .iter()
        .find(|t| t["title"] == "SharedJSContext")
        .and_then(|t| t["webSocketDebuggerUrl"].as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("SharedJSContext not found"))
}

/// Whether the live API is reachable right now.
pub async fn available() -> bool {
    shared_context().await.is_ok()
}

/// Evaluates JS in Steam's shared context (awaiting promises) and returns the value.
async fn eval(expression: &str) -> anyhow::Result<Value> {
    let url = shared_context().await.map_err(|_| {
        anyhow!(tr!(
            "Steam's client API isn't reachable (debugging port closed)",
            "a API do cliente Steam não está acessível (porta de depuração fechada)"
        ))
    })?;
    let (mut ws, _) = tokio::time::timeout(TIMEOUT, tokio_tungstenite::connect_async(url.as_str())).await??;
    let call = json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": { "expression": expression, "awaitPromise": true, "returnByValue": true },
    });
    ws.send(Message::Text(call.to_string().into())).await?;
    let reply = tokio::time::timeout(TIMEOUT, async {
        while let Some(msg) = ws.next().await {
            if let Message::Text(t) = msg? {
                let v: Value = serde_json::from_str(&t)?;
                if v["id"] == 1 {
                    return Ok::<Value, anyhow::Error>(v);
                }
            }
        }
        bail!("connection closed")
    })
    .await??;
    let _ = ws.close(None).await;
    if let Some(ex) = reply["result"]["exceptionDetails"].as_object() {
        let text = ex.get("exception").and_then(|e| e["description"].as_str()).or(ex.get("text").and_then(|t| t.as_str()));
        bail!("Steam: {}", text.unwrap_or("script error"));
    }
    Ok(reply["result"]["result"]["value"].clone())
}

/// JS string literal for an argument.
fn js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

#[derive(Serialize, Clone, Debug)]
pub struct CompatTool {
    /// Internal name for SpecifyCompatTool, e.g. "proton_experimental".
    pub name: String,
    pub display: String,
}

/// Compatibility tools exactly as Steam lists them (Proton versions, GE-Proton…).
pub async fn compat_tools() -> anyhow::Result<Vec<CompatTool>> {
    let v = eval("SteamClient.Apps.GetAvailableCompatTools(0).then(l => l.map(t => ({ name: t.strToolName, display: t.strDisplayName })))").await?;
    Ok(serde_json::from_value::<Vec<Value>>(v)?
        .into_iter()
        .filter_map(|t| Some(CompatTool { name: t["name"].as_str()?.to_string(), display: t["display"].as_str()?.to_string() }))
        .collect())
}

/// Adds a non-Steam shortcut and returns its appid. Paths go unquoted: Steam
/// quotes the exe itself (a quoted one ends up double-quoted).
pub async fn add_shortcut(name: &str, exe: &str, start_dir: &str, launch_options: &str) -> anyhow::Result<u32> {
    let v = eval(&format!(
        "SteamClient.Apps.AddShortcut({}, {}, {}, {})",
        js(name),
        js(exe),
        js(start_dir),
        js(launch_options)
    ))
    .await?;
    let appid = v.as_u64().ok_or_else(|| anyhow!("AddShortcut returned {v}"))? as u32;
    // Older clients name the shortcut after the exe: set it explicitly.
    set_name(appid, name).await?;
    Ok(appid)
}

pub async fn set_name(appid: u32, name: &str) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.SetShortcutName({appid}, {})", js(name))).await.map(drop)
}

pub async fn set_exe(appid: u32, exe: &str) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.SetShortcutExe({appid}, {})", js(exe))).await.map(drop)
}

pub async fn set_start_dir(appid: u32, dir: &str) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.SetShortcutStartDir({appid}, {})", js(dir))).await.map(drop)
}

pub async fn set_launch_options(appid: u32, options: &str) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.SetShortcutLaunchOptions({appid}, {})", js(options))).await.map(drop)
}

pub async fn set_icon(appid: u32, path: &str) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.SetShortcutIcon({appid}, {})", js(path))).await.map(drop)
}

/// Runs the shortcut with a compatibility tool (Proton); "" clears it.
pub async fn set_compat_tool(appid: u32, tool: &str) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.SpecifyCompatTool({appid}, {})", js(tool))).await.map(drop)
}

/// A shortcut as Steam has it: paths quoted the way Steam stores them.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Shortcut {
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    /// Compatibility tool's internal name ("" when none).
    pub tool: String,
}

/// The shortcut's fields right now (None if Steam doesn't know the appid).
pub async fn shortcut(appid: u32) -> anyhow::Result<Option<Shortcut>> {
    let v = eval(&format!(
        "new Promise(res => {{ let h; h = SteamClient.Apps.RegisterForAppDetails({appid}, d => {{ \
         res(d ? {{ exe: d.strShortcutExe ?? '', start_dir: d.strShortcutStartDir ?? '', \
         launch_options: d.strShortcutLaunchOptions ?? '', tool: d.strCompatToolName ?? '' }} : null); \
         setTimeout(() => h?.unregister?.(), 0) }}); setTimeout(() => res(null), 4000) }})"
    ))
    .await?;
    Ok(serde_json::from_value(v).ok())
}

/// A non-Steam shortcut as Steam lists it, with what it runs.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ShortcutInfo {
    pub appid: u32,
    pub name: String,
    pub exe: String,
    pub start_dir: String,
    pub launch_options: String,
    /// Compatibility tool's internal name ("" when none).
    pub tool: String,
    /// Unix seconds (0: never played).
    #[serde(default)]
    pub last_played: i64,
}

/// Every non-Steam shortcut in the library (the Big Picture "Non-Steam"
/// collection), each with its fields, in one round trip.
pub async fn shortcuts() -> anyhow::Result<Vec<ShortcutInfo>> {
    let v = eval(
        "Promise.all((collectionStore.deckDesktopApps?.allApps ?? []).map(a => new Promise(res => { \
         let h, done = false; \
         const out = d => { if (done) return; done = true; \
           res({ appid: a.appid, name: a.display_name ?? '', exe: d?.strShortcutExe ?? '', \
             start_dir: d?.strShortcutStartDir ?? '', launch_options: d?.strShortcutLaunchOptions ?? '', \
             tool: d?.strCompatToolName ?? '', last_played: a.rt_last_time_played ?? 0 }); \
           setTimeout(() => h?.unregister?.(), 0) }; \
         h = SteamClient.Apps.RegisterForAppDetails(a.appid, out); setTimeout(() => out(null), 4000) })))",
    )
    .await?;
    Ok(serde_json::from_value(v)?)
}

/// Library artwork slots, as SetCustomArtworkForApp numbers them.
#[derive(Clone, Copy, Debug)]
pub enum Art {
    /// Portrait capsule (600×900).
    Cover = 0,
    Hero = 1,
    Logo = 2,
    /// Wide capsule / banner (920×430).
    Wide = 3,
}

pub async fn set_artwork(appid: u32, slot: Art, bytes: &[u8], ext: &str) -> anyhow::Result<()> {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    eval(&format!("SteamClient.Apps.SetCustomArtworkForApp({appid}, {}, {}, {})", js(&data), js(ext), slot as u32))
        .await
        .map(drop)
}

/// Logo placement over the hero, as Steam's own "adjust logo" stores it
/// (pinned "BottomLeft", "UpperCenter", "CenterCenter"…, size in percent).
pub async fn set_logo_position(appid: u32, pinned: &str, width_pct: f64, height_pct: f64) -> anyhow::Result<()> {
    let pos = json!({ "nVersion": 1, "logoPosition": { "pinnedPosition": pinned, "nWidthPct": width_pct, "nHeightPct": height_pct } });
    eval(&format!("SteamClient.Apps.SetCustomLogoPositionForApp({appid}, {})", js(&pos.to_string()))).await.map(drop)
}

/// Steam's 64-bit game id for a shortcut (what rungameid / RunGame take).
pub fn game_id(appid: u32) -> u64 {
    ((appid as u64) << 32) | 0x0200_0000
}

/// Starts the shortcut, as pressing Play would. Not for an app that's
/// already running: Steam starts a second launch, waits on the first one and
/// fails with an "already running" error (use `resume`).
pub async fn run(appid: u32) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.RunGame({}, '', -1, 100)", js(&game_id(appid).to_string()))).await.map(drop)
}

/// Puts an app that's already running back on screen, the way Game Mode's own
/// "Resume" does. False if Steam doesn't list it as running.
pub async fn resume(appid: u32) -> anyhow::Result<bool> {
    let v = eval(&format!(
        "(() => {{ const s = window.SteamUIStore; \
         if (!s?.RunningApps?.some(a => a.appid == {appid})) return false; \
         s.NavigateToRunningApp(); s.SetRunningApp({appid}); s.CloseSideMenus?.(); return true }})()"
    ))
    .await?;
    Ok(v.as_bool().unwrap_or(false))
}

pub async fn remove_shortcut(appid: u32) -> anyhow::Result<()> {
    eval(&format!("SteamClient.Apps.RemoveShortcut({appid})")).await.map(drop)
}

/// Whether a game launched by Steam is still running: its reaper process
/// carries `AppId=<appid>` on the command line.
pub fn running(appid: u32) -> bool {
    let needle = format!("AppId={appid}");
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit())
            && std::fs::read(e.path().join("cmdline"))
                .map(|c| c.split(|b| *b == 0).any(|arg| arg == needle.as_bytes()))
                .unwrap_or(false)
    })
}

/// Every appid Steam has running now (one pass over the processes).
pub fn running_appids() -> std::collections::HashSet<u32> {
    let mut out = std::collections::HashSet::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(cmd) = std::fs::read(e.path().join("cmdline")) else { continue };
        if !cmd.starts_with(b"/") || !cmd.windows(8).any(|w| w == b"SteamLau") {
            continue;
        }
        for arg in cmd.split(|b| *b == 0) {
            if let Some(id) = arg.strip_prefix(b"AppId=").and_then(|v| std::str::from_utf8(v).ok()?.parse().ok()) {
                out.insert(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_game_id() {
        // piShop's own shortcut: appid 3682868229 → rungameid 15817798599065993216.
        assert_eq!(game_id(3_682_868_229), 15_817_798_599_065_993_216);
    }

    #[test]
    fn js_strings_are_escaped() {
        assert_eq!(js(r#"C:\a "b""#), r#""C:\\a \"b\"""#);
    }
}
