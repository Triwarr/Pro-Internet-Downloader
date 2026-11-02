//! 0.2.3: own data folder `%APPDATA%\Pro Internet Downloader`.
//!
//! Up to 0.2.2 Pro Internet Manager kept its files in `%APPDATA%\RustDownloadManager`, the
//! folder of the other download manager (config.json was even shared). From 0.2.3 on:
//! - logs\pim.log, config.json, pim_restyle.json, update.json and ytdlp_update.json live in
//!   `%APPDATA%\Pro Internet Downloader`;
//! - on the first start the files that exist in the old folder are COPIED once (never moved,
//!   never changed, never deleted); a file that already exists in the new folder is not
//!   overwritten;
//! - after that the app never writes to `RustDownloadManager` again.

use std::fs;
use std::path::PathBuf;

pub const DATA_DIR_NAME: &str = "Pro Internet Downloader";
const LEGACY_DIR_NAME: &str = "RustDownloadManager";
/// Files copied from the old folder (relative paths).
const MIGRATE: [&str; 5] = ["config.json", "pim_restyle.json", "update.json", "ytdlp_update.json", "logs\\pim.log"];
const MARKER: &str = "migrated.json";

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// `%APPDATA%\Pro Internet Downloader`
pub fn data_dir() -> PathBuf {
    appdata().join(DATA_DIR_NAME)
}

/// Path of an own file, e.g. `data_file("config.json")`.
pub fn data_file(name: &str) -> PathBuf {
    data_dir().join(name)
}

fn legacy_dir() -> PathBuf {
    appdata().join(LEGACY_DIR_NAME)
}

/// Creates the data folder and copies (once) the files of an older version.
/// Returns log lines; called before the logger writes anything.
pub fn prepare() -> Vec<String> {
    let mut lines = Vec::new();
    let dir = data_dir();
    let fresh = !dir.exists();
    if let Err(e) = fs::create_dir_all(dir.join("logs")) {
        lines.push(format!("Data folder {} could not be created: {e}", dir.display()));
        return lines;
    }
    if fresh {
        lines.push(format!("Data folder created: {}", dir.display()));
    }
    if dir.join(MARKER).exists() {
        return lines;
    }
    let legacy = legacy_dir();
    let mut copied = Vec::new();
    if legacy.is_dir() {
        for rel in MIGRATE {
            let from = legacy.join(rel);
            let to = dir.join(rel);
            if !from.is_file() {
                continue;
            }
            if to.exists() {
                lines.push(format!("Migration: {rel} already exists in the new folder, kept"));
                continue;
            }
            if let Some(parent) = to.parent() {
                let _ = fs::create_dir_all(parent);
            }
            match fs::copy(&from, &to) {
                Ok(bytes) => {
                    lines.push(format!("Migration: copied {rel} ({bytes} bytes) from {}", legacy.display()));
                    copied.push(rel);
                }
                Err(e) => lines.push(format!("Migration: could not copy {rel}: {e}")),
            }
        }
        lines.push(format!("Migration: the files in {} are left unchanged", legacy.display()));
    }
    let marker = serde_json::json!({
        "from": legacy.display().to_string(),
        "copied": copied,
        "time": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    let _ = fs::write(dir.join(MARKER), marker.to_string());
    lines
}
