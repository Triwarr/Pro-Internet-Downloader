//! Triwar Design theme for egui (PhotoCraft / Universal Upscaler look) - Pro Internet Manager.
//!
//! Self-contained: no app state, no download logic. Usage:
//!   theme::set_kind(kind); theme::apply_theme(ctx);   // when the theme changes
//!   theme::card().show(ui, |ui| ...);                // rounded card with border
//!   theme::action_button(ui, "Add", true)            // primary (green) / secondary (dark)
//!   theme::paint_progress(ui, rect, Some(p), "42%")  // rounded green bar
//!   theme::icon(painter, theme::Icon::Gear, rect, theme::ACCENT())  // vector icons
//! Colours are functions (ACCENT(), TEXT(), ...) so the presets from the Appearance tab
//! recolour the whole layout. `ThemeKind::DarkLime` is kept as the config value for
//! compatibility with the original app's config.json, but is shown and drawn as "Pro Lime".
//! Assets (mesh background + window icon) are embedded via include_bytes.
#![allow(non_snake_case, dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use egui::{Color32, Context, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde::{Deserialize, Serialize};

// ---------- Theme presets (same variants as the original, so config.json stays compatible) ----------

/// Serialize is written by hand (below): the restyle-only presets Graphite and Azeroth are stored
/// in config.json as "DarkLime", so the original app can still read the file. The real choice is
/// kept in pim_restyle.json next to it (see `save_own_theme`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum ThemeKind {
    /// Pro Lime (Triwar Design) - the default look of Pro Internet Manager.
    #[default]
    DarkLime,
    Midnight,
    Paper,
    Compact,
    /// Restyle only: black / grey, no colour.
    Graphite,
    /// Restyle only: dark leather, gold borders, gold primary buttons (0.2.1), serif titles.
    Azeroth,
}

impl Serialize for ThemeKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (i, name) = match self {
            ThemeKind::DarkLime | ThemeKind::Graphite | ThemeKind::Azeroth => (0, "DarkLime"),
            ThemeKind::Midnight => (1, "Midnight"),
            ThemeKind::Paper => (2, "Paper"),
            ThemeKind::Compact => (3, "Compact"),
        };
        s.serialize_unit_variant("ThemeKind", i, name)
    }
}

impl ThemeKind {
    pub const ALL: [ThemeKind; 6] = [ThemeKind::DarkLime, ThemeKind::Midnight, ThemeKind::Paper, ThemeKind::Compact, ThemeKind::Graphite, ThemeKind::Azeroth];

