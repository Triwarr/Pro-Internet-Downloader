#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

mod autostart;
mod extras_ui;
mod paths;
mod pimlog;
mod theme;
mod tray;
mod ui;
mod update;
mod ytdlp_update;

use theme::ThemeKind;

const CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Queued,
    Downloading,
    Paused,
    Done,
    Error,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Queued => "Queued",
            Status::Downloading => "Downloading",
            Status::Paused => "Paused",
            Status::Done => "Done",
            Status::Error => "Error",
        }
    }
}

#[derive(Clone)]
struct Job {
    url: String,
    name: String,
    dest: PathBuf,
    connections: usize,
    headers: Vec<(String, String)>,
    stream: bool,
    ytdlp: bool,
    quality: Option<u32>,
    status: Arc<Mutex<Status>>,
    total: Arc<AtomicU64>,
    done: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
    cancel: Arc<AtomicBool>,
    sample: Arc<Mutex<Sample>>,
    // 0.2.2: live progress reported by yt-dlp (speed in bytes/s, 0 = unknown; ETA in s,
    // u64::MAX = unknown) and the current phase ("Starting…", "Merging…", ...).
    ext_speed: Arc<AtomicU64>,
    ext_eta: Arc<AtomicU64>,
    phase: Arc<Mutex<String>>,
}

struct Sample {
    at: Instant,
    bytes: u64,
    speed: f64,
}

impl Job {
    fn new(url: String, dest: PathBuf, name: String, connections: usize, headers: Vec<(String, String)>, stream: bool, ytdlp: bool, quality: Option<u32>) -> Self {
        Self {
            url,
            name,
            dest,
            connections: connections.clamp(1, 16),
            headers,
            stream,
            ytdlp,
            quality,
            status: Arc::new(Mutex::new(Status::Queued)),
            total: Arc::new(AtomicU64::new(0)),
            done: Arc::new(AtomicU64::new(0)),
            error: Arc::new(Mutex::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
            sample: Arc::new(Mutex::new(Sample { at: Instant::now(), bytes: 0, speed: 0.0 })),
            ext_speed: Arc::new(AtomicU64::new(0)),
            ext_eta: Arc::new(AtomicU64::new(u64::MAX)),
            phase: Arc::new(Mutex::new(String::new())),
        }
    }

    fn status(&self) -> Status {
        *self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_status(&self, value: Status) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = value;
    }

    fn set_error(&self, message: String) {
        *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
    }

    fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 0.2.2: short, understandable reason for a failed download (full text is in pim.log).
    pub(crate) fn short_error(&self) -> Option<String> {
        let error = self.error()?;
        if error.trim().is_empty() {
            return None;
        }
        Some(short_reason(&error))
    }

    pub(crate) fn phase(&self) -> String {
        self.phase.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_phase(&self, value: &str) {
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = value.to_string();
    }

    /// 0.2.2: true while yt-dlp merges / post-processes (no bytes are transferred then).
    pub(crate) fn post_processing(&self) -> bool {
        let phase = self.phase();
        phase.starts_with("Merging") || phase.starts_with("Processing")
    }

    fn speed(&self) -> f64 {
        if self.ytdlp {
            // yt-dlp reports its own (smoothed) speed; 0 while starting or merging.
            return self.ext_speed.load(Ordering::Relaxed) as f64;
        }
        let mut sample = self.sample.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let bytes = self.done.load(Ordering::Relaxed);
        let elapsed = now.duration_since(sample.at).as_secs_f64();
        if elapsed >= 0.5 {
            sample.speed = (bytes.saturating_sub(sample.bytes)) as f64 / elapsed;
            sample.at = now;
            sample.bytes = bytes;
        }
        sample.speed
    }

    fn progress(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            (self.done.load(Ordering::Relaxed) as f64 / total as f64).clamp(0.0, 1.0) as f32
        }
    }

    fn display_size(&self) -> String {
        let done = self.done.load(Ordering::Relaxed);
        let total = self.total.load(Ordering::Relaxed);
        // 0.2.1: a finished download only shows its total ("650 MB"); "x / y" only while it is not done yet.
        if self.status() == Status::Done {
            return human_compact(if total > 0 { total } else { done });
        }
        if total == 0 {
            if done == 0 { "\u{2013}".to_string() } else { human_compact(done) }
        } else {
            format!("{} / {}", human_compact(done), human_compact(total))
        }
    }

    fn eta(&self) -> String {
        if self.ytdlp {
            if self.post_processing() {
                return self.phase();
            }
            let eta = self.ext_eta.load(Ordering::Relaxed);
            return if eta == u64::MAX { String::new() } else { format!("{} left", human_time(eta)) };
        }
        let speed = self.speed();
        let total = self.total.load(Ordering::Relaxed);
        let done = self.done.load(Ordering::Relaxed);
        if speed <= 1.0 || total <= done {
            return String::new();
        }
        let seconds = ((total - done) as f64 / speed) as u64;
        format!("{} left", human_time(seconds))
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Like `human`, but drops a trailing ".0" ("650 MB" instead of "650.0 MB").
fn human_compact(bytes: u64) -> String {
    human(bytes).replace(".0 ", " ")
}

fn human_time(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    }
}

/// 0.2.2: one short line for the row (the complete yt-dlp / ffmpeg / HTTP text stays in pim.log).
fn short_reason(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let known: [(&[&str], &str); 18] = [
        (&["sign in to confirm", "confirm you\u{2019}re not a bot", "confirm you're not a bot"], "YouTube asks to sign in (cookies needed)"),
        (&["private video"], "Private video"),
        (&["video unavailable", "this video is not available"], "Video unavailable"),
        (&["members-only", "join this channel"], "Members-only video"),
        (&["requested format is not available"], "Requested quality not available"),
        (&["http error 403", "403 forbidden", "http 403"], "Server refused access (HTTP 403)"),
        (&["http error 404", "404 not found", "http 404"], "Not found on the server (HTTP 404)"),
        (&["http error 429", "too many requests"], "Too many requests (HTTP 429), try later"),
        (&["timed out", "timeout"], "Connection timed out"),
        (&["connection refused", "actively refused", "error trying to connect", "could not connect", "dns error", "getaddrinfo"], "Could not connect to the server"),
        (&["destination already exists"], "File already exists in the folder"),
        (&["yt-dlp.exe not found"], "yt-dlp.exe is missing"),
        (&["ffmpeg.exe is required", "ffmpeg.exe not found"], "ffmpeg.exe is missing"),
        (&["could not be started"], "Helper program could not be started"),
        (&["os error 112", "no space left", "not enough space"], "Disk is full"),
        (&["os error 5", "access is denied", "permission denied"], "No write access to the folder"),
        (&["server no longer supports resume"], "Server does not support resume"),
        (&["server ignored range request"], "Server does not support segments"),
    ];
    for (needles, text) in known {
        if needles.iter().any(|n| lower.contains(n)) {
            return text.to_string();
        }
    }
    // Otherwise: the first ERROR line without the tool / extractor prefixes.
    let line = error
        .lines()
        .map(str::trim)
        .find(|l| l.contains("ERROR"))
        .or_else(|| error.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("Download failed");
    let mut text = line.trim_start_matches("yt-dlp failed:").trim_start_matches("ffmpeg failed:").trim();
    text = text.trim_start_matches("ERROR:").trim();
    if text.starts_with('[') {
        if let Some(end) = text.find(']') {
            text = text[end + 1..].trim();
        }
    }
    let text = if text.is_empty() { "Download failed" } else { text };
    let mut short: String = text.chars().take(90).collect();
    if text.chars().count() > 90 {
        short.push('\u{2026}');
    }
    short
}

struct Probe {
    len: Option<u64>,
    ranges: bool,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("RustDownloadManager/0.1")
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

fn with_headers(mut request: reqwest::blocking::RequestBuilder, headers: &[(String, String)]) -> reqwest::blocking::RequestBuilder {
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            request = request.header(name, value);
        }
    }
    request
}

fn probe(client: &reqwest::blocking::Client, url: &str, headers: &[(String, String)]) -> Result<Probe, String> {
    let mut len = None;
    let mut ranges = false;
    if let Ok(response) = with_headers(client.head(url), headers).send() {
        len = response.content_length();
        ranges = response
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("bytes"));
    }
    if len.is_none() || !ranges {
        if let Ok(response) = with_headers(client.get(url), headers)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
        {
            if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                ranges = true;
                if len.is_none() {
                    len = response
                        .headers()
                        .get(reqwest::header::CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_total);
                }
            } else if len.is_none() {
                len = response.content_length();
            }
        }
    }
    Ok(Probe { len, ranges })
}

