//! Components for a game's prefix — the runtimes Windows games expect
//! (Visual C++, DirectX, .NET, XNA…) — from two places:
//! - Steam's redistributables (`redist`), run exactly as Steam's
//!   installscript says: offline, per prefix;
//! - winetricks, bundled with piShop (`bin/winetricks`), for the rest. It is
//!   fed from Steam's files whenever Steam has what it would download:
//!   byte-identical installers go into its download cache, and a few verbs
//!   get a local variant (a `.verb` file) that takes Steam's files through
//!   winetricks' own steps (same DLL overrides, same registrations).
//!
//! Everything runs inside the Steam Linux Runtime container the game's Proton
//! uses, with that Proton's wine, from one user unit per prefix so it
//! outlives piShop. `cabextract`/`unzip`, which the runtime lacks, are
//! piShop itself (`archivetools`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use anyhow::{anyhow, bail};
use axum::extract::Path as UrlPath;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::redist::{self, Program, Redist};
use crate::{archivetools, install, library, log, proton, steamclient, tr};

/// Winetricks verbs piShop offers (the UI names them).
pub const VERBS: &[&str] = &[
    "vcrun2022", "d3dx9", "xact", "xinput", "d3dx11_43", "d3dcompiler_47", "vcrun2013", "vcrun2012", "vcrun2010", "vcrun2008",
    "vcrun2005", "dotnet48", "dotnetdesktop8", "xna40", "physx", "openal", "corefonts",
];

pub fn router() -> Router {
    Router::new()
        .route(
            "/api/install/{hash}/components",
            get(|UrlPath(h): UrlPath<String>| async move { install::reply(status(&h).await) })
                .post(|UrlPath(h): UrlPath<String>, Json(r): Json<StartReq>| async move { install::reply(start(&h, r).await) }),
        )
        .route("/api/install/{hash}/components/cancel", post(|UrlPath(h): UrlPath<String>| async move { install::reply(cancel(&h)) }))
}

fn unit(appid: u32) -> String {
    format!("pishop-components-{appid}")
}

fn log_file(appid: u32) -> PathBuf {
    crate::data_dir().join("logs").join(format!("components-{appid}.log"))
}

fn script() -> PathBuf {
    crate::base_dir().join("bin/winetricks")
}

fn cache_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".cache/winetricks")
}

/// Whether components are being installed in this shortcut's prefix.
pub fn active(appid: u32) -> bool {
    Command::new("systemctl").args(["--user", "is-active", "--quiet", &unit(appid)]).status().is_ok_and(|s| s.success())
}

/// Verbs winetricks has installed in the prefix (it keeps a log there).
fn installed_verbs(pfx: &Path) -> HashSet<String> {
    std::fs::read_to_string(pfx.join("winetricks.log"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(String::from)
        .collect()
}

// ---------- Steam's files for winetricks ----------

#[derive(Serialize, Deserialize, Clone)]
struct Indexed {
    path: PathBuf,
    size: u64,
    mtime: u64,
    sha: String,
}

/// SHA-256 of every installer in Steam's redistributables, kept between runs
/// (hashing them all takes a few seconds the first time).
fn local_index() -> Vec<Indexed> {
    let file = crate::data_dir().join("redist-index.json");
    let old: Vec<Indexed> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = redist::shared_dirs().into_iter().map(|d| d.join("_CommonRedist")).collect();
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ext != "exe" && ext != "msi" {
                continue;
            }
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            if let Some(o) = old.iter().find(|o| o.path == path && o.size == meta.len() && o.mtime == mtime) {
                out.push(o.clone());
                continue;
            }
            let Ok(mut f) = std::fs::File::open(&path) else { continue };
            let mut hasher = Sha256::new();
            let mut buf = vec![0u8; 1 << 20];
            let read = loop {
                match std::io::Read::read(&mut f, &mut buf) {
                    Ok(0) => break true,
                    Ok(n) => hasher.update(&buf[..n]),
                    Err(_) => break false,
                }
            };
            if !read {
                continue;
            }
            let sha = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            out.push(Indexed { path, size: meta.len(), mtime, sha });
        }
    }
    if let Ok(bytes) = serde_json::to_vec(&out) {
        let _ = std::fs::write(&file, bytes);
    }
    out
}

/// A file winetricks downloads: into `W_CACHE/<dir>/<file>`, checked by sha256.
#[derive(Debug, PartialEq)]
struct Download {
    /// The verb whose load_ function downloads it.
    verb: Option<String>,
    dir: String,
    file: String,
    sha: String,
}

