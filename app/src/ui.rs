//! Layout / presentation of Pro Internet Manager (restyle). Replaces the original ui.rs.
//! Only reads and drives App state through the same calls the original UI used
//! (add, start, remove, save_config, ...). All styling comes from `theme`.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use egui::{Color32, Rect, RichText, Sense, Stroke, pos2, vec2};

use super::*;
use crate::theme::*;

impl App {
    fn settings_window(&mut self, ctx: &egui::Context) {
        crate::autostart::sync_on_open(self.settings_open);
        let mut open = self.settings_open;
        let mut close = false;
        egui::Window::new(RichText::new("Options").family(semibold()).size(15.0).color(TEXT()))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(780.0)
            .min_width(680.0)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .frame(
                egui::Frame::new()
                    .fill(CARD())
                    .stroke(Stroke::new(1.0, alpha(ACCENT(), 110)))
                    .corner_radius(CARD_RADIUS + 2.0)
                    .inner_margin(egui::Margin::same(14))
                    .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(170) }),
            )
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new("SETTINGS").family(semibold()).size(11.0).color(ACCENT()));
                        ui.add_space(4.0);
                        for (tab, label, kind) in [
                            (SettingsTab::General, "General", Icon::Gear),
                            (SettingsTab::Appearance, "Appearance", Icon::Palette),
                            (SettingsTab::Connection, "Connection", Icon::Down),
                            (SettingsTab::Scheduler, "Scheduler", Icon::Clock),
                            (SettingsTab::Streams, "Streams", Icon::Wave),
                            (SettingsTab::Firefox, "Browser bridge", Icon::Plug),
                            (SettingsTab::About, "About", Icon::Info),
                        ] {
                            if nav_row(ui, kind, label, self.settings_tab == tab) {
                                self.settings_tab = tab;
                            }
                        }
                        ui.add_space(10.0);
                        ui.label(RichText::new("Changes are saved automatically.").size(11.0).color(DIM()));
                    });
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.set_min_width(540.0);
                        card().show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.set_min_height(300.0);
                            match self.settings_tab {
                                SettingsTab::General => {
                                    section_header(ui, "General", "Where files go and how the queue starts.");
                                    field_label(ui, "Default download folder");
                                    ui.horizontal(|ui| {
                                        let edit = ui.add_sized([380.0, 30.0], egui::TextEdit::singleline(&mut self.folder_text).margin(vec2(8.0, 6.0)));
                                        if edit.lost_focus() {
                                            let candidate = PathBuf::from(self.folder_text.trim());
                                            if candidate.is_dir() {
                                                self.folder = candidate;
                                                self.save_config();
                                            } else {
                                                self.folder_text = self.folder.display().to_string();
                                            }
                                        }
                                        if action_button(ui, "Browse", false).clicked() {
                                            if let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
                                                self.folder = folder;
                                                self.folder_text = self.folder.display().to_string();
                                                self.save_config();
                                            }
                                        }
                                    });
                                    ui.add_space(8.0);
                                    if check_box(ui, &mut self.auto_start, "Start queued downloads automatically").changed() {
                                        self.save_config();
                                    }
                                    ui.add_space(4.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Maximum active downloads").color(TEXT()));
                                        if ui.add(egui::DragValue::new(&mut self.max_active).range(1..=8)).changed() {
                                            self.save_config();
                                        }
                                    });
                                    ui.add_space(8.0);
                                    let mut autostart = crate::autostart::cached();
                                    if check_box(ui, &mut autostart, "Start with Windows").changed() {
                                        crate::autostart::toggle(autostart);
                                    }
                                    ui.label(RichText::new("Starts minimized when you sign in, so the browser bridge is ready right away.").size(11.5).color(DIM()));
                                    if let Some(error) = crate::autostart::last_error() {
                                        ui.label(RichText::new(error).size(11.5).color(ERR()));
                                    }
                                    ui.add_space(10.0);
                                    field_label(ui, "Log file");
                                    ui.horizontal(|ui| {
                                        if action_button(ui, "Open log", false).clicked() {
                                            open_log_file();
                                        }
                                        if action_button(ui, "Open log folder", false).clicked() {
                                            let path = crate::pimlog::ensure_exists();
                                            let _ = std::process::Command::new("explorer").arg(path.parent().unwrap_or(&path)).spawn();
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{}  -  why a download failed (yt-dlp / ffmpeg output, HTTP codes). Cookie and token values are never written.",
                                            crate::pimlog::log_path().display()
                                        ))
                                        .size(11.5)
                                        .color(DIM()),
                                    );
                                }
                                SettingsTab::Appearance => {
                                    section_header(ui, "Appearance", "Colour presets. Pro Lime is the default look.");
                                    for kind in ThemeKind::ALL {
                                        if theme_row(ui, kind, self.theme_kind == kind) && self.theme_kind != kind {
                                            self.theme_kind = kind;
                                            self.save_config();
                                            if !crate::autostart::is_demo() {
                                                crate::theme::save_own_theme(kind);
                                            }
                                        }
                                    }
                                    ui.add_space(4.0);
                                    ui.label(RichText::new("All colours, icons and helpers live in src/theme.rs; ui.rs only uses them. The choice is stored in config.json.").size(11.5).color(DIM()));
                                }
                                SettingsTab::Connection => {
                                    section_header(ui, "Connection", "Segmented downloads over HTTP Range.");
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Parallel connections per file").color(TEXT()));
                                        if ui.add(egui::DragValue::new(&mut self.connections).range(1..=16)).changed() {
                                            self.save_config();
                                        }
                                    });
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("Servers without Range support always use one connection. Paused part files are reused when a download resumes.")
                                            .color(DIM()),
                                    );
                                }
                                SettingsTab::Scheduler => {
                                    section_header(ui, "Scheduler", "A daily time window for the queue.");
                                    if check_box(ui, &mut self.schedule_enabled, "Enable daily scheduler").changed() {
                                        self.save_config();
                                    }
                                    ui.add_space(6.0);
                                    // 0.2.1: Start / Stop are greyed out while the scheduler is off.
                                    let enabled = self.schedule_enabled;
                                    ui.add_enabled_ui(enabled, |ui| {
                                        ui.horizontal(|ui| {
                                            field_label(ui, "Start");
                                            if ui.add_sized([80.0, 30.0], egui::TextEdit::singleline(&mut self.schedule_start).margin(vec2(8.0, 6.0))).changed() {
                                                self.save_config();
                                            }
                                            ui.add_space(8.0);
                                            field_label(ui, "Stop");
                                            if ui.add_sized([80.0, 30.0], egui::TextEdit::singleline(&mut self.schedule_stop).margin(vec2(8.0, 6.0))).changed() {
                                                self.save_config();
                                            }
                                        });
                                    });
                                    ui.add_space(4.0);
                                    if !enabled {
                                        ui.label(RichText::new("The scheduler is off: downloads start as soon as they are queued.").color(DIM()));
                                    } else if parse_clock(&self.schedule_start).is_none() || parse_clock(&self.schedule_stop).is_none() {
                                        ui.label(RichText::new("Use HH:MM, for example 01:00 and 07:00.").color(ERR()));
                                    } else {
                                        ui.label(RichText::new("Times are local. A start after the stop time runs overnight.").color(TEXT()));
                                        ui.add_space(2.0);
                                        let (c, t) = if self.schedule_active {
                                            (OK(), "Scheduler window is open; queued downloads may start.")
                                        } else {
                                            (WARN(), "Scheduler window is closed; active downloads are paused.")
                                        };
                                        chip(ui, c, t);
                                    }
                                    ui.add_space(8.0);
                                    ui.horizontal(|ui| {
                                        if action_button(ui, "Start queue now", true).clicked() {
                                            for job in &self.jobs {
                                                if matches!(job.status(), Status::Queued | Status::Paused) {
                                                    start(job);
                                                }
                                            }
                                        }
                                        if action_button(ui, "Stop active now", false).clicked() {
                                            for job in &self.jobs {
                                                job.cancel.store(true, Ordering::SeqCst);
                                                if job.status() == Status::Downloading {
                                                    job.set_status(Status::Paused);
                                                }
                                            }
                                        }
                                    });
                                }
                                SettingsTab::Streams => {
                                    section_header(ui, "HLS / DASH / YouTube", "Streams are handled by ffmpeg, YouTube links by yt-dlp.");
                                    match find_ffmpeg() {
                                        Some(path) => {
                                            chip(ui, OK(), "ffmpeg found");
                                            ui.label(RichText::new(path.display().to_string()).monospace().color(TEXT()));
                                        }
                                        None => {
                                            chip(ui, ERR(), "ffmpeg.exe not found");
                                            ui.label(RichText::new("Place ffmpeg.exe next to the Pro Internet Manager exe, in bin\\, or on PATH.").color(TEXT()));
                                        }
                                    }
                                    ui.add_space(4.0);
                                    match find_ytdlp() {
                                        Some(path) => {
                                            // 0.2.2: version of the yt-dlp that downloads use
                                            let info = crate::ytdlp_update::info();
                                            let label = match &info.version {
                                                Some(v) => format!("yt-dlp {v}"),
                                                None => "yt-dlp found".to_string(),
                                            };
                                            chip(ui, OK(), &label);
                                            ui.label(RichText::new(path.display().to_string()).monospace().color(TEXT()));
                                        }
                                        None => {
                                            chip(ui, ERR(), "yt-dlp.exe not found");
                                            ui.label(RichText::new("YouTube links need yt-dlp.exe next to the exe, in bin\\, or on PATH.").color(TEXT()));
                                        }
                                    }
                                    // 0.2.2: automatic yt-dlp update (at most once per 24 h, never during downloads)
                                    ui.add_space(4.0);
                                    let mut keep = crate::ytdlp_update::enabled();
                                    if check_box(ui, &mut keep, "Keep yt-dlp up to date").changed() {
                                        crate::ytdlp_update::set_enabled(keep);
                                    }
                                    ui.label(
                                        RichText::new("Checks once a day at start, in the background and never while downloads run. The previous yt-dlp.exe is kept and restored if an update fails.")
                                            .size(11.5)
                                            .color(DIM()),
                                    );
                                    ui.horizontal(|ui| {
                                        let busy = crate::ytdlp_update::is_busy();
                                        let downloads = crate::ACTIVE_DOWNLOADS.load(Ordering::SeqCst) > 0;
                                        let demo = crate::autostart::is_demo();
                                        let r = ui.add_enabled_ui(!busy && !downloads && !demo, |ui| action_button(ui, "Update yt-dlp now", false)).inner;
                                        let r = if downloads { r.on_disabled_hover_text("Not while downloads are running") } else { r };
                                        if r.clicked() {
                                            crate::ytdlp_update::update_async(Some(ctx.clone()));
                                        }
                                        if busy {
                                            ui.add(egui::Spinner::new().size(14.0).color(ACCENT()));
                                            ui.label(RichText::new("Updating yt-dlp\u{2026}").size(12.0).color(DIM()));
                                        }
                                    });
                                    let last = crate::ytdlp_update::info().last_result;
                                    if !last.is_empty() {
                                        let color = if last.starts_with("Update failed") { ERR() } else { DIM() };
                                        ui.label(RichText::new(format!("Last check: {last}")).size(11.5).color(color));
                                    }
                                    ui.add_space(6.0);
                                    if action_button(ui, "Open application folder", false).clicked() {
                                        if let Ok(exe) = std::env::current_exe() {
                                            if let Some(dir) = exe.parent() {
                                                let _ = std::process::Command::new("explorer").arg(dir).spawn();
                                            }
                                        }
                                    }
                                    ui.add_space(6.0);
                                    ui.label(RichText::new("Streams are remuxed with ffmpeg -c copy. DRM (Widevine, PlayReady, FairPlay) is not supported.").color(DIM()));
                                }
                                SettingsTab::Firefox => {
                                    section_header(ui, "Browser bridge", "Send downloads from Firefox or a Chromium browser (Chrome, Edge, Brave).");
                                    ui.horizontal(|ui| {
                                        field_label(ui, "Status");
                                        match bridge_status() {
                                            Some((ago, browser)) => chip(ui, OK(), &format!("Connected \u{00b7} {browser} extension, last ping {ago} s ago")),
                                            None => chip(ui, WARN(), "Not connected \u{00b7} no extension ping in the last 75 s"),
                                        };
                                    });
                                    ui.horizontal(|ui| {
                                        field_label(ui, "Local API");
                                        match api_state() {
                                            ApiState::Listening => chip(ui, ACCENT(), &format!("127.0.0.1:{API_PORT}")),
                                            ApiState::Failed => chip(ui, ERR(), &format!("127.0.0.1:{API_PORT} is used by another app")),
                                            ApiState::Off => chip(ui, DIM(), &format!("127.0.0.1:{API_PORT} (off in preview)")),
                                        };
                                    });
                                    // 0.2.2: test 127.0.0.1:18500/ping and the update URL
                                    ui.add_space(4.0);
                                    self.connection_test_ui(ui, ctx);
                                    ui.add_space(6.0);
                                    ui.label(RichText::new("Firefox").family(semibold()).size(14.0).color(TEXT()));
                                    numbered(ui, 1, "Install the signed add-on \"Pro Internet Manager Bridge\" (button below, or from the installer's last page).");
                                    numbered(ui, 2, "Keep Pro Internet Manager running. Closing the window keeps it in the tray.");
                                    numbered(ui, 3, "Within about 15 seconds the status above changes to Connected.");
                                    if action_button(ui, "Get the Firefox add-on", false).clicked() {
                                        open_firefox_addon();
                                    }
                                    ui.add_space(8.0);
                                    ui.label(RichText::new("Chrome, Edge, Brave (Chromium)").family(semibold()).size(14.0).color(TEXT()));
                                    let folder = chromium_extension_folder();
                                    numbered(ui, 1, "Open chrome://extensions (edge://extensions in Edge) and switch on Developer mode.");
                                    numbered(ui, 2, "Click \"Load unpacked\" and choose the chromium-extension folder:");
                                    egui::Frame::new().fill(FIELD()).stroke(Stroke::new(1.0, CARD_BORDER())).corner_radius(8.0).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new(folder.display().to_string()).size(11.5).color(TEXT()));
                                    });
                                    numbered(ui, 3, "Within about 30 seconds the status above changes to Connected.");
                                    if action_button(ui, "Open Chromium extension folder", false).clicked() {
                                        let _ = std::process::Command::new("explorer").arg(&folder).spawn();
                                    }
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("The extensions forward cookies, Referer and User-Agent with each download. Stream takeover is configured in the extension.")
                                            .size(11.5)
                                            .color(DIM()),
                                    );
                                }
                                SettingsTab::About => {
                                    ui.horizontal(|ui| {
                                        if let Some(tex) = &self.icon_tex {
                                            let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                                            ui.painter().image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                                        }
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 0.0;
                                            ui.label(RichText::new("Pro Internet Manager").family(semibold()).size(18.0).color(TEXT()));
                                            ui.label(RichText::new("by Triwar Design").family(semibold()).size(12.0).color(ACCENT()));
                                        });
                                    });
                                    ui.add_space(6.0);
                                    ui.label(RichText::new(format!("Version {} - local build  (Help > Check for updates)", crate::update::APP_VERSION)).color(TEXT()));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new("Segmented HTTP downloads, resume, queue, scheduler, HLS/DASH via ffmpeg, YouTube via yt-dlp, Firefox bridge.").color(TEXT()));
                                    ui.add_space(4.0);
                                    ui.label(RichText::new("Clean-room implementation. No Internet Download Manager code or assets are used.").color(DIM()));
                                }
                            }
                        });
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Pro Internet Manager \u{00b7} by Triwar Design").size(11.0).color(DIM()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if action_button(ui, "Close", true).clicked() {
                            close = true;
                        }
                    });
                });
            });
        self.settings_open = open && !close;
        if !self.settings_open {
            // 0.2.4: Options always opens on General again (buttons that open a specific tab set it themselves).
            self.settings_tab = SettingsTab::General;
        }
    }
}