    pub fn label(self) -> &'static str {
        match self {
            ThemeKind::DarkLime => "Pro Lime (default)",
            ThemeKind::Midnight => "Midnight",
            ThemeKind::Paper => "Paper",
            ThemeKind::Compact => "Compact",
            ThemeKind::Graphite => "Graphite",
            ThemeKind::Azeroth => "Azeroth (bonus)",
        }
    }

    fn index(self) -> u8 {
        match self {
            ThemeKind::DarkLime => 0,
            ThemeKind::Midnight => 1,
            ThemeKind::Paper => 2,
            ThemeKind::Compact => 3,
            ThemeKind::Graphite => 4,
            ThemeKind::Azeroth => 5,
        }
    }

    fn from_index(i: u8) -> Self {
        match i {
            1 => ThemeKind::Midnight,
            2 => ThemeKind::Paper,
            3 => ThemeKind::Compact,
            4 => ThemeKind::Graphite,
            5 => ThemeKind::Azeroth,
            _ => ThemeKind::DarkLime,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub light: bool,
    pub mesh: bool,
    pub bg: Color32,
    pub bar: Color32,
    pub card: Color32,
    pub card_border: Color32,
    pub field: Color32,
    pub btn: Color32,
    pub btn_hover: Color32,
    pub btn_border: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub dim: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    /// Fill / text of primary buttons (same as accent except in Azeroth).
    pub primary: Color32,
    pub primary_text: Color32,
    /// Serif fonts (Georgia) instead of Segoe UI.
    pub serif: bool,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub info: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const PRO_LIME: Palette = Palette {
    light: false,
    mesh: true,
    bg: rgb(9, 12, 9),
    bar: rgb(6, 8, 6),
    card: rgb(17, 21, 17),
    card_border: rgb(48, 56, 48),
    field: rgb(13, 16, 13),
    btn: rgb(30, 35, 30),
    btn_hover: rgb(44, 51, 44),
    btn_border: rgb(64, 72, 64),
    text: rgb(242, 245, 240),
    text2: rgb(210, 216, 206),
    dim: rgb(150, 158, 148),
    faint: rgb(92, 100, 90),
    accent: rgb(118, 185, 0), // #76B900
    accent_text: rgb(10, 14, 6),
    primary: rgb(118, 185, 0),
    primary_text: rgb(10, 14, 6),
    serif: false,
    ok: rgb(132, 196, 20),
    warn: rgb(240, 170, 72),
    err: rgb(240, 100, 100),
    info: rgb(110, 190, 230),
};

const MIDNIGHT: Palette = Palette {
    light: false,
    mesh: false,
    bg: rgb(7, 12, 22),
    bar: rgb(5, 9, 17),
    card: rgb(14, 22, 38),
    card_border: rgb(38, 62, 99),
    field: rgb(9, 15, 27),
    btn: rgb(22, 34, 56),
    btn_hover: rgb(32, 48, 76),
    btn_border: rgb(48, 72, 110),
    text: rgb(226, 240, 255),
    text2: rgb(198, 214, 234),
    dim: rgb(139, 159, 184),
    faint: rgb(86, 104, 128),
    accent: rgb(96, 214, 255),
    accent_text: rgb(6, 16, 28),
    primary: rgb(96, 214, 255),
    primary_text: rgb(6, 16, 28),
    serif: false,
    ok: rgb(110, 220, 160),
    warn: rgb(240, 180, 90),
    err: rgb(255, 147, 137),
    info: rgb(150, 170, 255),
};

const PAPER: Palette = Palette {
    light: true,
    mesh: false,
    bg: rgb(244, 245, 239),
    bar: rgb(232, 235, 226),
    card: rgb(255, 255, 251),
    card_border: rgb(189, 198, 178),
    field: rgb(238, 241, 232),
    btn: rgb(232, 236, 226),
    btn_hover: rgb(220, 228, 210),
    btn_border: rgb(180, 190, 168),
    text: rgb(26, 34, 24),
    text2: rgb(52, 62, 48),
    dim: rgb(91, 103, 84),
    faint: rgb(150, 160, 142),
    accent: rgb(34, 112, 72),
    accent_text: rgb(250, 255, 248),
    primary: rgb(34, 112, 72),
    primary_text: rgb(250, 255, 248),
    serif: false,
    ok: rgb(40, 130, 60),
    warn: rgb(150, 88, 10),
    err: rgb(178, 52, 40),
    info: rgb(40, 110, 170),
};

const COMPACT: Palette = Palette {
    light: false,
    mesh: true,
    bg: rgb(10, 13, 10),
    bar: rgb(7, 9, 7),
    card: rgb(22, 29, 20),
    card_border: rgb(49, 68, 38),
    field: rgb(15, 20, 14),
    btn: rgb(32, 42, 30),
    btn_hover: rgb(44, 58, 40),
    btn_border: rgb(62, 82, 52),
    text: rgb(232, 244, 224),
    text2: rgb(204, 218, 196),
    dim: rgb(160, 176, 148),
    faint: rgb(86, 100, 78),
    accent: rgb(142, 235, 74),
    accent_text: rgb(10, 16, 6),
    primary: rgb(142, 235, 74),
    primary_text: rgb(10, 16, 6),
    serif: false,
    ok: rgb(142, 220, 60),
    warn: rgb(240, 170, 72),
    err: rgb(255, 137, 120),
    info: rgb(110, 190, 230),
};

const GRAPHITE: Palette = Palette {
    light: false,
    mesh: false,
    bg: rgb(10, 10, 11),
    bar: rgb(6, 6, 7),
    card: rgb(20, 20, 22),
    card_border: rgb(56, 56, 60),
    field: rgb(14, 14, 15),
    btn: rgb(32, 32, 35),
    btn_hover: rgb(46, 46, 50),
    btn_border: rgb(72, 72, 77),
    text: rgb(240, 240, 240),
    text2: rgb(208, 208, 210),
    dim: rgb(150, 150, 154),
    faint: rgb(96, 96, 100),
    accent: rgb(206, 206, 210),
    accent_text: rgb(12, 12, 13),
    primary: rgb(206, 206, 210),
    primary_text: rgb(12, 12, 13),
    serif: false,
    ok: rgb(214, 214, 214),
    warn: rgb(172, 172, 172),
    err: rgb(140, 140, 144),
    info: rgb(190, 190, 194),
};

const AZEROTH: Palette = Palette {
    light: false,
    mesh: false,
    bg: rgb(20, 13, 8),
    bar: rgb(12, 8, 5),
    card: rgb(36, 24, 14),
    card_border: rgb(146, 112, 48),
    field: rgb(24, 16, 10),
    btn: rgb(52, 36, 20),
    btn_hover: rgb(72, 50, 26),
    btn_border: rgb(168, 128, 52),
    text: rgb(240, 226, 196),
    text2: rgb(216, 198, 162),
    dim: rgb(196, 174, 134),
    faint: rgb(110, 92, 64),
    accent: rgb(232, 186, 70),
    accent_text: rgb(26, 16, 6),
    primary: rgb(232, 186, 70),
    primary_text: rgb(26, 16, 6),
    serif: true,
    ok: rgb(150, 200, 80),
    warn: rgb(240, 170, 60),
    err: rgb(230, 80, 60),
    info: rgb(120, 170, 230),
};

static KIND: AtomicU8 = AtomicU8::new(0);

/// Select the preset used by all colour functions below.
pub fn set_kind(kind: ThemeKind) {
    KIND.store(kind.index(), Ordering::Relaxed);
}

pub fn current_kind() -> ThemeKind {
    ThemeKind::from_index(KIND.load(Ordering::Relaxed))
}

pub fn palette_for(kind: ThemeKind) -> Palette {
    match kind {
        ThemeKind::DarkLime => PRO_LIME,
        ThemeKind::Midnight => MIDNIGHT,
        ThemeKind::Paper => PAPER,
        ThemeKind::Compact => COMPACT,
        ThemeKind::Graphite => GRAPHITE,
        ThemeKind::Azeroth => AZEROTH,
    }
}

pub fn pal() -> Palette {
    palette_for(current_kind())
}

pub fn BG() -> Color32 { pal().bg }
pub fn BAR() -> Color32 { pal().bar }
pub fn CARD() -> Color32 { pal().card }
pub fn CARD_BORDER() -> Color32 { pal().card_border }
pub fn FIELD() -> Color32 { pal().field }
pub fn BTN() -> Color32 { pal().btn }
pub fn BTN_HOVER() -> Color32 { pal().btn_hover }
pub fn BTN_BORDER() -> Color32 { pal().btn_border }
pub fn TEXT() -> Color32 { pal().text }
pub fn TEXT2() -> Color32 { pal().text2 }
pub fn DIM() -> Color32 { pal().dim }
pub fn FAINT() -> Color32 { pal().faint }
pub fn ACCENT() -> Color32 { pal().accent }
pub fn ACCENT_TEXT() -> Color32 { pal().accent_text }
pub fn PRIMARY() -> Color32 { pal().primary }
pub fn PRIMARY_TEXT() -> Color32 { pal().primary_text }
pub fn OK() -> Color32 { pal().ok }
pub fn WARN() -> Color32 { pal().warn }
pub fn ERR() -> Color32 { pal().err }
pub fn INFO() -> Color32 { pal().info }

/// Subtle highlight used for zebra rows (white on dark themes, black on light).
pub fn STRIPE() -> Color32 {
    if pal().light { Color32::from_rgba_unmultiplied(0, 0, 0, 8) } else { Color32::from_rgba_unmultiplied(255, 255, 255, 6) }
}

// ---------- Layout tokens (kept from the original theme.rs; main.rs reads the window size) ----------

#[derive(Clone)]
pub struct Layout {
    pub window_width: f32,
    pub window_height: f32,
    pub sidebar_width: f32,
    pub settings_width: f32,
    pub settings_min_width: f32,
}

#[derive(Clone)]
pub struct Theme {
    pub kind: ThemeKind,
    pub palette: Palette,
    pub layout: Layout,
}

impl Theme {
    pub fn for_kind(kind: ThemeKind) -> Self {
        Self {
            kind,
            palette: palette_for(kind),
            layout: Layout { window_width: 1180.0, window_height: 740.0, sidebar_width: 232.0, settings_width: 780.0, settings_min_width: 680.0 },
        }
    }
}

pub fn get(kind: ThemeKind) -> Theme {
    Theme::for_kind(kind)
}

pub fn apply(kind: ThemeKind, ctx: &Context) {
    set_kind(kind);
    apply_theme(ctx);
}

pub const RADIUS: f32 = 10.0;
pub const CARD_RADIUS: f32 = 12.0;

pub static BG_RGBA: &[u8] = include_bytes!("../assets/rdm_bg.rgba");
pub const BG_SIZE: [usize; 2] = [1280, 800];
pub static ICON_RGBA: &[u8] = include_bytes!("../assets/pim_icon.rgba");
pub const ICON_SIZE: u32 = 128;

pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}

/// Segoe UI from %WINDIR%\Fonts when present; otherwise egui's built-in font.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Fonts");
    let mut prop = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let mut semi = prop.clone();
    if let Ok(bytes) = std::fs::read(dir.join("segoeui.ttf")) {
        fonts.font_data.insert("segoe_ui".into(), Arc::new(egui::FontData::from_owned(bytes)));
        prop.insert(0, "segoe_ui".into());
        semi.insert(0, "segoe_ui".into());
    }
    if let Ok(bytes) = std::fs::read(dir.join("seguisb.ttf")) {
        fonts.font_data.insert("segoe_ui_semibold".into(), Arc::new(egui::FontData::from_owned(bytes)));
        semi.insert(0, "segoe_ui_semibold".into());
    }
    // Azeroth: serif (Georgia from C:\Windows\Fonts) in front, Segoe UI stays as fallback.
    if pal().serif {
        if let Ok(bytes) = std::fs::read(dir.join("georgia.ttf")) {
            fonts.font_data.insert("serif".into(), Arc::new(egui::FontData::from_owned(bytes)));
            prop.insert(0, "serif".into());
            semi.insert(0, "serif".into());
        }
        if let Ok(bytes) = std::fs::read(dir.join("georgiab.ttf")) {
            fonts.font_data.insert("serif_bold".into(), Arc::new(egui::FontData::from_owned(bytes)));
            semi.insert(0, "serif_bold".into());
        }
    }
    fonts.families.insert(egui::FontFamily::Proportional, prop);
    fonts.families.insert(semibold(), semi);
    ctx.set_fonts(fonts);
}

