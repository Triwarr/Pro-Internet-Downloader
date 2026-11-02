//! Pro Internet Manager: close-to-tray behaviour, exit confirmation and the Help > Updates window.
//! Kept separate from ui.rs so the existing layout code stays untouched.

use std::sync::atomic::Ordering;

use egui::{Color32, RichText, Stroke};

use super::*;
use crate::theme::*;
use crate::update::{self, Phase};

impl App {
    pub(crate) fn active_download_count(&self) -> usize {
        self.jobs.iter().filter(|job| matches!(job.status(), Status::Downloading | Status::Queued)).count()
    }

    /// Called from `logic` every frame (also while hidden): close-to-tray, tray Exit and
    /// the one-time `--minimized` start to tray.
    pub(crate) fn tray_logic(&mut self, ctx: &egui::Context) {
        self.frames = self.frames.saturating_add(1);
        if !self.tray_started && !crate::autostart::is_demo() {
            self.tray_started = true;
            update::auto_check(&self.update_state, ctx.clone());
            self.auto_check_pending = true;
        }
        // 0.2.3: the startup check found a newer version -> in-app prompt (once per start).
        if self.auto_check_pending {
            match update::get(&self.update_state) {
                Phase::Available(_) => {
                    self.auto_check_pending = false;
                    self.update_prompt = true;
                }
                Phase::UpToDate(_) | Phase::CheckFailed(_) | Phase::NotConfigured => self.auto_check_pending = false,
                _ => {}
            }
        }
        if crate::autostart::take_start_minimized() {
            self.start_hidden = true;
        }
        if self.close_selftest {
            self.close_selftest_step(ctx);
        }
        // `--minimized`: hide to tray after the first frames (eframe shows the window after frame 1).
        if self.start_hidden && self.frames >= 3 {
            self.start_hidden = false;
            if crate::tray::is_running() {
                crate::pimlog::info("Started with --minimized: window hidden to the tray.");
                crate::tray::hide(ctx);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        if crate::tray::take_exit_request() {
            self.request_exit(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.allow_exit {
                crate::pimlog::info("Exit: closing Pro Internet Manager.");
            } else if crate::tray::is_running() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::pimlog::info("Window closed: Pro Internet Manager keeps running in the tray.");
                crate::tray::hide(ctx);
            } else {
                // No tray icon: closing really exits (same as before), but still ask about downloads.
                if self.active_download_count() > 0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    self.exit_confirm = true;
                } else {
                    crate::pimlog::info("Window closed without a tray icon: exiting.");
                }
            }
        }
        if self.exit_now {
            self.exit_now = false;
            self.allow_exit = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// `--demo --close-selftest`: presses the window's close button via a viewport Close command,
    /// checks that the window is hidden and the tray icon exists, shows it again from the tray
    /// code path, then exits through the confirmation dialog. Everything goes to pim.log.
    fn close_selftest_step(&mut self, ctx: &egui::Context) {
        let f = self.frames;
        let state = |label: &str| {
            crate::pimlog::info(format!(
                "Close self-test [{label}]: tray icon added={}, tray window exists={}, hidden flag={}, main window visible (Win32)={}",
                crate::tray::is_running(),
                crate::tray::tray_window_exists(),
                crate::tray::is_hidden(),
                crate::tray::main_window_visible()
            ));
        };
        match f {
            20 => {
                state("before close");
                crate::pimlog::info("Close self-test: sending Close (same as clicking X).");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            40 => {
                state("after close");
                crate::pimlog::info("Close self-test: showing the window again (same code as tray left click).");
                crate::tray::show_main();
            }
            60 => {
                state("after show");
                crate::pimlog::info("Close self-test: requesting Exit (same code as tray menu Exit).");
                self.request_exit(ctx);
            }
            80 => {
                crate::pimlog::info(format!("Close self-test: exit confirmation dialog open={}", self.exit_confirm));
                if self.exit_confirm {
                    crate::pimlog::info("Close self-test: confirming Exit.");
                    self.exit_confirm = false;
                    self.exit_now = true;
                }
            }
            _ => {}
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    /// File > Exit and tray > Exit: really quit, confirm first when downloads are active.
    pub(crate) fn request_exit(&mut self, ctx: &egui::Context) {
        let active = self.active_download_count();
        if active > 0 {
            crate::pimlog::info(format!("Exit requested with {active} active download(s): asking for confirmation."));
            crate::tray::show_main();
            self.exit_confirm = true;
            ctx.request_repaint();
        } else {
            crate::pimlog::info("Exit requested.");
            self.exit_now = true;
            ctx.request_repaint();
        }
    }

    fn modal_frame() -> egui::Frame {
        egui::Frame::new()
            .fill(CARD())
            .stroke(Stroke::new(1.0, alpha(ACCENT(), 110)))
            .corner_radius(CARD_RADIUS + 2.0)
            .inner_margin(egui::Margin::same(14))
            .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(170) })
    }

    pub(crate) fn exit_dialogs(&mut self, ctx: &egui::Context) {
        if self.exit_confirm {
            let active = self.active_download_count();
            egui::Window::new(RichText::new("Exit Pro Internet Manager?").family(semibold()).size(15.0).color(TEXT()))
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .frame(Self::modal_frame())
                .show(ctx, |ui| {
                    ui.label(RichText::new(format!("{active} download(s) are still active or queued. They stop when you exit.")).color(TEXT()));
                    ui.label(RichText::new("Unfinished downloads can be resumed later.").size(11.5).color(DIM()));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if action_button(ui, "Exit", true).clicked() {
                            crate::pimlog::info("Exit confirmed by the user.");
                            self.stop_all_for_exit();
                            self.exit_confirm = false;
                            self.exit_now = true;
                        }
                        if action_button(ui, "Cancel", false).clicked() {
                            crate::pimlog::info("Exit cancelled by the user.");
                            self.exit_confirm = false;
                        }
                    });
                });
        }
        if self.installer_close_prompt {
            egui::Window::new(RichText::new("Installer started").family(semibold()).size(15.0).color(TEXT()))
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .frame(Self::modal_frame())
                .show(ctx, |ui| {
                    ui.label(RichText::new("Close Pro Internet Manager now so the installer can replace its files?").color(TEXT()));
                    let active = self.active_download_count();
                    if active > 0 {
                        ui.label(RichText::new(format!("{active} download(s) are still active and will stop.")).size(11.5).color(WARN()));
                    }
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if action_button(ui, "Close Pro Internet Manager", true).clicked() {
                            crate::pimlog::info("Update: user closes the app for the installer.");
                            self.stop_all_for_exit();
                            self.installer_close_prompt = false;
                            self.exit_now = true;
                        }
                        if action_button(ui, "Keep running", false).clicked() {
                            self.installer_close_prompt = false;
                        }
                    });
                });
        }
    }

