//! Games → Patches: quick workarounds to try when a game won't run on Linux.
//! Each one is a change to the shortcut's launch options — environment set
//! before `%command%`, arguments after it — and whether it's on is read back
//! from the launch options themselves, so edits made in Steam show up here
//! and turning a patch off takes out exactly what it put in.

use axum::extract::Path as UrlPath;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::install::{Stage, reply};
use crate::{library, log, steamclient, tr};

pub fn router() -> Router {
    Router::new()
        .route("/api/games/{appid}/patches", get(|UrlPath(a): UrlPath<u32>| async move { reply(list(a).await) }))
        .route("/api/games/{appid}/patches/{id}", post(set_route))
}

/// What a patch changes in the launch options.
enum Change {
    /// Wine loads this DLL the given way, e.g. "n,b": the game's own copy
    /// first, Wine's if there's none (an entry of WINEDLLOVERRIDES).
    DllOverride { dll: &'static str, mode: &'static str },
}

struct Patch {
    id: &'static str,
    change: Change,
}

const PATCHES: &[Patch] = &[Patch { id: "winmm", change: Change::DllOverride { dll: "winmm", mode: "n,b" } }];

/// A patch's name and what it's for, in the UI's language.
fn texts(id: &str) -> (String, String) {
    match id {
        "winmm" => (
            tr!("Override winmm", "Sobrescrever o winmm"),
            tr!(
                "Used in some FitGirl repacks: the game's own winmm.dll loads before Wine's. Adds WINEDLLOVERRIDES=\"winmm=n,b\" %command% to the launch options.",
                "Usado em alguns repacks da FitGirl: o winmm.dll do próprio jogo carrega antes do do Wine. Adiciona WINEDLLOVERRIDES=\"winmm=n,b\" %command% às opções de inicialização."
            ),
        ),
        _ => (id.to_string(), String::new()),
    }
}

// ---------- launch options ----------

const COMMAND: &str = "%command%";
const DLL_VAR: &str = "WINEDLLOVERRIDES";

/// Launch options split around `%command%`: environment and wrappers before
/// it, the game's arguments after. Without `%command%` they're all
/// arguments (Steam appends them to the executable).
#[derive(Debug, Clone, PartialEq)]
struct Options {
    before: Vec<String>,
    after: Vec<String>,
    command: bool,
}

/// Words of a command line; quoted parts stay whole, quotes included.
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                cur.push(c);
                quote = Some(c);
            }
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse(s: &str) -> Options {
    let w = words(s);
    match w.iter().position(|t| t == COMMAND) {
        Some(i) => Options { before: w[..i].to_vec(), after: w[i + 1..].to_vec(), command: true },
        None => Options { before: Vec::new(), after: w, command: false },
    }
}

fn render(o: &Options) -> String {
    if o.before.is_empty() {
        // Nothing to set: plain arguments (or nothing at all).
        return if o.command && !o.after.is_empty() { format!("{COMMAND} {}", o.after.join(" ")) } else { o.after.join(" ") };
    }
    let mut parts = o.before.clone();
    parts.push(COMMAND.into());
    parts.extend(o.after.iter().cloned());
    parts.join(" ")
}

/// `NAME=…` (an environment assignment).
fn is_assignment(t: &str) -> bool {
    let Some((name, _)) = t.split_once('=') else { return false };
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit())
}