/// Install fonts + dark/green Visuals and Style. Call once per context.
pub fn apply_theme(ctx: &egui::Context) {
    install_fonts(ctx);
    let light = pal().light;
    ctx.set_theme(if light { egui::Theme::Light } else { egui::Theme::Dark });
    ctx.all_styles_mut(|style| {
        use egui::{CornerRadius, FontId, TextStyle};
        let r = CornerRadius::same(RADIUS as u8);
        let v = &mut style.visuals;
        v.dark_mode = !light;
        v.panel_fill = Color32::TRANSPARENT;
        v.window_fill = CARD();
        v.window_stroke = Stroke::new(1.0, alpha(ACCENT(), 90));
        v.window_corner_radius = CornerRadius::same(CARD_RADIUS as u8);
        v.menu_corner_radius = r;
        v.extreme_bg_color = FIELD();
        v.text_edit_bg_color = Some(FIELD());
        v.faint_bg_color = CARD();
        v.code_bg_color = FIELD();
        v.hyperlink_color = ACCENT();
        v.slider_trailing_fill = true;
        v.selection.bg_fill = alpha(ACCENT(), 110);
        v.selection.stroke = Stroke::new(1.0, ACCENT());
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = CARD();
        w.noninteractive.weak_bg_fill = CARD();
        w.noninteractive.bg_stroke = Stroke::new(1.0, CARD_BORDER());
        w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT());
        w.noninteractive.corner_radius = r;
        w.inactive.bg_fill = BTN();
        w.inactive.weak_bg_fill = BTN();
        w.inactive.bg_stroke = Stroke::new(1.0, BTN_BORDER());
        w.inactive.fg_stroke = Stroke::new(1.0, TEXT());
        w.inactive.corner_radius = r;
        w.hovered.bg_fill = BTN_HOVER();
        w.hovered.weak_bg_fill = BTN_HOVER();
        w.hovered.bg_stroke = Stroke::new(1.0, alpha(ACCENT(), 160));
        w.hovered.fg_stroke = Stroke::new(1.5, TEXT());
        w.hovered.corner_radius = r;
        w.active.bg_fill = alpha(ACCENT(), 80);
        w.active.weak_bg_fill = alpha(ACCENT(), 80);
        w.active.bg_stroke = Stroke::new(1.0, ACCENT());
        w.active.fg_stroke = Stroke::new(2.0, TEXT());
        w.active.corner_radius = r;
        w.open.bg_fill = BTN();
        w.open.weak_bg_fill = BTN();
        w.open.bg_stroke = Stroke::new(1.0, ACCENT());
        w.open.fg_stroke = Stroke::new(1.0, TEXT());
        w.open.corner_radius = r;
        style.spacing.item_spacing = vec2(8.0, 8.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.spacing.interact_size.y = 26.0;
        // 0.2.1: solid, always visible scroll bars (the floating ones were invisible until hovered).
        let mut scroll = egui::style::ScrollStyle::solid();
        scroll.bar_width = 10.0;
        scroll.dormant_background_opacity = 0.35;
        scroll.dormant_handle_opacity = 0.8;
        scroll.active_handle_opacity = 0.9;
        style.spacing.scroll = scroll;
        style.text_styles = [
            (TextStyle::Heading, FontId::new(19.0, semibold())),
            (TextStyle::Body, FontId::proportional(13.5)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
    });
}

pub fn window_icon() -> egui::IconData {
    egui::IconData { width: ICON_SIZE, height: ICON_SIZE, rgba: ICON_RGBA.to_vec() }
}

/// Window icon in the accent colour of `kind` (the exe icon itself stays green).
pub fn window_icon_for(kind: ThemeKind) -> egui::IconData {
    egui::IconData { width: ICON_SIZE, height: ICON_SIZE, rgba: tinted_icon_rgba(kind) }
}

fn to_hsv(c: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|x| x as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max <= 0.0 { 0.0 } else { d / max }, max)
}

fn from_hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r, g, b].map(|t| ((t + m).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The embedded globe with its green moved to the accent of `kind`
/// (hue -> accent hue, saturation and brightness scaled; white and grey pixels stay as they are).
pub fn tinted_icon_rgba(kind: ThemeKind) -> Vec<u8> {
    let mut out = ICON_RGBA.to_vec();
    if kind == ThemeKind::DarkLime {
        return out;
    }
    let base = PRO_LIME.accent;
    let target = palette_for(kind).accent;
    let (_, bs, bv) = to_hsv([base.r(), base.g(), base.b()]);
    let (th, ts, tv) = to_hsv([target.r(), target.g(), target.b()]);
    for px in out.chunks_exact_mut(4) {
        if px[3] == 0 {
            continue;
        }
        let (_, s, v) = to_hsv([px[0], px[1], px[2]]);
        let w = ((s - 0.10) / 0.20).clamp(0.0, 1.0);
        if w <= 0.0 {
            continue;
        }
        let n = from_hsv(th, (s * ts / bs).min(1.0), (v * tv / bv).min(1.0));
        for i in 0..3 {
            px[i] = (px[i] as f32 * (1.0 - w) + n[i] as f32 * w).round() as u8;
        }
    }
    out
}

/// Short key of a preset ("lime", "midnight", ...): the bridge extension picks icons/<key>/ by it.
pub fn key(kind: ThemeKind) -> &'static str {
    match kind {
        ThemeKind::DarkLime => "lime",
        ThemeKind::Midnight => "midnight",
        ThemeKind::Paper => "paper",
        ThemeKind::Compact => "compact",
        ThemeKind::Graphite => "graphite",
        ThemeKind::Azeroth => "azeroth",
    }
}

static THEME_INFO: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Called by the UI when the preset changes; read by the API thread for /ping.
pub fn set_theme_info(kind: ThemeKind) {
    let p = palette_for(kind);
    let info = serde_json::json!({
        "name": kind.label(),
        "key": key(kind),
        "icon": key(kind),
        "accent": hex(p.accent),
        "accent_text": hex(p.accent_text),
        "primary": hex(p.primary),
        "primary_text": hex(p.primary_text),
        "bg": hex(p.bg),
        "card": hex(p.card),
        "border": hex(p.card_border),
        "text": hex(p.text),
        "dim": hex(p.dim),
        "light": p.light,
        "serif": p.serif,
    });
    if let Ok(mut slot) = THEME_INFO.lock() {
        *slot = info.to_string();
    }
}

/// /ping body: the original `{"ok":true}` plus "app" and "theme" (old extensions ignore the extras).
pub fn ping_json() -> String {
    let theme = THEME_INFO.lock().map(|s| s.clone()).unwrap_or_default();
    if theme.is_empty() {
        "{\"ok\":true,\"app\":\"Pro Internet Manager\"}".to_string()
    } else {
        format!("{{\"ok\":true,\"app\":\"Pro Internet Manager\",\"theme\":{theme}}}")
    }
}

/// "#rrggbb" for the /ping theme info (Firefox bridge).
pub fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn own_theme_path() -> Option<PathBuf> {
    Some(crate::paths::data_file("pim_restyle.json"))
}

/// Restyle-only presets (Graphite, Azeroth) live here; config.json gets "DarkLime" for them.
pub fn save_own_theme(kind: ThemeKind) {
    let Some(path) = own_theme_path() else { return };
    let name = match kind {
        ThemeKind::Graphite => "\"Graphite\"",
        ThemeKind::Azeroth => "\"Azeroth\"",
        _ => "null",
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, format!("{{\n  \"theme\": {name}\n}}\n"));
}

pub fn load_own_theme() -> Option<ThemeKind> {
    let text = std::fs::read_to_string(own_theme_path()?).ok()?;
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    match value.get("theme")?.as_str()? {
        "Graphite" => Some(ThemeKind::Graphite),
        "Azeroth" => Some(ThemeKind::Azeroth),
        _ => None,
    }
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(alpha(CARD(), 236))
        .stroke(Stroke::new(1.0, CARD_BORDER()))
        .corner_radius(CARD_RADIUS)
        .inner_margin(egui::Margin::same(12))
}

pub fn section_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).family(semibold()).size(17.0).color(TEXT()));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).size(11.5).color(DIM()));
    }
    ui.add_space(4.0);
}

