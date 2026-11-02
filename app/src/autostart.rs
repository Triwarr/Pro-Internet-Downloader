//! Pro Internet Manager: "Start with Windows" (per-user Run key, no admin, no extra crates).
//! HKCU\Software\Microsoft\Windows\CurrentVersion\Run, value "ProInternetManager" =
//! "<current exe>" --minimized. Only this one value is ever written or deleted.
//! The state is read from the registry (not stored in config.json).

use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "ProInternetManager";

/// Set by `--demo`: previews never write to the registry.
static DEMO: AtomicBool = AtomicBool::new(false);
/// Cached checkbox state, refreshed each time the Options window opens.
static ENABLED: AtomicBool = AtomicBool::new(false);
static WAS_OPEN: AtomicBool = AtomicBool::new(false);
static START_MINIMIZED: AtomicBool = AtomicBool::new(false);
/// Last registry error, shown under the checkbox.
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub fn set_demo(demo: bool) {
    DEMO.store(demo, Ordering::Relaxed);
}

pub fn is_demo() -> bool {
    DEMO.load(Ordering::Relaxed)
}

pub fn set_start_minimized(on: bool) {
    START_MINIMIZED.store(on, Ordering::Relaxed);
}

/// True once (first frame) when started with `--minimized`.
pub fn take_start_minimized() -> bool {
    START_MINIMIZED.swap(false, Ordering::Relaxed)
}

pub fn last_error() -> Option<String> {
    LAST_ERROR.lock().ok().and_then(|e| e.clone())
}

fn reg() -> Command {
    let mut command = Command::new("reg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

/// True when the Run value exists.
pub fn is_enabled() -> bool {
    reg()
        .args(["query", RUN_KEY, "/v", VALUE_NAME])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Enable (write the Run value) or disable (delete only that value).
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let data = format!("\"{}\" --minimized", exe.display());
        let out = reg()
            .args(["add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &data, "/f"])
            .output()
            .map_err(|e| format!("reg add: {e}"))?;
        if !out.status.success() {
            return Err(format!("reg add failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
    } else if is_enabled() {
        let out = reg()
            .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
            .output()
            .map_err(|e| format!("reg delete: {e}"))?;
        if !out.status.success() {
            return Err(format!("reg delete failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
    }
    Ok(())
}

/// Call every frame with the Options window state; re-reads the registry when it opens.
pub fn sync_on_open(open: bool) {
    let was = WAS_OPEN.swap(open, Ordering::Relaxed);
    if open && !was {
        ENABLED.store(is_enabled(), Ordering::Relaxed);
    }
}

pub fn cached() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Checkbox changed: apply to the registry (skipped in demo), then re-read the real state.
pub fn toggle(enabled: bool) {
    if is_demo() {
        ENABLED.store(enabled, Ordering::Relaxed);
        return;
    }
    let result = set_enabled(enabled);
    ENABLED.store(is_enabled(), Ordering::Relaxed);
    if let Ok(mut slot) = LAST_ERROR.lock() {
        *slot = result.err();
    }
}
