//! Whether piShop is the app on screen. gamescope publishes the focused app on
//! the root window of Steam's X server; while a game piShop launched (or
//! Steam's own menus) is in front, the controller belongs to it and the UI
//! must ignore the gamepad (Chromium reads it even when it isn't shown).

use std::io::{BufRead, BufReader};
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static FOCUSED: AtomicBool = AtomicBool::new(true);

pub fn focused() -> bool {
    FOCUSED.load(Ordering::Relaxed)
}

/// Follows gamescope's `GAMESCOPE_FOCUSED_APP` with `xprop -spy` (one line per
/// change). Outside gamescope (or launched outside Steam) piShop counts as focused.
pub fn start() {
    let Ok(me) = std::env::var("SteamAppId") else { return };
    std::thread::spawn(move || {
        loop {
            let mut cmd = Command::new("xprop");
            cmd.args(["-display", ":0", "-root", "-spy", "GAMESCOPE_FOCUSED_APP"]).stdout(Stdio::piped()).stderr(Stdio::null());
            #[cfg(target_os = "linux")]
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
            if let Ok(mut child) = cmd.spawn() {
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        // GAMESCOPE_FOCUSED_APP(CARDINAL) = 3682868229
                        if let Some(v) = line.rsplit('=').next().map(str::trim).filter(|v| v.chars().all(|c| c.is_ascii_digit())) {
                            let now = v == me;
                            if FOCUSED.swap(now, Ordering::Relaxed) != now {
                                crate::log!("foco: {}", if now { "piShop na frente" } else { "outro app na frente" });
                            }
                        }
                    }
                }
                let _ = child.wait();
            }
            // xprop ended (Steam restarting, Desktop Mode…): don't lock the UI out.
            FOCUSED.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}
