//! Proton on this machine: Steam's libraries, the folders of compatibility
//! tools and the Steam Linux Runtime each one runs in, and a shortcut's
//! prefix — its drive letters and what installers recorded in its registry.

use std::path::{Path, PathBuf};

use serde::Serialize;

pub fn steam_root() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/share/Steam")
}

/// Steam library folders that exist, as libraryfolders.vdf lists them.
pub fn library_paths() -> Vec<PathBuf> {
    let text = std::fs::read_to_string(steam_root().join("steamapps/libraryfolders.vdf")).unwrap_or_default();
    let mut out: Vec<PathBuf> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("\"path\""))
        .map(|v| PathBuf::from(v.trim().trim_matches('"').replace("\\\\", "\\")))
        .filter(|p| p.is_dir())
        .collect();
    if out.is_empty() {
        out.push(steam_root());
    }
    out
}

fn vdf_value(text: &str, key: &str) -> Option<String> {
    let key = format!("\"{key}\"");
    text.lines().find_map(|l| l.trim().strip_prefix(key.as_str()).map(|v| v.trim().trim_matches('"').to_string()))
}

/// An installed Steam app's folder, from its appmanifest.
pub fn app_dir(appid: &str) -> Option<PathBuf> {
    library_paths().into_iter().find_map(|lib| {
        let text = std::fs::read_to_string(lib.join(format!("steamapps/appmanifest_{appid}.acf"))).ok()?;
        let dir = lib.join("steamapps/common").join(vdf_value(&text, "installdir")?);
        dir.is_dir().then_some(dir)
    })
}

/// Folder names and Steam's display names, compared loosely
/// ("Proton - Experimental" ~ "Proton Experimental", "Proton 9.0 (Beta)" ~ "Proton 9.0").
pub fn norm(s: &str) -> String {
    s.to_lowercase().replace("(beta)", "").chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Installed compatibility tools (folders with a `proton` script): Valve's
/// live in the libraries' steamapps/common, the rest in compatibilitytools.d.
pub fn tool_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for lib in library_paths() {
        dirs.extend(std::fs::read_dir(lib.join("steamapps/common")).into_iter().flatten().flatten().map(|e| e.path()));
    }
    dirs.extend(std::fs::read_dir(steam_root().join("compatibilitytools.d")).into_iter().flatten().flatten().map(|e| e.path()));
    dirs.retain(|d| d.join("proton").is_file());
    dirs
}

/// The folder of the tool Steam calls `name` and shows as `display`.
pub fn tool_dir(name: &str, display: &str) -> Option<PathBuf> {
    // "Proton 9.0-4" is installed as "Proton 9.0 (Beta)": match on "proton90".
    let short = norm(display.split('-').next().unwrap_or(display));
    tool_dirs().into_iter().find(|d| {
        let n = norm(&d.file_name().unwrap_or_default().to_string_lossy());
        n == norm(display) || n == norm(name) || n == short || (n.starts_with(&short) && short.len() > 6)
    })
}

/// The Steam Linux Runtime a tool runs in (its toolmanifest's require_tool_appid).
pub fn runtime_for(tool_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(tool_dir.join("toolmanifest.vdf")).ok()?;
    app_dir(&vdf_value(&text, "require_tool_appid")?).filter(|d| d.join("_v2-entry-point").is_file())
}

/// A shortcut's compatdata folder; its Wine prefix is `pfx` inside.
pub fn compatdata(appid: u32) -> PathBuf {
    library_paths()
        .into_iter()
        .map(|lib| lib.join(format!("steamapps/compatdata/{appid}")))
        .find(|p| p.is_dir())
        .unwrap_or_else(|| steam_root().join(format!("steamapps/compatdata/{appid}")))
}

/// Whether anything runs in the shortcut's prefix right now: the game, its
/// installer, winetricks or a lingering wineserver.
pub fn in_use(appid: u32) -> bool {
    let id = appid.to_string();
    let ours = |v: &[u8]| {
        let v = String::from_utf8_lossy(v);
        let p = Path::new(v.trim_end_matches('/'));
        let p = if p.ends_with("pfx") { p.parent().unwrap_or(p) } else { p };
        p.file_name().is_some_and(|n| n == id.as_str()) && p.parent().is_some_and(|d| d.ends_with("compatdata"))
    };
    std::fs::read_dir("/proc").into_iter().flatten().flatten().any(|e| {
        e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit())
            && std::fs::read(e.path().join("environ")).is_ok_and(|env| {
                env.split(|b| *b == 0).any(|kv| {
                    kv.strip_prefix(b"STEAM_COMPAT_DATA_PATH=").or_else(|| kv.strip_prefix(b"WINEPREFIX=")).is_some_and(ours)
                })
            })
    })
}

