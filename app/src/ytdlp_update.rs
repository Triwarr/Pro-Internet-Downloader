//! 0.2.2: keeps yt-dlp.exe up to date without user action.
//!
//! - At start (not in --demo) a background thread checks at most once per 24 hours
//!   (`%APPDATA%\Pro Internet Downloader\ytdlp_update.json`), only when no download is running.
//! - The update uses yt-dlp's own `-U` (official GitHub release, signature checked by yt-dlp).
//! - Before updating, yt-dlp.exe is copied to `yt-dlp.exe.pimbak`; when `-U` fails or the
//!   updated exe does not answer `--version`, the backup is restored.
//! - When the install folder is not writable (Program Files), the update runs on a copy in
//!   `%LOCALAPPDATA%\ProInternetManager\bin`, which `find_ytdlp` then prefers.
//! - Every step is written to pim.log. Switch: Options > Streams > "Keep yt-dlp up to date".

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

static UPDATING: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static INFO: Mutex<Option<YtInfo>> = Mutex::new(None);

#[derive(Clone, Default)]
pub struct YtInfo {
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub last_result: String,
    pub last_check: Option<u64>,
}

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_check: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_result: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn settings_path() -> PathBuf {
    crate::paths::data_file("ytdlp_update.json")
}

fn load() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

fn save(settings: &Settings) {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        if let Err(e) = fs::write(&path, text) {
            crate::pimlog::warn(format!("yt-dlp update: could not write {}: {e}", path.display()));
        }
    }
}

static ENABLED_CACHE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// "Keep yt-dlp up to date" (default on). Cached after the first read (UI calls it every frame).
pub fn enabled() -> bool {
    match ENABLED_CACHE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let value = load().enabled.unwrap_or(true);
            ENABLED_CACHE.store(if value { 1 } else { 2 }, Ordering::Relaxed);
            value
        }
    }
}

pub fn set_enabled(value: bool) {
    let mut settings = load();
    settings.enabled = Some(value);
    save(&settings);
    ENABLED_CACHE.store(if value { 1 } else { 2 }, Ordering::Relaxed);
    crate::pimlog::info(format!("yt-dlp update: 'Keep yt-dlp up to date' switched {}", if value { "on" } else { "off" }));
}

/// True while yt-dlp.exe is being replaced (yt-dlp downloads wait for it).
pub fn is_updating() -> bool {
    UPDATING.load(Ordering::SeqCst)
}

/// True while a check / update thread runs (button disabled in Options).
pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

pub fn local_bin_dir() -> Option<PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|p| PathBuf::from(p).join("ProInternetManager").join("bin"))
}

/// The updated copy in %LOCALAPPDATA%\ProInternetManager\bin, when it exists.
pub fn local_bin_ytdlp() -> Option<PathBuf> {
    local_bin_dir().map(|d| d.join("yt-dlp.exe")).filter(|p| p.is_file())
}

fn app_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
}

/// PATH for yt-dlp child processes: the app folder (deno.exe, ffmpeg.exe) and its bin\ first.
pub fn child_path() -> std::ffi::OsString {
    let mut parts: Vec<PathBuf> = Vec::new();
    if let Some(dir) = app_dir() {
        parts.push(dir.join("bin"));
        parts.push(dir);
    }
    if let Some(path) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(parts).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

pub fn info() -> YtInfo {
    INFO.lock().map(|g| g.clone()).unwrap_or_default().unwrap_or_default()
}

fn set_info(version: Option<String>, path: Option<PathBuf>, result: Option<String>) {
    if let Ok(mut guard) = INFO.lock() {
        let mut info = guard.clone().unwrap_or_default();
        info.version = version;
        info.path = path;
        let settings = load();
        info.last_result = result.or(settings.last_result).unwrap_or_default();
        info.last_check = settings.last_check;
        *guard = Some(info);
    }
}

/// Runs `exe args` hidden, returns (success, stdout + stderr). Killed after `timeout`.
fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<(bool, String), String> {
    let mut command = Command::new(exe);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    command.env("PATH", child_path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| format!("could not start {}: {e}", exe.display()))?;
    let out = child.stdout.take();
    let err = child.stderr.take();
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).to_string()
        })
    };
    let out_t = read(out.map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let err_t = read(err.map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(150)),
            Err(e) => return Err(format!("could not wait for {}: {e}", exe.display())),
        }
    };
    let text = format!("{}{}", out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default());
    match status {
        Some(status) => Ok((status.success(), text)),
        None => Err(format!("{} did not finish within {} s", exe.display(), timeout.as_secs())),
    }
}

