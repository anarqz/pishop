//! Game metadata of torrents downloaded from the Store, keyed by info hash, so
//! Transfers shows the game (cover, name, details) instead of the torrent
//! name. Stored in `<data>/library.json`.
//!
//! Non-Steam shortcuts piShop didn't install get an entry of their own the
//! first time they're managed in Games, keyed `sc-<appid>` (`shortcut_key`),
//! so their state (Proton, game folder, a borrowed shortcut, the artwork the
//! user picked) lives in the same place and the install routes work for them.
//!
//! Each field comes from the first source that has it, in this order:
//! Steam (when the game has an appid) → TheGamesDB (when its key is set) →
//! SteamGridDB (public) → isitcracked (when configured; also the crack info).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{steam_store, tgdb};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Game {
    pub name: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub crack_date: Option<String>,
    #[serde(default)]
    pub scene_group: Option<String>,
    #[serde(default)]
    pub drm: Option<String>,
    #[serde(default)]
    pub steam_appid: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    /// Services the data came from, in priority order (e.g. ["Steam", "isitcracked"]).
    #[serde(default)]
    pub sources: Vec<String>,
    /// SteamGridDB game picked by hand for the artwork (Games → Find artwork).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgdb_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgdb_name: Option<String>,
    /// Where the shortcut's artwork comes from when picked by hand: "steam"
    /// (the appid above) or "sgdb" (the SteamGridDB game above).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art_source: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub info_hash: String,
    pub game: Game,
    /// Torrent title as listed by the indexer.
    pub release: String,
    pub indexer: String,
    pub size: u64,
    #[serde(default)]
    pub dest: Option<String>,
    /// Unix seconds.
    pub added: i64,
    /// False while Steam/TheGamesDB are still being asked.
    #[serde(default = "yes")]
    pub resolved: bool,
    /// The install wizard's progress for this download (see `install`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<crate::install::InstallState>,
}

fn yes() -> bool {
    true
}