// ---------- drive letters ----------

/// The prefix's drive letters and the folders they stand for.
pub fn drives(pfx: &Path) -> Vec<(char, PathBuf)> {
    let dev = pfx.join("dosdevices");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dev).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        let mut chars = name.chars();
        let (Some(letter), Some(':'), None) = (chars.next(), chars.next(), chars.next()) else { continue };
        if !letter.is_ascii_lowercase() {
            continue;
        }
        let Ok(target) = std::fs::read_link(e.path()) else { continue };
        let target = if target.is_absolute() { target } else { dev.join(target) };
        let target = target.canonicalize().unwrap_or(target);
        out.push((letter.to_ascii_uppercase(), target));
    }
    out.sort();
    out
}

/// A Windows path in the prefix as a Linux path, each part matched
/// case-insensitively like Windows does. None when it doesn't exist.
pub fn to_linux(pfx: &Path, win: &str) -> Option<PathBuf> {
    let win = win.trim().trim_matches('"');
    let mut chars = win.chars();
    let letter = chars.next().filter(|c| c.is_ascii_alphabetic())?.to_ascii_uppercase();
    if chars.next()? != ':' {
        return None;
    }
    let (_, mut path) = drives(pfx).into_iter().find(|(l, _)| *l == letter)?;
    for part in win[2..].split(['\\', '/']).filter(|p| !p.is_empty() && *p != ".") {
        // The name as it is on disk: exact, else differing only in case.
        let names: Vec<_> = std::fs::read_dir(&path).ok()?.flatten().map(|e| e.file_name()).collect();
        let lower = part.to_lowercase();
        let found = names
            .iter()
            .find(|n| n.to_string_lossy() == part)
            .or_else(|| names.iter().find(|n| n.to_string_lossy().to_lowercase() == lower))?;
        path = path.join(found);
    }
    Some(path)
}

/// Every spelling the prefix's drives give a Linux path ("D:\Games\X",
/// "Z:\run\media\deck\SN01T\Games\X"), the most specific drive first.
pub fn windows_paths(pfx: &Path, path: &Path) -> Vec<String> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut found: Vec<(usize, String)> = drives(pfx)
        .into_iter()
        .filter_map(|(letter, root)| {
            let rest = path.strip_prefix(&root).ok()?;
            let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            Some((root.components().count(), format!("{letter}:\\{}", parts.join("\\"))))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, s)| s).collect()
}

pub fn to_windows(pfx: &Path, path: &Path) -> String {
    windows_paths(pfx, path).into_iter().next().unwrap_or_else(|| crate::exeguess::windows_path(path))
}

// ---------- registry ----------

/// A program an installer recorded under Uninstall.
#[derive(Serialize, Clone, Debug)]
pub struct Program {
    pub name: String,
    /// Its folder as Windows sees it ("D:\Games\X").
    pub location: String,
    pub path: PathBuf,
    /// The executable it registered as its icon, often the game itself.
    pub icon: Option<PathBuf>,
    /// Unix seconds when the key was last written.
    pub modified: i64,
}

/// Programs installed in the prefix whose folders exist (Wine's own
/// components and redistributables left out).
pub fn programs(pfx: &Path) -> Vec<Program> {
    let Ok(bytes) = std::fs::read(pfx.join("system.reg")) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    let mut cur: Option<(i64, Vec<(String, String)>)> = None;
    for line in text.lines() {
        if line.starts_with('[') {
            if let Some(c) = cur.take() {
                program(c, pfx, &mut out);
            }
            let Some(end) = line.find(']') else { continue };
            if line[1..end].to_ascii_lowercase().contains("\\\\microsoft\\\\windows\\\\currentversion\\\\uninstall\\\\") {
                cur = Some((line[end + 1..].trim().parse().unwrap_or(0), Vec::new()));
            }
        } else if let Some((_, values)) = cur.as_mut() {
            if let Some(v) = parse_value(line) {
                values.push(v);
            }
        }
    }
    if let Some(c) = cur.take() {
        program(c, pfx, &mut out);
    }
    out
}

