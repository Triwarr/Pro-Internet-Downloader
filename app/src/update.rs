//! Pro Internet Manager: update checker and downloader (Help > Check for updates).
//!
//! - The current version is CARGO_PKG_VERSION (Cargo.toml).
//! - 0.2.3: the version file is always read from DEFAULT_VERSION_URL (link to version.txt);
//!   the update itself always comes from DOWNLOAD_URL (link to the public download
//!   folder; the file `ProInternetManager-<version>-patch.zip`, or `file=` from version.txt, is
//!   looked up in it). Neither link (nor any Drive link) is shown in the UI or written to pim.log.
//! - The version file is `key=value` lines (or a JSON object) with version=, type=, notes=,
//!   optional sha256= / size= / file= and patchnotes= (Drive id of patchnotes.txt). A download=
//!   line of older version files is ignored.
//! - "Download update" saves the file to %LOCALAPPDATA%\ProInternetManager\updates\
//!   (handles Google's "can't scan for viruses" confirmation page), checks size and sha256.
//! - 0.2.1: `type=patch` updates are zip files with a `pim-patch.json` manifest. "Apply update"
//!   starts a temporary copy of the exe as updater (`--apply-update=<zip> --target=<dir> ...`),
//!   which waits for the app to exit, backs up the replaced files to `<dir>\_backup_<old>\`,
//!   copies the patch files, verifies them, rolls back on any error and restarts the app.
//!   A non-writable install folder (Program Files) asks for UAC; never silently elevated.
//! - Nothing is installed without the user clicking "Apply update"; no notifications are shown.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Version file (version.txt). Never shown or logged.
/// The published source contains placeholders: set PIM_VERSION_URL at build time for a real update source.
pub const DEFAULT_VERSION_URL: &str = match option_env!("PIM_VERSION_URL") { Some(u) => u, None => "https://example.invalid/version.txt" };
/// Second source for the same version.txt, used when the first one fails (empty = none). Never shown or logged.
/// Set PIM_FALLBACK_VERSION_URL at build time.
pub const FALLBACK_VERSION_URL: &str = match option_env!("PIM_FALLBACK_VERSION_URL") { Some(u) => u, None => "" };
/// Download source: a link to the public download folder or file. Never shown or logged.
/// Set PIM_DOWNLOAD_URL at build time.
const DOWNLOAD_URL: &str = match option_env!("PIM_DOWNLOAD_URL") { Some(u) => u, None => "https://example.invalid/download/" };
/// Generic Google Drive hosts (no file ids), used to follow Drive share links.
const DRIVE_HOST: &str = concat!("drive", ".google.com");
const DOCS_HOST: &str = concat!("docs", ".google.com");
const DRIVE_CONTENT_HOST: &str = concat!("drive", ".usercontent.google.com");
/// Generic link-shortener host (short links are followed like any redirect).
const SHORT_LINK_HOST: &str = concat!("tiny", "url.com");
/// Name of the manifest inside a patch zip.
pub const PATCH_MANIFEST: &str = "pim-patch.json";
/// Patch notes of all versions, built in (shown once after an update).
pub const PATCH_NOTES: &str = include_str!("../patchnotes.txt");

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct UpdateConfig {
    #[serde(default)]
    pub version_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<u64>,
    /// 0.2.3: last version that ran (for the one-time "This app has been updated" prompt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    /// `file=`: file name of the update in the download folder (optional).
    pub file: String,
    /// `patchnotes=`: Drive id (or link) of patchnotes.txt (optional).
    pub patchnotes: String,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    /// `type=` in the version file: "patch" (zip applied by the built-in updater), "installer" or "".
    pub kind: String,
}

