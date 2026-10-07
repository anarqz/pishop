//! piShop doubles as the two archive tools winetricks needs and the Steam
//! Linux Runtime doesn't have: `cabextract` and `unzip`, with the options
//! winetricks uses. The binary is static, so it also runs inside the
//! runtime's container; winetricks finds it through symlinks with those names
//! (see `install_links`).

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

/// Runs as `cabextract` or `unzip` when called by one of those names.
pub fn multicall() -> Option<i32> {
    let argv0 = std::env::args_os().next()?;
    let name = Path::new(&argv0).file_name()?.to_string_lossy().into_owned();
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    match name.as_str() {
        "cabextract" => Some(cabextract(&args)),
        "unzip" => Some(unzip(&args)),
        _ => None,
    }
}

/// `cabextract` and `unzip` symlinks to this binary, in `dir`.
pub fn install_links(dir: &Path) -> io::Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    std::fs::create_dir_all(dir)?;
    for name in ["cabextract", "unzip"] {
        let link = dir.join(name);
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&exe, &link)?;
    }
    Ok(())
}

/// Shell-style wildcard match (`*`, `?`, `[a-z]`, `[!x]`), ASCII case-insensitive.
fn glob(pattern: &str, name: &str) -> bool {
    fn class(p: &[char], c: char) -> Option<(bool, usize)> {
        // p starts after '['; returns (matched, chars used including ']').
        let (neg, mut i) = if matches!(p.first(), Some('!' | '^')) { (true, 1) } else { (false, 0) };
        let mut hit = false;
        let start = i;
        while i < p.len() && (p[i] != ']' || i == start) {
            if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
                if p[i] <= c && c <= p[i + 2] {
                    hit = true;
                }
                i += 3;
            } else {
                hit |= p[i] == c;
                i += 1;
            }
        }
        (i < p.len()).then_some((hit != neg, i + 1))
    }
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let n: Vec<char> = name.to_ascii_lowercase().chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        let step = match p.get(pi) {
            Some('*') => {
                star = Some((pi, ni));
                pi += 1;
                continue;
            }
            Some('?') => Some(1),
            Some('[') => match class(&p[pi + 1..], n[ni]) {
                Some((true, used)) => Some(used + 1),
                Some((false, _)) => None,
                None => (n[ni] == '[').then_some(1),
            },
            Some(c) => (*c == n[ni]).then_some(1),
            None => None,
        };
        match step {
            Some(used) => {
                pi += used;
                ni += 1;
            }
            None => match star {
                Some((sp, sn)) => {
                    pi = sp + 1;
                    ni = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// A name from an archive as a safe relative path (no root, no `..`).
fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in Path::new(&name.replace('\\', "/")).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir | Component::RootDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// A line on stdout; a closed pipe (`| head`) isn't an error worth dying for.
fn say(line: impl std::fmt::Display) {
    let _ = writeln!(io::stdout().lock(), "{line}");
}

fn esay(line: impl std::fmt::Display) {
    let _ = writeln!(io::stderr().lock(), "{line}");
}

fn write_file(path: &Path, mut from: impl Read) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A link in the way (Wine's DLL folders have them) is replaced, not followed.
    if path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::remove_file(path)?;
    }
    let mut out = File::create(path)?;
    io::copy(&mut from, &mut out)?;
    out.flush()
}

// ---------- cabextract ----------

/// A window into a file: a cabinet embedded at some offset.
struct Slice {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
}

impl Read for Slice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len.saturating_sub(self.pos);
        if left == 0 {
            return Ok(0);
        }
        let want = buf.len().min(left as usize);
        self.file.seek(SeekFrom::Start(self.start + self.pos))?;
        let n = self.file.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Slice {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(d) => self.len as i64 + d,
            SeekFrom::Current(d) => self.pos as i64 + d,
        };
        if pos < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = pos as u64;
        Ok(self.pos)
    }
}

/// Length of a plausible cabinet whose header starts at `off`.
fn cab_header(file: &mut File, off: u64, size: u64) -> io::Result<Option<u64>> {
    let mut h = [0u8; 36];
    file.seek(SeekFrom::Start(off))?;
    if file.read(&mut h)? < 36 {
        return Ok(None);
    }
    let u32le = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]) as u64;
    let u16le = |i: usize| u16::from_le_bytes([h[i], h[i + 1]]);
    let len = u32le(8);
    let files_at = u32le(16);
    let plausible = &h[..4] == b"MSCF"
        && u32le(4) == 0
        && len >= 36
        && off + len <= size
        && files_at >= 36
        && files_at < len
        && h[24] == 3
        && h[25] == 1
        && u16le(26) > 0
        && u16le(28) > 0;
    Ok(plausible.then_some(len))
}