pub fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).family(semibold()).size(11.5).color(DIM()));
}

pub fn chip(ui: &mut egui::Ui, color: Color32, text: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(11.5), color);
    let size = vec2(galley.size().x + 29.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    paint_chip(ui.painter(), rect, color, galley);
    resp
}

/// Status chip with a small symbol instead of the dot (0.2.1: Paused / Queued / Done / Error
/// are recognisable without relying on colour).
pub fn paint_status_chip(painter: &egui::Painter, rect: Rect, color: Color32, galley: Arc<egui::Galley>, kind: Icon) {
    painter.rect_filled(rect, 11.0, alpha(color, 30));
    painter.rect_stroke(rect, 11.0, Stroke::new(1.0, alpha(color, 120)), egui::StrokeKind::Inside);
    icon(painter, kind, Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(13.0, 13.0)), color);
    painter.galley(pos2(rect.left() + 22.0, rect.center().y - galley.size().y / 2.0), galley, color);
}

pub fn paint_chip(painter: &egui::Painter, rect: Rect, color: Color32, galley: Arc<egui::Galley>) {
    painter.rect_filled(rect, 11.0, alpha(color, 30));
    painter.rect_stroke(rect, 11.0, Stroke::new(1.0, alpha(color, 120)), egui::StrokeKind::Inside);
    painter.circle_filled(pos2(rect.left() + 11.0, rect.center().y), 3.5, color);
    painter.galley(pos2(rect.left() + 19.0, rect.center().y - galley.size().y / 2.0), galley, color);
}