fn filename_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("download");
    let decoded = percent_decode(name);
    let cleaned: String = decoded
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_string();
    if cleaned.is_empty() { "download".to_string() } else { cleaned }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &input[i + 1..i + 3];
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn parse_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.trim().parse().ok()
}

fn unique_path(dest: &Path, name: &str) -> PathBuf {
    let initial = dest.join(name);
    if !initial.exists() {
        return initial;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    for n in 1..10000 {
        let candidate = if ext.is_empty() {
            dest.join(format!("{stem} ({n})"))
        } else {
            dest.join(format!("{stem} ({n}).{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    initial
}

fn start(job: &Job) {
    job.cancel.store(false, Ordering::SeqCst);
    job.set_error(String::new());
    job.set_status(Status::Downloading);
    pimlog::info(format!(
        "Download start: '{}' kind={} url={} connections={} quality={} headers=[{}]",
        job.name,
        if job.ytdlp { "yt-dlp" } else if job.stream { "ffmpeg-stream" } else { "http" },
        pimlog::short_url(&job.url),
        job.connections,
        job.quality.map(|q| q.to_string()).unwrap_or_else(|| "best".into()),
        pimlog::header_names(&job.headers),
    ));
    let job = job.clone();
    ACTIVE_DOWNLOADS.fetch_add(1, Ordering::SeqCst);
    thread::spawn(move || {
        let started = Instant::now();
        let result = download(&job);
        ACTIVE_DOWNLOADS.fetch_sub(1, Ordering::SeqCst);
        match result {
            Ok(()) => {
                let secs = started.elapsed().as_secs_f64();
                match job.status() {
                    Status::Paused => pimlog::info(format!(
                        "Download paused: '{}' after {:.1}s, {} bytes so far",
                        job.name, secs, job.done.load(Ordering::Relaxed)
                    )),
                    _ => pimlog::info(format!(
                        "Download done: '{}' in {:.1}s, {} -> {}",
                        job.name,
                        secs,
                        if job.stream && !job.ytdlp { "stream".to_string() } else { format!("{} bytes", job.done.load(Ordering::Relaxed)) },
                        job.dest.join(&job.name).display()
                    )),
                }
            }
            Err(error) => {
                let secrets = pimlog::secrets_of(&job.headers);
                pimlog::error(format!(
                    "Download FAILED: '{}' url={} after {:.1}s: {}",
                    job.name,
                    pimlog::short_url(&job.url),
                    started.elapsed().as_secs_f64(),
                    pimlog::scrub_with(&error, &secrets)
                ));
                job.set_error(error);
                job.set_status(Status::Error);
            }
        }
    });
}

/// 0.2.2: number of download threads that are running (the yt-dlp updater waits for 0).
pub(crate) static ACTIVE_DOWNLOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn is_stream_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".mpd")
}

fn is_ytdlp_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else { return false };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    (host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be")
        && !host.contains("googlevideo")
}

fn youtube_name(url: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if let Some(id) = parsed
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.to_string())
        {
            return format!("youtube-{id}.mp4");
        }
        if let Some(id) = parsed.path_segments().and_then(|mut segments| segments.next_back()) {
            if !id.is_empty() {
                return format!("youtube-{id}.mp4");
            }
        }
    }
    "youtube.mp4".to_string()
}

fn find_ytdlp() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    // 0.2.2: an updated copy in %LOCALAPPDATA%\ProInternetManager\bin (used when the install
    // folder, e.g. Program Files, is not writable) has priority.
    if let Some(local) = ytdlp_update::local_bin_ytdlp() {
        candidates.push(local);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("yt-dlp.exe"));
            candidates.push(dir.join("bin").join("yt-dlp.exe"));
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for entry in path.split(';') {
            if !entry.trim().is_empty() {
                candidates.push(Path::new(entry).join("yt-dlp.exe"));
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn find_ffmpeg() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ffmpeg.exe"));
            candidates.push(dir.join("bin").join("ffmpeg.exe"));
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for entry in path.split(';') {
            if !entry.trim().is_empty() {
                candidates.push(Path::new(entry).join("ffmpeg.exe"));
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn ytdlp_cookie_file(job: &Job) -> Result<Option<PathBuf>, String> {
    let Some((_, cookie_header)) = job
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("cookie"))
    else {
        return Ok(None);
    };
    let host = reqwest::Url::parse(&job.url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_default();
    if host.is_empty() {
        return Ok(None);
    }
    let domain = if host.ends_with("youtube.com") || host == "youtu.be" {
        ".youtube.com".to_string()
    } else if host.parse::<std::net::IpAddr>().is_ok() {
        host
    } else {
        format!(".{host}")
    };
    let mut cookie_file = String::from("# Netscape HTTP Cookie File\n");
    for part in cookie_header.split(';') {
        let Some((name, value)) = part.trim().split_once('=') else { continue };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.contains('\r') || value.contains('\n') || value.contains('\t') {
            continue;
        }
        cookie_file.push_str(&domain);
        cookie_file.push_str("\tTRUE\t/\tTRUE\t0\t");
        cookie_file.push_str(name);
        cookie_file.push('\t');
        cookie_file.push_str(value);
        cookie_file.push('\n');
    }
    if cookie_file.lines().count() <= 1 {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!("rdm-ytdlp-{}-{stamp}.cookies", std::process::id()));
    fs::write(&path, cookie_file).map_err(|e| format!("cookie file: {e}"))?;
    Ok(Some(path))
}

fn ytdlp_download(job: &Job) -> Result<(), String> {
    // 0.2.2: wait (max. 2 minutes) while the yt-dlp auto-update replaces yt-dlp.exe.
    let wait = Instant::now();
    while ytdlp_update::is_updating() && wait.elapsed() < Duration::from_secs(120) {
        job.set_phase("Waiting for yt-dlp update\u{2026}");
        thread::sleep(Duration::from_millis(300));
    }
    let ytdlp = find_ytdlp().ok_or_else(|| "yt-dlp.exe not found next to the app, in bin\\ or on PATH".to_string())?;
    let ffmpeg = find_ffmpeg().ok_or_else(|| "ffmpeg.exe is required for YouTube merging".to_string())?;
    // 0.2.2: real byte counts come from yt-dlp's progress lines (was a fixed 0 / 1).
    job.total.store(0, Ordering::Relaxed);
    job.done.store(0, Ordering::Relaxed);
    job.ext_speed.store(0, Ordering::Relaxed);
    job.ext_eta.store(u64::MAX, Ordering::Relaxed);
    job.set_phase("Starting\u{2026}");
    job.set_status(Status::Downloading);
    let final_path = job.dest.join(&job.name);
    if final_path.exists() {
        return Err("destination already exists".into());
    }
    let format = job
        .quality
        .map(|height| format!("bv*[height<={height}]+ba/b[height<={height}]"))
        .unwrap_or_else(|| "bv*+ba/b".to_string());
    let cookie_file = ytdlp_cookie_file(job)?;
    let secrets = pimlog::secrets_of(&job.headers);
    // Logged copy of the command: header values and cookies are never written to the log.
    let mut logged: Vec<String> = vec![
        pimlog::quote(&ytdlp.display().to_string()),
        "--no-playlist --newline --force-overwrites --merge-output-format mp4 -f".into(),
        pimlog::quote(&format),
        "--ffmpeg-location".into(),
        pimlog::quote(&ffmpeg.display().to_string()),
    ];
    if let Some(path) = &cookie_file {
        logged.push(format!("--cookies {} (temp file, cookie values not logged)", pimlog::quote(&path.display().to_string())));
    }
    for (name, _) in job.headers.iter().filter(|(n, _)| !n.eq_ignore_ascii_case("cookie")) {
        logged.push(format!("--add-header \"{name}: ...\""));
    }
    logged.push("--progress-template".into());
    logged.push(pimlog::quote(YTDLP_PROGRESS_TEMPLATE));
    logged.push("-o".into());
    logged.push(pimlog::quote(&final_path.display().to_string()));
    logged.push(pimlog::quote(&pimlog::short_url(&job.url)));
    pimlog::info(format!("yt-dlp command: {}", logged.join(" ")));
    let mut command = std::process::Command::new(ytdlp);
    command.args([
        "--no-playlist",
        "--newline",
        "--force-overwrites",
        "--merge-output-format",
        "mp4",
        "-f",
    ]);
    command.arg(&format);
    command.arg("--ffmpeg-location").arg(&ffmpeg);
    if let Some(path) = &cookie_file {
        command.arg("--cookies").arg(path);
    }
    for (name, value) in &job.headers {
        if name.eq_ignore_ascii_case("cookie") {
            continue;
        }
        command.arg("--add-header").arg(format!("{name}: {value}"));
    }
    // 0.2.2: machine-readable progress lines on stdout (one per update thanks to --newline).
    command.arg("--progress-template").arg(YTDLP_PROGRESS_TEMPLATE);
    command.arg("-o").arg(&final_path).arg(&job.url);
    command.stdin(std::process::Stdio::null());
    // 0.2.2: deno.exe (YouTube JS runtime) lives next to the app; when yt-dlp runs from the
    // updated copy in %LOCALAPPDATA% it finds deno through PATH.
    command.env("PATH", ytdlp_update::child_path());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            if let Some(path) = &cookie_file {
                let _ = fs::remove_file(path);
            }
            return Err(format!("yt-dlp could not be started: {e}"));
        }
    };
    // stderr is drained on its own thread so a full pipe can never block yt-dlp.
    let stderr_handle = child.stderr.take().map(|mut stderr| {
        thread::spawn(move || {
            let mut buffer = Vec::new();
            let _ = stderr.read_to_end(&mut buffer);
            buffer
        })
    });
    let (line_tx, line_rx) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut raw = Vec::new();
            loop {
                raw.clear();
                match reader.read_until(b'\n', &mut raw) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&raw);
                        for part in text.split('\r') {
                            let part = part.trim();
                            if !part.is_empty() && line_tx.send(part.to_string()).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
    } else {
        drop(line_tx);
    }
    let mut progress = YtdlpProgress::default();
    let mut cancelled = false;
    loop {
        if job.cancel.load(Ordering::SeqCst) {
            cancelled = true;
            let _ = child.kill();
            break;
        }
        match line_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => progress.feed(job, &line),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait();
    let stderr_bytes = stderr_handle.and_then(|h| h.join().ok()).unwrap_or_default();
    if let Some(path) = &cookie_file {
        let _ = fs::remove_file(path);
    }
    job.ext_speed.store(0, Ordering::Relaxed);
    job.ext_eta.store(u64::MAX, Ordering::Relaxed);
    if cancelled {
        pimlog::info(format!("yt-dlp stopped for '{}' (paused or removed)", job.name));
        job.set_phase("");
        job.set_status(Status::Paused);
        return Ok(());
    }
    let status = status.map_err(|e| format!("yt-dlp could not be waited for: {e}"))?;
    let message = String::from_utf8_lossy(&stderr_bytes);
    job.set_phase("");
    if !status.success() {
        pimlog::error(format!(
            "yt-dlp exit code {} for '{}'. stderr (last 4000 chars):\n{}",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "none".into()),
            job.name,
            pimlog::scrub_with(&pimlog::tail(message.trim(), 4000), &secrets)
        ));
        // UI keeps the ERROR lines (warnings are in the log).
        let errors: Vec<&str> = message.lines().filter(|l| !l.trim_start().starts_with("WARNING")).collect();
        let message = if errors.is_empty() { message.to_string() } else { errors.join("\n") };
        let short: String = message.chars().rev().take(1600).collect::<String>().chars().rev().collect();
        return Err(format!("yt-dlp failed: {}", short.trim()));
    }
    let warnings: Vec<String> = message
        .lines()
        .filter(|l| l.trim_start().starts_with("WARNING"))
        .take(10)
        .map(|l| pimlog::tail(&pimlog::scrub_with(l.trim(), &secrets), 300))
        .collect();
    if !warnings.is_empty() {
        pimlog::warn(format!("yt-dlp finished with warnings for '{}':\n{}", job.name, warnings.join("\n")));
    }
    pimlog::info(format!("yt-dlp exit code 0 for '{}'", job.name));
    // 0.2.2: the finished row shows the real file size (was "1 B").
    let size = fs::metadata(&final_path).map(|m| m.len()).unwrap_or_else(|_| job.done.load(Ordering::Relaxed));
    job.total.store(size, Ordering::Relaxed);
    job.done.store(size, Ordering::Relaxed);
    job.set_status(Status::Done);
    Ok(())
}

/// 0.2.2: one stdout line per progress update; fields that yt-dlp does not know are "NA".
const YTDLP_PROGRESS_TEMPLATE: &str = "download:PIMPROG|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s";

/// 0.2.2: turns yt-dlp's stdout into job progress. Video and audio are separate
/// downloads; bytes of finished parts are kept in `base` so the row and the status bar
/// show one growing total, then "Merging…" while ffmpeg joins them.
#[derive(Default)]
struct YtdlpProgress {
    base: u64,
    cur_done: u64,
    cur_total: u64,
    part: usize,
    parts: usize,
    merging: bool,
}

fn parse_num(value: &str) -> Option<f64> {
    let value = value.trim();
    if value.is_empty() || value == "NA" || value == "None" {
        return None;
    }
    value.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0)
}

impl YtdlpProgress {
    fn part_label(&self) -> String {
        if self.parts > 1 {
            format!("Downloading part {}/{}", self.part.max(1), self.parts)
        } else {
            "Downloading".to_string()
        }
    }

    fn publish(&self, job: &Job) {
        let done = self.base + self.cur_done;
        let total = (self.base + self.cur_total).max(done);
        job.done.store(done, Ordering::Relaxed);
        job.total.store(total, Ordering::Relaxed);
    }

    fn feed(&mut self, job: &Job, line: &str) {
        if let Some(rest) = line.strip_prefix("PIMPROG|") {
            let fields: Vec<&str> = rest.split('|').collect();
            if fields.len() < 6 || self.merging {
                return;
            }
            if self.part == 0 {
                self.part = 1;
            }
            let status = fields[0];
            let downloaded = parse_num(fields[1]).unwrap_or(0.0) as u64;
            let total = parse_num(fields[2]).or_else(|| parse_num(fields[3])).unwrap_or(0.0) as u64;
            self.cur_done = downloaded;
            self.cur_total = total.max(downloaded);
            if status == "finished" {
                self.cur_total = self.cur_done;
                job.ext_speed.store(0, Ordering::Relaxed);
                job.ext_eta.store(u64::MAX, Ordering::Relaxed);
            } else {
                job.ext_speed.store(parse_num(fields[4]).unwrap_or(0.0) as u64, Ordering::Relaxed);
                job.ext_eta.store(parse_num(fields[5]).map(|v| v as u64).unwrap_or(u64::MAX), Ordering::Relaxed);
            }
            if job.phase() != self.part_label() {
                job.set_phase(&self.part_label());
            }
            self.publish(job);
            return;
        }
        if let Some(list) = line.split("Downloading 1 format(s):").nth(1) {
            self.parts = list.trim().split('+').filter(|s| !s.trim().is_empty()).count().max(1);
            return;
        }
        if line.starts_with("[download] Destination:") || line.contains("has already been downloaded") {
            if self.part > 0 {
                self.base += self.cur_total.max(self.cur_done);
                self.cur_done = 0;
                self.cur_total = 0;
            }
            self.part += 1;
            job.set_phase(&self.part_label());
            pimlog::info(format!("yt-dlp '{}': {}", job.name, self.part_label().to_lowercase()));
            self.publish(job);
            return;
        }
        if line.starts_with("[Merger]") {
            self.merging = true;
            job.ext_speed.store(0, Ordering::Relaxed);
            job.ext_eta.store(u64::MAX, Ordering::Relaxed);
            job.set_phase("Merging\u{2026}");
            pimlog::info(format!("yt-dlp '{}': merging video and audio", job.name));
            return;
        }
        let post = ["[Fixup", "[ffmpeg]", "[VideoRemuxer]", "[VideoConvertor]", "[Metadata]", "[EmbedThumbnail]", "[ExtractAudio]", "[MoveFiles]"];
        if !self.merging && post.iter().any(|p| line.starts_with(p)) {
            job.ext_speed.store(0, Ordering::Relaxed);
            job.ext_eta.store(u64::MAX, Ordering::Relaxed);
            job.set_phase("Processing\u{2026}");
        }
    }
}

fn stream_download(job: &Job) -> Result<(), String> {
    if job.ytdlp {
        return ytdlp_download(job);
    }
    let ffmpeg = find_ffmpeg().ok_or_else(|| "ffmpeg.exe not found next to the app, in bin\\ or on PATH".to_string())?;
    job.total.store(1, Ordering::Relaxed);
    job.done.store(0, Ordering::Relaxed);
    job.set_status(Status::Downloading);
    let final_path = job.dest.join(&job.name);
    if final_path.exists() {
        return Err("destination already exists".into());
    }
    let mut command = std::process::Command::new(ffmpeg);
    command.args([
        "-hide_banner",
        "-y",
        "-loglevel",
        "error",
        "-protocol_whitelist",
        "file,http,https,tcp,tls,crypto",
    ]);
    let mut header_block = String::new();
    for (name, value) in &job.headers {
        header_block.push_str(name);
        header_block.push_str(": ");
        header_block.push_str(value);
        header_block.push_str("\r\n");
    }
    if !header_block.is_empty() {
        command.arg("-headers").arg(header_block);
    }
    command.arg("-i").arg(&job.url);
    command.args(["-map", "0:v:0?", "-map", "0:a:0?", "-c", "copy", "-movflags", "+faststart"]);
    command.arg(&final_path);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let secrets = pimlog::secrets_of(&job.headers);
    pimlog::info(format!(
        "ffmpeg command: {} -hide_banner -y -loglevel error -protocol_whitelist file,http,https,tcp,tls,crypto{} -i {} -map 0:v:0? -map 0:a:0? -c copy -movflags +faststart {}",
        pimlog::quote(&command.get_program().to_string_lossy()),
        if job.headers.is_empty() { String::new() } else { format!(" -headers \"<{}: values not logged>\"", pimlog::header_names(&job.headers)) },
        pimlog::quote(&pimlog::short_url(&job.url)),
        pimlog::quote(&final_path.display().to_string())
    ));
    let output = command.output().map_err(|e| format!("ffmpeg could not be started: {e}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        pimlog::error(format!(
            "ffmpeg exit code {} for '{}'. stderr (last 4000 chars):\n{}",
            output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "none".into()),
            job.name,
            pimlog::scrub_with(&pimlog::tail(message.trim(), 4000), &secrets)
        ));
        let short: String = message.chars().rev().take(1200).collect::<String>().chars().rev().collect();
        return Err(format!("ffmpeg failed: {}", short.trim()));
    }
    pimlog::info(format!("ffmpeg exit code 0 for '{}'", job.name));
    job.done.store(1, Ordering::Relaxed);
    job.set_status(Status::Done);
    Ok(())
}

