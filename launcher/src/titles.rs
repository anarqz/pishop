//! Turns messy release titles ("[Switch NSP] The Legend of Zelda: BotW +
//! 1.6.0 Update", "The_Legend_Of_Zelda_TP_PAL_NGC-HYPER-CUBE") into a clean
//! game name for artwork matching plus platform/region/version tags.

use serde::Serialize;

#[derive(Serialize, Debug, Clone, PartialEq, Default)]
pub struct Parsed {
    /// Clean game name used for matching.
    pub name: String,
    /// EmuDeck-style system id ("switch", "wii", "ps2"…), if recognised.
    pub platform: Option<&'static str>,
    pub platform_label: Option<&'static str>,
    pub region: Option<String>,
    pub tags: Vec<String>,
    pub version: Option<String>,
    pub group: Option<String>,
}

/// (pattern words, system id, label). Checked in order, longest first.
const PLATFORMS: &[(&[&str], &str, &str)] = &[
    (&["nintendo switch", "switch", "nsp", "xci", "nsz", "ns"], "switch", "Switch"),
    (&["wii u", "wiiu"], "wiiu", "Wii U"),
    (&["wii", "wbfs"], "wii", "Wii"),
    (&["gamecube", "game cube", "ngc", "gcn"], "gc", "GameCube"),
    (&["nintendo 3ds", "3ds", "cia"], "n3ds", "3DS"),
    (&["nintendo ds", "nds"], "nds", "DS"),
    (&["game boy advance", "gameboy advance", "gba"], "gba", "GBA"),
    (&["game boy color", "gameboy color", "gbc"], "gbc", "GBC"),
    (&["game boy", "gameboy"], "gb", "Game Boy"),
    (&["nintendo 64", "n64", "z64"], "n64", "N64"),
    (&["super nintendo", "snes", "sfc"], "snes", "SNES"),
    (&["nes", "famicom"], "nes", "NES"),
    (&["ps5", "playstation 5"], "ps5", "PS5"),
    (&["ps4", "playstation 4"], "ps4", "PS4"),
    (&["ps3", "playstation 3"], "ps3", "PS3"),
    (&["ps2", "playstation 2"], "ps2", "PS2"),
    (&["psp"], "psp", "PSP"),
    (&["ps vita", "psvita", "vita", "psv"], "psvita", "Vita"),
    (&["psx", "ps1", "playstation 1", "playstation"], "psx", "PS1"),
    (&["xbox series", "xbsx"], "xboxseries", "Xbox Series"),
    (&["xbox one", "xb1"], "xboxone", "Xbox One"),
    (&["xbox 360", "xbox360", "x360"], "xbox360", "Xbox 360"),
    (&["xbox"], "xbox", "Xbox"),
    (&["dreamcast"], "dreamcast", "Dreamcast"),
    (&["saturn"], "saturn", "Saturn"),
    (&["mega drive", "megadrive", "genesis"], "genesis", "Mega Drive"),
    (&["master system"], "mastersystem", "Master System"),
    (&["neo geo", "neogeo"], "neogeo", "Neo Geo"),
];

const REGIONS: &[(&str, &str)] = &[
    ("pal", "PAL"), ("ntsc", "NTSC"), ("ntsc-u", "NTSC-U"), ("ntsc-j", "NTSC-J"),
    ("usa", "USA"), ("us", "USA"), ("eur", "EUR"), ("europe", "EUR"), ("jpn", "JPN"),
    ("japan", "JPN"), ("jap", "JPN"), ("ue", "USA/EUR"), ("world", "World"),
];

/// Words that describe the release, not the game.
const NOISE: &[&str] = &[
    "iso", "rom", "roms", "wbfs", "nsp", "xci", "nsz", "cia", "pkg", "rar", "zip", "7z", "chd", "rvz",
    "scrubbed", "decrypted", "encrypted", "repack", "proper", "rip", "fixed", "multi", "multi2", "multi3",
    "multi4", "multi5", "multi6", "multi7", "multi8", "multi9", "update", "updates", "dlc", "dlcs",
    "incl", "including", "fitgirl", "dodi", "elamigos", "gog", "steam", "crack", "cracked", "codex",
    "skidrow", "plaza", "rune", "tenoke", "empress", "razor1911", "flt", "darksiders", "pt-br", "ptbr",
    "pt", "br", "eng", "english", "dublado", "legendado", "traduzido", "portugues", "hi-res", "texture",
    "pack", "emulator", "emu", "yuzu", "ryujinx", "base", "game", "full", "complete", "fake", "unlocked",
];

fn norm(s: &str) -> String {
    s.to_lowercase()
}

/// Whole-word / phrase containment on a space-separated, lowercase string.
fn has_phrase(hay: &str, phrase: &str) -> bool {
    let hay = format!(" {hay} ");
    hay.contains(&format!(" {phrase} "))
}