pub fn button_widget<'a>(label: &str, primary: bool, size: f32) -> egui::Button<'a> {
    // Primary buttons get a gold rim when their fill differs from the accent (Azeroth).
    let rim = if PRIMARY() != ACCENT() { Stroke::new(1.3, ACCENT()) } else { Stroke::NONE };
    let (fill, text, stroke) = if primary { (PRIMARY(), PRIMARY_TEXT(), rim) } else { (BTN(), TEXT(), Stroke::new(1.0, BTN_BORDER())) };
    egui::Button::new(RichText::new(label.to_string()).family(semibold()).size(size).color(text))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(RADIUS)
}

pub fn action_button(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    ui.add(button_widget(label, primary, 13.0).min_size(vec2(0.0, 30.0)))
}

/// Rounded green checkbox like the installer's component cards.
pub fn check_box(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let (rect, mut resp) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
        let label_resp = ui.add(egui::Label::new(RichText::new(label.to_string()).color(TEXT())).sense(Sense::click()));
        if resp.clicked() || label_resp.clicked() {
            *value = !*value;
            resp.mark_changed();
        }
        let painter = ui.painter();
        if *value {
            painter.rect_filled(rect, 4.0, ACCENT());
            let o = rect.left_top();
            let stroke = Stroke::new(2.0, ACCENT_TEXT());
            painter.line_segment([o + vec2(4.5, 9.5), o + vec2(7.8, 12.8)], stroke);
            painter.line_segment([o + vec2(7.8, 12.8), o + vec2(13.8, 5.5)], stroke);
        } else {
            painter.rect_filled(rect, 4.0, FIELD());
            painter.rect_stroke(rect, 4.0, Stroke::new(1.2, DIM()), egui::StrokeKind::Inside);
        }
        resp | label_resp
    })
    .inner
}

/// 0.2.1: middle ellipsis for file names, so the start and the extension stay visible
/// ("Ubuntu-24.04-des...amd64.iso"). Returns the galley and whether the text was shortened.
pub fn middle_ellipsis(ui: &egui::Ui, text: &str, size: f32, color: Color32, width: f32, strong: bool) -> (Arc<egui::Galley>, bool) {
    let font = if strong { egui::FontId::new(size, semibold()) } else { egui::FontId::proportional(size) };
    let full = ui.painter().layout_no_wrap(text.to_string(), font.clone(), color);
    let width = width.max(24.0);
    if full.size().x <= width {
        return (full, false);
    }
    let chars: Vec<char> = text.chars().collect();
    let ext_len = text.rfind('.').map(|i| text[i..].chars().count()).filter(|n| *n <= 8).unwrap_or(0);
    // keep the extension plus a few characters before it at the end
    let tail = (ext_len + 6).min(chars.len() / 2).max(1);
    let mut head = chars.len().saturating_sub(tail + 1);
    while head >= 3 {
        let candidate: String = chars[..head].iter().chain(std::iter::once(&'\u{2026}')).chain(chars[chars.len() - tail..].iter()).collect();
        let g = ui.painter().layout_no_wrap(candidate, font.clone(), color);
        if g.size().x <= width {
            return (g, true);
        }
        head -= 1;
    }
    (truncated(ui, text, size, color, width, strong), true)
}

pub fn truncated(ui: &egui::Ui, text: &str, size: f32, color: Color32, width: f32, strong: bool) -> Arc<egui::Galley> {
    let font = if strong { egui::FontId::new(size, semibold()) } else { egui::FontId::proportional(size) };
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(8.0));
    ui.painter().layout_job(job)
}

/// Rounded green progress bar in `area`; `label` (e.g. "41%") is drawn right of the bar.
/// `progress = None` draws an indeterminate sliding pill (streams / unknown size).
pub fn paint_progress(ui: &egui::Ui, area: Rect, progress: Option<f32>, label: &str) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(label.to_string(), egui::FontId::new(11.5, semibold()), TEXT());
    let label_w = galley.size().x.max(30.0) + 8.0;
    let h = area.height().min(12.0);
    let rect = Rect::from_min_max(pos2(area.left(), area.center().y - h / 2.0), pos2((area.right() - label_w).max(area.left() + 20.0), area.center().y + h / 2.0));
    let r = h / 2.0;
    painter.rect_filled(rect, r, FIELD());
    painter.rect_stroke(rect, r, Stroke::new(1.0, CARD_BORDER()), egui::StrokeKind::Inside);
    match progress {
        Some(p) => {
            let p = p.clamp(0.0, 1.0);
            if p > 0.0 {
                let w = (rect.width() * p).max(h);
                let fill = Rect::from_min_size(rect.min, vec2(w, h));
                painter.rect_filled(fill, r, ACCENT());
                painter.rect_filled(Rect::from_min_size(fill.min + vec2(3.0, 1.5), vec2((w - 6.0).max(0.0), h * 0.32)), r, Color32::from_rgba_unmultiplied(255, 255, 255, 34));
            }
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let seg = rect.width() * 0.32;
            let x = rect.left() + ((t * 0.6).fract() * (rect.width() + seg)) - seg;
            let a = x.max(rect.left());
            let b = (x + seg).min(rect.right());
            if b > a {
                painter.rect_filled(Rect::from_min_max(pos2(a, rect.top()), pos2(b, rect.bottom())), r, ACCENT());
            }
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
    }
    painter.galley(pos2(area.right() - galley.size().x, area.center().y - galley.size().y / 2.0), galley, TEXT());
}

// ---------- Hand-drawn vector icons (own design, 2px rounded lines) ----------