/// Cabinets in a file as (offset, length): the file itself, or the ones
/// embedded in it (installers carry theirs inside the .exe).
fn find_cabinets(file: &mut File) -> io::Result<Vec<(u64, u64)>> {
    let size = file.metadata()?.len();
    let mut found = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut pos = 0u64;
    while pos + 36 <= size {
        file.seek(SeekFrom::Start(pos))?;
        let mut n = 0;
        while n < buf.len() {
            let r = file.read(&mut buf[n..])?;
            if r == 0 {
                break;
            }
            n += r;
        }
        if n < 4 {
            break;
        }
        let mut next = pos + (n as u64).saturating_sub(3).max(1);
        let mut i = 0;
        while i + 4 <= n {
            if &buf[i..i + 4] == b"MSCF" {
                let off = pos + i as u64;
                if let Some(len) = cab_header(file, off, size)? {
                    found.push((off, len));
                    next = off + len;
                    break;
                }
            }
            i += 1;
        }
        pos = next;
    }
    Ok(found)
}

struct CabArgs {
    dir: PathBuf,
    lowercase: bool,
    filter: Option<String>,
    quiet: bool,
    list: bool,
    files: Vec<PathBuf>,
}

fn cabextract(args: &[String]) -> i32 {
    let mut a = CabArgs { dir: ".".into(), lowercase: false, filter: None, quiet: false, list: false, files: Vec::new() };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-d" | "--directory" => match it.next() {
                Some(d) => a.dir = d.into(),
                None => return cab_usage(),
            },
            "-F" | "--filter" => match it.next() {
                Some(f) => a.filter = Some(f.clone()),
                None => return cab_usage(),
            },
            "-q" | "--quiet" => a.quiet = true,
            "-L" | "--lowercase" => a.lowercase = true,
            "-l" | "--list" => a.list = true,
            "-v" | "--version" => {
                say(format_args!("cabextract version 1.11 (piShop)"));
                return 0;
            }
            "-h" | "--help" => return cab_usage(),
            "--" => a.files.extend(it.by_ref().map(PathBuf::from)),
            s if s.starts_with("--directory=") => a.dir = s["--directory=".len()..].into(),
            s if s.starts_with("--filter=") => a.filter = Some(s["--filter=".len()..].into()),
            s if s.starts_with("-d") => a.dir = s[2..].into(),
            s if s.starts_with("-F") => a.filter = Some(s[2..].into()),
            s if s.len() > 1 && s.starts_with('-') && s[1..].chars().all(|c| "qLlv".contains(c)) => {
                if s.contains('v') {
                    say(format_args!("cabextract version 1.11 (piShop)"));
                    return 0;
                }
                a.quiet |= s.contains('q');
                a.lowercase |= s.contains('L');
                a.list |= s.contains('l');
            }
            s if s.len() > 1 && s.starts_with('-') => {
                esay(format_args!("cabextract: unsupported option {s}"));
                return 1;
            }
            s => a.files.push(s.into()),
        }
    }
    if a.files.is_empty() {
        return cab_usage();
    }
    let mut failed = false;
    for f in &a.files {
        match extract_cabs(f, &a) {
            Ok(0) => {
                esay(format_args!("{}: no valid cabinets found", f.display()));
                failed = true;
            }
            Ok(_) => {}
            Err(e) => {
                esay(format_args!("{}: {e}", f.display()));
                failed = true;
            }
        }
    }
    i32::from(failed)
}

fn cab_usage() -> i32 {
    esay(format_args!("Usage: cabextract [-q] [-L] [-l] [-d dir] [-F pattern] file..."));
    1
}

