//! Launcher-wide preferences from Settings, stored in `<data>/settings.json`.
//! For now that's the language (English by default): the UI's, and also the
//! one used for Steam data, the bundled browser and the messages the API
//! returns.

use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    En,
    Pt,
}

impl Lang {
    /// Steam store `l=` / `language` parameter.
    pub fn steam(self) -> &'static str {
        match self {
            Lang::En => "english",
            Lang::Pt => "brazilian",
        }
    }

    /// BCP 47 tag for the browser.
    pub fn bcp47(self) -> &'static str {
        match self {
            Lang::En => "en-US",
            Lang::Pt => "pt-BR",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Default, Debug)]
pub struct Settings {
    #[serde(default)]
    pub lang: Lang,
}

static CURRENT: RwLock<Option<Settings>> = RwLock::new(None);

fn file() -> PathBuf {
    crate::data_dir().join("settings.json")
}

pub fn get() -> Settings {
    if let Some(s) = *CURRENT.read().unwrap() {
        return s;
    }
    let s: Settings = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    *CURRENT.write().unwrap() = Some(s);
    s
}

pub fn save(s: Settings) -> anyhow::Result<Settings> {
    let tmp = file().with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&s)?)?;
    std::fs::rename(&tmp, file())?;
    *CURRENT.write().unwrap() = Some(s);
    crate::log!("configurações: idioma {:?}", s.lang);
    Ok(s)
}

pub fn lang() -> Lang {
    get().lang
}

pub fn is_pt() -> bool {
    lang() == Lang::Pt
}

/// A message for the UI in the current language:
/// `tr!("Prowlarr answered {s}", "Prowlarr respondeu {s}")` → String.
#[macro_export]
macro_rules! tr {
    ($en:literal, $pt:literal $(,)?) => {
        if $crate::settings::is_pt() { format!($pt) } else { format!($en) }
    };
    ($en:literal, $pt:literal, $($arg:tt)+) => {
        if $crate::settings::is_pt() { format!($pt, $($arg)+) } else { format!($en, $($arg)+) }
    };
}