#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Add,
    Resume,
    Pause,
    StartAll,
    StopAll,
    Delete,
    Sweep,
    Clock,
    Gear,
    Folder,
    Close,
    Tray,
    Down,
    Check,
    Alert,
    Wave,
    Plug,
    Info,
    Palette,
    Retry,
    Log,
}

pub fn icon(p: &egui::Painter, kind: Icon, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 24.0; // design grid 24x24
    let w = (2.0 * u).max(1.4);
    let st = Stroke::new(w, color);
    let at = |x: f32, y: f32| pos2(c.x + (x - 12.0) * u, c.y + (y - 12.0) * u);
    let line = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| at(x, y)).collect();
        p.add(egui::Shape::line(v, st));
        for &(x, y) in pts {
            p.circle_filled(at(x, y), w / 2.0, color);
        }
    };
    let soft = alpha(color, 40);
    match kind {
        Icon::Add => {
            p.rect_stroke(Rect::from_center_size(c, vec2(19.0 * u, 19.0 * u)), 6.0 * u, st, egui::StrokeKind::Middle);
            line(&[(12.0, 7.5), (12.0, 16.5)]);
            line(&[(7.5, 12.0), (16.5, 12.0)]);
        }
        Icon::Resume => {
            let pts = vec![at(8.5, 5.5), at(18.5, 12.0), at(8.5, 18.5)];
            p.add(egui::Shape::convex_polygon(pts.clone(), soft, Stroke::NONE));
            p.add(egui::Shape::closed_line(pts, st));
        }
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(at(6.5, 5.5), at(10.0, 18.5)), 1.6 * u, color);
            p.rect_filled(Rect::from_min_max(at(14.0, 5.5), at(17.5, 18.5)), 1.6 * u, color);
        }
        Icon::StartAll => {
            for dx in [0.0, 7.0] {
                let pts = vec![at(4.0 + dx, 6.0), at(12.0 + dx, 12.0), at(4.0 + dx, 18.0)];
                p.add(egui::Shape::convex_polygon(pts.clone(), soft, Stroke::NONE));
                p.add(egui::Shape::closed_line(pts, st));
            }
        }
        Icon::StopAll => {
            p.rect_stroke(Rect::from_min_max(at(7.5, 3.5), at(20.5, 16.5)), 3.0 * u, Stroke::new(w * 0.8, alpha(color, 140)), egui::StrokeKind::Middle);
            let r = Rect::from_min_max(at(3.5, 7.5), at(16.5, 20.5));
            p.rect_filled(r, 3.0 * u, CARD());
            p.rect_filled(r, 3.0 * u, soft);
            p.rect_stroke(r, 3.0 * u, st, egui::StrokeKind::Middle);
        }
        Icon::Delete => {
            line(&[(4.5, 7.0), (19.5, 7.0)]);
            line(&[(9.5, 7.0), (10.0, 4.5), (14.0, 4.5), (14.5, 7.0)]);
            line(&[(6.5, 7.0), (7.5, 19.5), (16.5, 19.5), (17.5, 7.0)]);
            line(&[(10.3, 10.5), (10.6, 16.0)]);
            line(&[(13.7, 10.5), (13.4, 16.0)]);
        }
        Icon::Sweep => {
            // checked list being cleared
            line(&[(4.0, 8.0), (6.5, 10.5), (10.5, 5.5)]);
            line(&[(13.5, 8.0), (20.0, 8.0)]);
            line(&[(4.0, 14.0), (20.0, 14.0)]);
            line(&[(4.0, 19.0), (14.0, 19.0)]);
        }
        Icon::Clock => {
            p.circle_stroke(c, 8.5 * u, st);
            line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (sn, cs) = a.sin_cos();
                p.line_segment([c + vec2(cs, sn) * 7.0 * u, c + vec2(cs, sn) * 10.0 * u], Stroke::new(w * 1.6, color));
            }
            p.circle_stroke(c, 7.0 * u, st);
            p.circle_stroke(c, 3.0 * u, st);
        }
        Icon::Folder => {
            line(&[(3.5, 18.5), (3.5, 6.0), (9.0, 6.0), (11.0, 8.5), (20.5, 8.5), (20.5, 18.5), (3.5, 18.5)]);
        }
        Icon::Close => {
            line(&[(7.0, 7.0), (17.0, 17.0)]);
            line(&[(17.0, 7.0), (7.0, 17.0)]);
        }
        Icon::Tray => {
            line(&[(3.5, 13.0), (8.0, 13.0), (9.5, 16.0), (14.5, 16.0), (16.0, 13.0), (20.5, 13.0)]);
            line(&[(3.5, 13.0), (5.5, 5.5), (18.5, 5.5), (20.5, 13.0), (20.5, 19.0), (3.5, 19.0), (3.5, 13.0)]);
        }
        Icon::Down => {
            line(&[(12.0, 4.0), (12.0, 15.0)]);
            line(&[(7.5, 10.5), (12.0, 15.0), (16.5, 10.5)]);
            line(&[(5.0, 19.5), (19.0, 19.5)]);
        }
        Icon::Check => {
            p.circle_stroke(c, 8.5 * u, st);
            line(&[(8.0, 12.5), (11.0, 15.0), (16.0, 9.0)]);
        }
        Icon::Alert => {
            line(&[(12.0, 4.0), (20.5, 19.0), (3.5, 19.0), (12.0, 4.0)]);
            line(&[(12.0, 9.5), (12.0, 13.5)]);
            p.circle_filled(at(12.0, 16.3), w * 0.75, color);
        }
        Icon::Wave => {
            p.rect_stroke(Rect::from_min_max(at(3.0, 5.0), at(21.0, 19.0)), 3.5 * u, st, egui::StrokeKind::Middle);
            let pts = vec![at(10.0, 9.0), at(15.0, 12.0), at(10.0, 15.0)];
            p.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        Icon::Plug => {
            line(&[(9.0, 3.5), (9.0, 7.5)]);
            line(&[(15.0, 3.5), (15.0, 7.5)]);
            line(&[(6.0, 7.5), (18.0, 7.5), (18.0, 11.0), (15.0, 15.0), (9.0, 15.0), (6.0, 11.0), (6.0, 7.5)]);
            line(&[(12.0, 15.0), (12.0, 20.5)]);
        }
        Icon::Palette => {
            // own palette: rounded blob with three colour dots
            line(&[(12.0, 3.5), (6.5, 5.5), (3.8, 10.5), (4.5, 15.5), (8.5, 19.5), (13.0, 20.5), (15.0, 18.5), (14.0, 16.0), (15.5, 14.0), (18.5, 14.5), (20.5, 12.0), (19.5, 7.5), (16.0, 4.5), (12.0, 3.5)]);
            p.circle_filled(at(8.5, 11.0), 1.6 * u, color);
            p.circle_filled(at(11.5, 7.5), 1.6 * u, color);
            p.circle_filled(at(15.8, 8.8), 1.6 * u, color);
        }
        Icon::Info => {
            p.circle_stroke(c, 8.5 * u, st);
            line(&[(12.0, 11.0), (12.0, 16.0)]);
            p.circle_filled(at(12.0, 8.0), w * 0.75, color);
        }
        Icon::Retry => {
            // 0.2.2: circular arrow (three quarters of a circle plus an arrow head)
            let r = 7.5 * u;
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + (i as f32 / 24.0) * 1.5 * std::f32::consts::PI + 0.35;
                    pos2(c.x + r * a.cos(), c.y + r * a.sin())
                })
                .collect();
            let end = *pts.first().unwrap();
            p.add(egui::Shape::line(pts, st));
            p.add(egui::Shape::convex_polygon(
                vec![pos2(end.x - 3.6 * u, end.y - 3.4 * u), pos2(end.x + 2.8 * u, end.y - 3.8 * u), pos2(end.x + 0.2 * u, end.y + 2.6 * u)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Log => {
            // 0.2.2: sheet with text lines (open the log file)
            p.rect_stroke(Rect::from_center_size(c, vec2(14.0 * u, 18.0 * u)), 2.5 * u, st, egui::StrokeKind::Middle);
            line(&[(8.5, 8.5), (15.5, 8.5)]);
            line(&[(8.5, 12.0), (15.5, 12.0)]);
            line(&[(8.5, 15.5), (13.0, 15.5)]);
            let _ = soft;
        }
    }
}

/// Small file-type badge for the table: a folded sheet with a coloured tab.
/// `stream_tag`: Some("HLS") / Some("YT") for streams, None for normal files.
pub fn file_icon(p: &egui::Painter, rect: Rect, name: &str, stream_tag: Option<&str>) {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let (color, tag) = if let Some(tag) = stream_tag {
        (if tag == "YT" { ERR() } else { ACCENT() }, tag)
    } else {
        match ext.as_str() {
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "ts" => (ACCENT(), "VID"),
            "mp3" | "flac" | "wav" | "m4a" | "ogg" | "aac" | "opus" => (INFO(), "AUD"),
            "zip" | "7z" | "rar" | "gz" | "tar" | "xz" | "bz2" => (WARN(), "ZIP"),
            "exe" | "msi" | "msix" | "appx" => (Color32::from_rgb(200, 140, 240), "EXE"),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => (Color32::from_rgb(110, 220, 190), "IMG"),
            "pdf" | "doc" | "docx" | "txt" | "xlsx" | "pptx" | "epub" => (TEXT2(), "DOC"),
            "iso" | "img" | "bin" | "dmg" => (WARN(), "ISO"),
            _ => (DIM(), "BIN"),
        }
    };
    let h = rect.height();
    let w = h * 0.8;
    let r = Rect::from_center_size(rect.center(), vec2(w, h));
    let fold = w * 0.32;
    let pts = vec![r.left_top(), pos2(r.right() - fold, r.top()), pos2(r.right(), r.top() + fold), r.right_bottom(), r.left_bottom()];
    p.add(egui::Shape::convex_polygon(pts.clone(), alpha(color, 26), Stroke::NONE));
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.3, alpha(color, 200))));
    p.line_segment([pos2(r.right() - fold, r.top()), pos2(r.right() - fold, r.top() + fold)], Stroke::new(1.1, alpha(color, 200)));
    p.line_segment([pos2(r.right() - fold, r.top() + fold), pos2(r.right(), r.top() + fold)], Stroke::new(1.1, alpha(color, 200)));
    let band = Rect::from_min_max(pos2(r.left() - 3.0, r.bottom() - h * 0.42), pos2(r.right() + 3.0, r.bottom() - h * 0.08));
    p.rect_filled(band, 2.5, color);
    let g = p.layout_no_wrap(tag.to_string(), egui::FontId::new(7.0, semibold()), ACCENT_TEXT());
    p.galley(pos2(band.center().x - g.size().x / 2.0, band.center().y - g.size().y / 2.0), g, ACCENT_TEXT());
}

