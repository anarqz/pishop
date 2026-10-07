//! Steam's redistributables: `Steamworks Shared/_CommonRedist` (Visual C++,
//! DirectX, .NET, PhysX…), the installers Steam runs in a game's prefix
//! before its first start. Each component folder carries Steam's own
//! `installscript.vdf` — the programs to run, their silent flags and the
//! registry mark ("hasrunkey") Steam writes once it ran — and piShop follows
//! it to the letter, in any shortcut's prefix, offline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::proton;

/// What one installscript entry runs.
#[derive(Clone, Debug, PartialEq)]
pub enum Program {
    /// A Windows executable with its arguments.
    Exe { path: PathBuf, args: Vec<String> },
    /// A .cmd/.bat script (run through `cmd /c`).
    Cmd { path: PathBuf },
    /// `msiexec` with its arguments (paths already Windows paths).
    Msiexec { args: Vec<String> },
}

/// One entry of a component ("x86 14.51.36247.0", "x64 …", "dxsetup").
#[derive(Clone, Debug)]
pub struct Step {
    pub name: String,
    pub programs: Vec<Program>,
    /// Registry key Steam marks, without the hive ("Software\Valve\Steam\Apps\CommonRedist\vcredist\2022").
    pub mark: String,
}

#[derive(Clone, Debug)]
pub struct Redist {
    /// Folder under _CommonRedist ("vcredist/2022", "DirectX/Jun2010").
    pub id: String,
    pub title: String,
    pub steps: Vec<Step>,
}

/// Every library's Steamworks Shared folder that has redistributables.
pub fn shared_dirs() -> Vec<PathBuf> {
    proton::library_paths()
        .into_iter()
        .map(|l| l.join("steamapps/common/Steamworks Shared"))
        .filter(|d| d.join("_CommonRedist").is_dir())
        .collect()
}

/// The components on this machine (one per folder with an installscript).
pub fn list() -> Vec<Redist> {
    let mut out: Vec<Redist> = Vec::new();
    for shared in shared_dirs() {
        let root = shared.join("_CommonRedist");
        let mut stack = vec![(root.clone(), 0)];
        while let Some((dir, depth)) = stack.pop() {
            let script = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("installscript.vdf"))
                .map(|e| e.path());
            if let Some(script) = script {
                let id = dir.strip_prefix(&root).unwrap_or(&dir).to_string_lossy().into_owned();
                let text = std::fs::read_to_string(&script).unwrap_or_default();
                let steps = steps(&text, &shared);
                if !steps.is_empty() && !out.iter().any(|r| r.id == id) {
                    out.push(Redist { title: title(&id), id, steps });
                }
                continue;
            }
            if depth < 3 {
                for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    if e.path().is_dir() {
                        stack.push((e.path(), depth + 1));
                    }
                }
            }
        }
    }
    out.sort_by_key(|r| order(&r.id));
    out
}

/// Visual C++ newest first, then DirectX, .NET, the rest.
fn order(id: &str) -> (u8, std::cmp::Reverse<String>) {
    let lower = id.to_lowercase();
    let group = if lower.starts_with("vcredist") {
        0
    } else if lower.starts_with("directx") {
        1
    } else if lower.starts_with("dotnet") {
        2
    } else {
        3
    };
    (group, std::cmp::Reverse(lower))
}

fn title(id: &str) -> String {
    let (group, rest) = id.split_once('/').unwrap_or((id, ""));
    match group.to_lowercase().as_str() {
        "vcredist" => format!("Visual C++ {rest}"),
        "directx" if rest.eq_ignore_ascii_case("jun2010") => "DirectX (June 2010)".into(),
        "directx" => format!("DirectX ({rest})"),
        "dotnet" => format!(".NET Framework {rest}"),
        "physx" => format!("PhysX {rest}"),
        "xna" => format!("XNA Framework {rest}"),
        "openal" => format!("OpenAL {rest}"),
        _ => id.replace('/', " "),
    }
}

// ---------- installscript.vdf ----------

#[derive(Debug, Clone)]
enum Kv {
    Str(String),
    Map(Vec<(String, Kv)>),
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        match self {
            Kv::Map(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            Kv::Str(_) => None,
        }
    }
    fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Kv::Str(s) => Some(s),
            Kv::Map(_) => None,
        }
    }
}