    /// 0.2.1: "Remove" asks first (row button, toolbar and Delete key all come here).
    pub(crate) fn remove_dialog(&mut self, ctx: &egui::Context) {
        let Some((index, name)) = self.confirm_remove.clone() else { return };
        if self.jobs.get(index).map(|j| j.name.as_str()) != Some(name.as_str()) {
            self.confirm_remove = None;
            return;
        }
        let unfinished = self.jobs.get(index).is_some_and(|j| j.status() != Status::Done);
        egui::Window::new(RichText::new("Remove download?").family(semibold()).size(15.0).color(TEXT()))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(Self::modal_frame())
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(RichText::new(format!("Remove \"{name}\" from the list?")).color(TEXT()));
                ui.label(
                    RichText::new(if unfinished {
                        "The download stops and its unfinished part files are deleted. Files that are already complete stay on disk."
                    } else {
                        "The file itself stays on disk; only the entry is removed from the list."
                    })
                    .size(11.5)
                    .color(DIM()),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if action_button(ui, "Remove", true).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        crate::pimlog::info(format!("Removed from the list: {name}"));
                        self.confirm_remove = None;
                        self.remove(index);
                        self.selected = match self.selected {
                            Some(s) if s == index => None,
                            Some(s) if s > index => Some(s - 1),
                            other => other,
                        };
                    }
                    if action_button(ui, "Cancel", false).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.confirm_remove = None;
                    }
                });
            });
    }

    /// 0.2.1 empty state: "Paste URL from clipboard".
    pub(crate) fn paste_from_clipboard(&mut self) {
        let text = read_clipboard_text().unwrap_or_default();
        let url = text.lines().map(str::trim).find(|l| l.starts_with("http://") || l.starts_with("https://")).map(str::to_string);
        match url {
            Some(url) => {
                self.url = url;
                self.add();
            }
            None => self.status = "The clipboard does not contain an http(s) link. Copy a download link first.".into(),
        }
    }

    fn stop_all_for_exit(&self) {
        for job in &self.jobs {
            if matches!(job.status(), Status::Downloading) {
                job.cancel.store(true, Ordering::SeqCst);
                job.set_status(Status::Paused);
            }
        }
    }

    pub(crate) fn open_updates(&mut self, ctx: &egui::Context, check: bool) {
        self.updates_open = true;
        if check && !matches!(update::get(&self.update_state), Phase::Checking | Phase::Downloading { .. }) {
            update::check_async(&self.update_state, self.update_url_text.trim().to_string(), Some(ctx.clone()));
        }
    }

    /// Small hint in the menu bar when the startup check found an update.
    pub(crate) fn update_badge(&mut self, ui: &mut egui::Ui) {
        if let Phase::Available(info) = update::get(&self.update_state) {
            let text = RichText::new(format!("Update available: {}", info.version)).size(11.5).color(ACCENT());
            if ui.add(egui::Button::new(text).frame(false)).on_hover_text("Help > Check for updates").clicked() {
                self.updates_open = true;
            }
        }
    }

    pub(crate) fn updates_window(&mut self, ctx: &egui::Context) {
        if !self.updates_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        egui::Window::new(RichText::new("Help - Updates").family(semibold()).size(15.0).color(TEXT()))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(Self::modal_frame())
            .show(ctx, |ui| {
                ui.set_min_width(540.0);
                section_header(ui, "Updates", "Checks for a newer version of Pro Internet Manager (also once at every start).");
                ui.label(RichText::new(format!("Installed version: {}", update::APP_VERSION)).family(semibold()).color(TEXT()));
                ui.add_space(6.0);
                self.connection_test_ui(ui, ctx);
                ui.add_space(8.0);
                let phase = update::get(&self.update_state);
                let busy = matches!(phase, Phase::Checking | Phase::Downloading { .. });
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(!busy, |ui| action_button(ui, "Check for updates", true)).inner.clicked() {
                        update::check_async(&self.update_state, self.update_url_text.trim().to_string(), Some(ctx.clone()));
                    }
                });
                ui.add_space(8.0);
                card().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.update_result(ui, ctx, phase);
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Nothing is installed until you click Apply update.").size(11.0).color(DIM()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if action_button(ui, "Close", false).clicked() {
                            close = true;
                        }
                    });
                });
            });
        self.updates_open = open && !close;
    }

    fn update_result(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, phase: Phase) {
        match phase {
            Phase::Idle => {
                ui.label(RichText::new("Click \"Check for updates\" to look for a newer version.").color(DIM()));
            }
            Phase::NotConfigured => {
                ui.label(RichText::new("Update source not configured").family(semibold()).color(WARN()));
                ui.label(RichText::new("The update server could not be reached. Try again later.").size(11.5).color(TEXT()));
            }
            Phase::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Checking...").color(TEXT()));
                });
            }
            Phase::UpToDate(latest) => {
                ui.label(RichText::new("You have the latest version").family(semibold()).size(15.0).color(OK()));
                ui.label(RichText::new(format!("Installed: {}. Latest available: {latest}.", update::APP_VERSION)).size(11.5).color(DIM()));
            }
            Phase::CheckFailed(error) => {
                ui.label(RichText::new("Update check failed").family(semibold()).color(ERR()));
                ui.label(RichText::new(error).size(11.5).color(TEXT()));
                ui.label(RichText::new("Details are in pim.log (Options > General > Open log).").size(11.0).color(DIM()));
            }
            Phase::Available(info) => {
                ui.label(RichText::new(format!("New version {} available", info.version)).family(semibold()).size(15.0).color(ACCENT()));
                ui.label(RichText::new(format!("Installed: {}.", update::APP_VERSION)).size(11.5).color(DIM()));
                if !info.notes.is_empty() {
                    ui.add_space(2.0);
                    field_label(ui, "What's new");
                    ui.label(RichText::new(&info.notes).size(12.0).color(TEXT()));
                }
                if info.is_patch() {
                    ui.label(RichText::new("Patch update: only changed files are replaced; the old files are kept as a backup.").size(11.5).color(DIM()));
                }
                ui.add_space(6.0);
                if action_button(ui, "Download update", true).clicked() {
                    crate::pimlog::info(format!("Update: download {} started by the user", info.version));
                    update::download_async(&self.update_state, info, ctx.clone());
                }
            }
            Phase::Downloading { info, done, total } => {
                ui.label(RichText::new(format!("Downloading {}...", info.version)).color(TEXT()));
                let fraction = total.map(|t| done as f32 / t.max(1) as f32).unwrap_or(0.0);
                ui.add(egui::ProgressBar::new(fraction).desired_width(ui.available_width()).text(match total {
                    Some(t) => format!("{} / {}", update::human_bytes(done), update::human_bytes(t)),
                    None => update::human_bytes(done),
                }));
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Phase::Downloaded { info, path } => {
                ui.label(RichText::new(format!("Update {} downloaded", info.version)).family(semibold()).color(OK()));
                ui.label(RichText::new(path.display().to_string()).size(11.5).color(DIM()));
                ui.label(RichText::new(if info.sha256.is_some() { "Size and sha256 checksum verified." } else { "Size verified (no sha256 in the version file)." }).size(11.5).color(DIM()));
                ui.add_space(6.0);
                if info.is_patch() {
                    let dir = update::install_dir().unwrap_or_default();
                    let writable = update::dir_writable(&dir);
                    if !writable {
                        ui.label(
                            RichText::new(format!(
                                "{} needs administrator rights. Windows asks for permission (UAC) when you click Apply update.",
                                dir.display()
                            ))
                            .size(11.5)
                            .color(WARN()),
                        );
                    }
                    ui.label(RichText::new("Pro Internet Manager closes, applies the patch and starts again. Active downloads are paused and can be resumed.").size(11.5).color(DIM()));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if action_button(ui, "Apply update", true).clicked() {
                            crate::pimlog::info(format!("Update: user clicked Apply update ({})", info.version));
                            match update::start_patch_apply(&path, &[]) {
                                Ok(elevated) => {
                                    self.stop_all_for_exit();
                                    self.status = format!("Applying update {}{}...", info.version, if elevated { " (as administrator)" } else { "" });
                                    self.exit_now = true;
                                }
                                Err(error) => {
                                    crate::pimlog::error(format!("Update: {error}"));
                                    self.status = error;
                                }
                            }
                        }
                        if action_button(ui, "Show file", false).clicked() {
                            update::open_folder_select(&path);
                        }
                    });
                    return;
                }
                ui.horizontal(|ui| {
                    let is_installer = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi"));
                    if is_installer && action_button(ui, "Run installer", true).clicked() {
                        match update::run_installer(&path) {
                            Ok(true) => self.installer_close_prompt = true,
                            Ok(false) => {}
                            Err(error) => {
                                crate::pimlog::error(format!("Update: {error}"));
                                self.status = error;
                            }
                        }
                    }
                    if action_button(ui, "Show file", !is_installer).clicked() {
                        update::open_folder_select(&path);
                    }
                });
            }
            Phase::DownloadFailed { info, error } => {
                ui.label(RichText::new("Download failed").family(semibold()).color(ERR()));
                ui.label(RichText::new(error).size(11.5).color(TEXT()));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if action_button(ui, "Try again", true).clicked() {
                        update::download_async(&self.update_state, info, ctx.clone());
                    }
                    if action_button(ui, "Open log", false).clicked() {
                        crate::ui::open_log_file();
                    }
                });
            }
        }
    }
}