/// The fixed downloads in the winetricks script (lines with variables are skipped).
fn downloads(script: &str) -> Vec<Download> {
    let mut out = Vec::new();
    let mut verb: Option<String> = None;
    for line in script.lines() {
        let is_fn = line.ends_with("()") && line[..line.len() - 2].chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_fn && !line.is_empty() {
            verb = line[..line.len() - 2].strip_prefix("load_").map(String::from);
            continue;
        }
        let t = line.trim();
        let (rest, to) = if let Some(r) = t.strip_prefix("w_download_to ") {
            (r, true)
        } else if let Some(r) = t.strip_prefix("w_download ") {
            (r, false)
        } else {
            continue;
        };
        if rest.contains('$') {
            continue;
        }
        let args: Vec<&str> = rest.split_whitespace().collect();
        let (dir, url, sha, file) = if to {
            (args.first().map(|d| d.to_string()), args.get(1), args.get(2), args.get(3))
        } else {
            (verb.clone(), args.first(), args.get(1), args.get(2))
        };
        let (Some(dir), Some(url), Some(sha)) = (dir, url, sha) else { continue };
        if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let file = file.map(|f| f.to_string()).unwrap_or_else(|| url.split('?').next().unwrap_or(url).rsplit('/').next().unwrap_or("").to_string());
        if !file.is_empty() {
            out.push(Download { verb: verb.clone(), dir, file, sha: sha.to_lowercase() });
        }
    }
    out
}

/// Puts Steam's copies of what winetricks would download into its cache
/// (as links) and returns the verbs whose downloads are all covered.
fn seed(index: &[Indexed]) -> HashSet<String> {
    let text = std::fs::read(script()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let by_sha: HashMap<&str, &Path> = index.iter().map(|i| (i.sha.as_str(), i.path.as_path())).collect();
    let mut per_verb: HashMap<String, (usize, usize)> = HashMap::new();
    for d in downloads(&text) {
        let local = by_sha.get(d.sha.as_str());
        if let Some(v) = &d.verb {
            let e = per_verb.entry(v.clone()).or_default();
            e.0 += 1;
            e.1 += usize::from(local.is_some());
        }
        let Some(local) = local else { continue };
        let target = cache_dir().join(&d.dir).join(&d.file);
        if target.exists() {
            continue;
        }
        let _ = std::fs::remove_file(&target); // a dangling link
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::os::unix::fs::symlink(local, &target);
    }
    per_verb.into_iter().filter(|(_, (all, have))| all == have).map(|(v, _)| v).collect()
}

/// Steam's own files that some verbs can be made from: Visual C++ by year
/// (folder, x86 installer, x64 installer) and the June 2010 DirectX cabinets.
struct SteamFiles {
    vc: HashMap<&'static str, (PathBuf, String, String)>,
    dx: Option<PathBuf>,
}

/// A file in `dir` by name, ignoring case; its name as it is on disk.
fn named(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| n.eq_ignore_ascii_case(name))
}

fn steam_files() -> SteamFiles {
    let mut f = SteamFiles { vc: HashMap::new(), dx: None };
    for shared in redist::shared_dirs() {
        let root = shared.join("_CommonRedist");
        let pair = |dir: PathBuf, x86: &str, x64: &str| Some((dir.clone(), named(&dir, x86)?, named(&dir, x64)?));
        for (year, x86, x64) in [
            ("2022", "VC_redist.x86.exe", "VC_redist.x64.exe"),
            ("2013", "vcredist_x86.exe", "vcredist_x64.exe"),
            ("2010", "vcredist_x86.exe", "vcredist_x64.exe"),
            ("2008", "vcredist_x86.exe", "vcredist_x64.exe"),
        ] {
            if !f.vc.contains_key(year) {
                if let Some(p) = pair(root.join("vcredist").join(year), x86, x64) {
                    f.vc.insert(year, p);
                }
            }
        }
        let dx = root.join("DirectX/Jun2010");
        if f.dx.is_none() && dx.is_dir() {
            f.dx = Some(dx);
        }
    }
    f
}

fn has_local_variant(verb: &str, f: &SteamFiles) -> bool {
    match verb {
        "d3dx9" | "d3dx11_43" | "xinput" | "xact" => f.dx.is_some(),
        v => v.strip_prefix("vcrun").is_some_and(|year| ["2022", "2013", "2010", "2008"].contains(&year) && f.vc.contains_key(year)),
    }
}

