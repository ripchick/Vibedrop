//! Central visual identity of VibeDrop: CLASSIC SOUNDBOARD (dark grey + blue).
//!
//! All colors live here. `apply` installs the palette into egui, and the helpers below draw
//! buttons, cards and icons that match it. Icons are painted with shapes (not font glyphs), so
//! they look the same on every machine and never turn into "tofu" boxes.

use eframe::egui::{
    self, epaint::Shadow, vec2, Align2, Color32, CursorIcon, FontData, FontDefinitions, FontFamily, FontId, Margin,
    Pos2, Rect, Response, RichText, Rounding, Sense, Shape, Stroke, TextStyle, Ui,
};

// ---- Palette ---------------------------------------------------------------------------------

/// The colour set the whole UI is drawn with. It can be changed at runtime
/// (Settings -> Appearance) without restarting the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub sidebar: Color32,
    pub input_bg: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_dim: Color32,
    pub accent_fill: Color32,
    pub accent_alt: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub danger: Color32,
    pub warning: Color32,
}

/// The classic soundboard look (dark grey surfaces, blue accents).
const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x3C, 0x3C, 0x3C),
    card: Color32::from_rgb(0x4A, 0x4A, 0x4A),
    card_hover: Color32::from_rgb(0x56, 0x56, 0x56),
    sidebar: Color32::from_rgb(0x45, 0x45, 0x45),
    input_bg: Color32::from_rgb(0x33, 0x33, 0x33),
    accent: Color32::from_rgb(0x2E, 0x9B, 0xE6),
    accent_hover: Color32::from_rgb(0x4A, 0xB4, 0xF5),
    accent_dim: Color32::from_rgb(0x1E, 0x6B, 0xA6),
    accent_fill: Color32::from_rgb(0x1F, 0x3F, 0x5C),
    accent_alt: Color32::from_rgb(0x7C, 0xF0, 0xD4),
    border: Color32::from_rgb(0x2E, 0x2E, 0x2E),
    text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
    text_dim: Color32::from_rgb(0xC0, 0xC0, 0xC0),
    text_faint: Color32::from_rgb(0x8A, 0x8A, 0x8A),
    danger: Color32::from_rgb(0xFF, 0x6B, 0x6B),
    warning: Color32::from_rgb(0xFF, 0xD1, 0x66),
};

/// Deep blue-black surfaces with the same blue accent.
const MIDNIGHT: Palette = Palette {
    bg: Color32::from_rgb(0x14, 0x17, 0x20),
    card: Color32::from_rgb(0x1D, 0x21, 0x2C),
    card_hover: Color32::from_rgb(0x26, 0x2B, 0x38),
    sidebar: Color32::from_rgb(0x18, 0x1C, 0x26),
    input_bg: Color32::from_rgb(0x11, 0x14, 0x1C),
    accent: Color32::from_rgb(0x4C, 0xC2, 0xFF),
    accent_hover: Color32::from_rgb(0x6B, 0xDB, 0xFF),
    accent_dim: Color32::from_rgb(0x2A, 0x6B, 0xA8),
    accent_fill: Color32::from_rgb(0x1B, 0x3A, 0x52),
    accent_alt: Color32::from_rgb(0x7C, 0xF0, 0xD4),
    border: Color32::from_rgb(0x2A, 0x30, 0x3D),
    text: Color32::from_rgb(0xF2, 0xF5, 0xFA),
    text_dim: Color32::from_rgb(0xB4, 0xBE, 0xCE),
    text_faint: Color32::from_rgb(0x76, 0x82, 0x95),
    danger: Color32::from_rgb(0xFF, 0x6B, 0x6B),
    warning: Color32::from_rgb(0xFF, 0xD1, 0x66),
};