fn program((modified, values): (i64, Vec<(String, String)>), pfx: &Path, out: &mut Vec<Program>) {
    let get = |k: &str| values.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    let Some(name) = get("DisplayName") else { return };
    let lower = name.to_lowercase();
    let redist = ["visual c++", "redistributable", "directx", ".net", "xna", "physx", "openal", "wine mono", "wine gecko", "webview2"];
    if redist.iter().any(|r| lower.contains(r)) {
        return;
    }
    let Some(location) = get("InstallLocation").or_else(|| get("Inno Setup: App Path")) else { return };
    let location = location.trim_matches('"').trim_end_matches('\\').to_string();
    let Some(path) = to_linux(pfx, &location).filter(|p| p.is_dir()) else { return };
    if path.starts_with(pfx.join("drive_c/windows")) {
        return;
    }
    // "C:\Game\game.exe,0"
    let icon = get("DisplayIcon").and_then(|i| {
        let i = i.trim_matches('"');
        let i = i.rsplit_once(',').filter(|(_, n)| n.trim().parse::<i32>().is_ok()).map(|(p, _)| p).unwrap_or(i);
        to_linux(pfx, i.trim_matches('"'))
    });
    let icon = icon.filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")));
    out.push(Program { name, location, path, icon, modified });
}

/// `"Name"="value"` (or `str(2):"value"`) → (Name, value), unescaped.
fn parse_value(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix('"')?;
    let (name, used) = read_str(rest)?;
    let rest = rest[used..].strip_prefix('=')?;
    let rest = rest.strip_prefix("str(2):").unwrap_or(rest).strip_prefix('"')?;
    Some((name, read_str(rest)?.0))
}