impl UpdateInfo {
    pub fn is_patch(&self) -> bool {
        self.kind.eq_ignore_ascii_case("patch")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    NotConfigured,
    Checking,
    UpToDate(String),
    Available(UpdateInfo),
    CheckFailed(String),
    Downloading { info: UpdateInfo, done: u64, total: Option<u64> },
    Downloaded { info: UpdateInfo, path: PathBuf },
    DownloadFailed { info: UpdateInfo, error: String },
}

pub type Shared = Arc<Mutex<Phase>>;

pub fn new_shared() -> Shared {
    Arc::new(Mutex::new(Phase::Idle))
}

pub fn get(shared: &Shared) -> Phase {
    shared.lock().map(|g| g.clone()).unwrap_or(Phase::Idle)
}

fn set(shared: &Shared, phase: Phase) {
    if let Ok(mut g) = shared.lock() {
        *g = phase;
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn config_path() -> PathBuf {
    crate::paths::data_file("update.json")
}

pub fn load_config() -> UpdateConfig {
    let mut config: UpdateConfig = fs::read_to_string(config_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    // 0.2.3: the version file always comes from the built-in short link (an older update.json
    // may still hold another URL; it is ignored).
    config.version_url = DEFAULT_VERSION_URL.to_string();
    config
}

pub fn save_config(config: &UpdateConfig) {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(config) {
        Ok(text) => {
            if let Err(error) = fs::write(&path, text) {
                crate::pimlog::error(format!("Update: could not write {}: {error}", path.display()));
            }
        }
        Err(error) => crate::pimlog::error(format!("Update: could not serialise update.json: {error}")),
    }
}

pub fn updates_dir() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("ProInternetManager")
        .join("updates")
}

fn is_drive_id(text: &str) -> bool {
    text.len() >= 20 && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Google Drive file id from a /file/d/<id>/..., ?id=<id> or bare id.
pub fn drive_file_id(input: &str) -> Option<String> {
    let text = input.trim();
    if is_drive_id(text) {
        return Some(text.to_string());
    }
    if !text.contains(DRIVE_HOST) && !text.contains(DOCS_HOST) && !text.contains(DRIVE_CONTENT_HOST) {
        return None;
    }
    if let Some(pos) = text.find("/file/d/") {
        let id: String = text[pos + 8..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    let query = text.split_once('?').map(|(_, q)| q).unwrap_or("");
    for pair in query.split('&') {
        if let Some(id) = pair.strip_prefix("id=") {
            let id: String = id.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

/// Turn Google Drive file links (and bare file ids) into a direct download URL; others unchanged.
pub fn normalize_url(input: &str) -> String {
    let text = input.trim();
    if text.contains("/drive/folders/") {
        return text.to_string();
    }
    match drive_file_id(text) {
        Some(id) => format!("https://{DRIVE_HOST}/uc?export=download&id={id}"),
        None => text.to_string(),
    }
}

pub fn parse_version_file(text: &str) -> Result<UpdateInfo, String> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.starts_with('<') {
        return Err("The update server returned a web page instead of the version file.".into());
    }
    let mut info = UpdateInfo::default();
    if text.starts_with('{') {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("Version file is not valid JSON: {e}"))?;
        let s = |k: &str| value.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        info.version = s("version");
        info.notes = s("notes");
        info.file = s("file");
        info.patchnotes = s("patchnotes");
        info.sha256 = Some(s("sha256")).filter(|v| !v.is_empty());
        info.size = value.get("size").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())));
        info.kind = s("type");
    } else {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim().to_string();
            match key.trim().to_ascii_lowercase().as_str() {
                "version" => info.version = value,
                "file" => info.file = value,
                "patchnotes" => info.patchnotes = value,
                "notes" => {
                    if !info.notes.is_empty() {
                        info.notes.push('\n');
                    }
                    info.notes.push_str(&value);
                }
                "sha256" => info.sha256 = Some(value.to_ascii_lowercase()).filter(|v| !v.is_empty()),
                "size" => info.size = value.parse().ok(),
                "type" => info.kind = value.to_ascii_lowercase(),
                _ => {}
            }
        }
    }
    if parse_semver(&info.version).is_none() {
        return Err(format!("Version file has no valid version= line (got \"{}\").", info.version));
    }
    if let Some(hash) = &info.sha256 {
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Version file has an invalid sha256= value (expected 64 hex characters).".into());
        }
    }
    Ok(info)
}

pub fn parse_semver(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let core = text.split(['-', '+', ' ']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0))?;
    let patch = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_semver(remote), parse_semver(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("ProInternetManager/{APP_VERSION}"))
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

fn readable(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "The update server did not answer in time (timeout).".into()
    } else if error.is_connect() {
        "Could not connect to the update server (offline or address unreachable).".into()
    } else {
        // reqwest's text contains the URL ("... for url (https://...)"): never show or log it.
        format!("Network error: {}", strip_urls(&error.to_string()))
    }
}

/// Removes anything that looks like a link from a message (0.2.3: no URLs in UI or log).
pub fn strip_urls(text: &str) -> String {
    let mut out = String::new();
    for word in text.split(' ') {
        let bare = word.trim_start_matches(['(', '"', '\'', '<']);
        let lower = bare.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.") || lower.contains(SHORT_LINK_HOST) || lower.contains(DRIVE_HOST) || lower.contains("googleusercontent.com") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.replace(" for url ()", "").replace(" for url", "").trim().to_string()
}

/// Fetch and parse the version file. Blocking. Follows redirects (short links, Drive's uc -> usercontent hop), Drive file pages, the Drive "virus scan" page and
/// Drive folder pages (version.txt is looked up in the folder). When the short link does
/// not give a version file, the built-in direct Drive link is tried. Nothing logs a URL.
pub fn fetch(version_url: &str) -> Result<UpdateInfo, String> {
    let url = normalize_url(version_url);
    if url.is_empty() {
        return Err("Update source not configured.".into());
    }
    crate::pimlog::info("Update check: reading version file");
    match fetch_from(&url, 0) {
        Ok(info) => Ok(info),
        Err(error) if url != FALLBACK_VERSION_URL && !FALLBACK_VERSION_URL.is_empty() => {
            crate::pimlog::warn(format!("Update check: {error} Trying the second update source."));
            fetch_from(FALLBACK_VERSION_URL, 0).map_err(|second| format!("{error} (second source: {second})"))
        }
        Err(error) => Err(error),
    }
}

/// Section of `text` (patchnotes.txt) for `version`: the bullet lines below its heading
/// ("Version 0.2.3", "v0.2.3" or "0.2.3 ..."), without the heading.
pub fn notes_section(text: &str, version: &str) -> Option<String> {
    let want = parse_semver(version)?;
    let heading = |line: &str| -> Option<(u64, u64, u64)> {
        let l = line.trim();
        if l.starts_with('-') || l.starts_with('*') || l.starts_with('\u{2022}') {
            return None;
        }
        let l = l.strip_prefix("Version ").or_else(|| l.strip_prefix("version ")).unwrap_or(l);
        let first = l.split_whitespace().next()?;
        if !first.trim_start_matches(['v', 'V']).starts_with(|c: char| c.is_ascii_digit()) || !first.contains('.') {
            return None;
        }
        parse_semver(first.trim_end_matches([':', ',']))
    };
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some(v) = heading(line) {
            if inside {
                break;
            }
            inside = v == want;
            continue;
        }
        if inside && !line.trim().is_empty() && !line.trim_start().starts_with('=') {
            out.push(line.trim_end());
        }
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

/// Built-in patch notes of `version` (for the "This app has been updated" prompt).
pub fn notes_for(version: &str) -> String {
    notes_section(PATCH_NOTES, version).unwrap_or_else(|| "- Bug fixes and improvements.".to_string())
}

/// patchnotes.txt from Drive (`patchnotes=` in version.txt), section of `version`.
fn fetch_patch_notes(reference: &str, version: &str) -> Option<String> {
    let mut url = normalize_url(reference);
    if !url.starts_with("https://") {
        return None;
    }
    for _ in 0..3 {
        let response = client(Duration::from_secs(20)).ok()?.get(&url).send().ok()?;
        if !response.status().is_success() {
            return None;
        }
        let final_url = response.url().to_string();
        let mut body = String::new();
        response.take(512 * 1024).read_to_string(&mut body).ok()?;
        if body.trim_start().starts_with('<') {
            let id = drive_file_id(&final_url).or_else(|| drive_file_id(reference));
            url = drive_confirm_url(&body, id.as_deref())?;
            continue;
        }
        return notes_section(&body, version);
    }
    None
}

/// 0.2.2 ("Test connection"): like `fetch`, but without the direct Drive fallback, so the
/// configured URL itself is tested.
pub fn fetch_direct(version_url: &str) -> Result<UpdateInfo, String> {
    let url = normalize_url(version_url);
    if url.is_empty() {
        return Err("no update URL configured".into());
    }
    fetch_from(&url, 0)
}

/// Id of a file called `name` on a public Google Drive folder page.
pub fn drive_folder_file_id(html: &str, name: &str) -> Option<String> {
    let tooltip = format!("data-tooltip=\"{name}");
    if let Some(pos) = html.find(&tooltip) {
        let before = &html[pos.saturating_sub(600)..pos];
        if let Some(start) = before.rfind("data-id=\"") {
            let id: String = before[start + 9..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
            if is_drive_id(&id) {
                return Some(id);
            }
        }
    }
    // Escaped data block: \x22<id>\x22,\x5b\x22<folder>\x22\x5d,\x22version.txt\x22
    let marker = format!("\\x22{name}\\x22");
    if let Some(pos) = html.find(&marker) {
        let before = &html[pos.saturating_sub(300)..pos];
        let ids: Vec<&str> = before.split("\\x22").filter(|p| is_drive_id(p)).collect();
        // last id before the name is the parent folder, the one before it the file
        if ids.len() >= 2 {
            return Some(ids[ids.len() - 2].to_string());
        }
    }
    None
}

fn fetch_from(url: &str, depth: u32) -> Result<UpdateInfo, String> {
    if depth > 3 {
        return Err("Too many hops while looking for the version file.".into());
    }
    let response = client(Duration::from_secs(20))?.get(url).send().map_err(|e| readable(&e))?;
    let status = response.status();
    let final_url = response.url().to_string();
    if !status.is_success() {
        return Err(format!("The update server answered HTTP {status}."));
    }
    let mut body = String::new();
    response
        .take(4 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("Could not read the version file: {e}"))?;
    let trimmed = body.trim_start_matches('\u{feff}').trim_start();
    if trimmed.starts_with('<') {
        if final_url.contains("/drive/folders/") {
            return match drive_folder_file_id(&body, "version.txt") {
                Some(id) => fetch_from(&format!("https://{DRIVE_HOST}/uc?export=download&id={id}"), depth + 1),
                None => Err("The update folder has no (public) version.txt in it.".into()),
            };
        }
        let id = drive_file_id(&final_url);
        if let Some(id) = &id {
            if !final_url.contains("export=download") {
                return fetch_from(&format!("https://{DRIVE_HOST}/uc?export=download&id={id}"), depth + 1);
            }
            if body.contains("<form") || body.contains("confirm=") {
                if let Some(confirm) = drive_confirm_url(&body, Some(id)) {
                    return fetch_from(&confirm, depth + 1);
                }
            }
        }
    }
    parse_version_file(&body)
}

/// Background check; updates `shared`. `auto` = startup check (only logs, no "not configured" UI).
pub fn check_async(shared: &Shared, version_url: String, ctx: Option<egui::Context>) {
    if version_url.trim().is_empty() {
        crate::pimlog::warn("Update check: update source not configured.");
        set(shared, Phase::NotConfigured);
        return;
    }
    set(shared, Phase::Checking);
    let shared = shared.clone();
    thread::spawn(move || {
        let phase = check_now(&version_url);
        set(&shared, phase);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
}

/// Blocking check, also records last_check in update.json.
pub fn check_now(version_url: &str) -> Phase {
    let result = fetch(version_url);
    let mut config = load_config();
    config.last_check = Some(now_secs());
    save_config(&config);
    match result {
        Ok(mut info) if is_newer(&info.version, APP_VERSION) => {
            crate::pimlog::info(format!("Update check: update available {} (current {APP_VERSION})", info.version));
            if !info.patchnotes.trim().is_empty() {
                match fetch_patch_notes(&info.patchnotes, &info.version) {
                    Some(notes) => {
                        crate::pimlog::info(format!("Update check: patch notes for {} read", info.version));
                        info.notes = notes;
                    }
                    None => crate::pimlog::warn(format!("Update check: no patch notes for {} found (notes= is used)", info.version)),
                }
            }
            Phase::Available(info)
        }
        Ok(info) => {
            crate::pimlog::info(format!("Update check: up to date (current {APP_VERSION}, latest {})", info.version));
            Phase::UpToDate(info.version)
        }
        Err(error) => {
            crate::pimlog::error(format!("Update check failed: {error}"));
            Phase::CheckFailed(error)
        }
    }
}

/// Preview only (`--demo-update-prompt`): pretend a newer version was found.
pub fn demo_available(shared: &Shared, notes: String) {
    let (a, b, c) = parse_semver(APP_VERSION).unwrap_or((0, 0, 0));
    set(shared, Phase::Available(UpdateInfo { version: format!("{a}.{b}.{}", c + 1), notes, kind: "patch".into(), ..Default::default() }));
}

/// 0.2.3: one check at every app start (the result shows the in-app "A new update is available" prompt).
pub fn auto_check(shared: &Shared, ctx: egui::Context) {
    crate::pimlog::info("Update check: automatic check at startup.");
    check_async(shared, DEFAULT_VERSION_URL.to_string(), Some(ctx));
}

/// 0.2.3: the version to announce once with "This app has been updated to vX" (and records the
/// current version). Some when the last patch run succeeded for this version or when an older
/// version ran before; None on a fresh install or when already shown.
pub fn take_updated_version(last_result: Option<&(bool, String, String)>) -> Option<String> {
    let mut config: UpdateConfig = fs::read_to_string(config_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let by_result = last_result.is_some_and(|(ok, version, _)| *ok && (version.is_empty() || version.trim_start_matches('v') == APP_VERSION));
    let by_seen = config.last_seen_version.as_deref().is_some_and(|seen| is_newer(APP_VERSION, seen));
    if config.last_seen_version.as_deref() != Some(APP_VERSION) {
        config.last_seen_version = Some(APP_VERSION.to_string());
        if config.version_url.trim().is_empty() {
            config.version_url = DEFAULT_VERSION_URL.to_string();
        }
        save_config(&config);
    }
    (by_result || by_seen).then(|| APP_VERSION.to_string())
}

fn html_attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let key = format!("{name}={quote}");
        if let Some(pos) = tag.find(&key) {
            let rest = &tag[pos + key.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].replace("&amp;", "&"));
            }
        }
    }
    None
}

fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Google Drive "can't scan this file for viruses" page: build the confirmed download URL.
pub fn drive_confirm_url(html: &str, id: Option<&str>) -> Option<String> {
    if let Some(start) = html.find("<form") {
        let form = &html[start..];
        let end = form.find("</form>").unwrap_or(form.len());
        let form = &form[..end];
        let tag_end = form.find('>').unwrap_or(form.len());
        if let Some(action) = html_attr(&form[..tag_end], "action") {
            let mut params = Vec::new();
            let mut rest = form;
            while let Some(pos) = rest.find("<input") {
                let tail = &rest[pos..];
                let close = tail.find('>').unwrap_or(tail.len());
                let tag = &tail[..close];
                if let (Some(name), Some(value)) = (html_attr(tag, "name"), html_attr(tag, "value")) {
                    params.push(format!("{}={}", url_encode(&name), url_encode(&value)));
                }
                rest = &tail[close.min(tail.len())..];
                if close == 0 {
                    break;
                }
            }
            if !params.is_empty() {
                let sep = if action.contains('?') { '&' } else { '?' };
                return Some(format!("{action}{sep}{}", params.join("&")));
            }
        }
    }
    if let Some(pos) = html.find("confirm=") {
        let token: String = html[pos + 8..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if let (false, Some(id)) = (token.is_empty(), id) {
            return Some(format!("https://{DRIVE_HOST}/uc?export=download&confirm={token}&id={id}"));
        }
    }
    id.map(|id| format!("https://{DRIVE_CONTENT_HOST}/download?id={id}&export=download&confirm=t"))
}

fn filename_from_disposition(value: &str) -> Option<String> {
    if let Some(pos) = value.find("filename*=") {
        let raw = value[pos + 10..].split(';').next().unwrap_or("").trim();
        let raw = raw.rsplit("''").next().unwrap_or(raw).trim_matches('"');
        let decoded = crate::percent_decode(raw);
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    if let Some(pos) = value.find("filename=") {
        let raw = value[pos + 9..].split(';').next().unwrap_or("").trim().trim_matches('"');
        if !raw.is_empty() {
            return Some(raw.to_string());
        }
    }
    None
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() { "update.bin".into() } else { cleaned.chars().take(120).collect() }
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let head = head.trim_start();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}

/// File names looked up in the download folder for `info`.
fn wanted_names(info: &UpdateInfo) -> Vec<String> {
    let v = info.version.trim().trim_start_matches(['v', 'V']);
    let mut names = Vec::new();
    if !info.file.trim().is_empty() {
        names.push(info.file.trim().to_string());
    }
    if info.is_patch() {
        names.push(format!("ProInternetManager-{v}-patch.zip"));
    } else {
        names.push(format!("ProInternetManager-{v}-setup.exe"));
        names.push(format!("Pro Internet Manager Setup {v}.exe"));
        names.push(format!("ProInternetManager-{v}-full.zip"));
    }
    names
}

/// Opens the update file behind DOWNLOAD_URL: follows redirects, finds the file in the public
/// Drive folder, opens Drive file pages and passes the "can't scan for viruses" page.
fn open_download(client: &reqwest::blocking::Client, info: &UpdateInfo) -> Result<reqwest::blocking::Response, String> {
    let is_html = |r: &reqwest::blocking::Response| {
        r.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("text/html"))
    };
    let mut url = DOWNLOAD_URL.to_string();
    let mut file_id: Option<String> = None;
    for _ in 0..5 {
        let mut response = client.get(&url).send().map_err(|e| readable(&e))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("The update server answered HTTP {status}."));
        }
        if !is_html(&response) {
            return Ok(response);
        }
        let final_url = response.url().to_string();
        let mut html = String::new();
        let _ = (&mut response).take(2 * 1024 * 1024).read_to_string(&mut html);
        if final_url.contains("/drive/folders/") || final_url.contains("/embeddedfolderview") {
            let names = wanted_names(info);
            let Some(id) = names.iter().find_map(|name| drive_folder_file_id(&html, name)) else {
                return Err(format!("The update {} was not found in the download folder (expected {}).", info.version, names[0]));
            };
            file_id = Some(id.clone());
            url = format!("https://{DRIVE_HOST}/uc?export=download&id={id}");
            continue;
        }
        let id = drive_file_id(&final_url).or_else(|| file_id.clone());
        if let Some(id) = &id {
            if !final_url.contains("export=download") && !final_url.contains("/download") {
                url = format!("https://{DRIVE_HOST}/uc?export=download&id={id}");
                file_id = Some(id.clone());
                continue;
            }
        }
        match drive_confirm_url(&html, id.as_deref()) {
            Some(confirm) if confirm != url => {
                url = confirm;
                continue;
            }
            _ => return Err("The download server returned a web page instead of the update (not shared publicly, or download quota exceeded). Try again later.".into()),
        }
    }
    Err("Too many hops while looking for the update file.".into())
}

/// Blocking download of the update for `info` into `dir`. Progress via `progress(done, total)`.
pub fn download_to(info: &UpdateInfo, dir: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<PathBuf, String> {
    let client = client(Duration::from_secs(60 * 60))?;
    crate::pimlog::info("Update download: downloading update");
    let mut response = open_download(&client, info)?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("The update server answered HTTP {status}."));
    }
    let total = response.content_length().filter(|n| *n > 0);
    let name = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(filename_from_disposition)
        .or_else(|| wanted_names(info).into_iter().find(|n| n.contains(&info.version)))
        .unwrap_or_else(|| format!("ProInternetManager-update-{}.bin", info.version));
    let name = sanitize(&name);
    fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let final_path = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    let mut file = fs::File::create(&part).map_err(|e| format!("Could not create {}: {e}", part.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut first: Vec<u8> = Vec::new();
    loop {
        let n = response.read(&mut buffer).map_err(|e| {
            let _ = fs::remove_file(&part);
            format!("Download interrupted: {}", strip_urls(&e.to_string()))
        })?;
        if n == 0 {
            break;
        }
        if first.len() < 512 {
            first.extend_from_slice(&buffer[..n.min(512 - first.len())]);
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(|e| format!("Could not write the update file: {e}"))?;
        done += n as u64;
        progress(done, total);
    }
    drop(file);
    let fail = |message: String| {
        let _ = fs::remove_file(&part);
        Err(message)
    };
    if looks_like_html(&first) {
        return fail("The downloaded file is a web page, not the update. Check the Drive sharing settings.".into());
    }
    if let Some(total) = total {
        if done != total {
            return fail(format!("Download incomplete: {done} of {total} bytes."));
        }
    }
    if let Some(size) = info.size {
        if done != size {
            return fail(format!("Size mismatch: got {done} bytes, version file says {size}."));
        }
    }
    let digest = hex(&hasher.finish());
    match &info.sha256 {
        Some(expected) if !expected.eq_ignore_ascii_case(&digest) => {
            crate::pimlog::error(format!("Update download: sha256 mismatch (expected {expected}, got {digest})"));
            return fail("Checksum (sha256) mismatch: the downloaded file is not the expected update. It was deleted.".into());
        }
        Some(_) => crate::pimlog::info(format!("Update download: sha256 OK ({digest})")),
        None => crate::pimlog::info(format!("Update download: sha256 {digest} (no sha256= in version file, not verified)")),
    }
    let _ = fs::remove_file(&final_path);
    fs::rename(&part, &final_path).map_err(|e| format!("Could not rename the downloaded file: {e}"))?;
    crate::pimlog::info(format!("Update download: saved {name} ({done} bytes) in the updates folder"));
    Ok(final_path)
}

pub fn download_async(shared: &Shared, info: UpdateInfo, ctx: egui::Context) {
    set(shared, Phase::Downloading { info: info.clone(), done: 0, total: info.size });
    let shared = shared.clone();
    thread::spawn(move || {
        let mut last = std::time::Instant::now();
        let result = download_to(&info, &updates_dir(), |done, total| {
            if last.elapsed() >= Duration::from_millis(150) {
                last = std::time::Instant::now();
                set(&shared, Phase::Downloading { info: info.clone(), done, total });
                ctx.request_repaint();
            }
        });
        let result = result.and_then(|path| {
            if info.is_patch() {
                verify_patch(&path).map_err(|e| {
                    let _ = fs::remove_file(&path);
                    format!("The downloaded patch is not valid: {e}")
                })?;
            }
            Ok(path)
        });
        match result {
            Ok(path) => set(&shared, Phase::Downloaded { info, path }),
            Err(error) => {
                crate::pimlog::error(format!("Update download failed: {error}"));
                set(&shared, Phase::DownloadFailed { info, error });
            }
        }
        ctx.request_repaint();
    });
}

/// Start the downloaded installer (never silent). Zip files open the folder instead.
pub fn run_installer(path: &Path) -> Result<bool, String> {
    let is_exe = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi"));
    if is_exe {
        let result = if path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("msi")) {
            std::process::Command::new("msiexec").arg("/i").arg(path).spawn()
        } else {
            std::process::Command::new(path).spawn()
        };
        match result {
            Ok(_) => {
                crate::pimlog::info(format!("Update: installer started: {}", path.display()));
                Ok(true)
            }
            Err(error) => {
                // e.g. ERROR_ELEVATION_REQUIRED (740): let the shell show the UAC prompt.
                crate::pimlog::warn(format!("Update: direct start failed ({error}); retrying via the shell."));
                std::process::Command::new("explorer").arg(path).spawn().map_err(|e| format!("Could not start the installer: {e}"))?;
                Ok(true)
            }
        }
    } else {
        open_folder_select(path);
        Ok(false)
    }
}

pub fn open_folder_select(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
}

/// Opens a web page (only used for the browser add-on page; never for update links).
pub fn open_url(url: &str) {
    crate::pimlog::info("Opening a web page in the browser");
    let _ = std::process::Command::new("rundll32").arg("url.dll,FileProtocolHandler").arg(url).spawn();
}

pub fn human_bytes(bytes: u64) -> String {
    crate::human(bytes)
}

// ---------------------------------------------------------------------------
// Small SHA-256 (FIPS 180-4), std only.

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub struct Sha256 {
    state: [u32; 8],
    buffer: Vec<u8>,
    length: u64,
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
    0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
    0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
    0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
    0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
            buffer: Vec::with_capacity(64),
            length: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if !self.buffer.is_empty() {
            let need = 64 - self.buffer.len();
            let take = need.min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buffer.len() == 64 {
                let block: [u8; 64] = self.buffer[..].try_into().unwrap();
                self.compress(&block);
                self.buffer.clear();
            }
        }
        while data.len() >= 64 {
            let block: [u8; 64] = data[..64].try_into().unwrap();
            self.compress(&block);
            data = &data[64..];
        }
        self.buffer.extend_from_slice(data);
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *s = s.wrapping_add(v);
        }
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        while (self.buffer.len() + pad.len()) % 64 != 56 {
            pad.push(0);
        }
        pad.extend_from_slice(&bits.to_be_bytes());
        let length = self.length;
        self.update(&pad);
        self.length = length;
        let mut out = [0u8; 32];
        for (i, s) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&s.to_be_bytes());
        }
        out
    }
}

