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

/// Window / page background `#3C3C3C` (soundboard grey).
pub const BG: Color32 = Color32::from_rgb(0x3C, 0x3C, 0x3C);
/// Cards and panels `#4A4A4A`.
pub const CARD: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x4A);
/// Card under the pointer.
pub const CARD_HOVER: Color32 = Color32::from_rgb(0x56, 0x56, 0x56);
/// Rail / deck background, a touch darker than the cards.
pub const SIDEBAR: Color32 = Color32::from_rgb(0x45, 0x45, 0x45);
/// Input fields and slider rails.
pub const INPUT_BG: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);
/// Primary accent `#2E9BE6` (soundboard blue).
pub const ACCENT: Color32 = Color32::from_rgb(0x2E, 0x9B, 0xE6);
/// Accent on hover (slightly lighter).
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x4A, 0xB4, 0xF5);
/// Muted accent for secondary strokes.
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1E, 0x6B, 0xA6);
/// Dark blue tint used behind selected / hovered accent elements.
pub const ACCENT_FILL: Color32 = Color32::from_rgb(0x1F, 0x3F, 0x5C);
/// Secondary accent for highlights and the brand mark.
pub const ACCENT_ALT: Color32 = Color32::from_rgb(0x7C, 0xF0, 0xD4);
/// Borders `#2E2E2E`.
pub const BORDER: Color32 = Color32::from_rgb(0x2E, 0x2E, 0x2E);
/// Main text `#F2F2F2`.
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xF2);
/// Secondary text.
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xC0, 0xC0, 0xC0);
/// Disabled text and icons.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x8A, 0x8A, 0x8A);
/// Errors and destructive actions.
pub const DANGER: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);
/// Warnings.
pub const WARNING: Color32 = Color32::from_rgb(0xFF, 0xC8, 0x57);

// ---- egui style ------------------------------------------------------------------------------

/// Install the VibeDrop look into an egui context. Call once at start-up.
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = INPUT_BG;
    v.faint_bg_color = CARD;
    v.code_bg_color = CARD_HOVER;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
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
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.hyperlink_color = ACCENT;
    v.slider_trailing_fill = true;

    let r = Rounding::same(6.0);

    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.weak_bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = INPUT_BG;
    v.widgets.inactive.weak_bg_fill = CARD_HOVER;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = CARD_HOVER;
    v.widgets.hovered.weak_bg_fill = ACCENT_FILL;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_DIM);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, ACCENT);
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = ACCENT_FILL;
    v.widgets.active.weak_bg_fill = ACCENT_FILL;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(2.0_f32, ACCENT);
    v.widgets.active.rounding = r;

    v.widgets.open.bg_fill = CARD_HOVER;
    v.widgets.open.weak_bg_fill = CARD_HOVER;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT_DIM);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.open.rounding = r;

    ctx.set_visuals(v);

    // Fluent look: use the system UI font (Segoe UI on Windows) when available.
    load_system_font(ctx);

    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(13.0, 12.0);
        s.spacing.button_padding = vec2(17.0, 9.0);
        s.spacing.interact_size.y = 36.0;
        s.spacing.window_margin = Margin::same(22.0);
        s.spacing.slider_width = 180.0;
        s.spacing.combo_width = 230.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::new(34.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
    });
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
pub fn card() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(16.0, 12.0))
}

/// A sound row card: quietly tinted while it is playing.
pub fn sound_card(playing: bool) -> egui::Frame {
    egui::Frame::none()
        .fill(if playing { ACCENT_FILL } else { CARD })
        .stroke(Stroke::new(1.0_f32, if playing { ACCENT_DIM } else { BORDER }))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(15.0, 11.0))
}

/// A sound tile card: loudly tinted while it is playing.
pub fn tile_card(playing: bool) -> egui::Frame {
    egui::Frame::none()
        .fill(if playing { ACCENT_FILL } else { CARD })
        .stroke(Stroke::new(1.0_f32, if playing { ACCENT } else { BORDER }))
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(15.0, 13.0))
}

/// A right-hand control-deck card.
pub fn deck_card() -> egui::Frame {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(20.0))
        .inner_margin(Margin::symmetric(16.0, 13.0))
}

