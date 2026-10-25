// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The design's components (spec §3), each a plain function over `Ui`.

use egui::epaint::Shadow;
use egui::{
    Align, Color32, CornerRadius, Frame, ImageSource, Layout, Margin, Response, RichText, Sense,
    Stroke, StrokeKind, TextWrapMode, Ui, UiBuilder, Vec2, WidgetText, vec2,
};

use super::icons;
use super::theme::{self, Palette, radius, style, weight};

/// The palette of the theme `ui` is drawing in.
pub fn p(ui: &Ui) -> Palette {
    theme::palette(ui.ctx())
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Body text, medium weight (row titles).
pub fn title_text(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text)
        .text_style(egui::TextStyle::Body)
        .family(weight::medium())
        .color(color)
}

/// Body text, semibold (card titles, the bold instruction).
pub fn strong_text(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text)
        .text_style(egui::TextStyle::Body)
        .family(weight::semibold())
        .color(color)
}

/// A supporting line (13/18).
pub fn secondary_text(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text)
        .text_style(style::secondary())
        .color(color)
}

/// Times, hints, labels (12/16).
pub fn small_text(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text).text_style(style::small()).color(color)
}

/// A label that cuts off with an ellipsis instead of widening its parent.
pub fn line(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    ui.add(egui::Label::new(text).truncate())
}

/// Settings section label (12/16 semibold, upper case).
pub fn overline(ui: &mut Ui, text: &str) -> Response {
    let p = p(ui);
    ui.label(
        RichText::new(text.to_uppercase())
            .text_style(style::small())
            .family(weight::semibold())
            .color(p.tx2)
            .extra_letter_spacing(0.7),
    )
}

// ---------------------------------------------------------------------------
// Brand
// ---------------------------------------------------------------------------

/// ✦ in amber with its glow.
pub fn star(ui: &mut Ui, size: f32) -> Response {
    let p = p(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let painter = ui.painter();
    let c = rect.center();
    painter.circle_filled(c, size * 0.42, p.amber_glow.gamma_multiply(0.45));
    // The four-point star (✦), drawn: no bundled font has the glyph. One
    // convex quad per arm, from the centre out to its tip.
    let outer = size * 0.46;
    let inner = size * 0.13;
    for i in 0..4 {
        let angle = std::f32::consts::FRAC_PI_2 * i as f32;
        let at = |a: f32, r: f32| c + egui::vec2(a.cos(), a.sin()) * r;
        painter.add(egui::Shape::convex_polygon(
            vec![
                c,
                at(angle - std::f32::consts::FRAC_PI_4, inner),
                at(angle, outer),
                at(angle + std::f32::consts::FRAC_PI_4, inner),
            ],
            p.amber,
            Stroke::NONE,
        ));
    }
    response
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

/// A card's frame (§1.3).
pub fn card_frame(ui: &Ui) -> Frame {
    let p = p(ui);
    Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 12,
            bottom: 14,
        })
        .shadow(Shadow {
            offset: [0, 1],
            blur: 2,
            spread: 0,
            color: p.card_shadow,
        })
}

/// A card: a 32 px header (title, optional suffix, right-aligned actions)
/// over its body.
pub fn card<R>(
    ui: &mut Ui,
    title: &str,
    suffix: Option<&str>,
    actions: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let pal = p(ui);
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 32.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(strong_text(title, pal.tx));
                if let Some(suffix) = suffix {
                    line(ui, small_text(suffix, pal.tx3));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    actions(ui);
                });
            },
        );
        body(ui)
    })
}

/// A 1 px divider that bleeds to the card's edges.
pub fn divider(ui: &mut Ui) {
    let pal = p(ui);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, pal.line);
}

// ---------------------------------------------------------------------------
// Status hero and stepper
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroVariant {
    Neutral,
    Ok,
    Bad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Pending,
    Now,
    Done,
    Failed,
}

/// The disc at a hero's left.
#[derive(Clone, Copy)]
pub enum Disc {
    Icon {
        icon: fn() -> ImageSource<'static>,
        fill: fn(&Palette) -> Color32,
        ink: fn(&Palette) -> Color32,
    },
    Spinner,
}

/// The capture status hero: disc, title, line, the three steps, and an
/// optional body (chip, instruction, buttons).
pub fn status_hero(
    ui: &mut Ui,
    variant: HeroVariant,
    disc: Disc,
    title: &str,
    subtitle: &str,
    steps: [Step; 3],
    extra: impl FnOnce(&mut Ui),
) -> Response {
    let pal = p(ui);
    let (fill, line_color) = match variant {
        HeroVariant::Neutral => (pal.card, pal.line),
        HeroVariant::Ok => (pal.ok_con, pal.ok_line),
        HeroVariant::Bad => (pal.bad_con, pal.bad_line),
    };
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, line_color))
        .corner_radius(CornerRadius::same(radius::HERO))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 14.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                match disc {
                    Disc::Icon { icon, fill, ink } => {
                        ui.painter().circle_filled(rect.center(), 22.0, fill(&pal));
                        let icon_rect =
                            egui::Rect::from_center_size(rect.center(), Vec2::splat(22.0));
                        icons::image(icon(), 22.0, ink(&pal)).paint_at(ui, icon_rect);
                    }
                    Disc::Spinner => {
                        ui.painter().circle_filled(rect.center(), 22.0, pal.over);
                        let spin = egui::Rect::from_center_size(rect.center(), Vec2::splat(22.0));
                        ui.put(spin, egui::Spinner::new().size(22.0).color(pal.ac_tx));
                    }
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    line(
                        ui,
                        RichText::new(title).text_style(style::hero()).color(pal.tx),
                    );
                    line(ui, secondary_text(subtitle, pal.tx2));
                });
            });
            stepper(ui, steps);
            extra(ui);
        })
        .response
}