/// What the Store had on screen for the search: the SteamGridDB match and the
/// isitcracked entry. Steam and TheGamesDB are looked up here.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Hint {
    #[serde(default)]
    pub name: String,
    /// A Steam game the user picked by hand in the Store ("Match another
    /// game"): its data is used whatever the names say.
    #[serde(default, deserialize_with = "lenient_string")]
    pub steam_appid: Option<String>,
    #[serde(default)]
    pub sgdb: Option<SgdbHint>,
    #[serde(default)]
    pub crack: Option<CrackHint>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct SgdbHint {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
    #[serde(default)]
    pub year: Option<u32>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct CrackHint {
    #[serde(default)]
    pub title: String,
    #[serde(default, deserialize_with = "lenient_string")]
    pub steam_appid: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub crack_date: Option<String>,
    #[serde(default)]
    pub scene_group: Option<String>,
    #[serde(default)]
    pub drm: Option<String>,
}

/// appids arrive as numbers or strings.
fn lenient_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

static ENTRIES: Mutex<Option<BTreeMap<String, Entry>>> = Mutex::new(None);

fn file() -> PathBuf {
    crate::data_dir().join("library.json")
}

fn with<R>(f: impl FnOnce(&mut BTreeMap<String, Entry>) -> R) -> R {
    let mut guard = ENTRIES.lock().unwrap();
    let map = guard.get_or_insert_with(|| {
        std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    });
    f(map)
}

fn persist(map: &BTreeMap<String, Entry>) {
    let tmp = file().with_extension("json.tmp");
    match serde_json::to_vec_pretty(map) {
        Ok(bytes) if std::fs::write(&tmp, &bytes).is_ok() => {
            let _ = std::fs::rename(&tmp, file());
        }
        _ => crate::log!("biblioteca: falha ao salvar {}", file().display()),
    }
}

pub fn add(mut e: Entry) {
    e.info_hash = e.info_hash.trim().to_lowercase();
    if e.info_hash.is_empty() {
        return;
    }
    with(|m| {
        m.insert(e.info_hash.clone(), e);
        persist(m);
    });
}

/// Replaces the game data of an entry that still exists (it may have been
/// deleted while the lookups ran).
pub fn update_game(info_hash: &str, mut game: Game) {
    with(|m| {
        if let Some(e) = m.get_mut(info_hash) {
            // Artwork picked by hand survives a new lookup.
            game.sgdb_id = game.sgdb_id.or(e.game.sgdb_id);
            game.sgdb_name = game.sgdb_name.take().or_else(|| e.game.sgdb_name.clone());
            game.art_source = game.art_source.take().or_else(|| e.game.art_source.clone());
            e.game = game;
            e.resolved = true;
            persist(m);
        }
    });
}

pub fn get(info_hash: &str) -> Option<Entry> {
    with(|m| m.get(&info_hash.trim().to_lowercase()).cloned())
}

/// Saves the install wizard's state for a download.
pub fn set_install(info_hash: &str, state: Option<crate::install::InstallState>) {
    with(|m| {
        if let Some(e) = m.get_mut(&info_hash.trim().to_lowercase()) {
            e.install = state;
            persist(m);
        }
    });
}

pub fn all() -> BTreeMap<String, Entry> {
    with(|m| m.clone())
}

/// The key of a non-Steam shortcut's own entry (one piShop didn't install).
pub fn shortcut_key(appid: u32) -> String {
    format!("sc-{appid}")
}

/// Whether a key is a shortcut's own entry rather than a download's.
pub fn is_shortcut_key(key: &str) -> bool {
    key.starts_with("sc-")
}

/// Changes an entry in place; false if there is none.
pub fn update(key: &str, f: impl FnOnce(&mut Entry)) -> bool {
    with(|m| match m.get_mut(&key.trim().to_lowercase()) {
        Some(e) => {
            f(e);
            persist(m);
            true
        }
        None => false,
    })
}

/// Transfers deleted a download: its entry goes too, unless the game was
/// installed from it (Games still shows it).
pub fn forget_download(info_hash: &str) {
    with(|m| {
        let key = info_hash.trim().to_lowercase();
        let installed = m.get(&key).is_some_and(|e| e.install.as_ref().is_some_and(|s| s.appid.is_some()));
        if !installed && m.remove(&key).is_some() {
            persist(m);
        }
    });
}

pub fn remove(info_hash: &str) {
    with(|m| {
        if m.remove(&info_hash.trim().to_lowercase()).is_some() {
            persist(m);
        }
    });
}

/// Text from the UI: trimmed, bounded, empty → None.
fn clean(s: Option<&String>, max: usize) -> Option<String> {
    s.map(|s| s.trim().chars().take(max).collect::<String>()).filter(|s| !s.is_empty())
}

/// First 4-digit year in a date ("2004-03-01", "15 mar. 2024", "Q3 2026").
fn year_of(s: Option<&String>) -> Option<u32> {
    let s = s?;
    s.as_bytes()
        .windows(4)
        .find(|w| w.iter().all(u8::is_ascii_digit) && (w[0] == b'1' || w[0] == b'2'))
        .and_then(|w| std::str::from_utf8(w).ok()?.parse().ok())
}

/// Game data from the Store's hint alone (SteamGridDB, then isitcracked):
/// shown right away while Steam/TheGamesDB are asked.
pub fn quick(h: &Hint, fallback_name: &str, platform: Option<String>) -> Game {
    merge(h, fallback_name, platform, None, None, None, None)
}

/// Full lookup in priority order. `pc`: only PC releases are searched on Steam
/// by name (an appid from isitcracked is used either way).
pub async fn resolve(h: &Hint, fallback_name: &str, platform: Option<String>, pc: bool) -> Game {
    let name = lookup_name(h, fallback_name);
    let picked = clean(h.steam_appid.as_ref(), 20).filter(|a| a.chars().all(|c| c.is_ascii_digit()));
    let crack_appid = picked.or_else(|| h.crack.as_ref().and_then(|c| clean(c.steam_appid.as_ref(), 20)));
    let appid = match crack_appid {
        Some(a) => Some(a),
        None if pc => steam_store::find_appid(&name).await,
        None => None,
    };
    let (steam, steam_art) = match &appid {
        Some(a) => tokio::join!(steam_store::details(a), steam_store::library_art(a)),
        None => (None, None),
    };
    let tg = if tgdb::configured() { tgdb::details(&name).await } else { None };
    merge(h, fallback_name, platform, appid, steam, steam_art, tg)
}

/// The best name to search other services with: isitcracked mirrors Steam's
/// titles, SteamGridDB is next, then what the Store matched or parsed.
fn lookup_name(h: &Hint, fallback: &str) -> String {
    let crack = h.crack.as_ref().map(|c| c.title.trim()).filter(|t| !t.is_empty());
    let sgdb = h.sgdb.as_ref().map(|s| s.name.trim()).filter(|t| !t.is_empty());
    let hint = Some(h.name.trim()).filter(|t| !t.is_empty());
    crack.or(sgdb).or(hint).unwrap_or(fallback).to_string()
}

fn merge(
    h: &Hint,
    fallback_name: &str,
    platform: Option<String>,
    appid: Option<String>,
    steam: Option<steam_store::StoreInfo>,
    steam_art: Option<steam_store::LibraryArt>,
    tg: Option<tgdb::GameInfo>,
) -> Game {
    let sg = h.sgdb.as_ref();
    let cr = h.crack.as_ref();
    let nonempty = |v: &Vec<String>| (!v.is_empty()).then(|| v.clone());

    let name = steam
        .as_ref()
        .map(|s| s.name.clone())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| tg.as_ref().map(|t| t.title.clone()))
        .or_else(|| sg.map(|s| s.name.clone()).filter(|n| !n.trim().is_empty()))
        .or_else(|| cr.map(|c| c.title.clone()).filter(|n| !n.trim().is_empty()))
        .unwrap_or_else(|| lookup_name(h, fallback_name));
    let cover = steam_art
        .as_ref()
        .and_then(|a| a.cover.clone())
        .or_else(|| tg.as_ref().and_then(|t| t.boxart.clone()))
        .or_else(|| sg.and_then(|s| clean(s.cover.as_ref(), 1000)))
        .or_else(|| cr.and_then(|c| clean(c.cover.as_ref(), 1000)));
    let hero = steam_art
        .as_ref()
        .and_then(|a| a.hero.clone())
        .or_else(|| steam.as_ref().and_then(|s| s.screenshot.clone()))
        .or_else(|| sg.and_then(|s| clean(s.hero.as_ref(), 1000)))
        .or_else(|| cr.and_then(|c| clean(c.header.as_ref(), 1000)));
    let release_date = steam
        .as_ref()
        .and_then(|s| s.release_date.clone())
        .or_else(|| tg.as_ref().and_then(|t| t.release_date.clone()))
        .or_else(|| cr.and_then(|c| clean(c.release_date.as_ref(), 40)));
    let year = year_of(steam.as_ref().and_then(|s| s.release_date.as_ref()))
        .or_else(|| year_of(tg.as_ref().and_then(|t| t.release_date.as_ref())))
        .or_else(|| sg.and_then(|s| s.year))
        .or_else(|| year_of(cr.and_then(|c| c.release_date.as_ref())));

    let mut sources = Vec::new();
    if steam.is_some() || steam_art.is_some() {
        sources.push("Steam".to_string());
    }
    if tg.is_some() {
        sources.push("TheGamesDB".to_string());
    }
    if sg.is_some() {
        sources.push("SteamGridDB".to_string());
    }
    if cr.is_some() {
        sources.push("isitcracked".to_string());
    }

    Game {
        name: name.trim().chars().take(200).collect(),
        cover,
        hero,
        year,
        platform: clean(platform.as_ref(), 60),
        crack_date: cr.and_then(|c| clean(c.crack_date.as_ref(), 40)),
        scene_group: cr.and_then(|c| clean(c.scene_group.as_ref(), 60)),
        drm: cr.and_then(|c| clean(c.drm.as_ref(), 60)),
        steam_appid: appid,
        overview: steam.as_ref().and_then(|s| s.overview.clone()).or_else(|| tg.as_ref().and_then(|t| t.overview.clone())),
        genres: steam.as_ref().and_then(|s| nonempty(&s.genres)).or_else(|| tg.as_ref().and_then(|t| nonempty(&t.genres))).unwrap_or_default(),
        developers: steam
            .as_ref()
            .and_then(|s| nonempty(&s.developers))
            .or_else(|| tg.as_ref().and_then(|t| nonempty(&t.developers)))
            .unwrap_or_default(),
        publishers: steam
            .as_ref()
            .and_then(|s| nonempty(&s.publishers))
            .or_else(|| tg.as_ref().and_then(|t| nonempty(&t.publishers)))
            .unwrap_or_default(),
        release_date,
        sources,
        sgdb_id: None,
        sgdb_name: None,
        art_source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn years() {
        assert_eq!(year_of(Some(&"2004-03-01".into())), Some(2004));
        assert_eq!(year_of(Some(&"15 mar. 2024".into())), Some(2024));
        assert_eq!(year_of(Some(&"Em breve".into())), None);
    }

    #[test]
    fn priority_without_steam_or_tgdb() {
        let h = Hint {
            name: "Game".into(),
            steam_appid: None,
            sgdb: Some(SgdbHint { name: "Game (SGDB)".into(), cover: Some("sg.png".into()), hero: None, year: Some(2020) }),
            crack: Some(CrackHint { title: "Game (iic)".into(), cover: Some("iic.webp".into()), header: Some("h.jpg".into()), ..Default::default() }),
        };
        let g = quick(&h, "x", Some("PC".into()));
        assert_eq!(g.name, "Game (SGDB)");
        assert_eq!(g.cover.as_deref(), Some("sg.png"));
        assert_eq!(g.hero.as_deref(), Some("h.jpg"));
        assert_eq!(g.year, Some(2020));
        assert_eq!(g.sources, ["SteamGridDB", "isitcracked"]);
    }
}