/// Light surfaces for bright rooms.
const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF2, 0xF2, 0xF2),
    card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    card_hover: Color32::from_rgb(0xE9, 0xEE, 0xF5),
    sidebar: Color32::from_rgb(0xEC, 0xEC, 0xEC),
    input_bg: Color32::from_rgb(0xF7, 0xF7, 0xF7),
    accent: Color32::from_rgb(0x0F, 0x76, 0xBD),
    accent_hover: Color32::from_rgb(0x1B, 0x8A, 0xD8),
    accent_dim: Color32::from_rgb(0x9A, 0xC9, 0xE8),
    accent_fill: Color32::from_rgb(0xDC, 0xEC, 0xFA),
    accent_alt: Color32::from_rgb(0x10, 0x8A, 0x74),
    border: Color32::from_rgb(0xD6, 0xD6, 0xD6),
    text: Color32::from_rgb(0x1A, 0x1A, 0x1A),
    text_dim: Color32::from_rgb(0x4A, 0x4A, 0x4A),
    text_faint: Color32::from_rgb(0x8A, 0x8A, 0x8A),
    danger: Color32::from_rgb(0xC4, 0x28, 0x28),
    warning: Color32::from_rgb(0xB2, 0x7A, 0x00),
};

static PALETTE: std::sync::RwLock<Palette> = std::sync::RwLock::new(DARK);

/// The palette currently in use.
pub fn palette() -> Palette {
    *PALETTE.read().unwrap_or_else(|e| e.into_inner())
}

/// Replace the palette (called when the user changes the appearance settings).
pub fn set_palette(p: Palette) {
    *PALETTE.write().unwrap_or_else(|e| e.into_inner()) = p;
}

/// Corner radius currently in use.
static CORNER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(8);

pub fn corner() -> f32 {
    CORNER.load(std::sync::atomic::Ordering::Relaxed) as f32
}

pub fn set_corner(radius: f32) {
    CORNER.store(radius.clamp(0.0, 24.0) as u32, std::sync::atomic::Ordering::Relaxed);
}

pub fn c_bg() -> Color32 {
    palette().bg
}
pub fn c_card() -> Color32 {
    palette().card
}
pub fn c_card_hover() -> Color32 {
    palette().card_hover
}
pub fn c_sidebar() -> Color32 {
    palette().sidebar
}
pub fn c_input_bg() -> Color32 {
    palette().input_bg
}
pub fn c_accent() -> Color32 {
    palette().accent
}
pub fn c_accent_hover() -> Color32 {
    palette().accent_hover
}
pub fn c_accent_dim() -> Color32 {
    palette().accent_dim
}
pub fn c_accent_fill() -> Color32 {
    palette().accent_fill
}
pub fn c_accent_alt() -> Color32 {
    palette().accent_alt
}
pub fn c_border() -> Color32 {
    palette().border
}
pub fn c_text() -> Color32 {
    palette().text
}
pub fn c_text_dim() -> Color32 {
    palette().text_dim
}
pub fn c_text_faint() -> Color32 {
    palette().text_faint
}
pub fn c_danger() -> Color32 {
    palette().danger
}
pub fn c_warning() -> Color32 {
    palette().warning
}

/// `#RRGGBB` text for a colour.
pub fn hex_color(color: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b())
}

/// Accent of a theme variant when the user has not picked a custom colour.
///
/// The accent field in Settings shows this value by default, so the user always sees the exact
/// hex code that is in use instead of an empty box with a hint.
pub fn default_accent(variant: &str) -> Color32 {
    match variant {
        "midnight" => MIDNIGHT.accent,
        "light" => LIGHT.accent,
        _ => DARK.accent,
    }
}

/// Parse a `#RRGGBB` colour string.
pub fn parse_color(text: &str) -> Option<Color32> {
    let t = text.trim();
    let hex = t.strip_prefix('#')?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(Color32::from_rgb(
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        )),
        3 => {
            let r = ((value >> 8) & 0xF) as u8;
            let g = ((value >> 4) & 0xF) as u8;
            let b = (value & 0xF) as u8;
            Some(Color32::from_rgb(r * 17, g * 17, b * 17))
        }
        _ => None,
    }
}

/// The colour a custom accent is mixed with (darker variant of the accent).
fn dim_of(accent: Color32) -> Color32 {
    Color32::from_rgb(
        (accent.r() as f32 * 0.68) as u8,
        (accent.g() as f32 * 0.68) as u8,
        (accent.b() as f32 * 0.68) as u8,
    )
}

fn hover_of(accent: Color32) -> Color32 {
    Color32::from_rgb(
        (accent.r() as f32 * 1.25).min(255.0) as u8,
        (accent.g() as f32 * 1.25).min(255.0) as u8,
        (accent.b() as f32 * 1.25).min(255.0) as u8,
    )
}