/// Capture · Login · Key, each a 4 px bar over a label.
pub fn stepper(ui: &mut Ui, steps: [Step; 3]) {
    let pal = p(ui);
    let gap = 8.0;
    let width = (ui.available_width() - 2.0 * gap) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (step, label) in steps.into_iter().zip(["Capture", "Login", "Key"]) {
            let color = match step {
                Step::Pending => pal.line2,
                Step::Now => pal.ac_tx,
                Step::Done => pal.ok,
                Step::Failed => pal.bad,
            };
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 4.0;
                let (bar, _) = ui.allocate_exact_size(vec2(width, 4.0), Sense::hover());
                ui.painter().rect_filled(bar, 2.0, color);
                let text = if step == Step::Failed {
                    pal.bad
                } else {
                    pal.tx2
                };
                ui.label(small_text(label, text).family(weight::medium()));
            });
        }
    });
}

// ---------------------------------------------------------------------------
// Rows, tiles, switches
// ---------------------------------------------------------------------------

/// A stat tile: value over label, `—` when there is none.
pub fn stat_tile(ui: &mut Ui, width: f32, value: Option<usize>, label: &str, flash: f32) {
    let pal = p(ui);
    let fill = if flash > 0.0 {
        lerp_color(pal.over, pal.ok_con, flash)
    } else {
        pal.over
    };
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                match value {
                    Some(value) => line(
                        ui,
                        RichText::new(group_thousands(value))
                            .text_style(style::tile())
                            .color(pal.tx),
                    ),
                    None => line(
                        ui,
                        RichText::new("—")
                            .text_style(style::tile())
                            .family(weight::medium())
                            .color(pal.tx2),
                    ),
                };
                line(ui, small_text(label, pal.tx2));
            });
        });
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// The 36 × 20 switch.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let pal = p(ui);
    let (rect, mut response) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let painter = ui.painter();
    let track = lerp_color(pal.over, pal.fill, t);
    painter.rect_filled(rect, 10.0, track);
    if t < 1.0 {
        painter.rect_stroke(
            rect,
            10.0,
            Stroke::new(1.0_f32, pal.line2.gamma_multiply(1.0 - t)),
            StrokeKind::Inside,
        );
    }
    let knob_x = egui::lerp((rect.left() + 10.0)..=(rect.right() - 10.0), t);
    let knob = lerp_color(pal.tx3, Color32::WHITE, t);
    painter.circle_filled(egui::pos2(knob_x, rect.center().y), 7.0, knob);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    response
}

/// A row: optional leading icon, title over an optional secondary line,
/// and whatever `trailing` adds at the right, all centred on the row's
/// middle. As tall as its text plus 8 px, at least 40 px.
pub fn list_row<R>(
    ui: &mut Ui,
    leading: Option<(ImageSource<'static>, Color32)>,
    title: RichText,
    secondary: Option<RichText>,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    const MIN_HEIGHT: f32 = 40.0;
    const LINE_GAP: f32 = 1.0;
    const ICON: f32 = 18.0;
    const GAP: f32 = 10.0;
    // Lines are truncated, never wrapped, so the height doesn't depend on
    // the width the trailing controls leave.
    let height_of = |text: &RichText| {
        WidgetText::from(text.clone())
            .into_galley(
                ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Body,
            )
            .size()
            .y
    };
    let mut block = height_of(&title);
    if let Some(secondary) = &secondary {
        block += LINE_GAP + height_of(secondary);
    }
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), MIN_HEIGHT.max(block + 8.0)),
        Sense::hover(),
    );

    let mut controls = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    controls.spacing_mut().item_spacing.x = 6.0;
    let result = trailing(&mut controls);
    let used = controls.min_rect();
    let text_right = if used.width() > 0.0 {
        used.left() - GAP
    } else {
        rect.right()
    };

    let mut text_left = rect.left();
    if let Some((icon, color)) = leading {
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + ICON / 2.0, rect.center().y),
            Vec2::splat(ICON),
        );
        icons::image(icon, ICON, color).paint_at(ui, icon_rect);
        text_left += ICON + GAP;
    }
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(text_left, rect.center().y - block / 2.0),
        egui::pos2(text_right.max(text_left), rect.center().y + block / 2.0),
    );
    let mut text = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::LEFT)),
    );
    text.spacing_mut().item_spacing.y = LINE_GAP;
    line(&mut text, title);
    if let Some(secondary) = secondary {
        line(&mut text, secondary);
    }
    (response, result)
}