/// Text KeyValues ("key" "value" / "key" { … }), as Steam writes them.
fn parse_kv(text: &str) -> Kv {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        c => s.push(c),
                    }
                }
                tokens.push(Some(s));
            }
            '{' => tokens.push(Some("{".into())),
            '}' => tokens.push(None),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                let mut s = String::from(c);
                while let Some(&n) = chars.peek() {
                    if n.is_whitespace() || n == '{' || n == '}' || n == '"' {
                        break;
                    }
                    s.push(n);
                    chars.next();
                }
                // Platform conditionals ([$WIN32]) don't concern us.
                if !s.starts_with('[') {
                    tokens.push(Some(s));
                }
            }
        }
    }
    // A bare "{" token opens a map; None closes it.
    fn map(it: &mut std::iter::Peekable<std::vec::IntoIter<Option<String>>>) -> Vec<(String, Kv)> {
        let mut out = Vec::new();
        while let Some(tok) = it.next() {
            let Some(key) = tok else { break };
            match it.peek() {
                Some(Some(v)) if v == "{" => {
                    it.next();
                    out.push((key, Kv::Map(map(it))));
                }
                Some(Some(_)) => {
                    if let Some(Some(v)) = it.next() {
                        out.push((key, Kv::Str(v)));
                    }
                }
                _ => break,
            }
        }
        out
    }
    Kv::Map(map(&mut tokens.into_iter().peekable()))
}

/// Splits a command line the way Windows programs see it (quotes group).
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in args.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The steps an installscript asks for on 64-bit Windows 10, with
/// %INSTALLDIR% (the Steamworks Shared folder) filled in.
fn steps(text: &str, shared: &Path) -> Vec<Step> {
    let root = parse_kv(text);
    let script = match &root {
        Kv::Map(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case("installscript")).map(|(_, v)| v.clone()),
        Kv::Str(_) => None,
    };
    let Some(Kv::Map(entries)) = script.as_ref().and_then(|s| s.get("Run Process")).cloned() else { return Vec::new() };
    let unix = |s: &str| -> PathBuf {
        let rest = s.replace("%INSTALLDIR%", "").replace('\\', "/");
        shared.join(rest.trim_start_matches('/'))
    };
    let windows_dir = format!("Z:{}", shared.display().to_string().replace('/', "\\"));
    let mut out = Vec::new();
    for (name, entry) in entries {
        // Entries for other systems ("OSType" "Windows XP") are skipped.
        if entry.get("Requirement_OS").is_some_and(|r| r.str("OSType").is_some()) {
            continue;
        }
        let mut programs = Vec::new();
        for n in 1..10 {
            let Some(process) = entry.str(&format!("process {n}")) else { break };
            let command = entry.str(&format!("command {n}")).unwrap_or("").replace("%INSTALLDIR%", &windows_dir);
            let lower = process.to_lowercase();
            let program = if lower == "msiexec" || lower.ends_with("\\msiexec.exe") {
                Program::Msiexec { args: split_args(&command) }
            } else if lower.ends_with(".cmd") || lower.ends_with(".bat") {
                Program::Cmd { path: unix(process) }
            } else {
                Program::Exe { path: unix(process), args: split_args(&command) }
            };
            programs.push(program);
        }
        let mark = entry
            .str("hasrunkey")
            .map(|k| k.split_once('\\').map(|(_, rest)| rest).unwrap_or(k).to_string())
            .unwrap_or_default();
        let present = programs.iter().all(|p| match p {
            Program::Exe { path, .. } | Program::Cmd { path } => path.is_file(),
            Program::Msiexec { .. } => true,
        });
        if !programs.is_empty() && present && !mark.is_empty() {
            out.push(Step { name, programs, mark });
        }
    }
    out
}

// ---------- what's installed in a prefix ----------