fn fill_of(accent: Color32) -> Color32 {
    Color32::from_rgb(
        (accent.r() as f32 * 0.22) as u8,
        (accent.g() as f32 * 0.22) as u8,
        (accent.b() as f32 * 0.22) as u8,
    )
}

/// Build the palette from the user's appearance settings.
pub fn palette_for(settings: &crate::settings::ThemeSettings) -> Palette {
    let mut p = match settings.variant.as_str() {
        "midnight" => MIDNIGHT,
        "light" => LIGHT,
        _ => DARK,
    };
    if let Some(custom) = parse_color(&settings.accent) {
        p.accent = custom;
        p.accent_hover = hover_of(custom);
        p.accent_dim = dim_of(custom);
        p.accent_fill = fill_of(custom);
    }
    if let Some(custom) = parse_color(&settings.bg) {
        p.bg = custom;
    }
    p
}
// ---- egui style ------------------------------------------------------------------------------

/// Install the VibeDrop look into an egui context. Call once at start-up.
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = palette().bg;
    v.window_fill = palette().card;
    v.extreme_bg_color = palette().input_bg;
    v.faint_bg_color = palette().card;
    v.code_bg_color = palette().card_hover;
    v.window_stroke = Stroke::new(1.0_f32, c_border());
    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(8.0);
    v.window_shadow = Shadow {
        offset: vec2(0.0, 16.0),
        blur: 38.0,
        spread: 0.0,
        color: Color32::from_black_alpha(205),
    };
    v.popup_shadow = Shadow {
        offset: vec2(0.0, 9.0),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(175),
    };
    v.selection.bg_fill = c_accent_dim();
    v.selection.stroke = Stroke::new(1.0_f32, c_accent());
    v.hyperlink_color = c_accent();
    v.slider_trailing_fill = true;

    let r = Rounding::same(corner());

    v.widgets.noninteractive.bg_fill = c_card();
    v.widgets.noninteractive.weak_bg_fill = c_card();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c_border());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, c_text());
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = c_input_bg();
    v.widgets.inactive.weak_bg_fill = c_card_hover();
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, c_border());
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, c_text());
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = c_card_hover();
    v.widgets.hovered.weak_bg_fill = c_accent_fill();
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, c_accent_dim());
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, c_accent());
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = c_accent_fill();
    v.widgets.active.weak_bg_fill = c_accent_fill();
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, c_accent());
    v.widgets.active.fg_stroke = Stroke::new(2.0_f32, c_accent());
    v.widgets.active.rounding = r;

    v.widgets.open.bg_fill = c_card_hover();
    v.widgets.open.weak_bg_fill = c_card_hover();
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, c_accent_dim());
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, c_text());
    v.widgets.open.rounding = r;

    ctx.set_visuals(v);

    // Fluent look: use the system UI font (Segoe UI on Windows) when available.
    load_system_font(ctx);

    ctx.style_mut(|s| {
        let font = font_scale();
        s.spacing.item_spacing = vec2(13.0, 12.0);
        s.spacing.button_padding = vec2(17.0, 9.0);
        s.spacing.interact_size.y = 36.0;
        s.spacing.window_margin = Margin::same(22.0);
        s.spacing.slider_width = 180.0;
        s.spacing.combo_width = 230.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::new(font + 19.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(font, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(font - 0.5, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(font - 2.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(font - 1.0, FontFamily::Monospace)),
        ]
        .into();
    });
}

/// The base font size the user chose.
static FONT_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(150);

pub fn font_scale() -> f32 {
    FONT_SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 10.0
}

pub fn set_font_scale(size: f32) {
    FONT_SCALE.store(
        (size.clamp(12.0, 20.0) * 10.0) as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Apply the user's appearance settings: palette, corner radius and font size.
pub fn apply_settings(ctx: &egui::Context, settings: &crate::settings::ThemeSettings) {
    set_palette(palette_for(settings));
    set_corner(settings.corner_radius);
    set_font_scale(settings.font_scale);
    apply(ctx);
}

// ---- Frames ----------------------------------------------------------------------------------

/// Fluent look: prefer the system UI font (Segoe UI on Windows). Falls back silently.
fn load_system_font(ctx: &egui::Context) {
    if !cfg!(target_os = "windows") {
        return;
    }
    let path = std::path::Path::new(r"C:\Windows\Fonts\segoeui.ttf");
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("segoe".to_owned(), FontData::from_static(bytes));
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "segoe".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// A rounded card with a thin border.
pub fn frame_card() -> egui::Frame {
    egui::Frame::none()
        .fill(c_card())
        .stroke(Stroke::new(1.0_f32, c_border()))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(16.0, 12.0))
}

/// A sound row card: quietly tinted while it is playing.
pub fn sound_card(playing: bool) -> egui::Frame {
    egui::Frame::none()
        .fill(if playing { c_accent_fill() } else { c_card() })
        .stroke(Stroke::new(1.0_f32, if playing { c_accent_dim() } else { c_border() }))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(15.0, 11.0))
}

/// A sound tile card: loudly tinted while it is playing.
pub fn tile_card(playing: bool) -> egui::Frame {
    egui::Frame::none()
        .fill(if playing { c_accent_fill() } else { c_card() })
        .stroke(Stroke::new(1.0_f32, if playing { c_accent() } else { c_border() }))
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(15.0, 13.0))
}

/// A right-hand control-deck card.
pub fn deck_card() -> egui::Frame {
    egui::Frame::none()
        .fill(c_card())
        .stroke(Stroke::new(1.0_f32, c_border()))
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(16.0, 13.0))
}