/// A soft inset field used for search and filters.
pub fn inset() -> egui::Frame {
    egui::Frame::none()
        .fill(INPUT_BG)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(6.0))
        .inner_margin(Margin::symmetric(13.0, 7.0))
}

/// Smaller section title used inside the settings page.
pub fn section_title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).strong().color(ACCENT));
}

// ---- Text buttons ----------------------------------------------------------------------------

/// Filled amber call-to-action button.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(BG).strong())
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .rounding(Rounding::same(12.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Outlined neutral button.
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(TEXT))
            .fill(CARD_HOVER)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .rounding(Rounding::same(12.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Red-tinted button for destructive actions.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(DANGER).strong())
            .fill(Color32::from_rgb(0x3B, 0x1B, 0x1B))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x6B, 0x2B, 0x2B)))
            .rounding(Rounding::same(12.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
}

/// Pill-shaped label that can be clicked (used for hotkeys).
pub fn chip(ui: &mut Ui, text: &str, color: Color32, filled: bool) -> Response {
    let fill = if filled { ACCENT_FILL } else { Color32::TRANSPARENT };
    let border = if filled { color } else { BORDER };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.5).color(color))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, border))
            .rounding(Rounding::same(20.0))
            .min_size(vec2(0.0, 26.0)),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
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
    }
}

/// Brand mark for the rail: the VibeDrop pulse on a rounded blue tile.
pub fn brand_mark(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, Rounding::same(rect.width() * 0.32), ACCENT);
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
            p.rect_filled(rect, Rounding::same(18.0), ACCENT_FILL);
            p.rect_stroke(rect, Rounding::same(18.0), Stroke::new(1.0_f32, ACCENT_DIM));
        } else if hovered {
            p.rect_filled(rect, Rounding::same(18.0), CARD);
        }
        let color = if selected {
            ACCENT
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
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
            ui.painter().rect_filled(rect, Rounding::same(6.0), CARD_HOVER);
        }
        let color = if hovered { ACCENT } else { TEXT_DIM };
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
            ui.painter().rect_filled(rect, Rounding::same(8.0), CARD_HOVER);
        }
        let color = if active {
            ACCENT
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
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
            (CARD_HOVER, TEXT_FAINT, Stroke::new(1.0_f32, BORDER))
        } else if primary {
            (if hovered { ACCENT_HOVER } else { ACCENT }, BG, Stroke::NONE)
        } else {
            (
                if hovered { ACCENT_FILL } else { CARD_HOVER },
                ACCENT,
                Stroke::new(1.0_f32, if hovered { ACCENT } else { BORDER }),
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

/// Sidebar navigation row.
pub fn nav_item(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, Rounding::same(10.0), ACCENT_FILL);
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(rect.left(), rect.top() + 9.0),
                    Pos2::new(rect.left() + 3.0, rect.bottom() - 9.0),
                ),
                Rounding::same(2.0),
                ACCENT,
            );
        } else if hovered {
            p.rect_filled(rect, Rounding::same(10.0), CARD);
        }
        let color = if selected {
            ACCENT
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
        };
        paint_icon(p, icon, Pos2::new(rect.left() + 24.0, rect.center().y), 20.0, color);
        p.text(
            Pos2::new(rect.left() + 48.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::new(15.0, FontFamily::Proportional),
            color,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Top-bar navigation pill. Selected pills get a quiet accent wash and border.
pub fn nav_pill(ui: &mut Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), FontId::new(14.5, FontFamily::Proportional), TEXT)
        .rect
        .width();
    let (rect, resp) = ui.allocate_exact_size(vec2(text_width + 58.0, 42.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = resp.hovered();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, Rounding::same(14.0), ACCENT_FILL);
            p.rect_stroke(rect, Rounding::same(14.0), Stroke::new(1.0_f32, ACCENT_DIM));
        } else if hovered {
            p.rect_filled(rect, Rounding::same(14.0), CARD);
        }
        let color = if selected {
            ACCENT
        } else if hovered {
            TEXT
        } else {
            TEXT_DIM
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