fn download(job: &Job) -> Result<(), String> {
    if job.stream {
        return stream_download(job);
    }
    let client = client()?;
    let probe = probe(&client, &job.url, &job.headers)?;
    pimlog::info(format!(
        "Probe '{}': size={} range-support={}",
        job.name,
        probe.len.map(|l| format!("{l} bytes")).unwrap_or_else(|| "unknown".into()),
        probe.ranges
    ));
    let final_path = job.dest.join(&job.name);
    let part_dir = job.dest.join(format!(".{}.rdm", job.name));
    fs::create_dir_all(&part_dir).map_err(|e| e.to_string())?;
    if let Some(len) = probe.len {
        job.total.store(len, Ordering::Relaxed);
    }
    if job.cancel.load(Ordering::SeqCst) {
        job.set_status(Status::Paused);
        return Ok(());
    }
    let total = job.total.load(Ordering::Relaxed);
    let threads = if probe.ranges && total > 1_000_000 { job.connections } else { 1 };
    if threads > 1 {
        let ranges = split_ranges(total, threads);
        let mut existing_total = 0u64;
        for (index, _) in ranges.iter().enumerate() {
            if let Ok(meta) = fs::metadata(part_dir.join(format!("part{index}"))) {
                existing_total = existing_total.saturating_add(meta.len());
            }
        }
        job.done.store(existing_total, Ordering::Relaxed);
        let mut handles = Vec::new();
        for (index, (start, end)) in ranges.into_iter().enumerate() {
            let client = client.clone();
            let url = job.url.clone();
            let headers = job.headers.clone();
            let part = part_dir.join(format!("part{index}"));
            let done = job.done.clone();
            let cancel = job.cancel.clone();
            handles.push(thread::spawn(move || -> Result<(), String> {
                let existing = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                let size = end.saturating_sub(start);
                if existing >= size {
                    return Ok(());
                }
                let from = start + existing;
                let response = with_headers(client.get(&url), &headers)
                    .header(reqwest::header::RANGE, format!("bytes={from}-{}", end - 1))
                    .send()
                    .map_err(|e| e.to_string())?;
                if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    return Err(format!("server ignored range request (HTTP {})", response.status()));
                }
                let mut file = OpenOptions::new().create(true).append(true).open(&part).map_err(|e| e.to_string())?;
                let mut response = response;
                let mut buffer = vec![0u8; CHUNK];
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    let count = response.read(&mut buffer).map_err(|e| e.to_string())?;
                    if count == 0 {
                        break;
                    }
                    file.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
                    done.fetch_add(count as u64, Ordering::Relaxed);
                }
                Ok(())
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err("download thread panicked".into()),
            }
        }
        if job.cancel.load(Ordering::SeqCst) {
            job.set_status(Status::Paused);
            return Ok(());
        }
        let temp = part_dir.join("complete");
        let mut out = File::create(&temp).map_err(|e| e.to_string())?;
        for index in 0..threads {
            let part = part_dir.join(format!("part{index}"));
            let mut input = File::open(&part).map_err(|e| e.to_string())?;
            std::io::copy(&mut input, &mut out).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(part);
        }
        out.flush().map_err(|e| e.to_string())?;
        drop(out);
        if final_path.exists() {
            let _ = fs::remove_file(&final_path);
        }
        fs::rename(&temp, &final_path).map_err(|e| e.to_string())?;
        let _ = fs::remove_dir_all(&part_dir);
        job.done.store(total, Ordering::Relaxed);
        job.set_status(Status::Done);
        return Ok(());
    }
    // Single connection: resumable via a .part file.
    let part = part_dir.join("single.part");
    let existing = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    job.done.store(existing, Ordering::Relaxed);
    let mut request = with_headers(client.get(&job.url), &job.headers);
    if existing > 0 && probe.ranges {
        request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let mut response = request.send().map_err(|e| format!("request failed: {e}"))?;
    let code = response.status();
    if code.is_client_error() || code.is_server_error() {
        pimlog::warn(format!("HTTP {code} for '{}' url={}", job.name, pimlog::short_url(&job.url)));
    } else {
        pimlog::info(format!(
            "HTTP {code} for '{}' (content-length {})",
            job.name,
            response.content_length().map(|l| l.to_string()).unwrap_or_else(|| "unknown".into())
        ));
    }
    if existing > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err("server no longer supports resume".into());
    }
    if let Some(len) = response.content_length() {
        if existing + len > job.total.load(Ordering::Relaxed) {
            job.total.store(existing + len, Ordering::Relaxed);
        }
    }
    let mut file = if existing > 0 {
        OpenOptions::new().create(true).append(true).open(&part).map_err(|e| e.to_string())?
    } else {
        File::create(&part).map_err(|e| e.to_string())?
    };
    let mut buffer = vec![0u8; CHUNK];
    loop {
        if job.cancel.load(Ordering::SeqCst) {
            job.set_status(Status::Paused);
            return Ok(());
        }
        let count = response.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
        job.done.fetch_add(count as u64, Ordering::Relaxed);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if final_path.exists() {
        let _ = fs::remove_file(&final_path);
    }
    fs::rename(&part, &final_path).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&part_dir);
    job.set_status(Status::Done);
    Ok(())
}