/// A soft inset field used for search and filters.
pub fn inset() -> egui::Frame {
    egui::Frame::none()
        .fill(c_input_bg())
        .stroke(Stroke::new(1.0_f32, c_border()))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(13.0, 7.0))
}

/// Smaller section title used inside the settings page.
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).strong().color(c_accent()));
}

// ---- Text buttons ----------------------------------------------------------------------------

/// Filled amber call-to-action button.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(c_bg()).strong())
            .fill(c_accent())
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(12.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Outlined neutral button.
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(c_text()))
            .fill(c_card_hover())
            .stroke(Stroke::new(1.0_f32, c_border()))
            .rounding(Rounding::same(12.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Red-tinted button for destructive actions.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(c_danger()).strong())
            .fill(Color32::from_rgb(0x3B, 0x1B, 0x1B))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x6B, 0x2B, 0x2B)))
            .rounding(Rounding::same(12.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Pill-shaped label that can be clicked (used for hotkeys).
pub fn chip(ui: &mut Ui, text: &str, color: Color32, filled: bool) -> Response {
    let fill = if filled { c_accent_fill() } else { Color32::TRANSPARENT };
    let border = if filled { color } else { c_border() };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.5).color(color))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, border))
            .rounding(Rounding::same(20.0))
            .min_size(vec2(0.0, 26.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Compact slider painted by hand.
///
/// egui's own slider draws the handle as a circle that reaches past the right edge of the
/// widget, so at 100 % the handle sat on top of the percentage next to it. This widget keeps
/// the handle inside its own rectangle and paints the track the way the rest of the app does.
/// It is used for every setting that is a number (volume, corner radius, font size, ...).
pub fn range_slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, enabled: bool) -> Response {
    let width = ui.spacing_mut().slider_width.max(40.0);
    let height = 18.0;
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, height), sense);
    let min = *range.start();
    let max = *range.end();
    let span = (max - min).max(f32::EPSILON);

    if enabled && (response.clicked() || response.dragged()) {
        if let Some(pos) = response.interact_pointer_pos() {
            let frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            let new_value = min + frac * span;
            if (new_value - *value).abs() > f32::EPSILON {
                *value = new_value;
                response.mark_changed();
            }
        }
    }

    let frac = ((*value - min) / span).clamp(0.0, 1.0);
    let rail = Rect::from_center_size(rect.center(), vec2(rect.width(), 6.0));
    ui.painter().rect_filled(rail, 3.0, c_border());
    let filled = Rect::from_min_size(rail.min, vec2(rail.width() * frac, rail.height()));
    ui.painter().rect_filled(filled, 3.0, c_accent());

    // The handle is a solid dark circle with a light rim. It is clamped inside the widget, so it
    // never reaches over the label that follows the slider.
    let radius = (height / 2.0 - 1.0).clamp(4.0, 7.0);
    let center_x = (rail.left() + rail.width() * frac).clamp(rect.left() + radius, rect.right() - radius);
    let center = Pos2::new(center_x, rect.center().y);
    ui.painter().circle_filled(center, radius, Color32::BLACK);
    ui.painter().circle_stroke(
        center,
        radius,
        Stroke::new(1.4_f32, if response.hovered() { c_accent() } else { c_text_dim() }),
    );
    response
}