impl App {
    fn ensure_assets(&mut self, ctx: &egui::Context) {
        if self.themed == Some(self.theme_kind) {
            return;
        }
        crate::theme::apply(self.theme_kind, ctx);
        if self.bg_tex.is_none() {
            self.bg_tex = background_texture(ctx);
        }
        // The globe follows the accent of the active preset (header, About and window icon).
        self.icon_tex = icon_texture(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(window_icon_for(self.theme_kind)))));
        crate::theme::set_theme_info(self.theme_kind);
        self.themed = Some(self.theme_kind);
    }

    fn visible(&self, job: &Job) -> bool {
        match self.filter {
            Filter::All => true,
            Filter::Active => matches!(job.status(), Status::Downloading | Status::Queued | Status::Paused),
            Filter::Finished => job.status() == Status::Done,
            Filter::Errors => job.status() == Status::Error,
            Filter::Streams => job.stream,
        }
    }

    fn start_all(&self) {
        for job in &self.jobs {
            if matches!(job.status(), Status::Paused | Status::Error | Status::Queued) {
                start(job);
            }
        }
    }

    fn stop_all(&self) {
        for job in &self.jobs {
            job.cancel.store(true, Ordering::SeqCst);
            if job.status() == Status::Downloading {
                job.set_status(Status::Paused);
            }
        }
    }

    fn clear_finished(&mut self) {
        self.jobs.retain(|job| job.status() != Status::Done);
        self.selected = None;
    }

    fn menu_bar(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let frame = egui::Frame::new().fill(alpha(BAR(), 235)).inner_margin(egui::Margin { left: 10, right: 10, top: 3, bottom: 3 });
        egui::Panel::top("menu").show_separator_line(false).frame(frame).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().button_padding = vec2(10.0, 4.0);
                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
                ui.menu_button(RichText::new("File").color(TEXT()), |ui| {
                    if ui.button("Add URL").clicked() {
                        ui.close();
                    }
                    if ui.button("Options").clicked() {
                        self.settings_open = true;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        self.request_exit(&ctx);
                        ui.close();
                    }
                });
                ui.menu_button(RichText::new("Downloads").color(TEXT()), |ui| {
                    if ui.button("Start all").clicked() {
                        self.start_all();
                        ui.close();
                    }
                    if ui.button("Stop all").clicked() {
                        self.stop_all();
                        ui.close();
                    }
                    if ui.button("Clear finished").clicked() {
                        self.clear_finished();
                        ui.close();
                    }
                });
                ui.menu_button(RichText::new("View").color(TEXT()), |ui| {
                    if ui.button("Options").clicked() {
                        self.settings_open = true;
                        ui.close();
                    }
                });
                ui.menu_button(RichText::new("Help").color(TEXT()), |ui| {
                    if ui.button("Check for updates\u{2026}").clicked() {
                        self.open_updates(&ctx, true);
                        ui.close();
                    }
                    if ui.button("About").clicked() {
                        self.settings_tab = SettingsTab::About;
                        self.settings_open = true;
                        ui.close();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("F12  Options").size(11.0).color(DIM()));
                    self.update_badge(ui);
                });
            });
        });
    }

    fn header(&mut self, root: &mut egui::Ui, actions: &mut Vec<(usize, &'static str)>) {
        // 0.2.1: compacter header (34 px logo, smaller title and subtitle, smaller toolbar).
        let frame = egui::Frame::new().fill(Color32::TRANSPARENT).inner_margin(egui::Margin { left: 16, right: 16, top: 8, bottom: 0 });
        egui::Panel::top("head").show_separator_line(false).frame(frame).show(root, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                if let Some(tex) = &self.icon_tex {
                    ui.painter().image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                ui.add_space(2.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new("Pro Internet Manager").family(semibold()).size(18.0).color(TEXT()));
                    ui.label(RichText::new("by Triwar Design").family(semibold()).size(10.5).color(ACCENT()));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 0.2.1: bridge status instead of the port number (the port is in Options > Browser bridge).
                    let (color, text, hint) = match bridge_status() {
                        Some((ago, browser)) => (OK(), "Bridge connected", format!("{browser} extension last seen {ago} s ago.\nOptions > Browser bridge shows the details.")),
                        None => (
                            DIM(),
                            "Bridge not connected",
                            "No browser extension contacted Pro Internet Manager in the last 75 seconds.\nOptions > Browser bridge explains how to install it.".to_string(),
                        ),
                    };
                    let pill = chip(ui, color, text).interact(Sense::click()).on_hover_text(hint);
                    if pill.clicked() {
                        self.settings_tab = SettingsTab::Firefox;
                        self.settings_open = true;
                    }
                    let active = self.jobs.iter().filter(|j| j.status() == Status::Downloading).count();
                    chip(ui, if active > 0 { OK() } else { DIM() }, &format!("{active} active"));
                });
            });
            ui.add_space(6.0);
            // toolbar: one rounded bar with large icon tiles
            let sel = self.selected.and_then(|i| self.jobs.get(i).map(|j| (i, j.status())));
            let can_resume = sel.is_some_and(|(_, s)| matches!(s, Status::Paused | Status::Error | Status::Queued));
            let can_pause = sel.is_some_and(|(_, s)| s == Status::Downloading);
            egui::Frame::new()
                .fill(alpha(CARD(), 230))
                .stroke(Stroke::new(1.0, CARD_BORDER()))
                .corner_radius(14.0)
                .inner_margin(egui::Margin::symmetric(5, 3))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::horizontal().id_salt("toolbar").show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if tool_tile(ui, Icon::Add, "Add URL", true, true, "Add the URL from the address field") {
                                if self.url.trim().is_empty() {
                                    ui.memory_mut(|m| m.request_focus(egui::Id::unique("pim_url")));
                                    self.status = "Paste or type a download link in the URL field.".into();
                                } else {
                                    self.add();
                                }
                            }
                            tool_divider(ui);
                            if tool_tile(ui, Icon::Resume, "Resume", can_resume, false, "Start or resume the selected download") {
                                if let Some((i, _)) = sel {
                                    actions.push((i, "start"));
                                }
                            }
                            if tool_tile(ui, Icon::Pause, "Pause", can_pause, false, "Pause the selected download") {
                                if let Some((i, _)) = sel {
                                    actions.push((i, "pause"));
                                }
                            }
                            if tool_tile(ui, Icon::Delete, "Remove", sel.is_some(), false, "Remove the selected download from the list (asks first)") {
                                if let Some((i, _)) = sel {
                                    actions.push((i, "remove"));
                                }
                            }
                            tool_divider(ui);
                            if tool_tile(ui, Icon::StartAll, "Start all", !self.jobs.is_empty(), false, "Start all paused, queued and failed downloads") {
                                self.start_all();
                            }
                            if tool_tile(ui, Icon::StopAll, "Stop all", !self.jobs.is_empty(), false, "Pause every active download") {
                                self.stop_all();
                            }
                            if tool_tile(ui, Icon::Sweep, "Clear done", !self.jobs.is_empty(), false, "Remove finished downloads from the list") {
                                self.clear_finished();
                            }
                            tool_divider(ui);
                            if tool_tile(ui, Icon::Clock, "Scheduler", true, false, "Daily scheduler settings") {
                                self.settings_tab = SettingsTab::Scheduler;
                                self.settings_open = true;
                            }
                            if tool_tile(ui, Icon::Gear, "Options", true, false, "Options (F12)") {
                                self.settings_open = true;
                            }
                        });
                    });
                });
            ui.add_space(6.0);
            green_rule(ui);
        });
    }

    fn footer(&mut self, root: &mut egui::Ui) {
        let frame = egui::Frame::new().fill(alpha(BAR(), 225)).inner_margin(egui::Margin::symmetric(16, 7));
        egui::Panel::bottom("foot").show_separator_line(false).frame(frame).show(root, |ui| {
            ui.horizontal(|ui| {
                let (c, t) = if self.status.is_empty() { (DIM(), "Ready".to_string()) } else { (TEXT(), self.status.clone()) };
                ui.painter().circle_filled(ui.cursor().left_center() + vec2(4.0, 0.0), 3.5, if self.status.is_empty() { ACCENT() } else { c });
                ui.add_space(12.0);
                ui.label(RichText::new(t).size(12.0).color(c));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Folder: {}", self.folder.display())).size(11.5).color(DIM()));
                    // 0.2.2: total transfer rate of all running downloads (HTTP and yt-dlp).
                    let running: Vec<&Job> = self.jobs.iter().filter(|j| j.status() == Status::Downloading).collect();
                    if !running.is_empty() {
                        let total: f64 = running.iter().map(|j| j.speed()).sum();
                        let merging = running.iter().filter(|j| j.ytdlp && j.post_processing()).count();
                        ui.add_space(14.0);
                        let mut text = format!(
                            "{} active \u{00b7} {}/s",
                            running.len(),
                            human(total as u64)
                        );
                        if merging > 0 {
                            text.push_str(&format!(" \u{00b7} {merging} merging"));
                        }
                        ui.label(RichText::new(text).size(12.0).family(semibold()).color(ACCENT()));
                    }
                });
            });
        });
    }

    fn categories(&mut self, root: &mut egui::Ui) {
        let counts = [
            self.jobs.len(),
            self.jobs.iter().filter(|j| matches!(j.status(), Status::Downloading | Status::Queued | Status::Paused)).count(),
            self.jobs.iter().filter(|j| j.status() == Status::Done).count(),
            self.jobs.iter().filter(|j| j.status() == Status::Error).count(),
            self.jobs.iter().filter(|j| j.stream).count(),
        ];
        let frame = egui::Frame::new().fill(Color32::TRANSPARENT).inner_margin(egui::Margin { left: 16, right: 0, top: 12, bottom: 12 });
        egui::Panel::left("cats").resizable(false).exact_size(232.0).show_separator_line(false).frame(frame).show(root, |ui| {
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(RichText::new("CATEGORIES").family(semibold()).size(11.0).color(ACCENT()));
                ui.add_space(4.0);
                if category_row(ui, Icon::Tray, "All Downloads", counts[0], self.filter == Filter::All, 0.0, None) {
                    self.filter = Filter::All;
                }
                let children = [
                    (Icon::Down, "Unfinished", Filter::Active, counts[1]),
                    (Icon::Check, "Finished", Filter::Finished, counts[2]),
                    (Icon::Alert, "Errors", Filter::Errors, counts[3]),
                    (Icon::Wave, "Streams", Filter::Streams, counts[4]),
                ];
                for (n, (kind, label, filter, count)) in children.into_iter().enumerate() {
                    if category_row(ui, kind, label, count, self.filter == filter, 26.0, Some(n == 3)) {
                        self.filter = filter;
                    }
                }
            });
            ui.add_space(10.0);
            card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                    icon(ui.painter(), Icon::Plug, rect, ACCENT());
                    ui.label(RichText::new("Browser bridge").family(semibold()).size(13.0).color(TEXT()));
                });
                match bridge_status() {
                    Some((_, browser)) => ui.label(RichText::new(format!("\u{25cf} Connected ({browser})")).size(11.5).color(OK())),
                    None => ui.label(RichText::new("\u{25cb} Not connected").size(11.5).color(DIM())),
                };
                ui.label(RichText::new("Downloads from the Firefox or Chromium extension arrive here with cookies, Referer and User-Agent.").size(11.0).color(DIM()));
                if action_button(ui, "Bridge settings", false).clicked() {
                    self.settings_tab = SettingsTab::Firefox;
                    self.settings_open = true;
                }
            });
        });
    }

    fn address_card(&mut self, ui: &mut egui::Ui) {
        card().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("address").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                field_label(ui, "URL");
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 132.0).max(180.0);
                    ui.add_sized([w, 32.0], egui::TextEdit::singleline(&mut self.url).id(egui::Id::unique("pim_url")).hint_text("https://example.com/file.zip").margin(vec2(10.0, 7.0)));
                    if ui.add_sized([124.0, 32.0], button_widget("Add download", true, 13.0)).clicked() {
                        self.add();
                    }
                });
                ui.end_row();
                field_label(ui, "Save to");
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 268.0).max(160.0);
                    let folder_edit = ui.add_sized([w, 30.0], egui::TextEdit::singleline(&mut self.folder_text).margin(vec2(10.0, 6.0)));
                    if folder_edit.lost_focus() {
                        let candidate = PathBuf::from(self.folder_text.trim());
                        if candidate.is_dir() {
                            self.folder = candidate;
                            self.save_config();
                        } else {
                            self.folder_text = self.folder.display().to_string();
                        }
                    }
                    if ui.add_sized([92.0, 30.0], button_widget("Browse", false, 13.0)).clicked() {
                        if let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
                            self.folder = folder;
                            self.folder_text = self.folder.display().to_string();
                            self.save_config();
                        }
                    }
                    ui.add_space(6.0);
                    field_label(ui, "Connections");
                    if ui.add_sized([52.0, 30.0], egui::DragValue::new(&mut self.connections).range(1..=16)).changed() {
                        self.save_config();
                    }
                });
                ui.end_row();
            });
        });
    }

    fn table(&mut self, ui: &mut egui::Ui, actions: &mut Vec<(usize, &'static str)>) {
        let shown = self.jobs.iter().filter(|j| self.visible(j)).count();
        let total = self.jobs.len();
        // 0.2.1: the card gets exactly the remaining height (margins + border included), so the
        // last row is never cut off; the list scrolls inside it with a visible scroll bar.
        let card_h = (ui.available_height() - 2.0).max(140.0);
        card().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let inner_h = card_h - 22.0;
            ui.set_min_height(inner_h);
            ui.set_max_height(inner_h);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Downloads").family(semibold()).size(15.0).color(TEXT()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{shown} / {total} shown")).size(11.5).color(DIM()));
                });
            });
            ui.add_space(2.0);
            // columns: name (flex), size, status, time left, transfer rate, actions
            const FIXED: [f32; 5] = [150.0, 178.0, 92.0, 108.0, 144.0];
            let scroll_h = ui.available_height().max(60.0);
            egui::ScrollArea::both().id_salt("downloads").auto_shrink([false, false]).max_height(scroll_h).show(ui, |ui| {
                let full = ui.available_width().max(170.0 + FIXED.iter().sum::<f32>());
                let name_w = full - FIXED.iter().sum::<f32>();
                let mut xs = vec![0.0f32];
                for w in std::iter::once(name_w).chain(FIXED.iter().copied()) {
                    let last = *xs.last().unwrap();
                    xs.push(last + w);
                }
                ui.set_min_width(full);
                // header
                let (hrect, _) = ui.allocate_exact_size(vec2(full, 28.0), Sense::hover());
                ui.painter().rect_filled(hrect, 9.0, alpha(FIELD(), 240));
                ui.painter().rect_stroke(hrect, 9.0, Stroke::new(1.0, CARD_BORDER()), egui::StrokeKind::Inside);
                for (i, title) in ["File name", "Size", "Status", "Time left", "Transfer rate", "Actions"].iter().enumerate() {
                    let x = hrect.left() + xs[i] + if i == 0 { 40.0 } else { 10.0 };
                    let g = ui.painter().layout_no_wrap(title.to_string(), egui::FontId::new(11.5, semibold()), DIM());
                    ui.painter().galley(pos2(x, hrect.center().y - g.size().y / 2.0), g, DIM());
                    if i > 0 {
                        ui.painter().vline(hrect.left() + xs[i], hrect.y_range().shrink(8.0), Stroke::new(1.0, CARD_BORDER()));
                    }
                }
                ui.add_space(2.0);
                if self.jobs.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                        icon(ui.painter(), Icon::Down, r, ACCENT());
                        ui.label(RichText::new("No downloads yet").family(semibold()).size(17.0).color(TEXT()));
                        ui.label(RichText::new("Copy a download link (or a YouTube link) and paste it here.").color(DIM()));
                        ui.add_space(8.0);
                        if ui.add_sized([250.0, 36.0], button_widget("Paste URL from clipboard", true, 13.5)).on_hover_text("Adds the http(s) link that is on the clipboard").clicked() {
                            actions.push((usize::MAX, "paste"));
                        }
                        ui.add_space(2.0);
                        if ui.add_sized([250.0, 30.0], button_widget("Type a URL\u{2026}", false, 13.0)).clicked() {
                            ui.memory_mut(|m| m.request_focus(egui::Id::unique("pim_url")));
                        }
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new("Tip: with the browser extension (Firefox, Chrome, Edge, Brave) downloads arrive here automatically.")
                                .size(11.5)
                                .color(DIM()),
                        );
                        if ui.add(egui::Button::new(RichText::new("Set up the browser bridge").size(11.5).color(ACCENT())).frame(false)).clicked() {
                            actions.push((usize::MAX, "bridge"));
                        }
                    });
                } else if shown == 0 {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Nothing in this category.").color(DIM()));
                    });
                }
                ui.spacing_mut().item_spacing.y = 3.0;
                let mut stripe = false;
                for (index, job) in self.jobs.iter().enumerate() {
                    if !self.visible(job) {
                        continue;
                    }
                    stripe = !stripe;
                    let status = job.status();
                    let (rect, resp) = ui.allocate_exact_size(vec2(full, 38.0), Sense::click());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let selected = self.selected == Some(index);
                    let hovered = ui.rect_contains_pointer(rect);
                    {
                        let p = ui.painter();
                        if selected {
                            p.rect_filled(rect, 10.0, alpha(ACCENT(), 26));
                            p.rect_stroke(rect, 10.0, Stroke::new(1.5, alpha(ACCENT(), 210)), egui::StrokeKind::Inside);
                        } else if hovered {
                            p.rect_filled(rect, 10.0, alpha(BTN_HOVER(), 200));
                        } else if stripe {
                            p.rect_filled(rect, 10.0, STRIPE());
                        }
                    }
                    let col = |i: usize| rect.left() + xs[i];
                    let cy = rect.center().y;
                    let tag = if job.ytdlp { Some("YT") } else if job.stream { Some("HLS") } else { None };
                    file_icon(ui.painter(), Rect::from_center_size(pos2(col(0) + 20.0, cy), vec2(20.0, 22.0)), &job.name, tag);
                    let (g, _) = middle_ellipsis(ui, &job.name, 13.0, TEXT(), name_w - 52.0, selected);
                    ui.painter().galley(pos2(col(0) + 40.0, cy - g.size().y / 2.0), g, TEXT());
                    let size_text = job.display_size();
                    let g = truncated(ui, &size_text, 12.0, DIM(), FIXED[0] - 16.0, false);
                    ui.painter().galley(pos2(col(1) + 10.0, cy - g.size().y / 2.0), g, DIM());
                    // status column: rounded green bar while downloading, otherwise a chip with a symbol
                    let srect = Rect::from_min_max(pos2(col(2) + 10.0, cy - 9.0), pos2(col(3) - 10.0, cy + 9.0));
                    if status == Status::Downloading {
                        let total_bytes = job.total.load(Ordering::Relaxed);
                        if job.ytdlp {
                            // 0.2.2: live yt-dlp progress; merging / starting are shown as text.
                            let phase = job.phase();
                            if job.post_processing() {
                                paint_progress(ui, srect, None, &phase);
                            } else if total_bytes > 0 {
                                let p = job.progress();
                                paint_progress(ui, srect, Some(p), &format!("{:.0}%", p * 100.0));
                            } else {
                                paint_progress(ui, srect, None, if phase.is_empty() { "Starting\u{2026}" } else { phase.as_str() });
                            }
                        } else if job.stream {
                            paint_progress(ui, srect, None, "live");
                        } else if total_bytes == 0 {
                            paint_progress(ui, srect, None, &human_compact(job.done.load(Ordering::Relaxed)));
                        } else {
                            let p = job.progress();
                            paint_progress(ui, srect, Some(p), &format!("{:.0}%", p * 100.0));
                        }
                    } else {
                        let c = status_color(status);
                        let text = if status == Status::Paused && job.total.load(Ordering::Relaxed) > 0 {
                            format!("{} \u{00b7} {:.0}%", status.label(), job.progress() * 100.0)
                        } else {
                            status.label().to_string()
                        };
                        let g = ui.painter().layout_no_wrap(text, egui::FontId::proportional(11.5), c);
                        let chip_rect = Rect::from_min_size(pos2(srect.left(), cy - 11.0), vec2((g.size().x + 32.0).min(srect.width()), 22.0));
                        paint_status_chip(ui.painter(), chip_rect, c, g, status_icon(status));
                    }
                    let short_error = if status == Status::Error { job.short_error() } else { None };
                    if status == Status::Error {
                        // 0.2.2: short reason in the row (time left + transfer rate columns); full text in the tooltip and pim.log.
                        let reason = short_error.clone().unwrap_or_else(|| "Download failed".to_string());
                        let g = truncated(ui, &reason, 12.0, ERR(), FIXED[2] + FIXED[3] - 20.0, false);
                        ui.painter().galley(pos2(col(3) + 10.0, cy - g.size().y / 2.0), g, ERR());
                    } else {
                        let speed = job.speed();
                        let eta = job.eta();
                        let (t, c) = if status == Status::Downloading && !eta.is_empty() { (eta.as_str(), TEXT()) } else { ("\u{2013}", DIM()) };
                        let g = ui.painter().layout_no_wrap(t.to_string(), egui::FontId::proportional(12.0), c);
                        ui.painter().galley(pos2(col(3) + 10.0, cy - g.size().y / 2.0), g, c);
                        let (t, c) = if status == Status::Downloading && speed > 0.0 { (format!("{}/s", human(speed as u64)), ACCENT()) } else { ("\u{2013}".to_string(), DIM()) };
                        let g = ui.painter().layout_no_wrap(t, egui::FontId::new(12.0, semibold()), c);
                        ui.painter().galley(pos2(col(4) + 10.0, cy - g.size().y / 2.0), g, c);
                    }
                    // row actions: 0.2.1 only on the hovered or selected row; 0.2.2 failed rows always show Retry + Open log
                    if hovered || selected || status == Status::Error {
                        let id = ui.scope_id().with(("row", index));
                        let mut bx = col(5) + 8.0;
                        let mut slot = || {
                            let r = Rect::from_center_size(pos2(bx + 14.0, cy), vec2(28.0, 28.0));
                            bx += 32.0;
                            r
                        };
                        if status == Status::Error {
                            if row_icon_button(ui, slot(), id.with("retry"), Icon::Retry, ACCENT(), "Retry") {
                                actions.push((index, "retry"));
                            }
                            if row_icon_button(ui, slot(), id.with("log"), Icon::Log, TEXT(), "Open log (pim.log)") {
                                actions.push((index, "log"));
                            }
                        } else if matches!(status, Status::Paused | Status::Queued) {
                            if row_icon_button(ui, slot(), id.with("start"), Icon::Resume, ACCENT(), "Start") {
                                actions.push((index, "start"));
                            }
                        } else if status == Status::Downloading {
                            if row_icon_button(ui, slot(), id.with("pause"), Icon::Pause, WARN(), "Pause") {
                                actions.push((index, "pause"));
                            }
                        } else {
                            slot();
                        }
                        if row_icon_button(ui, slot(), id.with("open"), Icon::Folder, TEXT(), "Open folder") {
                            actions.push((index, "open"));
                        }
                        if row_icon_button(ui, slot(), id.with("remove"), Icon::Delete, ERR(), "Remove from list\u{2026}") {
                            actions.push((index, "remove"));
                        }
                    }
                    let mut hover = format!("{}\n{}\nSize: {size_text}", job.name, job.url);
                    if status == Status::Downloading && job.ytdlp {
                        let phase = job.phase();
                        if !phase.is_empty() {
                            hover = format!("{hover}\n{phase}");
                        }
                    }
                    if let Some(q) = job.quality {
                        hover = format!("{hover}\nQuality: up to {q}p");
                    }
                    if let Some(error) = job.error() {
                        if !error.is_empty() {
                            hover = format!("{hover}\n\n{error}");
                        }
                    }
                    let resp = resp.on_hover_text(hover);
                    if resp.clicked() {
                        self.selected = Some(index);
                    }
                }
            });
        });
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        clear_color()
    }

    /// Pro Internet Manager: eframe only calls `logic` (not `ui`) while the window is minimized,
    /// e.g. after "Start with Windows". Run the same queue calls as at the top of `ui` then,
    /// so Firefox bridge requests and the scheduler keep working in the background.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Close-to-tray, tray Exit and --minimized (runs while hidden too).
        self.tray_logic(ctx);
        if ctx.input(|i| i.viewport().minimized == Some(true)) || crate::tray::is_hidden() {
            while let Ok(request) = self.api_rx.try_recv() {
                self.add_url(request.url, request.headers, request.quality);
            }
            self.apply_schedule();
            if self.auto_start {
                self.schedule();
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ensure_assets(&ctx);
        while let Ok(request) = self.api_rx.try_recv() {
            self.add_url(request.url, request.headers, request.quality);
        }
        self.apply_schedule();
        let mut actions: Vec<(usize, &'static str)> = Vec::new();
        if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            self.settings_open = true;
        }
        if self.confirm_remove.is_none() && !self.settings_open && ui.memory(|m| m.focused().is_none()) && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            if let Some(index) = self.selected {
                actions.push((index, "remove"));
            }
        }

        paint_background(ui, self.bg_tex.as_ref());
        self.menu_bar(ui);
        self.header(ui, &mut actions);
        self.footer(ui);
        self.categories(ui);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::TRANSPARENT).inner_margin(egui::Margin { left: 12, right: 16, top: 12, bottom: 12 }))
            .show(ui, |ui| {
                self.address_card(ui);
                ui.add_space(4.0);
                self.table(ui, &mut actions);
            });

        for (index, action) in actions {
            match action {
                "start" => {
                    if let Some(job) = self.jobs.get(index) {
                        start(job);
                    }
                }
                "retry" => {
                    if let Some(job) = self.jobs.get(index) {
                        crate::pimlog::info(format!("Retry by the user: '{}'", job.name));
                        start(job);
                        self.status = format!("Retrying {}\u{2026}", job.name);
                    }
                }
                "log" => open_log_file(),
                "pause" => {
                    if let Some(job) = self.jobs.get(index) {
                        job.cancel.store(true, Ordering::SeqCst);
                        job.set_status(Status::Paused);
                    }
                }
                "open" => {
                    if let Some(job) = self.jobs.get(index) {
                        let _ = std::process::Command::new("explorer").arg(&job.dest).spawn();
                    }
                }
                "remove" => {
                    // 0.2.1: ask first (see extras_ui.rs remove_dialog)
                    if let Some(job) = self.jobs.get(index) {
                        self.confirm_remove = Some((index, job.name.clone()));
                    }
                }
                "paste" => self.paste_from_clipboard(),
                "bridge" => {
                    self.settings_tab = SettingsTab::Firefox;
                    self.settings_open = true;
                }
                _ => {}
            }
        }
        if self.selected.is_some_and(|s| s >= self.jobs.len()) {
            self.selected = None;
        }
        if self.auto_start {
            self.schedule();
        }
        self.settings_window(&ctx);
        self.updates_window(&ctx);
        self.update_prompts(&ctx);
        self.exit_dialogs(&ctx);
        self.remove_dialog(&ctx);
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/// Screenshot/preview helper: `--demo` fills the list with sample rows (no network, no API, no config writes).
pub(crate) fn demo_jobs(app: &mut App) {
    let rows: [(&str, Status, u64, u64, bool); 8] = [
        ("Ubuntu-24.04-desktop-amd64.iso", Status::Downloading, 6_012_954_624, 2_405_181_849, false),
        ("trailer_1080p.m3u8.mp4", Status::Downloading, 0, 48_000_000, true),
        ("youtube-dQw4w9WgXcQ.mp4", Status::Queued, 0, 0, true),
        ("ComfyUI_portable_v0.4.7z", Status::Paused, 1_932_735_283, 1_101_659_111, false),
        ("project-assets-final.zip", Status::Done, 681_574_400, 681_574_400, false),
        ("lecture-week-07.mp3", Status::Done, 54_525_952, 54_525_952, false),
        ("installer-setup-x64.exe", Status::Error, 0, 0, false),
        ("annual-report-2026.pdf", Status::Done, 4_823_449, 4_823_449, false),
    ];
    for (name, status, total, done, stream) in rows {
        let ytdlp = name.starts_with("youtube-");
        let url = if ytdlp { "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string() } else { format!("https://example.invalid/{name}") };
        let job = Job::new(url, app.folder.clone(), name.to_string(), app.connections, Vec::new(), stream, ytdlp, if ytdlp { Some(1080) } else { None });
        job.total.store(total, Ordering::Relaxed);
        job.done.store(done, Ordering::Relaxed);
        job.set_status(status);
        if status == Status::Error {
            job.set_error("demo: connection refused".into());
        }
        app.jobs.push(job);
    }
    let animated: Vec<Job> = app.jobs.iter().filter(|j| j.status() == Status::Downloading).cloned().collect();
    thread::spawn(move || loop {
        for (n, job) in animated.iter().enumerate() {
            job.done.fetch_add(if n == 0 { 1_480_000 } else { 260_000 }, Ordering::Relaxed);
        }
        thread::sleep(Duration::from_millis(100));
    });
    app.selected = Some(0);
    app.status = "Added to queue.".into();
}

fn status_color(status: Status) -> Color32 {
    match status {
        Status::Queued => DIM(),
        Status::Downloading => ACCENT(),
        Status::Paused => WARN(),
        Status::Done => OK(),
        Status::Error => ERR(),
    }
}

/// Appearance tab row: swatches of the preset plus its name; returns true when clicked.
fn theme_row(ui: &mut egui::Ui, kind: ThemeKind, selected: bool) -> bool {
    let p = palette_for(kind);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 42.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 10.0, alpha(ACCENT(), 30));
        painter.rect_stroke(rect, 10.0, Stroke::new(1.2, alpha(ACCENT(), 200)), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        painter.rect_filled(rect, 10.0, BTN_HOVER());
    } else {
        painter.rect_stroke(rect, 10.0, Stroke::new(1.0, CARD_BORDER()), egui::StrokeKind::Inside);
    }
    // swatch strip: background, card, border, accent, text
    let mut x = rect.left() + 12.0;
    for c in [p.bg, p.card, p.card_border, p.accent, p.text] {
        let r = Rect::from_center_size(pos2(x + 9.0, rect.center().y), vec2(18.0, 18.0));
        painter.rect_filled(r, 5.0, c);
        painter.rect_stroke(r, 5.0, Stroke::new(1.0, alpha(DIM(), 120)), egui::StrokeKind::Inside);
        x += 22.0;
    }
    let g = painter.layout_no_wrap(kind.label().to_string(), egui::FontId::new(13.5, semibold()), TEXT());
    painter.galley(pos2(x + 10.0, rect.center().y - g.size().y / 2.0), g, TEXT());
    if selected {
        let r = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(20.0, 20.0));
        icon(painter, Icon::Check, r, ACCENT());
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// "1  text" line for the step-by-step help.
fn numbered(ui: &mut egui::Ui, n: usize, text: &str) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 9.0, alpha(ACCENT(), 40));
        ui.painter().circle_stroke(rect.center(), 9.0, Stroke::new(1.0, alpha(ACCENT(), 160)));
        let g = ui.painter().layout_no_wrap(n.to_string(), egui::FontId::new(11.0, semibold()), ACCENT());
        ui.painter().galley(rect.center() - g.size() / 2.0, g, ACCENT());
        ui.add(egui::Label::new(RichText::new(text).color(TEXT())).wrap());
    });
}