fn split_ranges(total: u64, count: usize) -> Vec<(u64, u64)> {
    let mut out = Vec::with_capacity(count);
    let chunk = total / count as u64;
    let mut start = 0;
    for index in 0..count {
        let end = if index + 1 == count { total } else { start + chunk };
        if end > start {
            out.push((start, end));
        }
        start = end;
    }
    out
}

#[derive(Default, Serialize, Deserialize)]
struct Config {
    folder: Option<String>,
    connections: Option<usize>,
    max_active: Option<usize>,
    auto_start: Option<bool>,
    schedule_enabled: Option<bool>,
    schedule_start: Option<String>,
    schedule_stop: Option<String>,
    theme: Option<ThemeKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Active,
    Finished,
    Errors,
    Streams,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Appearance,
    Connection,
    Scheduler,
    Streams,
    Firefox,
    About,
}

struct App {
    jobs: Vec<Job>,
    url: String,
    folder: PathBuf,
    folder_text: String,
    connections: usize,
    max_active: usize,
    auto_start: bool,
    schedule_enabled: bool,
    schedule_start: String,
    schedule_stop: String,
    schedule_active: bool,
    theme_kind: ThemeKind,
    status: String,
    filter: Filter,
    settings_open: bool,
    settings_tab: SettingsTab,
    api_rx: Receiver<Incoming>,
    // Pro Internet Manager restyle: view state only
    selected: Option<usize>,
    themed: Option<ThemeKind>,
    bg_tex: Option<egui::TextureHandle>,
    icon_tex: Option<egui::TextureHandle>,
    // Pro Internet Manager: tray, exit and updates (see extras_ui.rs)
    frames: u64,
    tray_started: bool,
    start_hidden: bool,
    allow_exit: bool,
    exit_now: bool,
    exit_confirm: bool,
    installer_close_prompt: bool,
    close_selftest: bool,
    updates_open: bool,
    update_state: update::Shared,
    update_url_text: String,
    // 0.2.1: remove confirmation (index + name, validated before removing) and the
    // result of a patch update applied before this start (from last_update.json).
    confirm_remove: Option<(usize, String)>,
    update_note: Option<(bool, String)>,
    /// 0.2.3: "A new update is available" prompt (after the startup check).
    update_prompt: bool,
    auto_check_pending: bool,
    /// 0.2.3: "This app has been updated to vX" (shown once).
    updated_prompt: Option<String>,
    // 0.2.2: "Test connection" result (Options > Browser bridge, Help > Updates).
    conn_test: extras_ui::ConnShared,
    conn_test_autostart: bool,
}