/// Volume slider (0..=1) with the same look as every other setting slider.
pub fn volume_slider(ui: &mut Ui, value: &mut f32, enabled: bool) -> Response {
    range_slider(ui, value, 0.0..=1.0, enabled)
}

// ---- Icons -----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Stop,
    Pause,
    Star,
    StarOutline,
    Library,
    History,
    Settings,
    Volume,
    Search,
    Quit,
    /// Headphones (listen device).
    Headphones,
    /// Microphone.
    Mic,
    /// "Add" / plus.
    Plus,
    /// Home / category.
    Home,
    /// Stop-all square.
    StopAll,
    /// Undo / reload.
    Reload,
    /// Wrench / settings shortcut.
    Wrench,
    /// Music note (Music section).
    Music,
    /// Palette / colours.
    Palette,
    /// Previous track.
    Previous,
    /// Next track.
    Next,
}

fn star_points(c: Pos2, outer: f32) -> Vec<Pos2> {
    (0..10)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let radius = if i % 2 == 0 { outer } else { outer * 0.42 };
            Pos2::new(c.x + radius * angle.cos(), c.y + radius * angle.sin())
        })
        .collect()
}

/// Paint an icon centered at `c`; `s` is the nominal icon size in points.
pub fn paint_icon(p: &egui::Painter, icon: Icon, c: Pos2, s: f32, color: Color32) {
    let stroke = Stroke::new((s * 0.09).max(1.4), color);
    match icon {
        Icon::Play => {
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.26, c.y - s * 0.38),
                    Pos2::new(c.x - s * 0.26, c.y + s * 0.38),
                    Pos2::new(c.x + s * 0.40, c.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Stop => {
            let r = Rect::from_center_size(c, vec2(s * 0.58, s * 0.58));
            p.rect_filled(r, Rounding::same(s * 0.08), color);
        }
        Icon::Star | Icon::StarOutline => {
            let pts = star_points(c, s * 0.5);
            if icon == Icon::Star {
                for i in 0..10 {
                    p.add(Shape::convex_polygon(
                        vec![c, pts[i], pts[(i + 1) % 10]],
                        color,
                        Stroke::NONE,
                    ));
                }
            }
            p.add(Shape::closed_line(pts, stroke));
        }
        Icon::Library => {
            let cell = s * 0.34;
            let gap = s * 0.10;
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let center = Pos2::new(c.x + dx * (cell + gap) / 2.0, c.y + dy * (cell + gap) / 2.0);
                p.rect_filled(
                    Rect::from_center_size(center, vec2(cell, cell)),
                    Rounding::same(s * 0.07),
                    color,
                );
            }
        }
        Icon::History => {
            p.circle_stroke(c, s * 0.42, stroke);
            p.add(Shape::line_segment([c, Pos2::new(c.x, c.y - s * 0.26)], stroke));
            p.add(Shape::line_segment(
                [c, Pos2::new(c.x + s * 0.20, c.y + s * 0.10)],
                stroke,
            ));
        }
        Icon::Settings => {
            let half = s * 0.42;
            for (dy, knob) in [(-0.27, -0.18), (0.0, 0.20), (0.27, -0.05)] {
                let y = c.y + dy * s;
                p.add(Shape::line_segment(
                    [Pos2::new(c.x - half, y), Pos2::new(c.x + half, y)],
                    stroke,
                ));
                p.circle_filled(Pos2::new(c.x + knob * s, y), s * 0.09, color);
            }
        }
        Icon::Volume => {
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(c.x - s * 0.40, c.y - s * 0.15),
                    Pos2::new(c.x - s * 0.14, c.y + s * 0.15),
                ),
                Rounding::same(s * 0.04),
                color,
            );
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.14, c.y - s * 0.15),
                    Pos2::new(c.x + s * 0.10, c.y - s * 0.38),
                    Pos2::new(c.x + s * 0.10, c.y + s * 0.38),
                    Pos2::new(c.x - s * 0.14, c.y + s * 0.15),
                ],
                color,
                Stroke::NONE,
            ));
            let arc: Vec<Pos2> = (-4..=4)
                .map(|i| {
                    let a = i as f32 * 0.14;
                    Pos2::new(c.x + s * 0.12 + s * 0.26 * a.cos(), c.y + s * 0.26 * a.sin())
                })
                .collect();
            p.add(Shape::line(arc, stroke));
        }
        Icon::Quit => {
            p.circle_stroke(c, s * 0.38, stroke);
            p.add(Shape::line_segment(
                [Pos2::new(c.x, c.y - s * 0.46), Pos2::new(c.x, c.y - s * 0.08)],
                Stroke::new(stroke.width + 0.6, color),
            ));
        }
        Icon::Search => {
            let center = Pos2::new(c.x - s * 0.08, c.y - s * 0.08);
            p.circle_stroke(center, s * 0.28, stroke);
            p.add(Shape::line_segment(
                [
                    Pos2::new(center.x + s * 0.20, center.y + s * 0.20),
                    Pos2::new(c.x + s * 0.38, c.y + s * 0.38),
                ],
                Stroke::new(stroke.width + 0.5, color),
            ));
        }
        Icon::Pause => {
            let h = s * 0.34;
            let w = s * 0.11;
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x - s * 0.16, c.y), vec2(w, h * 2.0)),
                Rounding::same(s * 0.03),
                color,
            );
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x + s * 0.16, c.y), vec2(w, h * 2.0)),
                Rounding::same(s * 0.03),
                color,
            );
        }
        Icon::Headphones => {
            p.circle_stroke(c, s * 0.40, stroke);
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x - s * 0.30, c.y + s * 0.12), vec2(s * 0.16, s * 0.26)),
                Rounding::same(s * 0.05),
                color,
            );
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x + s * 0.30, c.y + s * 0.12), vec2(s * 0.16, s * 0.26)),
                Rounding::same(s * 0.05),
                color,
            );
        }
        Icon::Mic => {
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x, c.y - s * 0.10), vec2(s * 0.24, s * 0.42)),
                Rounding::same(s * 0.12),
                color,
            );
            let arc = (-3..=3)
                .map(|i| {
                    let a = i as f32 * 0.28;
                    Pos2::new(c.x + a * s * 0.34, c.y + s * 0.16 + a * a * s * 0.06)
                })
                .collect::<Vec<_>>();
            p.add(Shape::line(arc, stroke));
            p.add(Shape::line_segment(
                [Pos2::new(c.x, c.y + s * 0.30), Pos2::new(c.x, c.y + s * 0.42)],
                stroke,
            ));
        }
        Icon::Plus => {
            p.add(Shape::line_segment(
                [Pos2::new(c.x - s * 0.34, c.y), Pos2::new(c.x + s * 0.34, c.y)],
                stroke,
            ));
            p.add(Shape::line_segment(
                [Pos2::new(c.x, c.y - s * 0.34), Pos2::new(c.x, c.y + s * 0.34)],
                stroke,
            ));
        }
        Icon::Home => {
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - s * 0.40, c.y + s * 0.02),
                    Pos2::new(c.x, c.y - s * 0.34),
                    Pos2::new(c.x + s * 0.40, c.y + s * 0.02),
                ],
                color,
                Stroke::NONE,
            ));
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x, c.y + s * 0.20), vec2(s * 0.54, s * 0.34)),
                Rounding::same(s * 0.05),
                color,
            );
        }
        Icon::StopAll => {
            p.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.52, s * 0.52)),
                Rounding::same(s * 0.06),
                color,
            );
        }
        Icon::Reload => {
            let arc = (0..=10)
                .map(|i| {
                    let a = i as f32 * 0.42 - 1.2;
                    Pos2::new(c.x + a.cos() * s * 0.30, c.y + a.sin() * s * 0.30)
                })
                .collect::<Vec<_>>();
            p.add(Shape::line(arc, stroke));
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x + s * 0.16, c.y - s * 0.44),
                    Pos2::new(c.x + s * 0.40, c.y - s * 0.20),
                    Pos2::new(c.x + s * 0.10, c.y - s * 0.16),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Wrench => {
            p.add(Shape::line_segment(
                [
                    Pos2::new(c.x - s * 0.30, c.y + s * 0.30),
                    Pos2::new(c.x + s * 0.16, c.y - s * 0.16),
                ],
                stroke,
            ));
            p.circle_stroke(Pos2::new(c.x + s * 0.22, c.y - s * 0.22), s * 0.20, stroke);
        }
        Icon::Music => {
            // Stem with a filled note head and a small flag.
            p.add(Shape::line_segment(
                [
                    Pos2::new(c.x + s * 0.10, c.y + s * 0.16),
                    Pos2::new(c.x + s * 0.10, c.y - s * 0.40),
                ],
                stroke,
            ));
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x + s * 0.10, c.y - s * 0.40),
                    Pos2::new(c.x + s * 0.38, c.y - s * 0.28),
                    Pos2::new(c.x + s * 0.36, c.y - s * 0.04),
                    Pos2::new(c.x + s * 0.10, c.y - s * 0.14),
                ],
                color,
                Stroke::NONE,
            ));
            p.circle_filled(Pos2::new(c.x - s * 0.10, c.y + s * 0.24), s * 0.17, color);
        }
        Icon::Palette => {
            // A small colour wheel: an arc of the accent with three dots.
            p.circle_stroke(Pos2::new(c.x, c.y), s * 0.40, Stroke::new(1.4_f32, color));
            for (dx, dy, col) in [
                (-0.22, -0.12, Color32::from_rgb(0x2E, 0x9B, 0xE6)),
                (0.18, -0.18, Color32::from_rgb(0x63, 0xC4, 0x54)),
                (0.10, 0.22, Color32::from_rgb(0xE6, 0x9B, 0x2E)),
            ] {
                p.circle_filled(Pos2::new(c.x + dx * s, c.y + dy * s), s * 0.13, col);
            }
        }
        Icon::Previous => {
            let h = s * 0.34;
            for dx in [-0.16, 0.12] {
                p.add(Shape::convex_polygon(
                    vec![
                        Pos2::new(c.x + dx * s + s * 0.16, c.y - h),
                        Pos2::new(c.x + dx * s + s * 0.16, c.y + h),
                        Pos2::new(c.x + dx * s - s * 0.10, c.y),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x - s * 0.36, c.y), vec2(s * 0.08, h * 2.0)),
                Rounding::same(s * 0.03),
                color,
            );
        }
        Icon::Next => {
            let h = s * 0.34;
            for dx in [-0.12, 0.16] {
                p.add(Shape::convex_polygon(
                    vec![
                        Pos2::new(c.x + dx * s - s * 0.16, c.y - h),
                        Pos2::new(c.x + dx * s - s * 0.16, c.y + h),
                        Pos2::new(c.x + dx * s + s * 0.10, c.y),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            p.rect_filled(
                Rect::from_center_size(Pos2::new(c.x + s * 0.36, c.y), vec2(s * 0.08, h * 2.0)),
                Rounding::same(s * 0.03),
                color,
            );
        }
    }
}

/// Brand mark for the rail: the VibeDrop pulse on a rounded blue tile.
pub fn brand_mark(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, Rounding::same(rect.width() * 0.32), c_accent());
    let w = rect.width();
    let h = rect.height();
    // A single heartbeat line, drawn as a stroked polyline.
    let pts = [
        (-0.42, 0.02),
        (-0.26, 0.02),
        (-0.14, -0.30),
        (0.02, 0.34),
        (0.16, -0.12),
        (0.26, 0.02),
        (0.42, 0.02),
    ];
    let line: Vec<Pos2> = pts
        .iter()
        .map(|(x, y)| Pos2::new(rect.center().x + x * w, rect.center().y + y * h))
        .collect();
    p.add(Shape::line(line, Stroke::new((w * 0.085).max(1.8), Color32::WHITE)));
}

/// Rail navigation button with icon and tiny label.
pub fn rail_button(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(68.0, 66.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, Rounding::same(18.0), c_accent_fill());
            p.rect_stroke(rect, Rounding::same(18.0), Stroke::new(1.0_f32, c_accent_dim()));
        } else if hovered {
            p.rect_filled(rect, Rounding::same(18.0), c_card());
        }
        let color = if selected {
            c_accent()
        } else if hovered {
            c_text()
        } else {
            c_text_dim()
        };
        paint_icon(p, icon, Pos2::new(rect.center().x, rect.top() + 24.0), 20.0, color);
        p.text(
            Pos2::new(rect.center().x, rect.bottom() - 8.0),
            Align2::CENTER_BOTTOM,
            label,
            FontId::new(9.5, FontFamily::Proportional),
            color,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// A flat toolbar button with an icon and a tiny caption (soundboard style).
pub fn toolbar_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        if hovered {
            ui.painter().rect_filled(rect, Rounding::same(6.0), c_card_hover());
        }
        let color = if hovered { c_accent() } else { c_text_dim() };
        paint_icon(ui.painter(), icon, rect.center(), 18.0, color);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(tip)
}

/// Flat, square icon button. `active` tints it with the accent color.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, active: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size + 10.0, size + 10.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        if hovered {
            ui.painter().rect_filled(rect, Rounding::same(8.0), c_card_hover());
        }
        let color = if active {
            c_accent()
        } else if hovered {
            c_text()
        } else {
            c_text_dim()
        };
        paint_icon(ui.painter(), icon, rect.center(), size, color);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Round button with an icon. `primary` = solid accent circle, otherwise outlined.
pub fn round_button(ui: &mut Ui, icon: Icon, diameter: f32, primary: bool, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(diameter, diameter), sense);
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered() && enabled;
        let (bg, fg, stroke) = if !enabled {
            (c_card_hover(), c_text_faint(), Stroke::new(1.0_f32, c_border()))
        } else if primary {
            (
                if hovered { c_accent_hover() } else { c_accent() },
                c_bg(),
                Stroke::NONE,
            )
        } else {
            (
                if hovered { c_accent_fill() } else { c_card_hover() },
                c_accent(),
                Stroke::new(1.0_f32, if hovered { c_accent() } else { c_border() }),
            )
        };
        ui.painter().circle(rect.center(), diameter / 2.0, bg, stroke);
        paint_icon(ui.painter(), icon, rect.center(), diameter * 0.5, fg);
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Sidebar navigation row. `show_label` = false draws only the icon.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool, show_label: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, Rounding::same(10.0), c_accent_fill());
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + 9.0),
                    Pos2::new(rect.left() + 3.0, rect.bottom() - 9.0),
                ),
                Rounding::same(2.0),
                c_accent(),
            );
        } else if hovered {
            p.rect_filled(rect, Rounding::same(10.0), c_card());
        }
        let color = if selected {
            c_accent()
        } else if hovered {
            c_text()
        } else {
            c_text_dim()
        };
        let icon_x = if show_label {
            rect.left() + 24.0
        } else {
            rect.center().x
        };
        paint_icon(p, icon, Pos2::new(icon_x, rect.center().y), 20.0, color);
        if show_label {
            p.text(
                Pos2::new(rect.left() + 48.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::new(15.0, FontFamily::Proportional),
                color,
            );
        }
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Top-bar navigation pill. Selected pills get a quiet accent wash and border.
pub fn nav_pill(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::new(14.5, FontFamily::Proportional), c_text())
        .rect
        .width();
    let (rect, resp) = ui.allocate_exact_size(vec2(text_width + 58.0, 42.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, Rounding::same(14.0), c_accent_fill());
            p.rect_stroke(rect, Rounding::same(14.0), Stroke::new(1.0_f32, c_accent_dim()));
        } else if hovered {
            p.rect_filled(rect, Rounding::same(14.0), c_card());
        }
        let color = if selected {
            c_accent()
        } else if hovered {
            c_text()
        } else {
            c_text_dim()
        };
        paint_icon(p, icon, Pos2::new(rect.left() + 21.0, rect.center().y), 17.0, color);
        p.text(
            Pos2::new(rect.left() + 37.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::new(14.5, FontFamily::Proportional),
            color,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

// ---- Formatting helpers ----------------------------------------------------------------------

/// `m:ss` for a number of seconds.
pub fn fmt_time(secs: f64) -> String {
    let total = if secs.is_finite() && secs > 0.0 {
        secs.round() as u64
    } else {
        0
    };
    format!("{}:{:02}", total / 60, total % 60)
}

/// Rough "5 min ago" text for a unix timestamp.
pub fn fmt_ago(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=4 => "just now".to_string(),
        5..=59 => format!("{d} s ago"),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        _ => format!("{} d ago", d / 86400),
    }
}
