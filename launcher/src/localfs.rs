//! Deck-side file access for the explorer: well-known destinations ("places"),
//! directory listings and free space.

use std::path::{Path, PathBuf};

use serde::Serialize;
use crate::tr;

#[derive(Serialize, Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Unix seconds; 0 when unknown.
    pub mtime: i64,
}

#[derive(Serialize)]
pub struct Place {
    pub id: String,
    pub label: String,
    pub path: String,
    pub icon: &'static str,
    pub free: u64,
    pub total: u64,
    /// "place" (folders and cards), "library" (a Steam library) or
    /// "prefix" (a non-Steam game's drive C:).
    pub group: &'static str,
    /// The disk it's on ("Internal storage", a card's label).
    pub disk: String,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/home/deck"))
}

/// (free, total) bytes of the filesystem holding `path`.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    Some((st.f_bavail as u64 * frsize, st.f_blocks as u64 * frsize))
}

/// Removable media mounted by SteamOS under /run/media/<user>/<label>.
/// The same card can be mounted twice (/run/media/deck/X and /run/media/X);
/// it is listed once, preferring the per-user path.
fn sd_cards() -> Vec<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let user = std::env::var("USER").unwrap_or_else(|_| "deck".into());
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = Vec::new();
    for base in [PathBuf::from("/run/media").join(&user), PathBuf::from("/run/media")] {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_dir() || p.file_name().is_some_and(|n| n == user.as_str()) || !is_mountpoint(&p) {
                continue;
            }
            let Ok(dev) = std::fs::metadata(&p).map(|m| m.dev()) else { continue };
            if !seen.contains(&dev) {
                seen.push(dev);
                out.push(p);
            }
        }
    }
    out
}

fn is_mountpoint(p: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(p), p.parent().map(std::fs::metadata)) {
        (Ok(m), Some(Ok(parent))) => m.dev() != parent.dev(),
        _ => false,
    }
}

pub fn places() -> Vec<Place> {
    let home = home();
    let mut list: Vec<(String, String, PathBuf, &'static str)> = Vec::new();
    // EmuDeck keeps ROMs in ~/Emulation (or on the SD card, depending on setup).
    let mut rom_dirs = vec![home.join("Emulation/roms")];
    for sd in sd_cards() {
        rom_dirs.push(sd.join("Emulation/roms"));
    }
    for (i, dir) in rom_dirs.into_iter().filter(|d| d.is_dir()).enumerate() {
        let on_sd = !dir.starts_with(&home);
        let label = if on_sd { tr!("ROMs (SD card)", "ROMs (cartão SD)") } else { "ROMs (EmuDeck)".into() };
        list.push((format!("roms{i}"), label, dir, "roms"));
    }
    for sd in sd_cards() {
        let name = sd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        list.push((format!("sd:{name}"), tr!("SD card ({name})", "Cartão SD ({name})"), sd, "sd"));
    }
    let emus = home.join("Emulation");
    if emus.is_dir() {
        list.push(("emulation".into(), "Emulation".into(), emus, "folder"));
    }
    list.push(("downloads".into(), "Downloads".into(), home.join("Downloads"), "download"));
    list.push(("home".into(), tr!("Home folder", "Pasta pessoal"), home.clone(), "home"));
    let place = |(id, label, path, icon): (String, String, PathBuf, &'static str), group: &'static str| {
        let (free, total) = disk_space(&path).unwrap_or((0, 0));
        Place { id, label, disk: crate::install::disk_label(&path), path: path.display().to_string(), icon, free, total, group }
    };
    let mut out: Vec<Place> = list.into_iter().map(|p| place(p, "place")).collect();

    // Steam's libraries (internal storage, SD card, other drives).
    for lib in crate::proton::library_paths().into_iter().filter(|l| l.is_dir()) {
        let label = tr!("Steam library · {}", "Biblioteca Steam · {}", crate::install::disk_label(&lib));
        out.push(place((format!("lib:{}", lib.display()), label, lib, "library"), "library"));
    }
    // Each non-Steam game's drive C:, by the game's name.
    let mut prefixes: Vec<Place> = crate::steam::read_shortcuts()
        .into_iter()
        .filter_map(|s| {
            let c = crate::proton::compatdata(s.appid).join("pfx/drive_c");
            c.is_dir().then(|| place((format!("pfx:{}", s.appid), s.name, c, "prefix"), "prefix"))
        })
        .collect();
    prefixes.sort_by_key(|p| p.label.to_lowercase());
    out.extend(prefixes);
    out
}

/// Directories first, then case-insensitive natural order ("Disc 2" < "Disc 10").
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| natural_cmp(&a.name, &b.name)));
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

pub fn list(path: &Path) -> std::io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(path)? {
        let Ok(e) = e else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        // Follow symlinks so linked ROM folders behave like folders.
        let Ok(meta) = std::fs::metadata(e.path()) else { continue };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Entry { name, dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, mtime });
    }
    sort_entries(&mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn natural_order() {
        let mut v: Vec<_> = ["Disc 10", "disc 2", "Alpha", "Disc 1"]
            .iter()
            .map(|n| super::Entry { name: n.to_string(), dir: false, size: 0, mtime: 0 })
            .collect();
        super::sort_entries(&mut v);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Disc 1", "disc 2", "Disc 10"]);
    }
}