/// Text on the Windows clipboard (CF_UNICODETEXT), if any.
fn read_clipboard_text() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let mut out = None;
        let handle = GetClipboardData(CF_UNICODETEXT);
        if !handle.is_null() {
            let ptr = GlobalLock(handle) as *const u16;
            if !ptr.is_null() {
                let mut len = 0usize;
                while *ptr.add(len) != 0 && len < 1 << 20 {
                    len += 1;
                }
                out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len)));
                GlobalUnlock(handle);
            }
        }
        CloseClipboard();
        out
    }
}

// ---------------------------------------------------------------------------------------------
// 0.2.2: "Test connection" (Options > Browser bridge and Help > Updates).
// Tests the local bridge (GET http://127.0.0.1:18500/ping) and the update URL (version.txt,
// redirects such as short links are followed) and shows OK or the reason.

#[derive(Clone, Default)]
pub(crate) enum ConnPhase {
    #[default]
    Idle,
    Running,
    /// (level 0 = OK, 1 = warning, 2 = error, text)
    Done(Vec<(u8, String)>),
}

pub(crate) type ConnShared = std::sync::Arc<std::sync::Mutex<ConnPhase>>;

pub(crate) fn conn_new_shared() -> ConnShared {
    std::sync::Arc::new(std::sync::Mutex::new(ConnPhase::Idle))
}