/// Big rounded toolbar tile: icon on top, label underneath.
pub fn tool_tile(ui: &mut egui::Ui, kind: Icon, label: &str, enabled: bool, primary: bool, hint: &str) -> bool {
    // 0.2.1: compacter tiles (64 x 48 instead of 76 x 64); the hint is the tooltip.
    let (rect, resp) = ui.allocate_exact_size(vec2(64.0, 48.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let hovered = enabled && resp.hovered();
    let (fill, border) = if primary {
        (if hovered { alpha(ACCENT(), 60) } else { alpha(ACCENT(), 34) }, alpha(ACCENT(), if hovered { 230 } else { 150 }))
    } else if hovered {
        (BTN_HOVER(), alpha(ACCENT(), 150))
    } else {
        (Color32::TRANSPARENT, Color32::TRANSPARENT)
    };
    p.rect_filled(rect, CARD_RADIUS, fill);
    if border != Color32::TRANSPARENT {
        p.rect_stroke(rect, CARD_RADIUS, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    }
    let color = if !enabled { FAINT() } else if primary || hovered { ACCENT() } else { TEXT() };
    icon(p, kind, Rect::from_center_size(pos2(rect.center().x, rect.top() + 17.0), vec2(20.0, 20.0)), color);
    let text_color = if enabled { if primary { ACCENT() } else { TEXT() } } else { FAINT() };
    let g = p.layout_no_wrap(label.to_string(), egui::FontId::new(10.5, semibold()), text_color);
    p.galley(pos2(rect.center().x - g.size().x / 2.0, rect.bottom() - 16.0), g, text_color);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let resp = resp.on_hover_text(hint);
    enabled && resp.clicked()
}

pub fn tool_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 48.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range().shrink(10.0), Stroke::new(1.0, CARD_BORDER()));
}

/// Round icon button used inside table rows.
pub fn row_icon_button(ui: &egui::Ui, rect: Rect, id: egui::Id, kind: Icon, color: Color32, hint: &str) -> bool {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 8.0, BTN_HOVER());
        p.rect_stroke(rect, 8.0, Stroke::new(1.0, alpha(color, 170)), egui::StrokeKind::Inside);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else {
        p.rect_filled(rect, 8.0, alpha(BTN(), 200));
    }
    icon(p, kind, rect.shrink(5.0), color);
    resp.on_hover_text(hint).clicked()
}