/// `--check-update-selftest[=<version url>]`: headless check + download + sha256 test.
/// Prints a short result line and logs everything to pim.log. Exit code 0 = ran.
pub fn selftest(url_override: Option<String>) -> i32 {
    macro_rules! out {
        ($($arg:tt)*) => {{
            let line = format!($($arg)*);
            println!("{line}");
            crate::pimlog::info(format!("Update self-test: {line}"));
        }};
    }
    crate::pimlog::info("Update self-test start (--check-update-selftest)");
    let abc = hex(&{
        let mut h = Sha256::new();
        h.update(b"abc");
        h.finish()
    });
    let sha_ok = abc == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    out!("sha256(abc) self-check: {}", if sha_ok { "OK" } else { "FAILED" });
    crate::pimlog::info(format!("Update self-test: sha256(abc) {}", if sha_ok { "OK" } else { "FAILED" }));
    let id = drive_file_id(&format!("https://{DRIVE_HOST}/file/d/1AbCdEfGhIjKlMnOpQrStUvWxYz012345/view?usp=sharing"));
    out!("drive id ok: {}", id.as_deref() == Some("1AbCdEfGhIjKlMnOpQrStUvWxYz012345"));
    out!("built-in patch notes {APP_VERSION}: {} line(s)", notes_for(APP_VERSION).lines().count());
    out!("semver 0.2.1 > 0.2.0: {}, 0.2.0 > 0.2.0: {}, 0.10.0 > 0.9.9: {}", is_newer("0.2.1", "0.2.0"), is_newer("0.2.0", "0.2.0"), is_newer("0.10.0", "0.9.9"));
    let custom = url_override.is_some();
    let url = url_override.unwrap_or_else(|| DEFAULT_VERSION_URL.to_string());
    out!("current version: {APP_VERSION}");
    out!("version source: {}", if custom { "custom (test)" } else { "built-in" });
    // `--download-test`: also download + verify when this version is already the latest
    // (tests the download folder with the current release).
    let phase = match check_now(&url) {
        Phase::UpToDate(_) if std::env::args().any(|a| a == "--download-test") => match fetch(&url) {
            Ok(mut info) => {
                if !info.patchnotes.trim().is_empty() {
                    info.notes = fetch_patch_notes(&info.patchnotes, &info.version).unwrap_or_default();
                }
                out!("download test: latest {} (= this version), patch notes from Drive: {} line(s)", info.version, info.notes.lines().count());
                Phase::Available(info)
            }
            Err(error) => Phase::CheckFailed(error),
        },
        other => other,
    };
    match phase {
        Phase::Available(info) => {
            out!("RESULT: update available {} (type={}, sha256={}, notes: {} line(s))", info.version, info.kind, info.sha256.as_deref().unwrap_or("-"), info.notes.lines().count());
            let dir = std::env::temp_dir().join("pim-update-selftest");
            match download_to(&info, &dir, |_, _| {}) {
                Ok(path) => {
                    out!("DOWNLOAD: OK {} ({} bytes)", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
                    if info.is_patch() {
                        match verify_patch(&path) {
                            Ok(manifest) => {
                                out!("PATCH: OK version {} with {} file(s)", manifest.version, manifest.files.len());
                                // `--check-update-selftest=<url> --apply`: same code path as the "Apply update" button.
                                if std::env::args().any(|a| a == "--apply") {
                                    // the restarted app only logs its version (no second window during tests)
                                    match start_patch_apply(&path, &["--version-probe".to_string()]) {
                                        Ok(elevated) => out!("APPLY: updater started (elevated: {elevated})"),
                                        Err(error) => out!("APPLY: FAILED {error}"),
                                    }
                                }
                            }
                            Err(error) => out!("PATCH: INVALID {error}"),
                        }
                    }
                }
                Err(error) => out!("DOWNLOAD: FAILED {error}"),
            }
        }
        Phase::UpToDate(latest) => out!("RESULT: up to date (latest {latest})"),
        Phase::CheckFailed(error) => out!("RESULT: check failed: {error}"),
        other => out!("RESULT: {other:?}"),
    }
    crate::pimlog::info("Update self-test end");
    0
}

// ---------------------------------------------------------------------------
// 0.2.1: patch updates (zip + pim-patch.json manifest) and the built-in updater.

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PatchFile {
    /// Path inside the zip and relative to the install folder (forward or back slashes).
    pub path: String,
    pub sha256: String,
    pub size: u64,
    /// "main" = the application exe; it is written under the name of the installed exe.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PatchManifest {
    #[serde(default)]
    pub product: String,
    pub version: String,
    #[serde(default)]
    pub from: String,
    pub files: Vec<PatchFile>,
}

struct ZipEntry {
    name: String,
    method: u16,
    compressed: u64,
    size: u64,
    offset: u64,
}

fn le16(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| "zip: truncated".to_string())
}

