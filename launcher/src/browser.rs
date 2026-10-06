use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::{Child, Command};

use crate::log;

fn chromium_path() -> PathBuf {
    std::env::var_os("PISHOP_BROWSER")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::base_dir().join("chromium").join("chrome"))
}

pub fn launch(url: &str, data: &Path) -> std::io::Result<Child> {
    let exe = chromium_path();
    let profile = data.join("browser");
    let mut cmd = Command::new(&exe);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--app={url}"))
        .args([
            "--kiosk",
            "--start-fullscreen",
            // gamescope (Game Mode) and Plasma both offer an X server.
            "--ozone-platform=x11",
            "--no-first-run",
            "--no-default-browser-check",
            "--noerrdialogs",
            "--disable-infobars",
            "--disable-session-crashed-bubble",
            "--hide-crash-restore-bubble",
            "--test-type",
            "--password-store=basic",
            "--disable-sync",
            "--disable-component-update",
            "--disable-default-apps",
            "--disable-pinch",
            "--overscroll-history-navigation=0",
            "--autoplay-policy=no-user-gesture-required",
            "--ignore-gpu-blocklist",
            "--enable-gpu-rasterization",
            "--enable-zero-copy",
            "--disable-features=Translate,TranslateUI,MediaRouter,HardwareMediaKeyHandling,GlobalMediaControls,\
DialMediaRouteProvider,DownloadBubble,DownloadBubbleV2,PrivacySandboxSettings4,PrivacySandboxAdsAPIs,\
AutofillServerCommunication,OptimizationHints,InterestFeedContentSuggestions,SidePanelPinning",
            // No browser chrome of any kind: permission prompts, notifications,
            // in-product-help bubbles, search engine choice, phishing pings.
            "--deny-permission-prompts",
            "--disable-notifications",
            "--propagate-iph-for-testing",
            "--disable-search-engine-choice-screen",
            "--disable-client-side-phishing-detection",
            "--disable-domain-reliability",
            "--no-pings",
            "--enable-features=VaapiVideoDecoder,VaapiVideoDecodeLinuxGL,VaapiIgnoreDriverChecks",
            "--check-for-update-interval=31536000",
            // crashpad daemonizes out of our process group and would keep
            // Steam's "exiting game" screen waiting.
            "--disable-breakpad",
            "--disable-crash-reporter",
        ])
        // Steam injects its overlay via LD_PRELOAD; inside Chromium it
        // deadlocks the zygote before any window shows up. gamescope still
        // maps the window to the game through our SteamGameId env.
        .arg(format!("--lang={}", crate::settings::lang().bcp47()))
        .env("LANGUAGE", if crate::settings::is_pt() { "pt_BR:pt:en" } else { "en_US:en" })
        // Steam exports its own GTK/Qt input-method module to games; inside
        // Chromium it pops the on-screen keyboard on every text-field event,
        // even focus leaving the field. The UI opens the keyboard explicitly
        // instead (POST /api/keyboard).
        .env("GTK_IM_MODULE", "simple")
        .env_remove("QT_IM_MODULE")
        .env_remove("LD_PRELOAD")
        // Non-Steam games inherit the Steam Runtime library path; Chromium
        // must use the system libraries it was checked against.
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("STEAM_RUNTIME_LIBRARY_PATH")
        // Own process group, so shutdown can take down zygote/renderers too.
        .process_group(0)
        .kill_on_drop(true);
    if let Ok(f) = std::fs::File::create(data.join("browser.log")) {
        if let Ok(f2) = f.try_clone() {
            cmd.stdout(f2);
        }
        cmd.stderr(f);
    }
    // If the launcher dies unexpectedly, take the browser down with it.
    #[cfg(target_os = "linux")]
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    kill_leftovers();
    purge_stale_ui(data, &profile);
    if let Err(e) = seed_preferences(&profile) {
        log!("não foi possível ajustar as preferências do navegador: {e}");
    }
    log!("abrindo {}", exe.display());
    cmd.spawn()
}