/// Indexers sometimes pass HTML entities through ("Deluxe Edition &ndash; Build").
fn decode_entities(s: &str) -> String {
    let mut out = s.to_string();
    for (from, to) in [
        ("&ndash;", "–"), ("&mdash;", "—"), ("&amp;", "&"), ("&quot;", "\""), ("&#39;", "'"), ("&apos;", "'"),
        ("&lt;", "<"), ("&gt;", ">"), ("&nbsp;", " "), ("&trade;", ""), ("&reg;", ""), ("&copy;", ""),
    ] {
        out = out.replace(from, to);
    }
    out
}

pub fn parse(title: &str) -> Parsed {
    let mut p = Parsed::default();

    // Strip a trailing file extension.
    let decoded = decode_entities(title);
    let title = decoded.as_str();
    let mut t = title.trim().to_string();
    if let Some((stem, ext)) = t.rsplit_once('.') {
        if (2..=4).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && !ext.chars().all(|c| c.is_ascii_digit()) {
            let e = ext.to_lowercase();
            if ["iso", "wbfs", "nsp", "xci", "nsz", "cia", "3ds", "nds", "gba", "gbc", "gb", "n64", "z64", "sfc", "smc", "nes", "rar", "zip", "7z", "chd", "rvz", "pkg", "bin", "cue", "rom", "exe"].contains(&e.as_str()) {
                t = stem.to_string();
            }
        }
    }

    // Scene style ("Name.Of.Game.PAL_NGC-HYPER-CUBE"): the last segment holds
    // "<tag>-<GROUP>", and groups may contain hyphens themselves.
    if !t.contains(' ') {
        if let Some(sep) = t.rfind(['.', '_']) {
            let last = t[sep + 1..].to_string();
            if let Some((tag, group)) = last.split_once('-') {
                if !group.is_empty() && group.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    p.group = Some(group.to_string());
                    t = format!("{}{}{}", &t[..sep], &t[sep..sep + 1], tag);
                }
            }
        }
    }

    // Dots/underscores as separators (scene style), but keep "1.6.0"-like versions.
    let spaced = if t.contains(' ') && !t.contains('_') { t.clone() } else { t.replace('_', " ") };
    let mut s = String::new();
    let chars: Vec<char> = spaced.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let digit_around = i > 0 && i + 1 < chars.len() && chars[i - 1].is_ascii_digit() && chars[i + 1].is_ascii_digit();
        if c == '.' && !digit_around && !spaced.contains(' ') {
            s.push(' ');
        } else {
            s.push(c);
        }
    }

    // Pull bracketed groups out as tags: [Switch NSP] (3DS) {PAL}.
    let mut base = String::new();
    let mut tags_raw = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '[' | '(' | '{' => {
                depth += 1;
                if depth == 1 {
                    base.push(' ');
                    cur.clear();
                    continue;
                }
            }
            ']' | ')' | '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    tags_raw.push(cur.trim().to_string());
                    continue;
                }
            }
            _ => {}
        }
        if depth > 0 {
            cur.push(c);
        } else {
            base.push(c);
        }
    }

    // Everything after a " + " is extras ("+ 1.6.0 Update", "+ DLC").
    if let Some(i) = base.find(" + ") {
        tags_raw.push(base[i + 3..].trim().to_string());
        base.truncate(i);
    }

    let all_text = norm(&format!("{} {}", base, tags_raw.join(" ")));
    let words_text: String = all_text.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { ' ' }).collect();
    let words_text = words_text.split_whitespace().collect::<Vec<_>>().join(" ");

    'outer: for (pats, id, label) in PLATFORMS {
        for pat in *pats {
            if has_phrase(&words_text, pat) {
                p.platform = Some(id);
                p.platform_label = Some(label);
                break 'outer;
            }
        }
    }

    for (pat, label) in REGIONS {
        if has_phrase(&words_text, pat) {
            p.region = Some((*label).to_string());
            break;
        }
    }

    // Version like v1.2.3 or 1.6.0.
    for w in all_text.split_whitespace() {
        let v = w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.').trim_matches('.');
        let v = v.trim_start_matches('v');
        if v.contains('.') && v.split('.').all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit())) {
            p.version = Some(format!("v{v}"));
            break;
        }
    }

    for tag in &tags_raw {
        if !tag.is_empty() {
            p.tags.push(tag.clone());
        }
    }

    // Clean name: base text minus platform/region/noise words.
    let mut words: Vec<&str> = Vec::new();
    let base_norm: String = base.replace(" - ", " ").replace(" – ", " ").replace(" — ", " ");
    let platform_words: Vec<&str> = PLATFORMS.iter().flat_map(|(p, _, _)| p.iter().copied()).collect();
    let tokens: Vec<&str> = base_norm.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        let w = tokens[i];
        let lw = w.to_lowercase();
        let lw_clean = lw.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        // Two-word platform names ("Wii U", "Xbox 360").
        if i + 1 < tokens.len() {
            let pair = format!("{} {}", lw_clean, tokens[i + 1].to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()));
            if platform_words.contains(&pair.as_str()) {
                i += 2;
                continue;
            }
        }
        let is_version = lw_clean.trim_start_matches('v').contains('.')
            && lw_clean.trim_start_matches('v').split('.').all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()));
        let is_year = lw_clean.len() == 4 && lw_clean.starts_with(['1', '2']) && lw_clean.chars().all(|c| c.is_ascii_digit());
        let is_build_no = lw_clean.len() >= 5 && lw_clean.chars().all(|c| c.is_ascii_digit());
        if lw_clean == "build" && tokens.get(i + 1).is_some_and(|n| n.chars().all(|c| c.is_ascii_digit())) {
            i += 2;
            continue;
        }
        if lw_clean.is_empty()
            || platform_words.contains(&lw_clean)
            || REGIONS.iter().any(|(r, _)| *r == lw_clean)
            || NOISE.contains(&lw_clean)
            || is_version
            || is_year
            || is_build_no
        {
            i += 1;
            continue;
        }
        words.push(w);
        i += 1;
    }
    let name = words.join(" ");
    p.name = name.trim_matches(|c: char| c == '-' || c == ':' || c == ',' || c.is_whitespace()).to_string();
    if p.name.is_empty() {
        p.name = title.to_string();
    }
    p
}