fn unquote(v: &str) -> &str {
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

/// WINEDLLOVERRIDES's entries: (["winmm", "version"], "n,b") pairs.
type Entries = Vec<(Vec<String>, String)>;

/// Where WINEDLLOVERRIDES is among the words before %command%, and its entries.
fn overrides(o: &Options) -> Option<(usize, Entries)> {
    let prefix = format!("{DLL_VAR}=");
    let i = o.before.iter().position(|t| t.starts_with(&prefix))?;
    let entries = unquote(&o.before[i][prefix.len()..])
        .split(';')
        .filter(|e| !e.trim().is_empty())
        .map(|e| {
            let (dlls, mode) = e.split_once('=').unwrap_or((e, ""));
            (dlls.split(',').map(|d| d.trim().to_string()).filter(|d| !d.is_empty()).collect(), mode.trim().to_string())
        })
        .collect();
    Some((i, entries))
}

fn has_override(o: &Options, dll: &str, mode: &str) -> bool {
    overrides(o).is_some_and(|(_, es)| es.iter().any(|(dlls, m)| m == mode && dlls.iter().any(|d| d.eq_ignore_ascii_case(dll))))
}

/// Writes the entries back (or takes the variable out when none are left).
fn put_overrides(o: &mut Options, at: Option<usize>, entries: Entries) {
    let value: Vec<String> = entries.iter().filter(|(d, _)| !d.is_empty()).map(|(d, m)| format!("{}={m}", d.join(","))).collect();
    match (at, value.is_empty()) {
        (Some(i), true) => {
            o.before.remove(i);
        }
        (Some(i), false) => o.before[i] = format!("{DLL_VAR}=\"{}\"", value.join(";")),
        (None, false) => {
            // With the other variables, ahead of wrappers like gamemoderun.
            let i = o.before.iter().position(|t| !is_assignment(t)).unwrap_or(o.before.len());
            o.before.insert(i, format!("{DLL_VAR}=\"{}\"", value.join(";")));
            o.command = true;
        }
        (None, true) => {}
    }
}

fn set_override(o: &mut Options, dll: &str, mode: &str, on: bool) {
    let (at, mut entries) = overrides(o).map(|(i, e)| (Some(i), e)).unwrap_or((None, Vec::new()));
    for (dlls, _) in entries.iter_mut() {
        dlls.retain(|d| !d.eq_ignore_ascii_case(dll));
    }
    entries.retain(|(d, _)| !d.is_empty());
    if on {
        entries.push((vec![dll.to_string()], mode.to_string()));
    }
    put_overrides(o, at, entries);
}

fn applied(p: &Patch, o: &Options) -> bool {
    match p.change {
        Change::DllOverride { dll, mode } => has_override(o, dll, mode),
    }
}

/// The launch options with a patch on or off.
fn patched(p: &Patch, options: &str, on: bool) -> String {
    let mut o = parse(options);
    match p.change {
        Change::DllOverride { dll, mode } => set_override(&mut o, dll, mode, on),
    }
    render(&o)
}

// ---------- routes ----------

/// While an installer has the shortcut (its arguments are in the launch
/// options), patches wait.
fn busy(appid: u32) -> bool {
    library::all().values().any(|e| {
        e.install.as_ref().is_some_and(|s| s.appid == Some(appid) && (s.stage == Stage::Installing || s.restore.is_some()))
    })
}

async fn list(appid: u32) -> anyhow::Result<Value> {
    let sc = steamclient::shortcut(appid).await?.ok_or_else(|| anyhow::anyhow!(tr!("shortcut not found", "atalho não encontrado")))?;
    Ok(state(appid, &sc.launch_options))
}

/// The patches as these launch options have them.
fn state(appid: u32, launch_options: &str) -> Value {
    let o = parse(launch_options);
    let patches: Vec<Value> = PATCHES
        .iter()
        .map(|p| {
            let (title, about) = texts(p.id);
            json!({ "id": p.id, "title": title, "about": about, "applied": applied(p, &o) })
        })
        .collect();
    json!({ "launch_options": launch_options, "patches": patches, "busy": busy(appid) })
}

#[derive(Deserialize)]
struct SetReq {
    on: bool,
}

async fn set_route(UrlPath((appid, id)): UrlPath<(u32, String)>, Json(r): Json<SetReq>) -> axum::response::Response {
    reply(set(appid, &id, r.on).await)
}

async fn set(appid: u32, id: &str, on: bool) -> anyhow::Result<Value> {
    let p = PATCHES.iter().find(|p| p.id == id).ok_or_else(|| anyhow::anyhow!(tr!("unknown patch", "patch desconhecido")))?;
    if busy(appid) {
        anyhow::bail!(tr!("an installer is using this shortcut: wait for it to close", "um instalador está usando este atalho: espere ele fechar"));
    }
    let sc = steamclient::shortcut(appid).await?.ok_or_else(|| anyhow::anyhow!(tr!("shortcut not found", "atalho não encontrado")))?;
    let next = patched(p, &sc.launch_options, on);
    if next != sc.launch_options {
        steamclient::set_launch_options(appid, &next).await?;
        log!("patches: {id} {} no atalho {appid}: {:?} → {next:?}", if on { "ligado" } else { "desligado" }, sc.launch_options);
        // Steam's details lag a moment behind a write: wait until they show it,
        // so the next toggle starts from what's really there.
        for _ in 0..16 {
            if steamclient::shortcut(appid).await.ok().flatten().is_some_and(|s| s.launch_options == next) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    // What was just written: Steam's details answer with the old options for a moment.
    Ok(state(appid, &next))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn winmm(options: &str, on: bool) -> String {
        patched(&PATCHES[0], options, on)
    }
    fn is_on(options: &str) -> bool {
        applied(&PATCHES[0], &parse(options))
    }

    #[test]
    fn adds_and_removes_exactly_its_part() {
        assert_eq!(winmm("", true), r#"WINEDLLOVERRIDES="winmm=n,b" %command%"#);
        assert_eq!(winmm(r#"WINEDLLOVERRIDES="winmm=n,b" %command%"#, false), "");
        // Plain arguments stay arguments.
        assert_eq!(winmm("-dx11", true), r#"WINEDLLOVERRIDES="winmm=n,b" %command% -dx11"#);
        assert_eq!(winmm(r#"WINEDLLOVERRIDES="winmm=n,b" %command% -dx11"#, false), "%command% -dx11");
        // Other variables and arguments are kept, in place.
        assert_eq!(winmm("PROTON_LOG=1 %command% -windowed", true), r#"PROTON_LOG=1 WINEDLLOVERRIDES="winmm=n,b" %command% -windowed"#);
        assert_eq!(winmm(r#"PROTON_LOG=1 WINEDLLOVERRIDES="winmm=n,b" %command% -windowed"#, false), "PROTON_LOG=1 %command% -windowed");
        // Wrappers come after the environment.
        assert_eq!(winmm("gamemoderun %command%", true), r#"WINEDLLOVERRIDES="winmm=n,b" gamemoderun %command%"#);
        // Quoted arguments survive.
        assert_eq!(winmm(r#"%command% -path "C:\My Games\x""#, true), r#"WINEDLLOVERRIDES="winmm=n,b" %command% -path "C:\My Games\x""#);
    }

    #[test]
    fn merges_with_overrides_already_there() {
        assert_eq!(winmm(r#"WINEDLLOVERRIDES="dinput8=n,b" %command%"#, true), r#"WINEDLLOVERRIDES="dinput8=n,b;winmm=n,b" %command%"#);
        assert_eq!(winmm(r#"WINEDLLOVERRIDES="dinput8=n,b;winmm=n,b" %command%"#, false), r#"WINEDLLOVERRIDES="dinput8=n,b" %command%"#);
        // winmm in a shared entry, or set another way.
        assert!(is_on(r#"WINEDLLOVERRIDES="winmm,version=n,b" %command%"#));
        assert_eq!(winmm(r#"WINEDLLOVERRIDES="winmm,version=n,b" %command%"#, false), r#"WINEDLLOVERRIDES="version=n,b" %command%"#);
        assert!(!is_on(r#"WINEDLLOVERRIDES="winmm=b" %command%"#));
        assert_eq!(winmm(r#"WINEDLLOVERRIDES="winmm=b" %command%"#, true), r#"WINEDLLOVERRIDES="winmm=n,b" %command%"#);
        // Unquoted, as people type it.
        assert!(is_on("WINEDLLOVERRIDES=winmm=n,b %command%"));
        assert!(!is_on("-dx11"));
        assert!(!is_on(""));
    }
}