/// `yt-dlp --version` (e.g. "2026.10.26").
pub fn version_of(exe: &Path) -> Result<String, String> {
    let (ok, text) = run(exe, &["--version"], Duration::from_secs(60))?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    if ok && line.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        Ok(line)
    } else {
        Err(format!("--version gave no version ({})", crate::pimlog::tail(text.trim(), 200)))
    }
}

/// Reads the version of the yt-dlp that downloads use (shown in Options > Streams).
pub fn refresh_version() {
    match crate::find_ytdlp() {
        Some(path) => match version_of(&path) {
            Ok(version) => {
                crate::pimlog::info(format!("yt-dlp version {version} ({})", path.display()));
                set_info(Some(version), Some(path), None);
            }
            Err(e) => {
                crate::pimlog::warn(format!("yt-dlp version unknown: {e}"));
                set_info(None, Some(path), None);
            }
        },
        None => set_info(None, None, None),
    }
}

/// Called once at start. `demo` only reads the version (no update in previews).
pub fn startup(demo: bool) {
    thread::spawn(move || {
        refresh_version();
        if demo {
            return;
        }
        if !enabled() {
            crate::pimlog::info("yt-dlp update: automatic update is off (Options > Streams).");
            return;
        }
        let due = load().last_check.map(|t| now_secs().saturating_sub(t) >= CHECK_INTERVAL_SECS).unwrap_or(true);
        if !due {
            crate::pimlog::info("yt-dlp update: skipped at startup (last check less than 24 h ago).");
            return;
        }
        // give the app time to start and wait until no download runs (max. 6 hours)
        thread::sleep(Duration::from_secs(20));
        let started = Instant::now();
        while crate::ACTIVE_DOWNLOADS.load(Ordering::SeqCst) > 0 {
            if started.elapsed() > Duration::from_secs(6 * 3600) {
                crate::pimlog::info("yt-dlp update: postponed (downloads kept running); next start tries again.");
                return;
            }
            thread::sleep(Duration::from_secs(15));
        }
        let _ = update_now(false);
    });
}

/// "Update yt-dlp now" button: runs in the background.
pub fn update_async(ctx: Option<egui::Context>) {
    if is_busy() {
        return;
    }
    thread::spawn(move || {
        let _ = update_now(false);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
}

/// Check + update. `force_local` treats the install folder as not writable (self-test of the
/// Program Files route). Returns the result text (also logged and stored).
pub fn update_now(force_local: bool) -> Result<String, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("a yt-dlp update is already running".into());
    }
    let result = do_update(force_local);
    BUSY.store(false, Ordering::SeqCst);
    UPDATING.store(false, Ordering::SeqCst);
    let mut settings = load();
    settings.last_check = Some(now_secs());
    let text = match &result {
        Ok(t) => t.clone(),
        Err(e) => format!("Update failed: {e}"),
    };
    settings.last_result = Some(format!("{} ({})", text, chrono::Local::now().format("%Y-%m-%d %H:%M")));
    save(&settings);
    match &result {
        Ok(t) => crate::pimlog::info(format!("yt-dlp update: {t}")),
        Err(e) => crate::pimlog::error(format!("yt-dlp update failed: {e}")),
    }
    let path = crate::find_ytdlp();
    let version = path.as_deref().and_then(|p| version_of(p).ok());
    set_info(version, path, settings.last_result.clone());
    result
}