fn le32(d: &[u8], at: usize) -> Result<u32, String> {
    d.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| "zip: truncated".to_string())
}

/// Minimal zip reader (central directory, stored / deflate, no zip64 and no encryption).
fn zip_entries(data: &[u8]) -> Result<Vec<ZipEntry>, String> {
    if data.len() < 22 {
        return Err("not a zip file (too small)".into());
    }
    let lowest = data.len().saturating_sub(22 + 65535);
    let mut eocd = None;
    let mut i = data.len() - 22;
    loop {
        if data[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
            eocd = Some(i);
            break;
        }
        if i == lowest {
            break;
        }
        i -= 1;
    }
    let eocd = eocd.ok_or("not a zip file (no end of central directory)")?;
    let count = le16(data, eocd + 10)? as usize;
    let mut at = le32(data, eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if le32(data, at)? != 0x0201_4b50 {
            return Err("zip: bad central directory".into());
        }
        let flags = le16(data, at + 8)?;
        let method = le16(data, at + 10)?;
        let compressed = le32(data, at + 20)? as u64;
        let size = le32(data, at + 24)? as u64;
        let name_len = le16(data, at + 28)? as usize;
        let extra_len = le16(data, at + 30)? as usize;
        let comment_len = le16(data, at + 32)? as usize;
        let offset = le32(data, at + 42)? as u64;
        let name_bytes = data.get(at + 46..at + 46 + name_len).ok_or("zip: truncated name")?;
        let name = String::from_utf8_lossy(name_bytes).replace('\\', "/");
        if flags & 1 != 0 {
            return Err(format!("zip: {name} is encrypted"));
        }
        if compressed == 0xFFFF_FFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
            return Err("zip: zip64 archives are not supported".into());
        }
        out.push(ZipEntry { name, method, compressed, size, offset });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

fn zip_read(data: &[u8], entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let at = entry.offset as usize;
    if le32(data, at)? != 0x0403_4b50 {
        return Err(format!("zip: bad local header for {}", entry.name));
    }
    let start = at + 30 + le16(data, at + 26)? as usize + le16(data, at + 28)? as usize;
    let raw = data.get(start..start + entry.compressed as usize).ok_or_else(|| format!("zip: {} is truncated", entry.name))?;
    let bytes = match entry.method {
        0 => raw.to_vec(),
        8 => miniz_oxide::inflate::decompress_to_vec(raw).map_err(|e| format!("zip: {} could not be decompressed ({e:?})", entry.name))?,
        m => return Err(format!("zip: {} uses unsupported compression method {m}", entry.name)),
    };
    if bytes.len() as u64 != entry.size {
        return Err(format!("zip: {} has the wrong size", entry.name));
    }
    Ok(bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex(&h.finish())
}

/// Only plain relative paths (no drive letters, no "..", no leading slash).
fn safe_relative(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::Normal(part) => out.push(part),
            _ => return Err(format!("unsafe path in patch: {path}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("empty path in patch".into());
    }
    Ok(out)
}

/// Read the patch zip, check the manifest and every file (size + sha256). Returns manifest + contents.
fn load_patch(zip: &Path) -> Result<(PatchManifest, Vec<(PatchFile, Vec<u8>)>), String> {
    let data = fs::read(zip).map_err(|e| format!("could not read {}: {e}", zip.display()))?;
    let entries = zip_entries(&data)?;
    let manifest_entry = entries.iter().find(|e| e.name == PATCH_MANIFEST).ok_or(format!("{PATCH_MANIFEST} is missing in the zip"))?;
    let manifest_bytes = zip_read(&data, manifest_entry)?;
    let manifest: PatchManifest = serde_json::from_slice(&manifest_bytes).map_err(|e| format!("{PATCH_MANIFEST} is not valid: {e}"))?;
    if parse_semver(&manifest.version).is_none() || manifest.files.is_empty() {
        return Err(format!("{PATCH_MANIFEST} has no valid version or no files"));
    }
    let mut files = Vec::new();
    for file in &manifest.files {
        safe_relative(&file.path)?;
        let name = file.path.replace('\\', "/");
        let entry = entries.iter().find(|e| e.name == name).ok_or_else(|| format!("{} is listed in the manifest but missing in the zip", file.path))?;
        let bytes = zip_read(&data, entry)?;
        if bytes.len() as u64 != file.size {
            return Err(format!("{}: size {} does not match the manifest ({})", file.path, bytes.len(), file.size));
        }
        let digest = sha256_hex(&bytes);
        if !digest.eq_ignore_ascii_case(file.sha256.trim()) {
            return Err(format!("{}: sha256 does not match the manifest", file.path));
        }
        files.push((file.clone(), bytes));
    }
    Ok((manifest, files))
}

pub fn verify_patch(zip: &Path) -> Result<PatchManifest, String> {
    let (manifest, files) = load_patch(zip)?;
    crate::pimlog::info(format!("Update: patch {} verified ({} file(s), manifest sha256 OK)", manifest.version, files.len()));
    Ok(manifest)
}

/// True when a file can be created in `dir` (false for Program Files without admin rights).
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".pim_write_test_{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn install_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
}

fn quote_arg(arg: &str) -> String {
    if arg.contains(' ') || arg.contains('\t') || arg.is_empty() { format!("\"{arg}\"") } else { arg.to_string() }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// "Apply update": copy this exe as temporary updater and start it. Returns `Ok(true)` when the
/// updater runs elevated (UAC was accepted). The caller must exit the app right after `Ok`.
pub fn start_patch_apply(zip: &Path, restart_args: &[String]) -> Result<bool, String> {
    let manifest = verify_patch(zip)?;
    let exe = std::env::current_exe().map_err(|e| format!("could not find the running exe: {e}"))?;
    let target = exe.parent().ok_or("could not find the install folder")?.to_path_buf();
    let exe_name = exe.file_name().and_then(|n| n.to_str()).ok_or("exe name is not valid")?.to_string();
    let dir = updates_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let updater = dir.join(format!("pim-updater-{}.exe", std::process::id()));
    fs::copy(&exe, &updater).map_err(|e| format!("could not prepare the updater ({}): {e}", updater.display()))?;
    let _ = fs::remove_file(last_result_path());
    let writable = dir_writable(&target);
    let mut args = vec![
        format!("--apply-update={}", zip.display()),
        format!("--target={}", target.display()),
        format!("--exe-name={exe_name}"),
        format!("--wait-pid={}", std::process::id()),
        format!("--from-version={APP_VERSION}"),
        "--restart".to_string(),
    ];
    for extra in restart_args {
        args.push(format!("--restart-arg={extra}"));
    }
    crate::pimlog::info(format!(
        "Update: applying patch {} to {} (install folder writable: {writable}); updater {}",
        manifest.version,
        target.display(),
        updater.display()
    ));
    if writable {
        std::process::Command::new(&updater).args(&args).spawn().map_err(|e| format!("could not start the updater: {e}"))?;
        return Ok(false);
    }
    args.push("--elevated".into());
    let params = args.iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" ");
    crate::pimlog::info("Update: the install folder needs administrator rights; asking Windows for permission (UAC).");
    let verb = wide("runas");
    let file = wide(&updater.display().to_string());
    let params_w = wide(&params);
    let dir_w = wide(&dir.display().to_string());
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params_w.as_ptr(), dir_w.as_ptr(), 1)
    };
    if (result as isize) <= 32 {
        let _ = fs::remove_file(&updater);
        crate::pimlog::warn(format!("Update: UAC prompt refused or failed (code {})", result as isize));
        return Err("Administrator permission was not given, so the update was not applied. Pro Internet Manager keeps running.".into());
    }
    Ok(true)
}