/// A .reg string up to its closing quote: (text, bytes used with the quote).
fn read_str(s: &str) -> Option<(String, usize)> {
    let mut out = String::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '"' => return Some((out, i + 1)),
            '\\' => match it.next()?.1 {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '0' => out.push('\0'),
                'x' => {
                    let mut v = 0u32;
                    for _ in 0..4 {
                        match it.peek().and_then(|(_, h)| h.to_digit(16)) {
                            Some(d) => {
                                v = v * 16 + d;
                                it.next();
                            }
                            None => break,
                        }
                    }
                    out.push(char::from_u32(v).unwrap_or('?'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// A string the way Wine writes it in its .reg files.
fn reg_escape(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = String::new();
    for (i, &u) in units.iter().enumerate() {
        match u {
            0x5c => out.push_str("\\\\"),
            0x22 => out.push_str("\\\""),
            u if !(32..=127).contains(&u) => {
                let next_hex = units.get(i + 1).is_some_and(|n| *n < 128 && (*n as u8).is_ascii_hexdigit());
                if next_hex {
                    out.push_str(&format!("\\x{u:04x}"));
                } else {
                    out.push_str(&format!("\\x{u:x}"));
                }
            }
            u => out.push(u as u8 as char),
        }
    }
    out.into_bytes()
}

/// Replaces the path `from` with `to` wherever it's a whole path or the start
/// of one (followed by a backslash, quote, comma…), ignoring ASCII case.
fn replace_path(text: &[u8], from: &[u8], to: &[u8]) -> (Vec<u8>, usize) {
    if from.is_empty() {
        return (text.to_vec(), 0);
    }
    let mut out = Vec::with_capacity(text.len());
    let (mut i, mut n) = (0, 0);
    while i < text.len() {
        if text.len() - i >= from.len() && text[i..i + from.len()].eq_ignore_ascii_case(from) {
            let next = text.get(i + from.len()).copied();
            if matches!(next, None | Some(b'\\' | b'"' | b',' | b';' | b'/' | b'\n' | b'\r')) {
                out.extend_from_slice(to);
                i += from.len();
                n += 1;
                continue;
            }
        }
        out.push(text[i]);
        i += 1;
    }
    (out, n)
}

/// Points registry strings that lead into `from` (any of its spellings) into
/// `to` instead: install paths, uninstallers, icons, a game's own settings.
/// Only while nothing runs in the prefix — Wine rewrites these files.
/// Keeps the previous files as *.reg.pishop-bak.
pub fn relocate_registry(pfx: &Path, from: &[String], to: &str) -> anyhow::Result<usize> {
    let mut total = 0;
    let to = reg_escape(to);
    for file in ["system.reg", "user.reg"] {
        let path = pfx.join(file);
        let Ok(mut text) = std::fs::read(&path) else { continue };
        let mut n = 0;
        for f in from {
            let (t, c) = replace_path(&text, &reg_escape(f), &to);
            text = t;
            n += c;
        }
        if n == 0 {
            continue;
        }
        std::fs::copy(&path, pfx.join(format!("{file}.pishop-bak")))?;
        let tmp = pfx.join(format!("{file}.pishop-tmp"));
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &path)?;
        total += n;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_values_are_unescaped() {
        let (k, v) = parse_value(r#""Inno Setup: App Path"="D:\\Games\\Minecraft Dungeons II""#).unwrap();
        assert_eq!(k, "Inno Setup: App Path");
        assert_eq!(v, r"D:\Games\Minecraft Dungeons II");
        let (_, v) = parse_value(r#""UninstallString"="\"D:\\Games\\X\\unins000.exe\"""#).unwrap();
        assert_eq!(v, r#""D:\Games\X\unins000.exe""#);
        let (_, v) = parse_value(r#""DisplayName"="Marvel\x2019s Spider-Man""#).unwrap();
        assert_eq!(v, "Marvel’s Spider-Man");
        assert!(parse_value(r#""EstimatedSize"=dword:008e7feb"#).is_none());
    }

    #[test]
    fn escaping_matches_wine() {
        assert_eq!(reg_escape(r"D:\Games\X"), br"D:\\Games\\X".to_vec());
        assert_eq!(reg_escape("Marvel’s"), br"Marvel\x2019s".to_vec());
        // Padded to 4 digits when a hex digit follows.
        assert_eq!(reg_escape("é1"), br"\x00e91".to_vec());
        assert_eq!(reg_escape("éz"), br"\xe9z".to_vec());
    }

    #[test]
    fn paths_are_replaced_only_whole() {
        let text = br#""A"="D:\\Games\\Minecraft Dungeons II"
"B"="\"d:\\games\\minecraft dungeons ii\\unins000.exe\""
"C"="D:\\Games\\Minecraft Dungeons III\\x.exe"
"#;
        let (out, n) = replace_path(text, &reg_escape(r"D:\Games\Minecraft Dungeons II"), &reg_escape(r"C:\Program Files\Minecraft Dungeons II"));
        assert_eq!(n, 2);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains(r#""A"="C:\\Program Files\\Minecraft Dungeons II""#));
        assert!(out.contains(r#"\"C:\\Program Files\\Minecraft Dungeons II\\unins000.exe\""#));
        assert!(out.contains(r#""C"="D:\\Games\\Minecraft Dungeons III\\x.exe""#));
    }

    #[test]
    fn drive_letters_map_both_ways() {
        let dir = std::env::temp_dir().join(format!("pishop-pfx-{}", std::process::id()));
        let pfx = dir.join("pfx");
        let games = dir.join("SN01T/Games/My Game");
        std::fs::create_dir_all(pfx.join("dosdevices")).unwrap();
        std::fs::create_dir_all(pfx.join("drive_c/Program Files")).unwrap();
        std::fs::create_dir_all(&games).unwrap();
        std::os::unix::fs::symlink("../drive_c", pfx.join("dosdevices/c:")).unwrap();
        std::os::unix::fs::symlink(dir.join("SN01T"), pfx.join("dosdevices/d:")).unwrap();
        std::os::unix::fs::symlink("/", pfx.join("dosdevices/z:")).unwrap();
        std::os::unix::fs::symlink("/dev/null", pfx.join("dosdevices/d::")).unwrap();

        assert_eq!(drives(&pfx).iter().map(|d| d.0).collect::<String>(), "CDZ");
        let games = games.canonicalize().unwrap();
        assert_eq!(to_linux(&pfx, r"D:\GAMES\my game").unwrap(), games);
        assert_eq!(to_linux(&pfx, r"c:\program files").unwrap(), pfx.join("drive_c/Program Files").canonicalize().unwrap());
        assert!(to_linux(&pfx, r"D:\Nope").is_none());
        let spellings = windows_paths(&pfx, &games);
        assert_eq!(spellings[0], r"D:\Games\My Game");
        assert!(spellings[1].starts_with("Z:\\") && spellings[1].ends_with(r"\SN01T\Games\My Game"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