struct Incoming {
    url: String,
    headers: Vec<(String, String)>,
    quality: Option<u32>,
}

const API_PORT: u16 = 18500;

/// Port of the local API. Always 18500, except for tests: the hidden environment variable
/// PIM_API_PORT moves it so a test copy can run next to the installed app.
pub(crate) fn api_port() -> u16 {
    static PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| std::env::var("PIM_API_PORT").ok().and_then(|v| v.trim().parse().ok()).filter(|p| *p > 0).unwrap_or(API_PORT))
}

/// 0.2.1: last contact from a browser extension (unix seconds) and which browser sent it
/// (1 = Firefox, 2 = Chromium). Firefox pings every 15 s, Chromium every 30 s.
/// 0.2.5: a /ping or /add from the extension counts (before only a /ping with an
/// extension Origin header counted, which the browsers do not send on a GET).
static LAST_EXT_PING: AtomicU64 = AtomicU64::new(0);
static LAST_EXT_KIND: AtomicU64 = AtomicU64::new(0);
const BRIDGE_TIMEOUT_SECS: u64 = 75;

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `Some((seconds ago, browser))` when an extension pinged within the last 75 s.
fn bridge_status() -> Option<(u64, &'static str)> {
    let last = LAST_EXT_PING.load(Ordering::Relaxed);
    if last == 0 {
        return None;
    }
    let ago = unix_now().saturating_sub(last);
    if ago > BRIDGE_TIMEOUT_SECS {
        return None;
    }
    let kind = if LAST_EXT_KIND.load(Ordering::Relaxed) == 2 { "Chromium" } else { "Firefox" };
    Some((ago, kind))
}

/// 0.2.5: which browser extension sent this request (1 = Firefox, 2 = Chromium, 0 = none).
/// An extension Origin header decides when present. Firefox and Chromium send no Origin on an
/// extension's GET to a host it has permission for, so then a browser User-Agent counts too.
/// The app's own "Test connection" ping and tools like curl send no browser User-Agent.
fn extension_kind(headers: &HashMap<String, String>) -> u64 {
    if let Some(origin) = headers.get("origin") {
        return if origin.starts_with("moz-extension://") {
            1
        } else if origin.starts_with("chrome-extension://") {
            2
        } else {
            0
        };
    }
    // A web page loading the URL as an image, script or frame is not the extension.
    if let Some(dest) = headers.get("sec-fetch-dest") {
        if !dest.eq_ignore_ascii_case("empty") {
            return 0;
        }
    }
    let ua = headers.get("user-agent").map(String::as_str).unwrap_or("");
    if !ua.starts_with("Mozilla/") {
        return 0;
    }
    if ua.contains("Firefox/") {
        1
    } else {
        2
    }
}

/// 0.2.5: records contact from the browser extension (see `bridge_status`).
fn note_extension_contact(kind: u64, what: &str) {
    if kind == 0 {
        return;
    }
    let previous = LAST_EXT_PING.swap(unix_now(), Ordering::Relaxed);
    LAST_EXT_KIND.store(kind, Ordering::Relaxed);
    let browser = if kind == 1 { "Firefox" } else { "Chromium" };
    if previous == 0 {
        pimlog::info(format!("Browser bridge: connected, first {what} from the {browser} extension"));
    } else if unix_now().saturating_sub(previous) > BRIDGE_TIMEOUT_SECS {
        pimlog::info(format!("Browser bridge: connected again ({what} from the {browser} extension)"));
    }
}

/// 0.2.1: result of binding the local API (shown in Options > Browser bridge).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiState {
    Off,
    Listening,
    Failed,
}

static API_STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn api_state() -> ApiState {
    match API_STATE.load(Ordering::Relaxed) {
        1 => ApiState::Listening,
        2 => ApiState::Failed,
        _ => ApiState::Off,
    }
}

fn start_api(tx: Sender<Incoming>) {
    thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", api_port())) {
            Ok(listener) => {
                pimlog::info(format!("Local API listening on 127.0.0.1:{}", api_port()));
                API_STATE.store(1, Ordering::Relaxed);
                listener
            }
            Err(error) => {
                eprintln!("Rust Download Manager API: {error}");
                API_STATE.store(2, Ordering::Relaxed);
                pimlog::error(format!(
                    "Local API could not bind 127.0.0.1:{API_PORT}: {error} (another app, e.g. the other download manager, probably uses the port; the browser extension will talk to that app)"
                ));
                return;
            }
        };
        // Each connection gets its own short-lived thread, so one slow or half-closed browser
        // connection can never hold up /ping or /add for the others (max 32 at a time).
        static ACTIVE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if ACTIVE.load(Ordering::Relaxed) >= 32 {
                pimlog::warn("Local API: too many open connections, one was dropped");
                continue;
            }
            ACTIVE.fetch_add(1, Ordering::Relaxed);
            let tx = tx.clone();
            let spawned = thread::Builder::new().name("pim-api-conn".into()).spawn(move || {
                handle_api(stream, tx);
                ACTIVE.fetch_sub(1, Ordering::Relaxed);
            });
            if spawned.is_err() {
                ACTIVE.fetch_sub(1, Ordering::Relaxed);
            }
        }
    });
}