/// Category row in the left tree.
pub fn category_row(ui: &mut egui::Ui, kind: Icon, label: &str, count: usize, selected: bool, indent: f32, last_child: Option<bool>) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
    let p = ui.painter();
    if let Some(last) = last_child {
        // tree connector lines
        let x = rect.left() + 15.0;
        let tree = Stroke::new(1.0, alpha(ACCENT(), 70));
        p.vline(x, egui::Rangef::new(rect.top() - 2.0, if last { rect.center().y } else { rect.bottom() + 2.0 }), tree);
        p.hline(egui::Rangef::new(x, rect.left() + indent - 4.0), rect.center().y, tree);
    }
    let pill = Rect::from_min_max(pos2(rect.left() + indent, rect.top() + 1.0), pos2(rect.right(), rect.bottom() - 1.0));
    if selected {
        p.rect_filled(pill, 9.0, alpha(ACCENT(), 34));
        p.rect_stroke(pill, 9.0, Stroke::new(1.0, alpha(ACCENT(), 190)), egui::StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_filled(pill, 9.0, BTN_HOVER());
    }
    let color = if selected { ACCENT() } else { DIM() };
    icon(p, kind, Rect::from_center_size(pos2(pill.left() + 15.0, pill.center().y), vec2(17.0, 17.0)), color);
    let g = truncated(ui, label, 13.0, if selected { TEXT() } else { TEXT2() }, pill.width() - 70.0, selected);
    ui.painter().galley(pos2(pill.left() + 31.0, pill.center().y - g.size().y / 2.0), g, TEXT());
    let cg = ui.painter().layout_no_wrap(count.to_string(), egui::FontId::new(11.0, semibold()), if selected { ACCENT_TEXT() } else { DIM() });
    let badge = Rect::from_center_size(pos2(pill.right() - 8.0 - (cg.size().x + 12.0) / 2.0, pill.center().y), vec2((cg.size().x + 12.0).max(22.0), 18.0));
    ui.painter().rect_filled(badge, 9.0, if selected { ACCENT() } else { BTN() });
    ui.painter().galley(pos2(badge.center().x - cg.size().x / 2.0, badge.center().y - cg.size().y / 2.0), cg, TEXT());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

pub fn nav_row(ui: &mut egui::Ui, kind: Icon, label: &str, selected: bool) -> bool {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 9.0, ACCENT());
    } else if resp.hovered() {
        p.rect_filled(rect, 9.0, BTN_HOVER());
    }
    let color = if selected { ACCENT_TEXT() } else { DIM() };
    icon(p, kind, Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), vec2(17.0, 17.0)), color);
    let g = p.layout_no_wrap(label.to_string(), egui::FontId::new(13.0, semibold()), if selected { ACCENT_TEXT() } else { TEXT() });
    p.galley(pos2(rect.left() + 34.0, rect.center().y - g.size().y / 2.0), g, TEXT());
    resp.clicked()
}

pub fn green_rule(ui: &mut egui::Ui) {
    let y = ui.cursor().top();
    let x = ui.max_rect().x_range();
    let painter = ui.painter();
    painter.hline(x, y, Stroke::new(1.0, alpha(ACCENT(), 50)));
    painter.hline(egui::Rangef::new(x.min, x.min + x.span() * 0.45), y, Stroke::new(1.0, alpha(ACCENT(), 160)));
    ui.add_space(2.0);
}


/// Embedded dimmed mesh background as an egui texture.
pub fn background_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    (BG_RGBA.len() == BG_SIZE[0] * BG_SIZE[1] * 4)
        .then(|| ctx.load_texture("rdm_bg", egui::ColorImage::from_rgba_unmultiplied(BG_SIZE, BG_RGBA), egui::TextureOptions::LINEAR))
}

/// Embedded app icon as an egui texture (header / about).
pub fn icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    (ICON_RGBA.len() == (ICON_SIZE * ICON_SIZE * 4) as usize).then(|| {
        ctx.load_texture("rdm_icon", egui::ColorImage::from_rgba_unmultiplied([ICON_SIZE as usize; 2], &tinted_icon_rgba(current_kind())), egui::TextureOptions::LINEAR)
    })
}

/// Fill `rect` with the base colour and the mesh, scaled to cover and anchored at the bottom.
pub fn paint_background(ui: &egui::Ui, tex: Option<&egui::TextureHandle>) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, BG());
    if let Some(tex) = tex.filter(|_| pal().mesh) {
        let [w, h] = tex.size();
        let (w, h) = (w as f32, h as f32);
        let scale = (rect.width() / w).max(rect.height() / h);
        let size = vec2(w * scale, h * scale);
        let img = Rect::from_min_size(pos2(rect.center().x - size.x / 2.0, rect.bottom() - size.y), size);
        painter.image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
}

/// eframe clear colour matching the theme background.
pub fn clear_color() -> [f32; 4] {
    [BG().r() as f32 / 255.0, BG().g() as f32 / 255.0, BG().b() as f32 / 255.0, 1.0]
}