fn ping_bridge() -> Result<(u128, String), String> {
    use std::io::{Read as _, Write as _};
    let started = std::time::Instant::now();
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], api_port()));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::ConnectionRefused {
            format!("nothing is listening on 127.0.0.1:{API_PORT} (connection refused)")
        } else if e.kind() == std::io::ErrorKind::TimedOut {
            format!("127.0.0.1:{API_PORT} did not answer within 3 s")
        } else {
            format!("could not connect to 127.0.0.1:{API_PORT}: {e}")
        }
    })?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(4)));
    stream
        .write_all(format!("GET /ping HTTP/1.1\r\nHost: 127.0.0.1:{API_PORT}\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(|e| format!("could not send /ping: {e}"))?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 2048];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buf[..n]);
                if bytes.len() > 16 * 1024 {
                    break;
                }
            }
            Err(e) if bytes.is_empty() => return Err(format!("no answer to /ping: {e}")),
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    let status_line = text.lines().next().unwrap_or("").trim().to_string();
    if status_line.is_empty() {
        return Err(format!("127.0.0.1:{API_PORT} closed the connection without an answer"));
    }
    if !status_line.contains(" 200") {
        return Err(format!("/ping answered '{status_line}'"));
    }
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").trim().to_string();
    Ok((started.elapsed().as_millis(), body))
}