fn handle_api(mut stream: TcpStream, tx: Sender<Incoming>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let Ok(clone) = stream.try_clone() else { return };
    // Request line + headers are capped at 64 KB.
    let mut reader = BufReader::new(Read::take(clone, 64 * 1024 + 1024 * 1024));
    let mut line = String::new();
    // Fix: read_line returns Ok(0) when the client closed the connection (e.g. an aborted
    // browser fetch). Before, that empty line made the header loop below spin forever at
    // 100% CPU and the single API thread never answered /ping again ("Bridge not connected").
    match reader.read_line(&mut line) {
        Ok(n) if n > 0 => {}
        _ => return,
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    if method.is_empty() || path.is_empty() {
        return;
    }
    let mut headers = HashMap::new();
    let mut header_bytes = 0usize;
    let mut complete = false;
    for _ in 0..200 {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(n) => header_bytes += n,
        }
        if line == "\r\n" || line == "\n" {
            complete = true;
            break;
        }
        if header_bytes > 64 * 1024 {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    if !complete {
        // Client went away (or sent garbage) before the end of the headers: nothing to answer.
        return;
    }
    let origin = headers.get("origin").cloned();
    let allowed = origin
        .as_deref()
        .map(|o| o.starts_with("moz-extension://") || o.starts_with("chrome-extension://"))
        .unwrap_or(true);
    if !allowed {
        pimlog::warn(format!("API {method} {path} refused (403): origin {}", origin.as_deref().unwrap_or("?")));
        respond(&mut stream, 403, "Forbidden", &origin, "text/plain");
        return;
    }
    if method == "OPTIONS" {
        respond(&mut stream, 204, "", &origin, "text/plain");
        return;
    }
    let length = headers
        .get("content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(1024 * 1024);
    let mut body = vec![0u8; length];
    if length > 0 && reader.read_exact(&mut body).is_err() {
        pimlog::warn(format!("API {method} {path}: body could not be read (400)"));
        respond(&mut stream, 400, "Bad Request", &origin, "text/plain");
        return;
    }
    // 0.2.5: the path may carry a query string (e.g. /ping?t=...).
    let path = path.split('?').next().unwrap_or("").to_string();
    if method == "GET" && path == "/ping" {
        note_extension_contact(extension_kind(&headers), "ping");
        // Pro Internet Manager: same "ok" field as before, plus the active theme for the Firefox bridge.
        respond(&mut stream, 200, &theme::ping_json(), &origin, "application/json");
        return;
    }
    if method == "POST" && path == "/add" {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&body) else {
            pimlog::warn("API /add: invalid JSON (400)");
            respond(&mut stream, 400, "{\"ok\":false,\"error\":\"invalid json\"}", &origin, "application/json");
            return;
        };
        let Some(url) = value.get("url").and_then(|v| v.as_str()) else {
            pimlog::warn("API /add: url missing (400)");
            respond(&mut stream, 400, "{\"ok\":false,\"error\":\"url required\"}", &origin, "application/json");
            return;
        };
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            pimlog::warn(format!("API /add: not an http(s) url (400): {}", pimlog::short_url(url)));
            respond(&mut stream, 400, "{\"ok\":false,\"error\":\"http(s) only\"}", &origin, "application/json");
            return;
        }
        let mut forwarded = Vec::new();
        if let Some(map) = value.get("headers").and_then(|v| v.as_object()) {
            for (name, value) in map {
                if let Some(value) = value.as_str() {
                    if !name.contains('\r') && !name.contains('\n') && !value.contains('\r') && !value.contains('\n') {
                        forwarded.push((name.to_string(), value.to_string()));
                    }
                }
            }
        }
        let quality = value
            .get("quality")
            .and_then(|v| v.as_u64())
            .map(|q| (q as u32).clamp(144, 4320));
        pimlog::info(format!(
            "API /add received: url={} name={} quality={} headers={} [{}]",
            pimlog::short_url(url),
            value.get("filename").or_else(|| value.get("name")).and_then(|v| v.as_str()).unwrap_or("-"),
            quality.map(|q| q.to_string()).unwrap_or_else(|| "best".into()),
            forwarded.len(),
            pimlog::header_names(&forwarded)
        ));
        if tx.send(Incoming { url: url.to_string(), headers: forwarded, quality }).is_ok() {
            note_extension_contact(extension_kind(&headers), "download");
            respond(&mut stream, 200, "{\"ok\":true}", &origin, "application/json");
        } else {
            pimlog::error("API /add: could not hand the URL to the app window (500)");
            respond(&mut stream, 500, "{\"ok\":false}", &origin, "application/json");
        }
        return;
    }
    respond(&mut stream, 404, "Not Found", &origin, "text/plain");
}

fn respond(stream: &mut TcpStream, status: u16, body: &str, origin: &Option<String>, content_type: &str) {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let allow = origin
        .as_deref()
        .filter(|o| o.starts_with("moz-extension://") || o.starts_with("chrome-extension://"))
        .unwrap_or("*");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: {allow}\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nVary: Origin\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

impl App {
    fn new() -> Self {
        Self::build(true)
    }

    /// `api = false` is used by the `--demo` preview (no local API, no port 18500).
    fn build(api: bool) -> Self {
        let config = load_config();
        let folder = config
            .folder
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var("USERPROFILE").ok().map(|p| Path::new(&p).join("Downloads")))
            .unwrap_or_else(|| PathBuf::from("."));
        let (api_tx, api_rx) = mpsc::channel();
        if api {
            start_api(api_tx);
        } else {
            drop(api_tx);
        }
        Self {
            jobs: Vec::new(),
            url: String::new(),
            folder_text: folder.display().to_string(),
            folder,
            connections: config.connections.unwrap_or(8).clamp(1, 16),
            max_active: config.max_active.unwrap_or(2).clamp(1, 8),
            auto_start: config.auto_start.unwrap_or(true),
            schedule_enabled: config.schedule_enabled.unwrap_or(false),
            schedule_start: config.schedule_start.unwrap_or_else(|| "01:00".into()),
            schedule_stop: config.schedule_stop.unwrap_or_else(|| "07:00".into()),
            schedule_active: false,
            theme_kind: config.theme.unwrap_or_default(),
            status: String::new(),
            filter: Filter::All,
            settings_open: false,
            settings_tab: SettingsTab::General,
            api_rx,
            selected: None,
            themed: None,
            bg_tex: None,
            icon_tex: None,
            frames: 0,
            tray_started: false,
            start_hidden: false,
            allow_exit: false,
            exit_now: false,
            exit_confirm: false,
            installer_close_prompt: false,
            close_selftest: false,
            updates_open: false,
            update_state: update::new_shared(),
            update_url_text: update::DEFAULT_VERSION_URL.to_string(),
            confirm_remove: None,
            update_note: None,
            update_prompt: false,
            auto_check_pending: false,
            updated_prompt: None,
            conn_test: extras_ui::conn_new_shared(),
            conn_test_autostart: false,
        }
    }

    fn save_config(&self) {
        let config = Config {
            folder: Some(self.folder.display().to_string()),
            connections: Some(self.connections),
            max_active: Some(self.max_active),
            auto_start: Some(self.auto_start),
            schedule_enabled: Some(self.schedule_enabled),
            schedule_start: Some(self.schedule_start.clone()),
            schedule_stop: Some(self.schedule_stop.clone()),
            theme: Some(self.theme_kind),
        };
        if let Ok(path) = config_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(text) = serde_json::to_string_pretty(&config) {
                let _ = fs::write(path, text);
            }
        }
    }

    fn add(&mut self) {
        let url = self.url.trim().to_string();
        if self.add_url(url, Vec::new(), None) {
            self.url.clear();
        }
    }

    fn add_url(&mut self, url: String, headers: Vec<(String, String)>, quality: Option<u32>) -> bool {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            self.status = "Enter a valid http(s) URL.".into();
            pimlog::warn(format!("Add refused: not a valid http(s) URL: {}", pimlog::short_url(&url)));
            return false;
        }
        if !self.folder.is_dir() {
            self.status = "The destination folder does not exist.".into();
            pimlog::error(format!("Add refused: destination folder does not exist: {}", self.folder.display()));
            return false;
        }
        let ytdlp = is_ytdlp_url(&url);
        let stream = is_stream_url(&url) || ytdlp;
        let mut name = if ytdlp { youtube_name(&url) } else { filename_from_url(&url) };
        if stream {
            name = Path::new(&name).with_extension("mp4").display().to_string();
        }
        let target = unique_path(&self.folder, &name);
        let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("download").to_string();
        pimlog::info(format!(
            "Queued '{}' in {} (auto-start {})",
            name,
            self.folder.display(),
            if self.auto_start { "on" } else { "off" }
        ));
        let job = Job::new(url, self.folder.clone(), name, self.connections, headers, stream, ytdlp, quality);
        self.jobs.push(job);
        self.status = if self.auto_start { "Added to queue.".into() } else { "Added; auto-start is off.".into() };
        self.save_config();
        if self.auto_start {
            self.schedule();
        }
        true
    }



    fn apply_schedule(&mut self) {
        if !self.schedule_enabled {
            self.schedule_active = false;
            return;
        }
        let Some(start_minute) = parse_clock(&self.schedule_start) else { return };
        let Some(stop_minute) = parse_clock(&self.schedule_stop) else { return };
        let now = Local::now();
        let minute = now.hour() * 60 + now.minute();
        let active = if start_minute <= stop_minute {
            minute >= start_minute && minute < stop_minute
        } else {
            minute >= start_minute || minute < stop_minute
        };
        if active == self.schedule_active {
            return;
        }
        self.schedule_active = active;
        if active {
            self.status = "Scheduler window opened; starting queue.".into();
            for job in &self.jobs {
                if matches!(job.status(), Status::Queued | Status::Paused) {
                    start(job);
                }
            }
        } else {
            self.status = "Scheduler window closed; downloads paused.".into();
            for job in &self.jobs {
                job.cancel.store(true, Ordering::SeqCst);
                if job.status() == Status::Downloading {
                    job.set_status(Status::Paused);
                }
            }
        }
    }

    fn schedule(&mut self) {
        let active = self.jobs.iter().filter(|j| j.status() == Status::Downloading).count();
        let mut slots = self.max_active.saturating_sub(active);
        for job in &self.jobs {
            if slots == 0 {
                break;
            }
            if job.status() == Status::Queued {
                start(job);
                slots -= 1;
            }
        }
    }

    fn remove(&mut self, index: usize) {
        if let Some(job) = self.jobs.get(index) {
            job.cancel.store(true, Ordering::SeqCst);
            let part_dir = job.dest.join(format!(".{}.rdm", job.name));
            let _ = fs::remove_dir_all(part_dir);
        }
        self.jobs.remove(index);
    }


}

