//! Pro Internet Manager: small std-only file logger.
//! Writes to %APPDATA%\Pro Internet Downloader\logs\pim.log (0.2.3; before: RustDownloadManager), rotates to pim.log.1 above 2 MB.
//! Never log cookie / Authorization values: use `scrub`, `scrub_with` and `header_names`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const MAX_BYTES: u64 = 2 * 1024 * 1024;

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn log_dir() -> PathBuf {
    crate::paths::data_dir().join("logs")
}

pub fn log_path() -> PathBuf {
    log_dir().join("pim.log")
}

fn write(level: &str, message: &str) {
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    let dir = log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("pim.log");
    if fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let old = dir.join("pim.log.1");
        let _ = fs::remove_file(&old);
        let _ = fs::rename(&path, &old);
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let mut text = String::new();
    // Multi-line messages: indent continuation lines so every entry stays readable.
    for (index, line) in message.trim_end().lines().enumerate() {
        if index == 0 {
            text.push_str(&format!("{stamp} {level:<5} {line}\r\n"));
        } else {
            text.push_str(&format!("                          | {line}\r\n"));
        }
    }
    if text.is_empty() {
        text = format!("{stamp} {level:<5}\r\n");
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(text.as_bytes());
    }
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn warn(message: impl AsRef<str>) {
    write("WARN", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write("ERROR", message.as_ref());
}

/// Make sure the log file exists (used by the "Open log" button).
pub fn ensure_exists() -> PathBuf {
    let path = log_path();
    if !path.exists() {
        info("Log file created.");
    }
    path
}

/// Header names only, never values.
pub fn header_names(headers: &[(String, String)]) -> String {
    if headers.is_empty() {
        return "none".into();
    }
    headers.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")
}

fn is_secret_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["cookie", "auth", "token", "secret", "key", "session", "csrf"]
        .iter()
        .any(|k| name.contains(k))
}

/// Values of sensitive headers (whole value plus each `name=value` part) for `scrub_with`.
pub fn secrets_of(headers: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, value) in headers {
        if !is_secret_header(name) {
            continue;
        }
        if value.len() >= 4 {
            out.push(value.clone());
        }
        for part in value.split(';') {
            let part = part.trim();
            let v = part.split_once('=').map(|(_, v)| v.trim()).unwrap_or(part);
            if v.len() >= 4 {
                out.push(v.to_string());
            }
            if let Some(token) = part.strip_prefix("Bearer ").or_else(|| part.strip_prefix("bearer ")) {
                if token.len() >= 4 {
                    out.push(token.to_string());
                }
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    out.dedup();
    out
}

/// `scrub` plus literal replacement of known secret values.
pub fn scrub_with(text: &str, secrets: &[String]) -> String {
    let mut text = text.to_string();
    for secret in secrets {
        if !secret.is_empty() && text.contains(secret.as_str()) {
            text = text.replace(secret.as_str(), "***");
        }
    }
    scrub(&text)
}

const HEADER_KEYS: [&str; 4] = ["set-cookie:", "cookie:", "proxy-authorization:", "authorization:"];
const PARAM_KEYS: [&str; 16] = [
    "access_token=", "id_token=", "refresh_token=", "token=", "signature=", "lsig=", "sig=",
    "key=", "auth=", "password=", "passwd=", "secret=", "sid=", "session=", "bearer ", "cookie=",
];

/// Remove cookie / authorization header values and token-like query parameters.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        out.push_str(&scrub_line(line));
    }
    out
}

fn scrub_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for key in HEADER_KEYS {
        if let Some(pos) = lower.find(key) {
            let end = pos + key.len();
            let newline = if line.ends_with('\n') { "\n" } else { "" };
            return format!("{} ***{newline}", &line[..end]);
        }
    }
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    'outer: while i < line.len() {
        for key in PARAM_KEYS {
            let boundary = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            if boundary && lower[i..].starts_with(key) {
                out.push_str(&line[i..i + key.len()]);
                let mut j = i + key.len();
                while j < line.len() && !matches!(bytes[j], b'&' | b' ' | b'"' | b'\'' | b';' | b',' | b'\n' | b'\r' | b'\t') {
                    j += 1;
                }
                if j > i + key.len() {
                    out.push_str("***");
                }
                i = j;
                continue 'outer;
            }
        }
        let ch = line[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8().max(1);
    }
    out
}

/// Last `max` characters of `text`.
pub fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        let tail: String = text.chars().skip(count - max).collect();
        format!("...{tail}")
    }
}

/// URL for the log: token-like parameters scrubbed, kept short.
pub fn short_url(url: &str) -> String {
    let clean = scrub(url);
    if clean.chars().count() > 240 {
        let head: String = clean.chars().take(240).collect();
        format!("{head}...")
    } else {
        clean
    }
}

/// Quote an argument for the logged command line.
pub fn quote(arg: &str) -> String {
    if arg.is_empty() || arg.contains([' ', '\t', '"']) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

/// Log panics (message + location) before the default hook runs.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".into());
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        error(format!("PANIC in thread '{thread}' at {location}: {}", scrub(&message)));
        default(info);
    }));
}