const FIREFOX_ADDON_URL: &str = "https://addons.mozilla.org/firefox/downloads/file/5184228/87f89caacae34bb9b236-0.3.2.xpi";

/// Opens the signed add-on in Firefox (falls back to the default browser).
fn open_firefox_addon() {
    let candidates = [
        std::env::var("ProgramFiles").ok().map(|p| PathBuf::from(p).join(r"Mozilla Firefox\firefox.exe")),
        std::env::var("ProgramFiles(x86)").ok().map(|p| PathBuf::from(p).join(r"Mozilla Firefox\firefox.exe")),
        std::env::var("LOCALAPPDATA").ok().map(|p| PathBuf::from(p).join(r"Mozilla Firefox\firefox.exe")),
    ];
    if let Some(firefox) = candidates.into_iter().flatten().find(|p| p.is_file()) {
        if std::process::Command::new(firefox).arg(FIREFOX_ADDON_URL).spawn().is_ok() {
            return;
        }
    }
    crate::update::open_url(FIREFOX_ADDON_URL);
}

/// chromium-extension folder next to the exe (installed by the setup), else the exe folder.
fn chromium_extension_folder() -> PathBuf {
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())).unwrap_or_else(|| PathBuf::from("."));
    let folder = dir.join("chromium-extension");
    if folder.is_dir() { folder } else { dir }
}

/// Opens pim.log in Notepad (falls back to the default app).
pub(crate) fn open_log_file() {
    let path = crate::pimlog::ensure_exists();
    if std::process::Command::new("notepad.exe").arg(&path).spawn().is_err() {
        let _ = std::process::Command::new("explorer").arg(&path).spawn();
    }
}

fn status_icon(status: Status) -> Icon {
    match status {
        Status::Queued => Icon::Clock,
        Status::Downloading => Icon::Down,
        Status::Paused => Icon::Pause,
        Status::Done => Icon::Check,
        Status::Error => Icon::Alert,
    }
}