pub fn last_result_path() -> PathBuf {
    updates_dir().join("last_update.json")
}

fn write_result(ok: bool, version: &str, message: &str) {
    let value = serde_json::json!({ "ok": ok, "version": version, "message": message, "time": now_secs() });
    let _ = fs::create_dir_all(updates_dir());
    let _ = fs::write(last_result_path(), value.to_string());
}

/// Read (once) the result of the last patch update and clean up old updater copies.
/// Returns (ok, version, message).
pub fn take_last_result() -> Option<(bool, String, String)> {
    if let Ok(read) = fs::read_dir(updates_dir()) {
        let own = std::env::current_exe().ok();
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("pim-updater-") && name.ends_with(".exe") && Some(&path) != own.as_ref() {
                let _ = fs::remove_file(&path);
            }
        }
    }
    let path = last_result_path();
    let text = fs::read_to_string(&path).ok()?;
    let _ = fs::rename(&path, updates_dir().join("last_update.shown.json"));
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ok = value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let version = value.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Some((ok, version, message))
}

fn wait_for_pid(pid: u32, timeout: Duration) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return true; // already gone
        }
        let result = WaitForSingleObject(handle, timeout.as_millis() as u32);
        CloseHandle(handle);
        result == 0
    }
}

fn retry<T>(what: &str, mut f: impl FnMut() -> std::io::Result<T>) -> Result<T, String> {
    let mut last = None;
    for attempt in 0..30 {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt == 0 {
                    crate::pimlog::warn(format!("Updater: {what} failed ({e}); retrying"));
                }
                last = Some(e);
                thread::sleep(Duration::from_millis(500));
            }
        }
    }
    Err(format!("{what}: {}", last.map(|e| e.to_string()).unwrap_or_default()))
}