pub(crate) fn connection_test_async(shared: &ConnShared, version_url: String, ctx: egui::Context) {
    if let Ok(mut phase) = shared.lock() {
        if matches!(*phase, ConnPhase::Running) {
            return;
        }
        *phase = ConnPhase::Running;
    }
    let shared = shared.clone();
    std::thread::spawn(move || {
        let mut results: Vec<(u8, String)> = Vec::new();
        crate::pimlog::info("Test connection started");
        match ping_bridge() {
            Ok((ms, body)) => {
                let who = match api_state() {
                    ApiState::Listening => "answered by Pro Internet Manager",
                    ApiState::Failed => "answered by another app that uses the port (this app could not bind it)",
                    ApiState::Off => "answered by the app that uses the port",
                };
                let ok = body.contains("\"ok\"");
                results.push((
                    if ok { if api_state() == ApiState::Failed { 1 } else { 0 } } else { 1 },
                    format!("Local bridge 127.0.0.1:{API_PORT}/ping: OK in {ms} ms, {who}{}", if ok { "" } else { " (unexpected answer)" }),
                ));
            }
            Err(e) => results.push((2, format!("Local bridge: {e}"))),
        }
        match bridge_status() {
            Some((ago, kind)) => results.push((0, format!("Browser extension: {kind} extension pinged {ago} s ago"))),
            None => results.push((1, "Browser extension: no ping in the last 75 s (is the extension installed and enabled?)".to_string())),
        }
        let url = if version_url.trim().is_empty() { update::DEFAULT_VERSION_URL.to_string() } else { version_url.trim().to_string() };
        let started = std::time::Instant::now();
        match update::fetch_direct(&url) {
            Ok(info) => results.push((
                0,
                format!(
                    "Update server: OK in {} ms, version file found (latest {}{})",
                    started.elapsed().as_millis(),
                    info.version,
                    if update::is_newer(&info.version, update::APP_VERSION) { ", newer than this version" } else { ", you are up to date" }
                ),
            )),
            Err(e) => {
                results.push((2, format!("Update server: {}", crate::pimlog::tail(&update::strip_urls(&e), 240))));
                if !update::FALLBACK_VERSION_URL.is_empty() && update::normalize_url(&url) != update::FALLBACK_VERSION_URL {
                    match update::fetch_direct(update::FALLBACK_VERSION_URL) {
                        Ok(info) => results.push((1, format!("Second update source works (latest {}); update checks fall back to it", info.version))),
                        Err(e) => results.push((2, format!("Second update source: {}", crate::pimlog::tail(&update::strip_urls(&e), 200)))),
                    }
                }
            }
        }
        for (level, text) in &results {
            let line = format!("Test connection: {text}");
            match level {
                0 => crate::pimlog::info(line),
                1 => crate::pimlog::warn(line),
                _ => crate::pimlog::error(line),
            }
        }
        if let Ok(mut phase) = shared.lock() {
            *phase = ConnPhase::Done(results);
        }
        ctx.request_repaint();
    });
}