/// Extracts (or lists) the matching files of every cabinet in `path`.
fn extract_cabs(path: &Path, a: &CabArgs) -> anyhow::Result<usize> {
    let mut file = File::open(path)?;
    let cabinets = find_cabinets(&mut file)?;
    for &(start, len) in &cabinets {
        let slice = Slice { file: file.try_clone()?, start, len, pos: 0 };
        let mut cabinet = cab::Cabinet::new(slice)?;
        let names: Vec<String> =
            cabinet.folder_entries().flat_map(|f| f.file_entries().map(|e| e.name().to_string()).collect::<Vec<_>>()).collect();
        for name in names {
            let unix = name.replace('\\', "/");
            if a.filter.as_deref().is_some_and(|f| !glob(f, &unix)) {
                continue;
            }
            let Some(rel) = safe_relative(&if a.lowercase { unix.to_lowercase() } else { unix.clone() }) else { continue };
            if a.list {
                say(format_args!("{}", rel.display()));
                continue;
            }
            let dest = a.dir.join(&rel);
            if !a.quiet {
                say(format_args!("  extracting {}", dest.display()));
            }
            let reader = cabinet.read_file(&name)?;
            write_file(&dest, reader).map_err(|e| anyhow::anyhow!("{}: {e}", dest.display()))?;
        }
    }
    Ok(cabinets.len())
}

/// The CPU a Windows DLL/EXE is built for (PE "Machine": 0x14c x86,
/// 0x8664 x64, 0xaa64 ARM64).
fn pe_machine(b: &[u8]) -> Option<u16> {
    if b.len() < 0x40 || &b[..2] != b"MZ" {
        return None;
    }
    let at = u32::from_le_bytes([b[0x3c], b[0x3d], b[0x3e], b[0x3f]]) as usize;
    if b.get(at..at + 4)? != b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes([*b.get(at + 4)?, *b.get(at + 5)?]))
}

fn read_all<R: Read + Seek>(cabinet: &mut cab::Cabinet<R>, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut data = Vec::new();
    cabinet.read_file(name)?.read_to_end(&mut data)?;
    Ok(data)
}

pub const X86: u16 = 0x14c;
pub const X64: u16 = 0x8664;

/// A DLL for one CPU from the cabinets inside `bundle`, looking one level
/// into cabinets stored in them (Visual C++ redistributables keep their files
/// that way, sometimes named with an architecture suffix: "msvcp140.dll_amd64").
pub fn find_dll(bundle: &Path, name: &str, machine: u16) -> anyhow::Result<Option<Vec<u8>>> {
    let name = name.to_ascii_lowercase();
    let wanted = |n: &str| {
        let n = n.replace('\\', "/").rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        n == name || n.starts_with(&format!("{name}_"))
    };
    let mut file = File::open(bundle)?;
    for (start, len) in find_cabinets(&mut file)? {
        let mut outer = cab::Cabinet::new(Slice { file: file.try_clone()?, start, len, pos: 0 })?;
        let names: Vec<String> =
            outer.folder_entries().flat_map(|f| f.file_entries().map(|e| e.name().to_string()).collect::<Vec<_>>()).collect();
        for n in names {
            let data = read_all(&mut outer, &n)?;
            if data.starts_with(b"MSCF") {
                let mut inner = cab::Cabinet::new(io::Cursor::new(data))?;
                let inner_names: Vec<String> =
                    inner.folder_entries().flat_map(|f| f.file_entries().map(|e| e.name().to_string()).collect::<Vec<_>>()).collect();
                for m in inner_names.into_iter().filter(|m| wanted(m)) {
                    let dll = read_all(&mut inner, &m)?;
                    if pe_machine(&dll) == Some(machine) {
                        return Ok(Some(dll));
                    }
                }
            } else if wanted(&n) && pe_machine(&data) == Some(machine) {
                return Ok(Some(data));
            }
        }
    }
    Ok(None)
}

// ---------- unzip ----------