struct Applied {
    dest: PathBuf,
    backup: Option<PathBuf>,
}

fn rollback(applied: &[Applied]) -> Vec<String> {
    let mut problems = Vec::new();
    for item in applied.iter().rev() {
        let result = match &item.backup {
            Some(backup) => retry(&format!("restore {}", item.dest.display()), || fs::copy(backup, &item.dest).map(|_| ())),
            None => retry(&format!("remove {}", item.dest.display()), || fs::remove_file(&item.dest)),
        };
        match result {
            Ok(()) => crate::pimlog::info(format!("Updater: rolled back {}", item.dest.display())),
            Err(e) => {
                crate::pimlog::error(format!("Updater: rollback problem: {e}"));
                problems.push(e);
            }
        }
    }
    problems
}

/// Updater mode (`--apply-update=...`), runs from a temporary copy of the exe. Exit code 0 = applied.
pub fn run_updater(args: &[String]) -> i32 {
    let get = |key: &str| args.iter().find_map(|a| a.strip_prefix(key)).map(str::to_string);
    let zip = PathBuf::from(get("--apply-update=").unwrap_or_default());
    let target = PathBuf::from(get("--target=").unwrap_or_default());
    let exe_name = get("--exe-name=").unwrap_or_else(|| "Pro Internet Manager.exe".into());
    let from = get("--from-version=").unwrap_or_else(|| APP_VERSION.to_string());
    let restart = args.iter().any(|a| a == "--restart");
    let elevated = args.iter().any(|a| a == "--elevated");
    let restart_args: Vec<String> = args.iter().filter_map(|a| a.strip_prefix("--restart-arg=")).map(str::to_string).collect();
    crate::pimlog::info("------------------------------------------------------------");
    crate::pimlog::info(format!(
        "Updater {APP_VERSION} (pid {}): patch {} -> {} (exe {exe_name}, from {from}, elevated {elevated})",
        std::process::id(),
        zip.display(),
        target.display()
    ));
    if let Some(pid) = get("--wait-pid=").and_then(|p| p.parse::<u32>().ok()) {
        crate::pimlog::info(format!("Updater: waiting for Pro Internet Manager (pid {pid}) to exit"));
        if !wait_for_pid(pid, Duration::from_secs(60)) {
            let message = "Update not applied: Pro Internet Manager did not exit within 60 seconds.".to_string();
            crate::pimlog::error(format!("Updater: {message}"));
            write_result(false, "", &message);
            return 2;
        }
        thread::sleep(Duration::from_millis(400));
    }
    let main_exe = target.join(&exe_name);
    let outcome = apply_patch(&zip, &target, &exe_name, &from);
    let code = match &outcome {
        Ok(version) => {
            let message = format!("Updated to version {version}. A backup of the replaced files is in _backup_{from}.");
            crate::pimlog::info(format!("Updater: {message}"));
            write_result(true, version, &message);
            0
        }
        Err(error) => {
            crate::pimlog::error(format!("Updater: {error}"));
            write_result(false, "", error);
            1
        }
    };
    if restart {
        let started = if elevated {
            // Do not start the app with administrator rights: let Explorer start it as the user.
            std::process::Command::new("explorer.exe").arg(&main_exe).spawn().map(|_| ())
        } else {
            std::process::Command::new(&main_exe).args(&restart_args).current_dir(&target).spawn().map(|_| ())
        };
        match started {
            Ok(()) => crate::pimlog::info(format!("Updater: restarted {}", main_exe.display())),
            Err(e) => crate::pimlog::error(format!("Updater: could not restart {}: {e}", main_exe.display())),
        }
    }
    code
}