/// Steam's marks in a prefix: lowercased key (after `Valve\Steam\Apps\`) →
/// (when the key was written, its value names).
fn marks(pfx: &Path) -> HashMap<String, (i64, Vec<String>)> {
    let Ok(bytes) = std::fs::read(pfx.join("system.reg")) else { return HashMap::new() };
    let text = String::from_utf8_lossy(&bytes);
    let mut out = HashMap::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('[') {
            cur = None;
            let Some(end) = line.find(']') else { continue };
            let key = line[1..end].replace("\\\\", "\\").to_lowercase();
            if let Some(rest) = key.split_once("valve\\steam\\apps\\").map(|(_, r)| r.to_string()) {
                let ts = line[end + 1..].trim().parse().unwrap_or(0);
                out.insert(rest.clone(), (ts, Vec::new()));
                cur = Some(rest);
            }
        } else if let (Some(k), Some(name)) = (&cur, line.strip_prefix('"').and_then(|l| l.split_once("\"=")).map(|(n, _)| n)) {
            if let Some(entry) = out.get_mut(k) {
                entry.1.push(name.replace("\\\\", "\\").replace("\\\"", "\""));
            }
        }
    }
    out
}

/// When a file or folder was made: statx's birth time (Rust's `created()`
/// isn't available in piShop's static musl build).
#[cfg(target_os = "linux")]
fn birth(path: &Path) -> Option<i64> {
    use std::os::unix::ffi::OsStrExt;
    const STATX_BTIME: u32 = 0x800;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // struct statx: stx_mask at 0, stx_btime.tv_sec at 80.
    let mut buf = [0u8; 256];
    let r = unsafe { libc::syscall(libc::SYS_statx, libc::AT_FDCWD, c.as_ptr(), 0, STATX_BTIME, buf.as_mut_ptr()) };
    let mask = u32::from_ne_bytes(buf[0..4].try_into().ok()?);
    (r == 0 && mask & STATX_BTIME != 0).then(|| i64::from_ne_bytes(buf[80..88].try_into().unwrap_or_default()))
}

#[cfg(not(target_os = "linux"))]
fn birth(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).and_then(|m| m.created()).ok()?;
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// When the prefix was made (Proton copies its template then); Proton's
/// creation_sync_guard is a fallback where birth times aren't kept.
fn created(pfx: &Path) -> i64 {
    birth(pfx)
        .or_else(|| {
            let m = std::fs::metadata(pfx.join("creation_sync_guard")).and_then(|m| m.modified()).ok()?;
            m.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
        })
        .unwrap_or(0)
}

