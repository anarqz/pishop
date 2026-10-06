//! Registers the launcher as a non-Steam game by editing every local Steam
//! account's `shortcuts.vdf`. Steam only reads that file on startup, so it has
//! to be restarted afterwards.

use std::io;
use std::path::{Path, PathBuf};

use crate::APP_NAME;
use crate::vdf::{self, Value};

fn userdata_configs() -> io::Result<Vec<PathBuf>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::other("HOME não definido"))?;
    let root = home.join(".local/share/Steam/userdata");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|e| io::Error::other(format!("{}: {e}", root.display())))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "0" || !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let config = entry.path().join("config");
        if config.is_dir() {
            out.push(config);
        }
    }
    if out.is_empty() {
        return Err(io::Error::other("nenhuma conta Steam encontrada"));
    }
    Ok(out)
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Same id scheme Steam ROM Manager / BoilR use; it also names the grid art.
fn shortcut_appid(exe: &str) -> u32 {
    crc32(format!("{exe}{APP_NAME}").as_bytes()) | 0x8000_0000
}

fn load(path: &Path) -> io::Result<Value> {
    match std::fs::read(path) {
        Ok(buf) if !buf.is_empty() => vdf::parse(&buf),
        Ok(_) => Ok(Value::Map(vec![(b"shortcuts".to_vec(), Value::Map(vec![]))])),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Ok(Value::Map(vec![(b"shortcuts".to_vec(), Value::Map(vec![]))]))
        }
        Err(e) => Err(e),
    }
}

fn shortcuts_mut(root: &mut Value) -> io::Result<&mut Vec<(Vec<u8>, Value)>> {
    let Value::Map(top) = root else { return Err(io::Error::other("raiz inválida")) };
    if !top.iter().any(|(k, _)| k.eq_ignore_ascii_case(b"shortcuts")) {
        top.push((b"shortcuts".to_vec(), Value::Map(vec![])));
    }
    match top.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(b"shortcuts")) {
        Some((_, Value::Map(m))) => Ok(m),
        _ => Err(io::Error::other("'shortcuts' não é um mapa")),
    }
}

fn is_ours(entry: &Value) -> bool {
    entry.get("AppName").and_then(Value::as_str) == Some(APP_NAME.as_bytes())
}

fn reindex(list: &mut [(Vec<u8>, Value)]) {
    for (i, (k, _)) in list.iter_mut().enumerate() {
        *k = i.to_string().into_bytes();
    }
}

fn save(path: &Path, root: &Value) -> io::Result<()> {
    if path.exists() {
        std::fs::copy(path, path.with_extension("vdf.pishop-bak"))?;
    }
    let tmp = path.with_extension("vdf.pishop-tmp");
    std::fs::write(&tmp, vdf::serialize(root))?;
    std::fs::rename(&tmp, path)
}

fn entry(appid: u32, exe: &str, start_dir: &str, icon: &str) -> Value {
    let i = |n: i32| Value::Int(n);
    Value::Map(vec![
        (b"appid".to_vec(), i(appid as i32)),
        (b"AppName".to_vec(), Value::str(APP_NAME)),
        (b"Exe".to_vec(), Value::str(exe)),
        (b"StartDir".to_vec(), Value::str(start_dir)),
        (b"icon".to_vec(), Value::str(icon)),
        (b"ShortcutPath".to_vec(), Value::str("")),
        (b"LaunchOptions".to_vec(), Value::str("")),
        (b"IsHidden".to_vec(), i(0)),
        (b"AllowDesktopConfig".to_vec(), i(1)),
        (b"AllowOverlay".to_vec(), i(1)),
        (b"OpenVR".to_vec(), i(0)),
        (b"Devkit".to_vec(), i(0)),
        (b"DevkitGameID".to_vec(), Value::str("")),
        (b"DevkitOverrideAppID".to_vec(), i(0)),
        (b"LastPlayTime".to_vec(), i(0)),
        (b"FlatpakAppID".to_vec(), Value::str("")),
        (b"tags".to_vec(), Value::Map(vec![])),
    ])
}

/// Copies bundled artwork (`art/`) into Steam's grid folder for `appid`.
fn install_art(config: &Path, appid: u32) -> io::Result<()> {
    let art = crate::base_dir().join("art");
    let grid = config.join("grid");
    std::fs::create_dir_all(&grid)?;
    for (src, dst) in [
        ("capsule.png", format!("{appid}p.png")),
        ("wide.png", format!("{appid}.png")),
        ("hero.png", format!("{appid}_hero.png")),
        ("logo.png", format!("{appid}_logo.png")),
    ] {
        let from = art.join(src);
        if from.exists() {
            std::fs::copy(&from, grid.join(dst))?;
        }
    }
    Ok(())
}

pub fn install() -> io::Result<()> {
    let base = crate::base_dir();
    let exe_path = std::env::current_exe()?.canonicalize()?;
    let exe = format!("\"{}\"", exe_path.display());
    let start_dir = format!("\"{}\"", base.display());
    let icon_path = base.join("art/icon.png");
    let icon = if icon_path.exists() { icon_path.display().to_string() } else { String::new() };
    let appid = shortcut_appid(&exe);

    for config in userdata_configs()? {
        let path = config.join("shortcuts.vdf");
        let mut root = load(&path)?;
        let list = shortcuts_mut(&mut root)?;
        list.retain(|(_, v)| !is_ours(v));
        list.push((Vec::new(), entry(appid, &exe, &start_dir, &icon)));
        reindex(list);
        save(&path, &root)?;
        install_art(&config, appid)?;
        println!("atalho gravado em {}", path.display());
    }
    println!("appid {appid}. Reinicie a Steam para o {APP_NAME} aparecer na biblioteca.");
    Ok(())
}

pub fn uninstall() -> io::Result<()> {
    for config in userdata_configs()? {
        let path = config.join("shortcuts.vdf");
        let mut root = load(&path)?;
        let list = shortcuts_mut(&mut root)?;
        let before = list.len();
        list.retain(|(_, v)| !is_ours(v));
        if list.len() != before {
            reindex(list);
            save(&path, &root)?;
            println!("atalho removido de {}", path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc32_matches_reference() {
        assert_eq!(super::crc32(b"123456789"), 0xCBF4_3926);
    }
}