fn parse_clock(value: &str) -> Option<u32> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    if hour < 24 && minute < 60 {
        Some(hour * 60 + minute)
    } else {
        None
    }
}

fn load_config() -> Config {
    config_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| text.trim_start_matches('\u{feff}').to_string())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn config_path() -> Result<PathBuf, ()> {
    // 0.2.3: own copy in %APPDATA%\Pro Internet Downloader (the other download manager's
    // RustDownloadManager\config.json is only read once, for the migration copy).
    Ok(paths::data_file("config.json"))
}



fn main() -> eframe::Result {
    // Pro Internet Manager: `--demo` fills the list with sample rows for previews
    // (no API, no network, no config writes); `--options=<tab>` and `--theme=<preset>` are preview-only too.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    // 0.2.3: own data folder + one-time copy from %APPDATA%\RustDownloadManager (not for the
    // updater run, which belongs to the old version that started it).
    let migration = if args.iter().any(|a| a.starts_with("--apply-update=")) { Vec::new() } else { paths::prepare() };
    pimlog::install_panic_hook();
    for line in &migration {
        pimlog::info(line);
    }
    // 0.2.1 patch updater mode: a temporary copy of the exe applies a downloaded patch
    // (`--apply-update=<zip> --target=<dir> --exe-name=<name> --wait-pid=<pid> --restart`).
    if args.iter().any(|a| a.starts_with("--apply-update=")) {
        std::process::exit(update::run_updater(&args));
    }
    // `--version-probe`: only logs version + exe path and exits (used by the update tests).
    if args.iter().any(|a| a == "--version-probe") {
        pimlog::info(format!(
            "Version probe: Pro Internet Manager {} at {}",
            env!("CARGO_PKG_VERSION"),
            std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default()
        ));
        return Ok(());
    }
    // `--ytdlp-progress-selftest=<url>`: headless; downloads <url> with yt-dlp into a temp
    // folder, logs the progress the UI would show once per second, then deletes the file.
    if let Some(url) = args.iter().find_map(|a| a.strip_prefix("--ytdlp-progress-selftest=")) {
        ytdlp_progress_selftest(url);
        return Ok(());
    }
    // `--updated-selftest` (0.2.3): runs the start-up logic of the one-time
    // "This app has been updated" prompt and prints the result (no window).
    if args.iter().any(|a| a == "--updated-selftest") {
        let last = update::take_last_result();
        let shown = update::take_updated_version(last.as_ref());
        let line = format!("Updated self-test: last result {:?}, prompt {:?}", last.as_ref().map(|(ok, v, _)| (*ok, v.clone())), shown);
        println!("{line}");
        pimlog::info(line);
        if let Some(v) = &shown {
            println!("{}", update::notes_for(v));
        }
        std::process::exit(0);
    }
    // `--ytdlp-update-selftest[=local]`: headless yt-dlp update (`=local` = act as if the
    // install folder is not writable, so the %LOCALAPPDATA% copy route is used).
    if let Some(arg) = args.iter().find(|a| a.starts_with("--ytdlp-update-selftest")) {
        let local = arg.ends_with("=local");
        pimlog::info(format!("yt-dlp update self-test (local copy route: {local})"));
        let result = ytdlp_update::update_now(local);
        let found = find_ytdlp();
        pimlog::info(format!(
            "yt-dlp update self-test result: {:?}; active yt-dlp now {} ({})",
            result,
            found.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            found.as_deref().and_then(|p| ytdlp_update::version_of(p).ok()).unwrap_or_default()
        ));
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|a| a == "--log-selftest") {
        log_selftest();
        return Ok(());
    }
    // `--check-update-selftest[=<version url>]`: headless update check + download test.
    if let Some(arg) = args.iter().find(|a| a.starts_with("--check-update-selftest")) {
        let url = arg.strip_prefix("--check-update-selftest=").map(str::to_string);
        std::process::exit(update::selftest(url));
    }
    if !args.iter().any(|a| a == "--autostart-on" || a == "--autostart-off") {
        pimlog::info("------------------------------------------------------------");
        pimlog::info(format!(
            "Pro Internet Manager {} starting{} (pid {})",
            env!("CARGO_PKG_VERSION"),
            if demo { " in --demo preview mode (no API)" } else { "" },
            std::process::id()
        ));
        pimlog::info(format!(
            "exe: {}",
            std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|e| format!("unknown ({e})"))
        ));
        pimlog::info(format!(
            "yt-dlp: {}",
            find_ytdlp().map(|p| p.display().to_string()).unwrap_or_else(|| "NOT FOUND (YouTube downloads will fail)".into())
        ));
        pimlog::info(format!(
            "ffmpeg: {}",
            find_ffmpeg().map(|p| p.display().to_string()).unwrap_or_else(|| "NOT FOUND (streams and YouTube merging will fail)".into())
        ));
        pimlog::info(format!("log file: {}", pimlog::log_path().display()));
    }
    // Pro Internet Manager: `--autostart-on` / `--autostart-off` only set or clear the
    // "Start with Windows" Run value and exit (same function as the Options checkbox).
    if let Some(flag) = args.iter().find(|a| *a == "--autostart-on" || *a == "--autostart-off") {
        match autostart::set_enabled(flag == "--autostart-on") {
            Ok(()) => return Ok(()),
            Err(error) => {
                pimlog::error(format!("{flag} failed: {error}"));
                std::process::exit(1)
            }
        }
    }
    // `--minimized` (used by the Run value): start minimized, API starts as usual.
    autostart::set_demo(demo);
    autostart::set_start_minimized(!demo && args.iter().any(|a| a == "--minimized"));
    // Single instance (not for --demo previews): a second start shows the running window.
    if !demo && !tray::acquire_single_instance() {
        return Ok(());
    }
    let mut app = if demo { App::build(false) } else { App::new() };
    if !demo {
        let last = update::take_last_result();
        app.updated_prompt = update::take_updated_version(last.as_ref());
        if let Some(version) = &app.updated_prompt {
            pimlog::info(format!("Update: showing \"This app has been updated to v{version}\" once"));
        }
        if let Some((ok, _, text)) = &last {
            app.status = text.clone();
            if *ok {
                pimlog::info(format!("Shown after start: {text}"));
            }
        }
        app.update_note = last.map(|(ok, _, text)| (ok, text));
    }
    app.close_selftest = demo && args.iter().any(|a| a == "--close-selftest");
    if demo {
        app.auto_start = false;
        ui::demo_jobs(&mut app);
        if let Some(tab) = args.iter().find_map(|a| a.strip_prefix("--options=")) {
            app.settings_open = true;
            app.settings_tab = match tab {
                "appearance" => SettingsTab::Appearance,
                "connection" => SettingsTab::Connection,
                "scheduler" => SettingsTab::Scheduler,
                "streams" => SettingsTab::Streams,
                "firefox" => SettingsTab::Firefox,
                "about" => SettingsTab::About,
                _ => SettingsTab::General,
            };
        }
        if let Some(kind) = args.iter().find_map(|a| a.strip_prefix("--theme=")) {
            app.theme_kind = match kind {
                "midnight" => ThemeKind::Midnight,
                "paper" => ThemeKind::Paper,
                "compact" => ThemeKind::Compact,
                "graphite" => ThemeKind::Graphite,
                "azeroth" => ThemeKind::Azeroth,
                _ => ThemeKind::DarkLime,
            };
        }
        // Preview: `--updates` opens Help > Updates, `--update-url=<url>` also runs a check.
        if args.iter().any(|a| a == "--updates") {
            app.updates_open = true;
        }
        if let Some(url) = args.iter().find_map(|a| a.strip_prefix("--update-url=")) {
            app.updates_open = true;
            app.update_url_text = url.to_string();
            update::check_async(&app.update_state, url.to_string(), None);
        }
        // Preview (0.2.3): `--demo-update-prompt` shows "A new update is available",
        // `--demo-updated` shows "This app has been updated to v<this version>".
        if args.iter().any(|a| a == "--demo-update-prompt") {
            update::demo_available(&app.update_state, update::notes_for(update::APP_VERSION));
            app.update_prompt = true;
        }
        if args.iter().any(|a| a == "--demo-updated") {
            app.updated_prompt = Some(update::APP_VERSION.to_string());
        }
        // Preview: `--test-connection` runs "Test connection" once (Options > Browser bridge / Updates).
        app.conn_test_autostart = args.iter().any(|a| a == "--test-connection");
        if args.iter().any(|a| a == "--empty") {
            app.jobs.clear();
            app.selected = None;
            app.status.clear();
        }
        // Preview: `--demo-ytdlp=<url>` adds one real yt-dlp download into %TEMP%\pim-demo-ytdlp
        // (no config write, no API) so the live row and status bar can be checked.
        if let Some(url) = args.iter().find_map(|a| a.strip_prefix("--demo-ytdlp=")) {
            let dest = std::env::temp_dir().join("pim-demo-ytdlp");
            let _ = fs::create_dir_all(&dest);
            let name = youtube_name(url);
            let job = Job::new(url.to_string(), dest, name, app.connections, Vec::new(), true, true, Some(1080));
            start(&job);
            app.jobs.insert(0, job);
            app.selected = None;
        }
    } else {
        // Restyle-only presets are kept in pim_restyle.json (config.json says "DarkLime" for them).
        if app.theme_kind == ThemeKind::DarkLime {
            if let Some(kind) = theme::load_own_theme() {
                app.theme_kind = kind;
            }
        }
        if let Some(url) = args.iter().find(|a| !a.starts_with("--")).cloned() {
            app.url = url;
            app.add();
        }
    }
    // 0.2.2: read the yt-dlp version and (not in --demo) update yt-dlp at most once per 24 h.
    ytdlp_update::startup(demo);
    let theme = theme::get(app.theme_kind);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Pro Internet Manager")
            .with_icon(theme::window_icon_for(app.theme_kind))
            .with_inner_size([theme.layout.window_width, theme.layout.window_height])
            .with_min_inner_size([860.0, 540.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Rust Download Manager",
        options,
        Box::new(|cc| {
            use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
            let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
                _ => 0,
            };
            tray::start(cc.egui_ctx.clone(), hwnd);
            Ok(Box::new(app))
        }),
    );
    tray::shutdown();
    match &result {
        Ok(()) => pimlog::info("Pro Internet Manager closed."),
        Err(error) => pimlog::error(format!("Window could not be created / closed with an error: {error}")),
    }
    result
}