fn unzip(args: &[String]) -> i32 {
    let mut dir = PathBuf::from(".");
    let (mut zipfile, mut names) = (None::<PathBuf>, Vec::<String>::new());
    let (mut never, mut junk, mut list) = (false, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-d" {
            match it.next() {
                Some(d) => dir = d.into(),
                None => return 10,
            }
        } else if let Some(d) = a.strip_prefix("-d").filter(|d| !d.is_empty()) {
            dir = d.into();
        } else if a.len() > 1 && a.starts_with('-') && zipfile.is_none() {
            for c in a[1..].chars() {
                match c {
                    'n' => never = true,
                    'j' => junk = true,
                    'l' => list = true,
                    'o' | 'q' | 'C' | 'a' | 'b' | 'u' | 'X' => {}
                    other => {
                        esay(format_args!("unzip: unsupported option -{other}"));
                        return 10;
                    }
                }
            }
        } else if zipfile.is_none() {
            zipfile = Some(a.into());
        } else {
            names.push(a.clone());
        }
    }
    let Some(zipfile) = zipfile else {
        esay(format_args!("Usage: unzip [-o] [-q] [-n] [-j] [-l] [-d dir] file.zip [names...]"));
        return 10;
    };
    let file = match File::open(&zipfile) {
        Ok(f) => f,
        Err(e) => {
            esay(format_args!("unzip: cannot find or open {}: {e}", zipfile.display()));
            return 9;
        }
    };
    let mut archive = match zip::ZipArchive::new(BufReader::new(file)) {
        Ok(z) => z,
        Err(e) => {
            esay(format_args!("unzip: {}: {e}", zipfile.display()));
            return 9;
        }
    };
    let mut matched = 0;
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                esay(format_args!("unzip: {e}"));
                return 2;
            }
        };
        let name = entry.name().to_string();
        if !names.is_empty() && !names.iter().any(|p| glob(p, &name)) {
            continue;
        }
        let Some(rel) = safe_relative(&name) else { continue };
        matched += 1;
        if list {
            say(format_args!("{name}"));
            continue;
        }
        let dest = if junk { dir.join(rel.file_name().unwrap_or_default()) } else { dir.join(&rel) };
        if entry.is_dir() {
            if !junk {
                let _ = std::fs::create_dir_all(&dest);
            }
            continue;
        }
        if never && dest.exists() {
            continue;
        }
        let mode = entry.unix_mode();
        if let Err(e) = write_file(&dest, &mut entry) {
            esay(format_args!("unzip: {}: {e}", dest.display()));
            return 50;
        }
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    if !names.is_empty() && matched == 0 {
        esay(format_args!("unzip: no matching files in {}", zipfile.display()));
        return 11;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards_like_cabextract() {
        assert!(glob("*d3dx9*x86*", "Apr2005_d3dx9_25_x86.cab"));
        assert!(glob("d3dx9*.dll", "D3DX9_43.dll"));
        assert!(glob("*.dll", "sub/dir/x.DLL"));
        assert!(glob("vcredist.exe", "vcredist.exe"));
        assert!(!glob("vcredist.exe", "vcredist.exe.bak"));
        assert!(glob("a1?", "a10"));
        assert!(!glob("a1?", "a1"));
        assert!(glob("xinput1_[1-3].dll", "xinput1_2.dll"));
        assert!(!glob("xinput1_[!1-3].dll", "xinput1_2.dll"));
        assert!(glob("*", ""));
    }

    #[test]
    fn archive_names_stay_inside() {
        assert_eq!(safe_relative(r"sub\dir\a.dll").unwrap(), Path::new("sub/dir/a.dll"));
        assert_eq!(safe_relative("/abs/a.dll").unwrap(), Path::new("abs/a.dll"));
        assert!(safe_relative("../../etc/passwd").is_none());
        assert!(safe_relative("").is_none());
    }

    #[test]
    fn finds_cabinets_inside_other_files() {
        // A minimal valid header (one folder, one file) after 100 bytes of "exe".
        let mut cab = Vec::new();
        cab.extend_from_slice(b"MSCF");
        cab.extend_from_slice(&0u32.to_le_bytes());
        cab.extend_from_slice(&60u32.to_le_bytes()); // cbCabinet
        cab.extend_from_slice(&0u32.to_le_bytes());
        cab.extend_from_slice(&44u32.to_le_bytes()); // coffFiles
        cab.extend_from_slice(&0u32.to_le_bytes());
        cab.extend_from_slice(&[3, 1]);
        cab.extend_from_slice(&1u16.to_le_bytes());
        cab.extend_from_slice(&1u16.to_le_bytes());
        cab.extend_from_slice(&[0; 6]);
        cab.resize(60, 0);
        let mut data = vec![b'x'; 100];
        data.extend_from_slice(b"MSCF not a header");
        data.extend_from_slice(&cab);
        data.extend_from_slice(&[0; 10]);
        let path = std::env::temp_dir().join(format!("pishop-cab-{}", std::process::id()));
        std::fs::write(&path, &data).unwrap();
        let found = find_cabinets(&mut File::open(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(found, vec![(117, 60)]);
    }
}