/// Visual C++ 2008–2013 as winetricks installs them: its DLL overrides (and
/// for 2013, Wine's own copies out of the way first), then the installers.
fn vc_variant(verb: &str, overrides: &str, remove: &str, (dir, x86, x64): &(PathBuf, String, String)) -> String {
    let rm = |sys: &str| {
        if remove.is_empty() {
            String::new()
        } else {
            let files: Vec<String> = remove.split(' ').map(|d| format!("\"${{{sys}}}\"/{d}.dll")).collect();
            format!("    rm -f {}\n", files.join(" "))
        }
    };
    format!(
        r#"load_{verb}()
{{
    w_override_dlls native,builtin {overrides}
{rm32}    w_try_cd {dir}
    w_try_ms_installer "${{WINE}}" {x86} ${{W_OPT_UNATTENDED:+/q}}
    case "${{W_ARCH}}" in
        win64)
{rm64}            w_try_ms_installer "${{WINE}}" {x64} ${{W_OPT_UNATTENDED:+/q}}
            ;;
    esac
}}
"#,
        rm32 = rm("W_SYSTEM32_DLLS"),
        rm64 = rm("W_SYSTEM64_DLLS").replace("    rm", "            rm"),
        dir = sh_path(dir),
        x86 = sh(x86),
        x64 = sh(x64),
    )
}