impl App {
    /// "Test connection" button plus its result lines.
    pub(crate) fn connection_test_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if std::mem::take(&mut self.conn_test_autostart) {
            connection_test_async(&self.conn_test, self.update_url_text.clone(), ctx.clone());
        }
        let phase = self.conn_test.lock().map(|p| p.clone()).unwrap_or_default();
        let running = matches!(phase, ConnPhase::Running);
        ui.horizontal(|ui| {
            if ui
                .add_enabled_ui(!running, |ui| action_button(ui, "Test connection", false))
                .inner
                .on_hover_text("Tests the local browser bridge, the browser extension and the update server")
                .clicked()
            {
                connection_test_async(&self.conn_test, self.update_url_text.clone(), ctx.clone());
            }
            if running {
                ui.add(egui::Spinner::new().size(14.0).color(ACCENT()));
                ui.label(RichText::new("Testing\u{2026}").size(12.0).color(DIM()));
            }
        });
        if let ConnPhase::Done(results) = phase {
            ui.add_space(4.0);
            for (level, text) in results {
                let (mark, color) = match level {
                    0 => ("\u{2714}", OK()),
                    1 => ("!", WARN()),
                    _ => ("\u{2716}", ERR()),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(mark).size(12.0).family(semibold()).color(color));
                    ui.label(RichText::new(text).size(12.0).color(if level == 2 { ERR() } else { TEXT() }));
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// 0.2.3: in-app update prompts (no Windows notifications).

impl App {
    /// "A new update is available" (after the startup check) and, once after an update,
    /// "This app has been updated to vX" with the patch notes of that version.
    pub(crate) fn update_prompts(&mut self, ctx: &egui::Context) {
        if let Some(version) = self.updated_prompt.clone() {
            let notes = update::notes_for(&version);
            let mut ok = false;
            egui::Window::new(RichText::new(format!("This app has been updated to v{version}")).family(semibold()).size(15.0).color(TEXT()))
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .frame(Self::modal_frame())
                .show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.label(RichText::new(format!("Pro Internet Manager {version} is installed and running. What's new:")).size(12.5).color(DIM()));
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(BG())
                        .stroke(Stroke::new(1.0, CARD_BORDER()))
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .id_salt("pim_updated_notes")
                                .max_height(170.0)
                                .auto_shrink([false, false])
                                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                                .show(ui, |ui| {
                                    for line in notes.lines() {
                                        ui.label(RichText::new(line).size(12.5).color(TEXT()));
                                        ui.add_space(2.0);
                                    }
                                });
                        });
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if action_button(ui, "OK", true).clicked() {
                            ok = true;
                        }
                    });
                });
            if ok {
                crate::pimlog::info(format!("Update: \"updated to v{version}\" prompt closed"));
                self.updated_prompt = None;
            }
            return;
        }
        if !self.update_prompt {
            return;
        }
        let Phase::Available(info) = update::get(&self.update_state) else {
            self.update_prompt = false;
            return;
        };
        let mut update_now = false;
        let mut later = false;
        egui::Window::new(RichText::new("A new update is available").family(semibold()).size(15.0).color(TEXT()))
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(Self::modal_frame())
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(RichText::new(format!("Pro Internet Manager {} is available. You have {}.", info.version, update::APP_VERSION)).color(TEXT()));
                if !info.notes.trim().is_empty() {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .id_salt("pim_update_prompt_notes")
                        .max_height(110.0)
                        .show(ui, |ui| {
                            ui.label(RichText::new(info.notes.trim()).size(12.0).color(DIM()));
                        });
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if action_button(ui, "Update", true).clicked() {
                        update_now = true;
                    }
                    if action_button(ui, "Later", false).clicked() {
                        later = true;
                    }
                });
            });
        if update_now {
            crate::pimlog::info(format!("Update: user clicked Update in the prompt ({})", info.version));
            self.update_prompt = false;
            self.updates_open = true;
            if !crate::autostart::is_demo() {
                update::download_async(&self.update_state, info, ctx.clone());
            }
        } else if later {
            crate::pimlog::info(format!("Update: user chose Later ({})", info.version));
            self.update_prompt = false;
        }
    }
}