/// Whether every step of a component left Steam's mark in the prefix after
/// the prefix was made. Proton's template comes with .NET and XNA already
/// marked (so Steam skips them for Wine Mono): those older marks don't count.
pub fn installed(r: &Redist, pfx: &Path) -> bool {
    let marks = marks(pfx);
    let born = created(pfx) - 60;
    r.steps.iter().all(|s| {
        let key = s.mark.to_lowercase();
        let key = key.split_once("valve\\steam\\apps\\").map(|(_, r)| r).unwrap_or(&key);
        marks.get(key).is_some_and(|(ts, names)| *ts >= born && names.iter().any(|n| n.eq_ignore_ascii_case(&s.name)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VC2022: &str = r#""installscript"
{
	"Run Process"
	{
		"x86 14.51.36247.0"
		{
			"hasrunkey"		"HKEY_LOCAL_MACHINE\\Software\\Valve\\Steam\\Apps\\CommonRedist\\vcredist\\2022"
			"process 1"		"%INSTALLDIR%\\_CommonRedist\\vcredist\\2022\\Microsoft Visual C++ 2022 x86.cmd"
			"nocleanup"		"1"
		}
		"x64 14.51.36247.0"
		{
			"hasrunkey"		"HKEY_LOCAL_MACHINE\\Software\\Valve\\Steam\\Apps\\CommonRedist\\vcredist\\2022"
			"process 1"		"%INSTALLDIR%\\_CommonRedist\\vcredist\\2022\\Microsoft Visual C++ 2022 x64.cmd"
			"Requirement_OS"
			{
				"Is64BitWindows"		"1"
			}
		}
		"xp"
		{
			"hasrunkey"		"HKEY_LOCAL_MACHINE\\Software\\Valve\\Steam\\Apps\\CommonRedist\\vcredist\\2022"
			"process 1"		"%INSTALLDIR%\\_CommonRedist\\vcredist\\2022\\noop.cmd"
			"Requirement_OS" { "OSType" "Windows XP" }
		}
	}
}
"kvsignatures"
{
	"installscript"		"37b9"
}
"#;

    const PHYSX: &str = r#""installscript"
{
	"Run Process"
	{
		"9.12.1031"
		{
			"hasrunkey"		"HKEY_LOCAL_MACHINE\\Software\\Valve\\Steam\\Apps\\CommonRedist\\PhysX"
			"process 1"		"msiexec"
			"command 1"		"/i \"%INSTALLDIR%\\_CommonRedist\\PhysX\\9.12.1031\\PhysX-9.12.1031-SystemSoftware.msi\" /quiet /norestart"
		}
	}
}"#;

    fn shared_with(tag: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pishop-redist-{}-{tag}", std::process::id()));
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        }
        dir
    }

    #[test]
    fn installscripts_become_steps() {
        let shared = shared_with("t1", &[
            "_CommonRedist/vcredist/2022/Microsoft Visual C++ 2022 x86.cmd",
            "_CommonRedist/vcredist/2022/Microsoft Visual C++ 2022 x64.cmd",
        ]);
        let s = steps(VC2022, &shared);
        assert_eq!(s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["x86 14.51.36247.0", "x64 14.51.36247.0"]);
        assert_eq!(s[0].mark, r"Software\Valve\Steam\Apps\CommonRedist\vcredist\2022");
        assert_eq!(
            s[1].programs,
            [Program::Cmd { path: shared.join("_CommonRedist/vcredist/2022/Microsoft Visual C++ 2022 x64.cmd") }]
        );

        let p = steps(PHYSX, &shared);
        let Program::Msiexec { args } = &p[0].programs[0] else { panic!("{:?}", p[0].programs) };
        assert_eq!(args[0], "/i");
        assert!(args[1].starts_with("Z:\\") && args[1].ends_with(r"\_CommonRedist\PhysX\9.12.1031\PhysX-9.12.1031-SystemSoftware.msi"));
        assert_eq!(&args[2..], ["/quiet", "/norestart"]);
        std::fs::remove_dir_all(&shared).unwrap();
    }

    #[test]
    fn missing_programs_are_left_out() {
        let shared = shared_with("t2", &["_CommonRedist/vcredist/2022/Microsoft Visual C++ 2022 x86.cmd"]);
        assert_eq!(steps(VC2022, &shared).len(), 1);
        std::fs::remove_dir_all(&shared).unwrap();
    }

    #[test]
    fn titles_read_well() {
        assert_eq!(title("vcredist/2022"), "Visual C++ 2022");
        assert_eq!(title("DirectX/Jun2010"), "DirectX (June 2010)");
        assert_eq!(title("DotNet/4.0 Client Profile"), ".NET Framework 4.0 Client Profile");
        assert_eq!(title("PhysX/9.12.1031"), "PhysX 9.12.1031");
    }

    #[test]
    fn marks_written_after_the_prefix_count() {
        let shared = shared_with("t3", &[
            "_CommonRedist/vcredist/2022/Microsoft Visual C++ 2022 x86.cmd",
            "_CommonRedist/vcredist/2022/Microsoft Visual C++ 2022 x64.cmd",
        ]);
        let r = Redist { id: "vcredist/2022".into(), title: String::new(), steps: steps(VC2022, &shared) };
        let pfx = shared.join("pfx");
        std::fs::create_dir_all(&pfx).unwrap();
        let reg = |ts: i64, values: &str| {
            std::fs::write(
                pfx.join("system.reg"),
                format!("WINE REGISTRY Version 2\n\n[Software\\\\Wow6432Node\\\\Valve\\\\Steam\\\\Apps\\\\CommonRedist\\\\vcredist\\\\2022] {ts}\n{values}\n"),
            )
            .unwrap()
        };
        let now = created(&pfx);
        reg(now + 5, "\"x86 14.51.36247.0\"=dword:00000001");
        assert!(!installed(&r, &pfx), "x64 missing");
        reg(now + 5, "\"x86 14.51.36247.0\"=dword:00000001\n\"x64 14.51.36247.0\"=dword:00000001");
        assert!(installed(&r, &pfx));
        // Marked before the prefix existed: Proton's template, not an install.
        reg(1_000, "\"x86 14.51.36247.0\"=dword:00000001\n\"x64 14.51.36247.0\"=dword:00000001");
        assert_eq!(installed(&r, &pfx), now == 0);
        std::fs::remove_dir_all(&shared).unwrap();
    }
}