/// Kills every process started from the bundled Chromium folder (except via
/// the current group cleanup). Leftovers from a crashed session would grab our
/// profile lock and keep Steam thinking the game is still running.
fn kill_leftovers() {
    let Some(dir) = chromium_path().parent().map(|d| d.as_os_str().as_encoded_bytes().to_vec()) else { return };
    let me = std::process::id() as i32;
    let Ok(entries) = std::fs::read_dir("/proc") else { return };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let argv0 = cmdline.split(|&b| b == 0).next().unwrap_or_default();
        if argv0.starts_with(&dir) && argv0.get(dir.len()) == Some(&b'/') {
            log!("matando processo do navegador remanescente (pid {pid})");
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

/// The PWA's service worker would keep serving the previous UI after an
/// update; drop its caches whenever the embedded bundle changed.
fn purge_stale_ui(data: &Path, profile: &Path) {
    let marker = data.join("ui-version");
    let current = crate::server::ui_fingerprint();
    if std::fs::read_to_string(&marker).ok().as_deref() == Some(current.as_str()) {
        return;
    }
    for dir in ["Service Worker", "Cache", "Code Cache"] {
        let path = profile.join("Default").join(dir);
        if path.exists() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
    log!("interface atualizada ({current}), caches do navegador limpos");
    let _ = std::fs::write(&marker, current);
}

/// Profile preferences that keep Chromium silent: Chrome rewrites this file on
/// exit, so the keys are merged into it before every launch.
fn seed_preferences(profile: &Path) -> std::io::Result<()> {
    use serde_json::{Value, json};
    let overrides = json!({
        "translate": { "enabled": false },
        "translate_blocked_languages": ["pt", "pt-BR", "en"],
        "intl": if crate::settings::is_pt() {
            json!({ "accept_languages": "pt-BR,pt,en-US,en", "selected_languages": "pt-BR,pt,en-US,en" })
        } else {
            json!({ "accept_languages": "en-US,en", "selected_languages": "en-US,en" })
        },
        "credentials_enable_service": false,
        "credentials_enable_autosignin": false,
        "autofill": { "profile_enabled": false, "credit_card_enabled": false },
        "browser": { "has_seen_welcome_page": true, "check_default_browser": false },
        "download": { "prompt_for_download": false },
        "download_bubble": { "partial_view_enabled": false },
        "search": { "suggest_enabled": false },
        "privacy_sandbox": { "m1": { "prompt_suppressed": 1, "notice_shown": true } },
        "profile": {
            "password_manager_enabled": false,
            "exit_type": "Normal",
            "exited_cleanly": true,
            "default_content_setting_values": {
                "notifications": 2, "geolocation": 2, "media_stream_camera": 2,
                "media_stream_mic": 2, "popups": 2, "clipboard": 1
            }
        }
    });
    fn merge(base: &mut Value, over: &Value) {
        match (base, over) {
            (Value::Object(b), Value::Object(o)) => {
                for (k, v) in o {
                    merge(b.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
            (b, o) => *b = o.clone(),
        }
    }
    let path = profile.join("Default").join("Preferences");
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut prefs = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    merge(&mut prefs, &overrides);
    std::fs::write(&path, serde_json::to_vec(&prefs)?)
}

/// Graceful stop: SIGTERM lets Chromium flush its profile; force after a grace period.
pub async fn shutdown(child: &mut Child) {
    let pgid = child.id().map(|p| p as i32);
    if let Some(pgid) = pgid {
        unsafe { libc::kill(-pgid, libc::SIGTERM) };
    }
    match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
        Ok(status) => log!("navegador fechado: {status:?}"),
        Err(_) => log!("navegador não respondeu, forçando"),
    }
    if let Some(pgid) = pgid {
        kill_group(pgid);
    }
}

/// SIGKILLs whatever is left of the browser, in its group or escaped from it.
pub fn kill_group(pgid: i32) {
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
    kill_leftovers();
}

/// Kills and reaps every remaining child (as subreaper, that includes
/// detached Chromium helpers). Returns once none are left or after 3s.
pub fn reap_orphans() {
    let me = std::process::id().to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let mut alive = 0;
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
                let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
                // Fields after the parenthesised comm: state, ppid, ...
                let ppid = stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().nth(1));
                if ppid == Some(me.as_str()) {
                    alive += 1;
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
            }
        }
        while unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) } > 0 {}
        if alive == 0 {
            return;
        }
        if std::time::Instant::now() >= deadline {
            log!("{alive} processo(s) não encerraram a tempo");
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