fn ytdlp_progress_selftest(url: &str) {
    pimlog::info(format!("yt-dlp progress self-test start: {}", pimlog::short_url(url)));
    let dest = std::env::temp_dir().join("pim-ytdlp-selftest");
    let _ = fs::create_dir_all(&dest);
    let job = Job::new(url.to_string(), dest.clone(), youtube_name(url), 1, Vec::new(), true, true, Some(720));
    start(&job);
    let started = Instant::now();
    let mut seen = std::collections::BTreeSet::new();
    while matches!(job.status(), Status::Downloading | Status::Queued) && started.elapsed() < Duration::from_secs(600) {
        thread::sleep(Duration::from_millis(1000));
        let speed = job.speed();
        let phase = job.phase();
        seen.insert(phase.clone());
        pimlog::info(format!(
            "self-test sample: phase='{}' size='{}' progress={:.0}% speed={} eta='{}'",
            phase,
            job.display_size(),
            job.progress() * 100.0,
            if speed > 0.0 { format!("{}/s", human(speed as u64)) } else { "-".into() },
            job.eta()
        ));
    }
    pimlog::info(format!(
        "yt-dlp progress self-test end: status {} size='{}' phases seen: {:?}",
        job.status().label(),
        job.display_size(),
        seen
    ));
    let _ = fs::remove_dir_all(&dest);
}

/// `--log-selftest`: runs the real yt-dlp path once on an invalid YouTube URL with fake
/// cookie / Authorization headers, so the log shows an ERROR line and no secret values.
/// No window, no API, nothing is downloaded.
fn log_selftest() {
    pimlog::info("Log self-test start (--log-selftest)");
    let dest = std::env::temp_dir().join("pim-log-selftest");
    let _ = fs::create_dir_all(&dest);
    let headers = vec![
        ("Cookie".to_string(), "SID=SELFTESTSECRETCOOKIE111; HSID=SELFTESTSECRETCOOKIE222".to_string()),
        ("Authorization".to_string(), "Bearer SELFTESTSECRETTOKEN333".to_string()),
        ("Referer".to_string(), "https://www.youtube.com/".to_string()),
    ];
    let job = Job::new(
        "https://www.youtube.com/watch?v=xx_invalid_selftest_xx&token=SELFTESTSECRETTOKEN444".to_string(),
        dest.clone(),
        "youtube-selftest.mp4".to_string(),
        1,
        headers,
        true,
        true,
        Some(720),
    );
    start(&job);
    let started = Instant::now();
    while matches!(job.status(), Status::Downloading | Status::Queued) && started.elapsed() < Duration::from_secs(120) {
        thread::sleep(Duration::from_millis(200));
    }
    thread::sleep(Duration::from_millis(300));
    pimlog::info(format!("Log self-test end: status {}", job.status().label()));
    let _ = fs::remove_dir(&dest);
}