/// Similarity in 0..1 between two titles (token overlap, order-insensitive).
pub fn similarity(a: &str, b: &str) -> f64 {
    let toks = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .filter(|w| !["the", "of", "a", "and", "de", "e", "o", "da", "do"].contains(w))
            .map(String::from)
            .collect()
    };
    let (ta, tb) = (toks(a), toks(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let inter = ta.iter().filter(|w| tb.contains(w)).count() as f64;
    // Dice coefficient, slightly penalising extra words in the candidate.
    2.0 * inter / (ta.len() + tb.len()) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(title: &str, name: &str, platform: Option<&str>) {
        let p = parse(title);
        assert_eq!(p.name, name, "name for {title:?} → {p:?}");
        assert_eq!(p.platform, platform, "platform for {title:?} → {p:?}");
    }

    #[test]
    fn real_titles() {
        check("[Switch NSP] The Legend of Zelda: Breath of the Wild", "The Legend of Zelda: Breath of the Wild", Some("switch"));
        check("[Switch NSP] The Legend of Zelda: Breath of the Wild + 1.6.0 Update", "The Legend of Zelda: Breath of the Wild", Some("switch"));
        check("The Legend of Zelda: Twilight Princess HD - Wii U", "The Legend of Zelda: Twilight Princess HD", Some("wiiu"));
        check("[Wii]The.Legend.of.Zelda.Twilight.Princess[PAL][ScRuBBeD].wbfs", "The Legend of Zelda Twilight Princess", Some("wii"));
        check("The Legend of Zelda - Ocarina of Time 3D [Decrypted] (3DS)", "The Legend of Zelda Ocarina of Time 3D", Some("n3ds"));
        check("The Legend of Zelda: The Wind Waker Gamecube ISO", "The Legend of Zelda: The Wind Waker", Some("gc"));
        check("The Legend of Zelda : Breath of the Wild (WIIU)", "The Legend of Zelda Breath of the Wild", Some("wiiu"));
        check("The Legend of Zelda Skyward Sword (2011) [Wii][PAL][MULTi5]", "The Legend of Zelda Skyward Sword", Some("wii"));
        check("The_Legend_Of_Zelda_Twilight_Princess_PAL_NGC-HYPER-CUBE", "The Legend Of Zelda Twilight Princess", Some("gc"));
        check("The.Legend.of.Zelda.The.Wind.Waker.HD.USA.WiiU-FAKE", "The Legend of Zelda The Wind Waker HD", Some("wiiu"));
        check("The.Legend.of.Zelda.Breath.of.the.Wild.Master.Edition.Switch", "The Legend of Zelda Breath of the Wild Master Edition", Some("switch"));
        let p = parse("Resident Evil Requiem: Deluxe Edition &ndash; Build 22277314");
        assert_eq!(p.name, "Resident Evil Requiem: Deluxe Edition");
    }

    #[test]
    fn region_and_version() {
        let p = parse("[Wii]The.Legend.of.Zelda.Twilight.Princess[PAL][ScRuBBeD].wbfs");
        assert_eq!(p.region.as_deref(), Some("PAL"));
        let p = parse("[Switch NSP] The Legend of Zelda: Breath of the Wild + 1.6.0 Update");
        assert_eq!(p.version.as_deref(), Some("v1.6.0"));
    }

    #[test]
    fn similarity_prefers_exact() {
        let q = "The Legend of Zelda Twilight Princess";
        assert!(similarity(q, "The Legend of Zelda: Twilight Princess") > similarity(q, "The Legend of Zelda: Twilight Princess HD"));
        assert!(similarity(q, "Zelda II: The Adventure of Link") < 0.5);
    }
}