/// A string as a single-quoted shell word.
fn sh(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn sh_path(p: &Path) -> String {
    sh(&p.display().to_string())
}

/// Steam's June 2010 DirectX cabinets — the very ones inside
/// directx_Jun2010_redist.exe — lowercased like `cabextract -L` names them.
fn dx_cabs_fn(dir: &Path) -> String {
    format!(
        r#"_pishop_dx_cabs()
{{
    for _c in {}/*; do
        _n="$(basename "${{_c}}" | tr '[:upper:]' '[:lower:]')"
        case "${{_n}}" in
            $1) w_try cp -f "${{_c}}" "${{W_TMP}}/${{_n}}" ;;
        esac
    done
}}
"#,
        sh_path(dir)
    )
}

/// A `.verb` file giving `verb` Steam's files with winetricks' own steps
/// (mirrors load_<verb> in winetricks 20260125).
fn local_variant(verb: &str, f: &SteamFiles, run_dir: &Path) -> Option<String> {
    let head = format!("# piShop: {verb} from Steam's redistributables, with winetricks' own steps.\n");
    let body = match verb {
        "vcrun2022" => {
            let (dir, x86, x64) = f.vc.get("2022")?;
            // The installer won't replace Wine's msvcp140 (its version number is
            // higher), so winetricks puts the real one in place first.
            let mut dll = String::new();
            for (exe, machine, name, dest) in [(x86, archivetools::X86, "msvcp140-x86.dll", "W_SYSTEM32_DLLS"), (x64, archivetools::X64, "msvcp140-x64.dll", "W_SYSTEM64_DLLS")] {
                let out = run_dir.join(name);
                if let Ok(Some(bytes)) = archivetools::find_dll(&dir.join(exe), "msvcp140.dll", machine) {
                    if std::fs::write(&out, bytes).is_ok() {
                        dll.push_str(&format!("_pishop_{dest}={}\n", sh_path(&out)));
                    }
                }
            }
            format!(
                r#"{dll}load_vcrun2022()
{{
    w_override_dlls native,builtin concrt140 msvcp140 msvcp140_1 msvcp140_2 msvcp140_atomic_wait msvcp140_codecvt_ids vcamp140 vccorlib140 vcomp140 vcruntime140
    [ -n "${{_pishop_W_SYSTEM32_DLLS:-}}" ] && w_try_cp_dll "${{_pishop_W_SYSTEM32_DLLS}}" "${{W_SYSTEM32_DLLS}}/msvcp140.dll"
    w_try_cd {dir}
    w_try_ms_installer "${{WINE}}" {x86} ${{W_OPT_UNATTENDED:+/q}}
    case "${{W_ARCH}}" in
        win64)
            w_override_dlls native,builtin vcruntime140_1
            [ -n "${{_pishop_W_SYSTEM64_DLLS:-}}" ] && w_try_cp_dll "${{_pishop_W_SYSTEM64_DLLS}}" "${{W_SYSTEM64_DLLS}}/msvcp140.dll"
            w_try_ms_installer "${{WINE}}" {x64} ${{W_OPT_UNATTENDED:+/q}}
            ;;
    esac
}}
"#,
                dir = sh_path(dir),
                x86 = sh(x86),
                x64 = sh(x64),
            )
        }
        "vcrun2013" => vc_variant(verb, "atl120 msvcp120 msvcr120 vcomp120", "msvcp120 msvcr120 vcomp120", f.vc.get("2013")?),
        "vcrun2010" => vc_variant(verb, "msvcp100 msvcr100 vcomp100 atl100", "", f.vc.get("2010")?),
        "vcrun2008" => vc_variant(verb, "atl90 msvcm90 msvcp90 msvcr90 vcomp90", "", f.vc.get("2008")?),
        "d3dx9" => format!(
            r#"{cabs}load_d3dx9()
{{
    _pishop_dx_cabs '*d3dx9*x86*'
    for x in "${{W_TMP}}"/*.cab; do
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'd3dx9*.dll' "${{x}}"
    done
    if test "${{W_ARCH}}" = "win64"; then
        _pishop_dx_cabs '*d3dx9*x64*'
        for x in "${{W_TMP}}"/*x64.cab; do
            w_try_cabextract -d "${{W_SYSTEM64_DLLS}}" -L -F 'd3dx9*.dll' "${{x}}"
        done
    fi
    w_override_dlls native d3dx9_24 d3dx9_25 d3dx9_26 d3dx9_27 d3dx9_28 d3dx9_29 d3dx9_30
    w_override_dlls native d3dx9_31 d3dx9_32 d3dx9_33 d3dx9_34 d3dx9_35 d3dx9_36 d3dx9_37
    w_override_dlls native d3dx9_38 d3dx9_39 d3dx9_40 d3dx9_41 d3dx9_42 d3dx9_43
}}
"#,
            cabs = dx_cabs_fn(f.dx.as_ref()?)
        ),
        "d3dx11_43" => format!(
            r#"{cabs}load_d3dx11_43()
{{
    _pishop_dx_cabs '*d3dx11_43*x86*'
    for x in "${{W_TMP}}"/*.cab; do
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'd3dx11_43.dll' "${{x}}"
    done
    if test "${{W_ARCH}}" = "win64"; then
        _pishop_dx_cabs '*d3dx11_43*x64*'
        for x in "${{W_TMP}}"/*x64.cab; do
            w_try_cabextract -d "${{W_SYSTEM64_DLLS}}" -L -F 'd3dx11_43.dll' "${{x}}"
        done
    fi
    w_override_dlls native d3dx11_43
}}
"#,
            cabs = dx_cabs_fn(f.dx.as_ref()?)
        ),
        "xinput" => format!(
            r#"{cabs}load_xinput()
{{
    _pishop_dx_cabs '*_xinput_*x86*'
    for x in "${{W_TMP}}"/*.cab; do
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'xinput*.dll' "${{x}}"
    done
    if test "${{W_ARCH}}" = "win64"; then
        _pishop_dx_cabs '*_xinput_*x64*'
        for x in "${{W_TMP}}"/*x64.cab; do
            w_try_cabextract -d "${{W_SYSTEM64_DLLS}}" -L -F 'xinput*.dll' "${{x}}"
        done
    fi
    w_override_dlls native xinput1_1
    w_override_dlls native xinput1_2
    w_override_dlls native xinput1_3
    w_override_dlls native xinput9_1_0
}}
"#,
            cabs = dx_cabs_fn(f.dx.as_ref()?)
        ),
        "xact" => format!(
            r#"{cabs}load_xact()
{{
    _pishop_dx_cabs '*_xact_*x86*'
    _pishop_dx_cabs '*_x3daudio_*x86*'
    _pishop_dx_cabs '*_xaudio_*x86*'
    for x in "${{W_TMP}}"/*.cab ; do
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'xactengine*.dll' "${{x}}"
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'xaudio*.dll' "${{x}}"
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'x3daudio*.dll' "${{x}}"
        w_try_cabextract -d "${{W_SYSTEM32_DLLS}}" -L -F 'xapofx*.dll' "${{x}}"
    done
    w_override_dlls native,builtin xaudio2_0 xaudio2_1 xaudio2_2 xaudio2_3 xaudio2_4 xaudio2_5 xaudio2_6 xaudio2_7
    w_override_dlls native,builtin x3daudio1_0 x3daudio1_1 x3daudio1_2 x3daudio1_3 x3daudio1_4 x3daudio1_5 x3daudio1_6 x3daudio1_7
    w_override_dlls native,builtin xapofx1_1 xapofx1_2 xapofx1_3 xapofx1_4 xapofx1_5
    w_override_dlls native,builtin xactengine2_0 xactengine2_10 xactengine2_1 xactengine2_2 xactengine2_3 xactengine2_4 xactengine2_5 xactengine2_6 xactengine2_7 xactengine2_8 xactengine2_9 xactengine3_0 xactengine3_1 xactengine3_2 xactengine3_3 xactengine3_4 xactengine3_5 xactengine3_6 xactengine3_7
    for x in "${{W_SYSTEM32_DLLS}}"/xactengine*.dll ; do
        w_try_regsvr32 "$(basename "${{x}}")"
    done
    for x in 0 1 2 3 4 5 6 7 ; do
        w_try_regsvr32 "$(basename "${{W_SYSTEM32_DLLS}}/xaudio2_${{x}}")"
    done
}}
"#,
            cabs = dx_cabs_fn(f.dx.as_ref()?)
        ),
        _ => return None,
    };
    Some(head + &body)
}

// ---------- the run ----------

/// Steam's steps as shell lines: each program through Proton's wine, and
/// Steam's mark once it worked (so Steam itself won't run it again).
fn steam_lines(r: &Redist) -> String {
    let mut out = String::new();
    for step in &r.steps {
        let mut cmds = Vec::new();
        for p in &step.programs {
            let cmd = match p {
                Program::Exe { path, args } => {
                    format!("\"$WINE\" {} {}", sh_path(path), args.iter().map(|a| sh(a)).collect::<Vec<_>>().join(" "))
                }
                Program::Cmd { path } => format!("\"$WINE\" cmd /c {}", sh(&format!("Z:{}", path.display().to_string().replace('/', "\\")))),
                Program::Msiexec { args } => format!("\"$WINE\" msiexec {}", args.iter().map(|a| sh(a)).collect::<Vec<_>>().join(" ")),
            };
            cmds.push(format!("step {} {}", sh(&format!("{} · {}", r.title, step.name)), cmd.trim_end()));
        }
        out.push_str(&format!("{} && mark {} {}\n\"$WINESERVER\" -w\n", cmds.join(" && "), sh(&step.mark), sh(&step.name)));
    }
    out
}

/// The whole run as a script for the runtime's `sh`.
fn run_script(steam: &[Redist], winetricks: &Path, verbs: &[String]) -> String {
    let mut s = String::from(
        r#"# piShop: components for one prefix (generated for this run).
export PATH="$PISHOP_PATH:$PATH"
[ -n "$PISHOP_LD" ] && export LD_LIBRARY_PATH="$PISHOP_LD${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
status=0
step() {
    _title="$1"; shift
    echo "piShop: $_title"
    "$@"
    _code=$?
    # 1638: a newer version is there already; 3010: done, wants a reboot.
    case "$_code" in 0|1638|3010) return 0 ;; esac
    echo "piShop: $_title failed (code $_code)"
    status=1
    return 1
}
mark() {
    "$WINE" reg add "HKLM\\$1" /v "$2" /t REG_DWORD /d 1 /f /reg:32 >/dev/null 2>&1
}
"#,
    );
    for r in steam {
        s.push_str(&steam_lines(r));
    }
    if !verbs.is_empty() {
        s.push_str(&format!(
            "step {} sh {} --unattended {}\n",
            sh(&format!("winetricks {}", verbs.iter().map(|v| v.rsplit('/').next().unwrap_or(v).trim_end_matches(".verb")).collect::<Vec<_>>().join(" "))),
            sh_path(winetricks),
            verbs.iter().map(|v| sh(v)).collect::<Vec<_>>().join(" ")
        ));
    }
    s.push_str("\"$WINESERVER\" -w\necho \"pishop-exit:$status\"\nexit $status\n");
    s
}

/// The last run: what it was about, its last lines and (once over) exit code.
fn last_run(appid: u32) -> (String, Vec<String>, Option<i32>) {
    let text = std::fs::read(log_file(appid)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let current = text.lines().next().and_then(|l| l.strip_prefix("piShop: run ")).unwrap_or("").to_string();
    // curl redraws its progress with \r: keep the last state of each line.
    let mut lines: Vec<&str> = text.split(['\n', '\r']).map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let exit = lines.iter().rev().find_map(|l| l.strip_prefix("pishop-exit:")).and_then(|c| c.trim().parse().ok());
    lines.retain(|l| !l.starts_with("pishop-exit:") && !l.starts_with("piShop: run "));
    let from = lines.len().saturating_sub(14);
    (current, lines[from..].iter().map(|s| s.to_string()).collect(), exit)
}

fn appid_for(hash: &str) -> anyhow::Result<(library::Entry, u32)> {
    let e = library::get(hash).ok_or_else(|| anyhow!(tr!("this download has no game data", "este download não tem dados do jogo")))?;
    let appid = e.install.as_ref().and_then(|s| s.appid).ok_or_else(|| anyhow!(tr!("no shortcut yet", "ainda não há atalho")))?;
    Ok((e, appid))
}

async fn status(hash: &str) -> anyhow::Result<Value> {
    let (_, appid) = appid_for(hash)?;
    let pfx = proton::compatdata(appid).join("pfx");
    let running = active(appid);
    let (current, log, exit) = last_run(appid);
    let (steam, offline) = tokio::task::spawn_blocking(move || {
        let pfx = proton::compatdata(appid).join("pfx");
        let steam: Vec<Value> = redist::list()
            .iter()
            .map(|r| json!({ "id": r.id, "title": r.title, "installed": redist::installed(r, &pfx) }))
            .collect();
        let files = steam_files();
        let mut offline = seed(&local_index());
        offline.extend(VERBS.iter().filter(|v| has_local_variant(v, &files)).map(|v| v.to_string()));
        (steam, offline)
    })
    .await?;
    let done = installed_verbs(&pfx);
    let verbs: Vec<Value> =
        VERBS.iter().map(|v| json!({ "verb": v, "offline": offline.contains(*v), "installed": done.contains(*v) })).collect();
    Ok(json!({
        "available": script().is_file(),
        "prefix": pfx.join("system.reg").is_file(),
        "steam": steam,
        "verbs": verbs,
        "running": running,
        "current": current,
        "log": log,
        "exit": if running { None } else { exit },
    }))
}

#[derive(Deserialize)]
pub struct StartReq {
    #[serde(default)]
    steam: Vec<String>,
    #[serde(default)]
    verbs: Vec<String>,
}

async fn start(hash: &str, r: StartReq) -> anyhow::Result<Value> {
    let (e, appid) = appid_for(hash)?;
    let all = tokio::task::spawn_blocking(redist::list).await?;
    let steam: Vec<Redist> = all.into_iter().filter(|x| r.steam.contains(&x.id)).collect();
    let mut verbs: Vec<String> = Vec::new();
    for v in r.verbs {
        if VERBS.contains(&v.as_str()) && !verbs.contains(&v) {
            verbs.push(v);
        }
    }
    if steam.is_empty() && verbs.is_empty() {
        bail!(tr!("pick at least one component", "escolha pelo menos um componente"));
    }
    if active(appid) {
        bail!(tr!("components are already being installed in this game", "já há componentes sendo instalados neste jogo"));
    }
    if steamclient::running(appid) || proton::in_use(appid) || install::moving(hash) {
        bail!(tr!("close the game first", "feche o jogo primeiro"));
    }
    let compat = proton::compatdata(appid);
    let pfx = compat.join("pfx");
    if !pfx.join("system.reg").is_file() {
        bail!(tr!(
            "this game has no Proton prefix yet: play it once first",
            "este jogo ainda não tem prefixo do Proton: jogue uma vez primeiro"
        ));
    }
    let winetricks = script();
    if !verbs.is_empty() && !winetricks.is_file() {
        bail!(tr!("winetricks is missing from piShop's folder", "o winetricks não está na pasta do piShop"));
    }

    // The game's Proton as Steam has it now, and the runtime that Proton needs.
    let saved = e.install.as_ref().map(|s| s.tool.clone()).unwrap_or_default();
    let tool_name = match steamclient::shortcut(appid).await {
        Ok(Some(s)) if !s.tool.is_empty() => s.tool,
        _ => saved,
    };
    let display = steamclient::compat_tools()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|t| t.name == tool_name)
        .map(|t| t.display)
        .unwrap_or_else(|| tool_name.clone());
    let tool = proton::tool_dir(&tool_name, &display).ok_or_else(|| {
        anyhow!(tr!(
            "{} isn't downloaded yet: play the game once so Steam gets it",
            "{} ainda não foi baixado: jogue uma vez para a Steam baixá-lo",
            display
        ))
    })?;
    let runtime = proton::runtime_for(&tool).ok_or_else(|| {
        anyhow!(tr!(
            "the Steam Linux Runtime that {} needs isn't installed",
            "o Steam Linux Runtime que o {} precisa não está instalado",
            display
        ))
    })?;

    // This run's files: the script, local verb variants, extracted DLLs.
    let run_dir = crate::data_dir().join("runs").join(appid.to_string());
    let _ = std::fs::remove_dir_all(&run_dir);
    std::fs::create_dir_all(&run_dir)?;
    let tools_dir = crate::data_dir().join("tools");
    archivetools::install_links(&tools_dir)?;
    let (args, titles) = {
        let verbs = verbs.clone();
        let run_dir = run_dir.clone();
        tokio::task::spawn_blocking(move || {
            seed(&local_index());
            let files = steam_files();
            let mut args = Vec::new();
            for v in &verbs {
                match local_variant(v, &files, &run_dir) {
                    Some(text) => {
                        let file = run_dir.join(format!("{v}.verb"));
                        match std::fs::write(&file, text) {
                            Ok(()) => args.push(file.display().to_string()),
                            Err(_) => args.push(v.clone()),
                        }
                    }
                    None => args.push(v.clone()),
                }
            }
            (args, verbs)
        })
        .await?
    };
    let script_file = run_dir.join("run.sh");
    std::fs::write(&script_file, run_script(&steam, &winetricks, &args))?;
    let mut what: Vec<String> = steam.iter().map(|s| s.title.clone()).collect();
    what.extend(titles);
    let log = log_file(appid);
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&log, format!("piShop: run {}\n", what.join(", ")))?;

    let files = tool.join("files");
    let lib = files.join("lib");
    let ld: Vec<String> = ["x86_64-linux-gnu", "i386-linux-gnu"]
        .iter()
        .map(|d| lib.join(d))
        .filter(|d| d.is_dir())
        .map(|d| d.display().to_string())
        .collect();
    let mut env: Vec<(String, String)> = vec![
        ("WINEPREFIX".into(), pfx.display().to_string()),
        ("WINE".into(), files.join("bin/wine").display().to_string()),
        ("WINELOADER".into(), files.join("bin/wine").display().to_string()),
        ("WINESERVER".into(), files.join("bin/wineserver").display().to_string()),
        ("WINEDLLPATH".into(), format!("{}:{}", lib.join("vkd3d").display(), lib.join("wine").display())),
        ("WINEDEBUG".into(), "-all".into()),
        ("WINEDLLOVERRIDES".into(), "winemenubuilder.exe=d".into()),
        ("WINETRICKS_LATEST_VERSION_CHECK".into(), "disabled".into()),
        ("W_CACHE".into(), cache_dir().display().to_string()),
        ("STEAM_COMPAT_DATA_PATH".into(), compat.display().to_string()),
        ("STEAM_COMPAT_CLIENT_INSTALL_PATH".into(), proton::steam_root().display().to_string()),
        ("STEAM_COMPAT_TOOL_PATHS".into(), format!("{}:{}", tool.display(), runtime.display())),
        ("PISHOP_PATH".into(), format!("{}:{}", tools_dir.display(), files.join("bin").display())),
        ("PISHOP_LD".into(), ld.join(":")),
    ];
    for k in ["DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "PULSE_SERVER", "LANG"] {
        if let Ok(v) = std::env::var(k) {
            env.push((k.into(), v));
        }
    }
    let _ = Command::new("systemctl").args(["--user", "reset-failed", &unit(appid)]).stderr(std::process::Stdio::null()).status();
    let mut cmd = Command::new("systemd-run");
    cmd.args(["--user", "--quiet", "--collect", &format!("--unit={}", unit(appid))])
        .arg(format!("--property=StandardOutput=append:{}", log.display()))
        .arg(format!("--property=StandardError=append:{}", log.display()));
    for (k, v) in &env {
        cmd.arg(format!("--setenv={k}={v}"));
    }
    cmd.arg("--").arg(runtime.join("_v2-entry-point")).args(["--verb=waitforexitandrun", "--", "/bin/sh"]).arg(&script_file);
    let status = cmd.status().map_err(|e| anyhow!("systemd-run: {e}"))?;
    if !status.success() {
        bail!("systemd-run: {status}");
    }
    log!("componentes: {} no atalho {appid} ({display})", what.join(", "));
    Ok(json!({ "running": true }))
}

fn cancel(hash: &str) -> anyhow::Result<Value> {
    let (_, appid) = appid_for(hash)?;
    // Stopping the unit takes down everything in it: wine, wineserver, downloads.
    let _ = Command::new("systemctl").args(["--user", "stop", &unit(appid)]).status();
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winetricks_downloads_are_read() {
        let text = r#"
load_vcrun2013()
{
    w_download https://download.microsoft.com/x/vcredist_x86.exe 89f4e593ea5541d1c53f983923124f9fd061a1c0c967339109e375c661573c17
    w_download https://example.com/a?b=c 20e2645b7cd5873b1fa3462b99a665ac8d6e14aae83ded9d875fea35ffdd7d7e vcredist_x64.exe
    w_download "${url}" 20e2645b7cd5873b1fa3462b99a665ac8d6e14aae83ded9d875fea35ffdd7d7e
}

helper_directx_Jun2010()
{
    w_download_to directx9 https://host/directx_Jun2010_redist.exe 8746ee1a84a083a90e37899d71d50d5c7c015e69688a466aa80447f011780c0d
}
"#;
        let d = downloads(text);
        assert_eq!(d.len(), 3);
        assert_eq!((d[0].verb.as_deref(), d[0].dir.as_str(), d[0].file.as_str()), (Some("vcrun2013"), "vcrun2013", "vcredist_x86.exe"));
        assert_eq!(d[1].file, "vcredist_x64.exe");
        assert_eq!((d[2].verb.as_deref(), d[2].dir.as_str(), d[2].file.as_str()), (None, "directx9", "directx_Jun2010_redist.exe"));
    }

    #[test]
    fn vc_variants_follow_winetricks() {
        let v = vc_variant(
            "vcrun2013",
            "atl120 msvcp120 msvcr120 vcomp120",
            "msvcp120 msvcr120 vcomp120",
            &(PathBuf::from("/S W/2013"), "vcredist_x86.exe".into(), "vcredist_x64.exe".into()),
        );
        assert!(v.starts_with("load_vcrun2013()\n{\n    w_override_dlls native,builtin atl120 msvcp120 msvcr120 vcomp120\n"));
        assert!(v.contains("    rm -f \"${W_SYSTEM32_DLLS}\"/msvcp120.dll \"${W_SYSTEM32_DLLS}\"/msvcr120.dll \"${W_SYSTEM32_DLLS}\"/vcomp120.dll\n"));
        assert!(v.contains("            rm -f \"${W_SYSTEM64_DLLS}\"/msvcp120.dll"));
        assert!(v.contains("    w_try_cd '/S W/2013'\n    w_try_ms_installer \"${WINE}\" 'vcredist_x86.exe' ${W_OPT_UNATTENDED:+/q}\n"));
        let plain = vc_variant("vcrun2008", "atl90", "", &(PathBuf::from("/d"), "a.exe".into(), "b.exe".into()));
        assert!(!plain.contains("rm -f"));
    }

    #[test]
    fn shell_words_are_quoted() {
        assert_eq!(sh("a b"), "'a b'");
        assert_eq!(sh("it's"), r"'it'\''s'");
    }

    #[test]
    fn steam_steps_become_shell_lines() {
        let r = Redist {
            id: "vcredist/2010".into(),
            title: "Visual C++ 2010".into(),
            steps: vec![redist::Step {
                name: "x86".into(),
                programs: vec![Program::Exe { path: PathBuf::from("/S W/vcredist_x86.exe"), args: vec!["/quiet".into(), "/norestart".into()] }],
                mark: r"Software\Valve\Steam\Apps\CommonRedist\vcredist\2010".into(),
            }],
        };
        assert_eq!(
            steam_lines(&r),
            "step 'Visual C++ 2010 · x86' \"$WINE\" '/S W/vcredist_x86.exe' '/quiet' '/norestart' && mark 'Software\\Valve\\Steam\\Apps\\CommonRedist\\vcredist\\2010' 'x86'\n\"$WINESERVER\" -w\n"
        );
        let s = run_script(&[r], Path::new("/w/winetricks"), &["/r/d3dx9.verb".into(), "corefonts".into()]);
        assert!(s.contains("step 'winetricks d3dx9 corefonts' sh '/w/winetricks' --unattended '/r/d3dx9.verb' 'corefonts'\n"));
        assert!(s.trim_end().ends_with("exit $status"));
    }
}