fn do_update(force_local: bool) -> Result<String, String> {
    if crate::ACTIVE_DOWNLOADS.load(Ordering::SeqCst) > 0 {
        return Err("downloads are running; try again when they are finished".into());
    }
    let current = crate::find_ytdlp().ok_or("yt-dlp.exe not found")?;
    let local_dir = local_bin_dir().ok_or("LOCALAPPDATA is not set")?;
    let app = app_dir().ok_or("install folder unknown")?;
    let in_local = current.parent() == Some(local_dir.as_path());
    let in_app = current.parent() == Some(app.as_path()) || current.parent() == Some(app.join("bin").as_path());
    let mut created_copy = false;
    let target = if in_local {
        current.clone()
    } else if in_app {
        let writable = !force_local && crate::update::dir_writable(current.parent().unwrap_or(&app));
        if writable {
            current.clone()
        } else {
            fs::create_dir_all(&local_dir).map_err(|e| format!("could not create {}: {e}", local_dir.display()))?;
            let copy = local_dir.join("yt-dlp.exe");
            fs::copy(&current, &copy).map_err(|e| format!("could not copy yt-dlp.exe to {}: {e}", local_dir.display()))?;
            crate::pimlog::info(format!(
                "yt-dlp update: {} is not writable; updating a copy in {}",
                current.parent().unwrap_or(&app).display(),
                local_dir.display()
            ));
            created_copy = true;
            copy
        }
    } else {
        return Err(format!("{} is not part of Pro Internet Manager (PATH); it is not updated automatically", current.display()));
    };
    let old = version_of(&target).unwrap_or_else(|_| "unknown".into());
    crate::pimlog::info(format!("yt-dlp update: checking {} (version {old})", target.display()));
    let backup = target.with_file_name("yt-dlp.exe.pimbak");
    fs::copy(&target, &backup).map_err(|e| format!("could not back up yt-dlp.exe: {e}"))?;
    UPDATING.store(true, Ordering::SeqCst);
    let outcome = run(&target, &["-U"], Duration::from_secs(300));
    let restore = |why: &str| -> String {
        let mut restored = false;
        for _ in 0..20 {
            if fs::copy(&backup, &target).is_ok() {
                restored = true;
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
        if created_copy {
            // fall back to the original in the install folder
            let _ = fs::remove_file(&target);
            let _ = fs::remove_file(&backup);
        }
        let what = if created_copy {
            "the yt-dlp.exe in the install folder stays in use"
        } else if restored {
            "previous yt-dlp.exe restored"
        } else {
            "restore FAILED, backup is yt-dlp.exe.pimbak"
        };
        format!("{why}; {what}")
    };
    let (ok, text) = match outcome {
        Ok(v) => v,
        Err(e) => return Err(restore(&e)),
    };
    let summary: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    crate::pimlog::info(format!("yt-dlp -U output: {}", crate::pimlog::tail(&summary.join(" | "), 800)));
    if !ok {
        let line = summary.iter().find(|l| l.starts_with("ERROR")).or(summary.last()).copied().unwrap_or("no output");
        let line: String = if line.chars().count() > 180 { format!("{}\u{2026}", line.chars().take(180).collect::<String>()) } else { line.to_string() };
        return Err(restore(&format!("yt-dlp -U failed ({line})")));
    }
    let new = match version_of(&target) {
        Ok(v) => v,
        Err(e) => return Err(restore(&format!("updated yt-dlp does not start ({e})"))),
    };
    // remove a leftover yt-dlp.exe.old from yt-dlp's own updater
    let _ = fs::remove_file(target.with_file_name("yt-dlp.exe.old"));
    if new == old {
        if created_copy {
            // nothing changed: no need to keep a copy outside the install folder
            let _ = fs::remove_file(&target);
            let _ = fs::remove_file(&backup);
        }
        Ok(format!("yt-dlp {new} is up to date"))
    } else {
        Ok(format!("yt-dlp updated {old} -> {new} ({})", target.display()))
    }
}
