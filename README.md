# Pro Internet Downloader

Pro Internet Manager (by Triwar Design) is a fast, modern download manager for Windows. It downloads regular files and videos from sites such as YouTube, and picks up links straight from your browser with the "Pro Internet Manager Bridge" extension. Downloads (installer and portable zip) are on the [Releases page](https://github.com/Triwarr/Pro-Internet-Downloader/releases). The source code is in this repository for viewing (see [Source code](#source-code) and [License](#license)).

## What it does

- Downloads files and videos (video downloads use yt-dlp and ffmpeg, with a bundled JavaScript runtime, so you never have to install anything yourself).
- Browser extension for Firefox and Chromium browsers (Chrome, Edge, Brave) that lists the downloadable sources on the page you are on, with type and quality.
- Download queue with progress, speed, time left and size, plus a scheduler.
- Four themes (Pro Lime, Midnight, Graphite, Azeroth) plus Paper and Compact.
- Closes to the system tray and keeps running in the background. Right-click the tray icon to open it or exit.
- Built-in update check under Help, with automatic patch updates and rollback if something goes wrong.
- A real log file so you can see why something failed.

## Screenshots

> **Note:** all data in these screenshots is demo data: sample files, sample download states and sample statuses (for example "Bridge not connected" and the preview update prompt). The screenshots show the look of the app only and contain no relevant information about how the program itself works.

![Main window with the Pro Lime theme](docs/screenshots/main-pro-lime.png)
*Main window (Pro Lime theme) with sample downloads. Screenshots are taken in preview mode, so no browser is connected.*

| Midnight | Graphite | Azeroth |
| --- | --- | --- |
| ![Midnight theme](docs/screenshots/theme-midnight.png) | ![Graphite theme](docs/screenshots/theme-graphite.png) | ![Azeroth theme](docs/screenshots/theme-azeroth.png) |

![Options, General tab](docs/screenshots/options-general.png)
*Options > General: download folder, queue and log file.*

![Options, Browser bridge tab](docs/screenshots/options-browser-bridge.png)
*Options > Browser bridge: connection status and setup steps for Firefox and Chromium browsers.*

![Help, Updates window](docs/screenshots/help-updates.png)
*Help > Updates: installed version and update check.*

![Update available prompt](docs/screenshots/update-available.png)
*The in-app prompt when a new version is available (preview).*

![Updated to a new version](docs/screenshots/updated.png)
*After an update, the app shows once what changed.*

## Install

Download the latest version from the [Releases page](https://github.com/Triwarr/Pro-Internet-Downloader/releases/latest). There are two options:

**Installer (recommended):** `ProInternetManager-<version>-Setup.exe`
- Run it and follow the steps. The default location is `C:\Program Files\Pro Internet Manager` (Windows asks for administrator permission once).
- The browser extensions are optional components: the Chromium extension (Chrome, Edge, Brave) is placed as a folder, and the last screen links to the signed Firefox extension.
- Adds Start menu and desktop shortcuts and an entry in Installed apps, with an uninstaller.

**Portable zip (alternative):** `ProInternetManager-<version>-portable.zip`
- No installer: unpack the zip to any folder and start `Pro Internet Manager.exe`. yt-dlp, ffmpeg and the JavaScript runtime are included.
- Close a running Pro Internet Manager first (also from the system tray).
- Settings and logs are stored in `%APPDATA%\Pro Internet Downloader`, the same place the installed version uses.

## Source code

| Folder | Contents |
| --- | --- |
| `app/` | The app (Rust, egui): `src/`, `Cargo.toml`, `Cargo.lock`, `build.rs`, `build-restyle.bat`, icons in `assets/` |
| `extensions/firefox/` | Source of the Firefox extension "Pro Internet Manager Bridge" |
| `extensions/chromium/` | Source of the Chromium extension (Chrome, Edge, Brave) |
| `installer/` | Source of the Windows installer (`InstallerPIM.cs`, C# / WinForms) and its build script `build.ps1` |
| `docs/screenshots/` | Screenshots used in this README |

Build the app (Windows):
1. Install Rust (stable, MSVC toolchain, via rustup) and the Visual Studio Build Tools with the Windows SDK.
2. `cd app` and run `cargo build --release`, or run `build-restyle.bat`, which builds and copies the result to `Pro Internet Manager_restyle.exe` (log in `build.log`). Optional variables such as `VCVARS`, `PIM_SDKROOT` and `CARGO` are described at the top of the script.
3. yt-dlp, ffmpeg and Deno are not in this repository. Put `yt-dlp.exe`, `ffmpeg.exe` and `deno.exe` next to the built exe.

Update source: the published source contains placeholders for the update links (`https://example.invalid/...`), so a build from this repository does not find updates. To use your own update source, set `PIM_VERSION_URL` (link to a `version.txt`), and optionally `PIM_FALLBACK_VERSION_URL` and `PIM_DOWNLOAD_URL`, as environment variables when you build. The official releases have their own update source.

Build the installer: run `installer\build.ps1` in PowerShell after building the app. Put `yt-dlp.exe`, `ffmpeg.exe`, `deno.exe`, `FFMPEG-NOTICE.txt` and `YT-DLP-NOTICE.txt` in `installer\payload-src` (or set `PIM_PAYLOAD_DIR`). The background picture of the official installer is not included; without `installer\assets\background.jpg` Setup uses a plain background.

## License

Copyright (c) Triwar Design. All rights reserved. The source code is published for viewing only; using, copying, modifying or distributing it requires written permission. See [LICENSE](LICENSE).

## Patch notes

### 0.2.5
- Fixed the app showing "Bridge not connected" while downloads from the browser extension did arrive. Pings and downloads from the Firefox and Chromium extensions now both count, and the status stays "Bridge connected" while the extension is active.

Patch notes of older versions (0.2.0 to 0.2.4) are in [PATCHNOTES_ARCHIVE.md](PATCHNOTES_ARCHIVE.md).