/// Backup, copy, verify; on any error everything written so far is rolled back.
fn apply_patch(zip: &Path, target: &Path, exe_name: &str, from: &str) -> Result<String, String> {
    let (manifest, files) = load_patch(zip).map_err(|e| format!("Update not applied: the patch is not valid ({e})."))?;
    if !target.is_dir() {
        return Err(format!("Update not applied: install folder {} does not exist.", target.display()));
    }
    let backup_dir = target.join(format!("_backup_{from}"));
    let mut plan = Vec::new();
    for (file, bytes) in files {
        let rel = if file.role == "main" { PathBuf::from(exe_name) } else { safe_relative(&file.path)? };
        plan.push((target.join(&rel), backup_dir.join(&rel), file, bytes));
    }
    // 1. backups of every file that will be replaced
    for (dest, backup, _, _) in &plan {
        if dest.is_file() {
            if let Some(parent) = backup.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("Update not applied: could not create {}: {e}", parent.display()))?;
            }
            retry(&format!("backup {}", dest.display()), || fs::copy(dest, backup).map(|_| ())).map_err(|e| format!("Update not applied: {e}"))?;
            crate::pimlog::info(format!("Updater: backup {} -> {}", dest.display(), backup.display()));
        }
    }
    // 2. write the new files (temp file + rename), verify each one
    let mut applied: Vec<Applied> = Vec::new();
    for (dest, backup, file, bytes) in &plan {
        let existed = dest.is_file();
        let step = (|| -> Result<(), String> {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
            }
            let mut tmp_name = dest.as_os_str().to_owned();
            tmp_name.push(".pimnew");
            let tmp = PathBuf::from(tmp_name);
            fs::write(&tmp, bytes).map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
            if let Err(e) = retry(&format!("replace {}", dest.display()), || fs::rename(&tmp, dest)) {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
            let written = fs::read(dest).map_err(|e| format!("could not read back {}: {e}", dest.display()))?;
            if !sha256_hex(&written).eq_ignore_ascii_case(file.sha256.trim()) {
                return Err(format!("{} has the wrong checksum after copying", dest.display()));
            }
            Ok(())
        })();
        match step {
            Ok(()) => {
                crate::pimlog::info(format!("Updater: replaced {} ({} bytes, sha256 OK)", dest.display(), bytes.len()));
                applied.push(Applied { dest: dest.clone(), backup: existed.then(|| backup.clone()) });
            }
            Err(error) => {
                // the failed file itself may be half written: restore it as well
                if existed && dest.is_file() {
                    applied.push(Applied { dest: dest.clone(), backup: Some(backup.clone()) });
                }
                crate::pimlog::error(format!("Updater: {error}; rolling back {} file(s)", applied.len()));
                let problems = rollback(&applied);
                return Err(if problems.is_empty() {
                    format!("Update {} failed and was rolled back: {error}. The previous version is unchanged.", manifest.version)
                } else {
                    format!("Update {} failed ({error}) and the rollback had problems: {}. The backup is in {}.", manifest.version, problems.join("; "), backup_dir.display())
                });
            }
        }
    }
    Ok(manifest.version)
}