/// A row whose trailing control is a switch; the switch toggles it.
pub fn switch_row(
    ui: &mut Ui,
    title: &str,
    secondary: Option<RichText>,
    on: &mut bool,
    enabled: bool,
) -> Response {
    let pal = p(ui);
    let mut changed = None;
    let (row, ()) = list_row(ui, None, title_text(title, pal.tx), secondary, |ui| {
        changed = Some(ui.add_enabled_ui(enabled, |ui| switch(ui, on)).inner);
    });
    let mut response = changed.expect("switch drawn");
    if enabled {
        let row = ui.interact(row.rect, row.id.with("switch-row"), Sense::click());
        if row.clicked() && !response.clicked() {
            *on = !*on;
            response.mark_changed();
        }
    }
    response
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Fill,
    Secondary,
    Ghost,
    Danger,
}

/// A 32 px button (40 with `big`), optional 16 px icon.
pub fn button(
    ui: &mut Ui,
    kind: ButtonKind,
    icon: Option<ImageSource<'static>>,
    text: &str,
    big: bool,
) -> Response {
    let pal = p(ui);
    let (fill, stroke, ink) = match kind {
        ButtonKind::Fill => (pal.fill, Stroke::NONE, Color32::WHITE),
        ButtonKind::Secondary => (pal.card, Stroke::new(1.0_f32, pal.line2), pal.tx),
        ButtonKind::Ghost => (Color32::TRANSPARENT, Stroke::NONE, pal.ac_tx),
        ButtonKind::Danger => (
            Color32::TRANSPARENT,
            Stroke::new(1.0_f32, pal.bad_line),
            pal.bad,
        ),
    };
    let label = RichText::new(text)
        .text_style(style::secondary())
        .family(weight::semibold())
        .color(ink);
    let mut widget = match icon {
        Some(icon) => egui::Button::image_and_text(icons::image(icon, 16.0, ink), label),
        None => egui::Button::new(label),
    };
    widget = widget
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(radius::BUTTON))
        .min_size(vec2(0.0, if big { 40.0 } else { 32.0 }));
    ui.scope(|ui| {
        let padding = if kind == ButtonKind::Ghost { 8.0 } else { 12.0 };
        ui.spacing_mut().button_padding = vec2(padding, 6.0);
        ui.add(widget)
    })
    .inner
}

/// A 32 × 32 icon button with its name as the tooltip.
pub fn icon_button(ui: &mut Ui, icon: ImageSource<'static>, name: &str) -> Response {
    let pal = p(ui);
    let widget = egui::Button::image(icons::image(icon, 18.0, pal.tx2))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(radius::BUTTON))
        .min_size(Vec2::splat(32.0));
    ui.add(widget).on_hover_text(name)
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalloutKind {
    Warn,
    Error,
}

/// A warning or error note with its icon.
pub fn callout(ui: &mut Ui, kind: CalloutKind, text: &str) -> Response {
    let pal = p(ui);
    let (fill, ink, stroke, icon) = match kind {
        CalloutKind::Warn => (
            pal.warn_con,
            pal.warn_on,
            Stroke::NONE,
            icons::triangle_alert(),
        ),
        CalloutKind::Error => (
            pal.bad_con,
            pal.bad,
            Stroke::new(1.0_f32, pal.bad_line),
            icons::circle_x(),
        ),
    };
    Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(icons::image(icon, 16.0, ink));
                ui.add(egui::Label::new(secondary_text(text, ink)).wrap());
            });
        })
        .response
}

/// A 26 px chip.
pub fn chip(ui: &mut Ui, icon: ImageSource<'static>, text: &str) -> Response {
    let pal = p(ui);
    Frame::new()
        .stroke(Stroke::new(1.0_f32, pal.line2))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                ui.add(icons::image(icon, 14.0, pal.tx2));
                ui.label(small_text(text, pal.tx2).family(weight::medium()));
            });
        })
        .response
}

/// Choices on an `over` ground; the selected one on `card`.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let pal = p(ui);
    let mut changed = false;
    Frame::new()
        .fill(pal.over)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (option, label) in options {
                    let selected = *value == *option;
                    let text = small_text(*label, if selected { pal.tx } else { pal.tx2 })
                        .family(weight::medium());
                    let widget = egui::Button::new(text)
                        .fill(if selected {
                            pal.card
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(6))
                        .min_size(vec2(0.0, 26.0));
                    if ui.add(widget).clicked() && !selected {
                        *value = *option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

/// `1,684`.
pub fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "just now", "4 min ago", "1 h ago", "2 d ago".
pub fn ago(seconds: i64) -> String {
    match seconds {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        s => format!("{} d ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_by_thousands() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_684), "1,684");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn times_ago_read_as_the_spec_writes_them() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(240), "4 min ago");
        assert_eq!(ago(3_700), "1 h ago");
        assert_eq!(ago(200_000), "2 d ago");
    }
}
