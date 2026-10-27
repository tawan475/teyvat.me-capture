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
use crate::i18n::{tr, trf};

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
        // The actions first (right to left), so the title and its suffix
        // are cut at what they leave, never drawn under them.
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 32.0),
            Layout::right_to_left(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                actions(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    line(ui, strong_text(title, pal.tx));
                    if let Some(suffix) = suffix {
                        line(ui, small_text(suffix, pal.tx3));
                    }
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

/// `target`, eased in over a quarter second from whatever `id` showed last.
/// Asks for frames only while it moves. A theme switch lands at once, like
/// every other colour on screen: each theme eases on its own.
pub fn animated_color(ui: &Ui, id: egui::Id, target: Color32) -> Color32 {
    let ctx = ui.ctx();
    let id = id.with(ui.visuals().dark_mode);
    let [r, g, b, a] = target.to_array();
    let channel = |name: &str, value: u8| {
        ctx.animate_value_with_time(id.with(name), f32::from(value), 0.25)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_premultiplied(
        channel("r", r),
        channel("g", g),
        channel("b", b),
        channel("a", a),
    )
}

/// The capture status strip across the window (layout G): disc, title over
/// its line, the three steps, then `actions` at the right end (added right
/// to left: the first is the rightmost). `extra` (the bad states'
/// instruction and buttons) goes on a second line under the title. Narrow,
/// the steps wrap under the title too. `tooltip` is on the disc and text.
pub struct Strip<'a> {
    pub variant: HeroVariant,
    pub disc: Disc,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub steps: [Step; 3],
    pub tooltip: &'a str,
}

pub fn status_strip(
    ui: &mut Ui,
    strip: Strip<'_>,
    narrow: bool,
    actions: impl FnOnce(&mut Ui),
    extra: Option<&mut dyn FnMut(&mut Ui)>,
) -> Response {
    const DISC: f32 = 36.0;
    const GAP: f32 = 14.0;
    const STEPS: f32 = 300.0;
    let pal = p(ui);
    let Strip {
        variant,
        disc,
        title,
        subtitle,
        steps,
        tooltip,
    } = strip;
    let (fill, line_color) = match variant {
        HeroVariant::Neutral => (pal.card, pal.line),
        HeroVariant::Ok => (pal.ok_con, pal.ok_line),
        HeroVariant::Bad => (pal.bad_con, pal.bad_line),
    };
    // A new status fades in rather than jumping.
    let id = egui::Id::new("status_hero");
    let fill = animated_color(ui, id.with("fill"), fill);
    let line_color = animated_color(ui, id.with("line"), line_color);
    let mut frame = Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, line_color))
        .corner_radius(CornerRadius::same(radius::HERO))
        .inner_margin(Margin {
            left: 14,
            right: 10,
            top: 8,
            bottom: 8,
        });
    if variant == HeroVariant::Neutral {
        frame = frame.shadow(Shadow {
            offset: [0, 1],
            blur: 2,
            spread: 0,
            color: pal.card_shadow,
        });
    }
    frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            let width = ui.available_width();
            let row = ui.allocate_ui_with_layout(
                vec2(width, 36.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    let (rect, disc_response) =
                        ui.allocate_exact_size(Vec2::splat(DISC), Sense::hover());
                    let r = DISC / 2.0;
                    match disc {
                        Disc::Icon { icon, fill, ink } => {
                            let disc = animated_color(ui, id.with("disc"), fill(&pal));
                            ui.painter().circle_filled(rect.center(), r, disc);
                            let icon_rect =
                                egui::Rect::from_center_size(rect.center(), Vec2::splat(20.0));
                            icons::image(icon(), 20.0, ink(&pal)).paint_at(ui, icon_rect);
                        }
                        Disc::Spinner => {
                            ui.painter().circle_filled(rect.center(), r, pal.over);
                            let spin =
                                egui::Rect::from_center_size(rect.center(), Vec2::splat(20.0));
                            ui.put(spin, egui::Spinner::new().size(20.0).color(pal.ac_tx));
                        }
                    }
                    disc_response.on_hover_text(tooltip);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        actions(ui);
                        ui.add_space(GAP - 2.0);
                        ui.spacing_mut().item_spacing.x = GAP;
                        if !narrow {
                            ui.allocate_ui_with_layout(
                                vec2(STEPS, 26.0),
                                Layout::left_to_right(Align::Center),
                                |ui| stepper(ui, steps),
                            );
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                line(
                                    ui,
                                    RichText::new(title)
                                        .text_style(style::dialog())
                                        .color(pal.tx),
                                );
                                line(ui, secondary_text(subtitle, pal.tx2));
                            })
                            .response
                            .on_hover_text(tooltip);
                        });
                    });
                },
            );
            let indent = DISC + GAP;
            if narrow {
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    ui.allocate_ui_with_layout(
                        vec2((ui.available_width() - 4.0).max(0.0), 26.0),
                        Layout::left_to_right(Align::Center),
                        |ui| stepper(ui, steps),
                    );
                });
            }
            if let Some(extra) = extra {
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        extra(ui);
                    });
                });
            }
            row.response
        })
        .inner
}

/// Capture · Open Genshin · Enter world, each a 4 px bar over a label.
pub fn stepper(ui: &mut Ui, steps: [Step; 3]) {
    let pal = p(ui);
    let gap = 6.0;
    let width = (ui.available_width() - 2.0 * gap) / 3.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (step, (key, label)) in steps.into_iter().zip([
            ("Capture", tr("Capture")),
            ("Open Genshin", tr("Open Genshin")),
            ("Enter world", tr("Enter world")),
        ]) {
            let id = egui::Id::new("stepper").with(key);
            let color = match step {
                Step::Pending => pal.line2,
                Step::Now => pal.ac_tx,
                Step::Done => pal.ok,
                Step::Failed => pal.bad,
            };
            let color = animated_color(ui, id.with("color"), color);
            // A step reached fills its bar from the left.
            let reached = ui.ctx().animate_value_with_time(
                id.with("reached"),
                if step == Step::Pending { 0.0 } else { 1.0 },
                0.35,
            );
            ui.vertical(|ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 4.0;
                let (bar, _) = ui.allocate_exact_size(vec2(width, 4.0), Sense::hover());
                ui.painter().rect_filled(bar, 2.0, pal.line2);
                let mut filled = bar;
                filled.set_width(width * reached);
                ui.painter().rect_filled(filled, 2.0, color);
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

/// A stat tile `size` big: value over label, centred on its height, `—`
/// when there is none. `flash` (0–1) tints it `okCon` after an update.
pub fn stat_tile(ui: &mut Ui, size: Vec2, value: Option<usize>, label: &str, flash: f32) {
    let pal = p(ui);
    let fill = if flash > 0.0 {
        lerp_color(pal.over, pal.ok_con, flash)
    } else {
        pal.over
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    let inner = (size.x - 20.0).max(0.0);
    let shown = value.map_or_else(|| "—".to_owned(), group_thousands);
    let value_text = match value {
        Some(_) => RichText::new(&shown)
            .text_style(style::tile())
            .color(pal.tx),
        None => RichText::new(&shown)
            .text_style(style::tile())
            .family(weight::medium())
            .color(pal.tx2),
    };
    let value_galley = one_line(ui, value_text, inner);
    let label_galley = one_line(ui, small_text(label, pal.tx2), inner);
    let top = rect.center().y - (value_galley.size().y + label_galley.size().y) / 2.0;
    let left = rect.left() + 10.0;
    let value_height = value_galley.size().y;
    let painter = ui.painter();
    painter.galley(egui::pos2(left, top), value_galley, pal.tx);
    painter.galley(egui::pos2(left, top + value_height), label_galley, pal.tx2);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label}: {shown}"))
    });
}

/// The height a stat tile needs: padding 7 a side, value and label.
pub const STAT_TILE_HEIGHT: f32 = 7.0 + 24.0 + 16.0 + 7.0;

/// `text` laid out on one line, cut with an ellipsis at `width`.
pub fn one_line(ui: &Ui, text: RichText, width: f32) -> std::sync::Arc<egui::Galley> {
    WidgetText::from(text).into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        width,
        egui::TextStyle::Body,
    )
}

/// One of the Today card's figures (spec §4.2b): icon, label, the value at
/// the right, then an optional bar and an optional small line.
pub struct MiniStat<'a> {
    pub icon: ImageSource<'static>,
    pub label: &'a str,
    /// The value, and the total its "/ total" part names.
    pub value: Option<(String, Option<String>)>,
    /// A 0–1 bar under it.
    pub bar: Option<f32>,
    /// A small line under it, and whether it says "done" (`ok`).
    pub line: Option<(String, bool)>,
    pub tooltip: &'a str,
}

impl MiniStat<'_> {
    const ROW: f32 = 18.0;
    const BAR: f32 = 6.0;
    const LINE: f32 = 16.0;
    const GAP: f32 = 4.0;
    const INDENT: f32 = 24.0;

    /// Its content's height, without padding.
    pub fn height(&self) -> f32 {
        let mut h = Self::ROW;
        if self.bar.is_some() {
            h += Self::GAP + Self::BAR;
        }
        if self.line.is_some() {
            h += Self::GAP + Self::LINE;
        }
        h
    }

    /// Drawn in `rect`: a block `block` tall (the row's tallest figure)
    /// centred on the rect's height, this figure at its top, so the figures
    /// of a row line up.
    pub fn show(&self, ui: &mut Ui, rect: egui::Rect, block: f32) {
        let pal = p(ui);
        let response = ui.interact(rect, ui.id().with(self.label), Sense::hover());
        let spoken = match &self.value {
            Some((value, Some(total))) => format!("{}: {value} / {total}", self.label),
            Some((value, None)) => format!("{}: {value}", self.label),
            None => format!("{}: —", self.label),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
        if !self.tooltip.is_empty() {
            response.on_hover_text(self.tooltip);
        }
        let painter = ui.painter_at(rect);
        let mut y = rect.center().y - block.max(self.height()) / 2.0;
        let row_mid = y + Self::ROW / 2.0;
        let icon_rect =
            egui::Rect::from_center_size(egui::pos2(rect.left() + 8.0, row_mid), Vec2::splat(16.0));
        icons::image(self.icon.clone(), 16.0, pal.tx3).paint_at(ui, icon_rect);

        let value = match &self.value {
            Some((value, total)) => {
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    value,
                    0.0,
                    egui::TextFormat::simple(egui::FontId::new(14.0, weight::semibold()), pal.tx),
                );
                if let Some(total) = total {
                    job.append(
                        &format!(" / {total}"),
                        0.0,
                        egui::TextFormat::simple(
                            egui::FontId::new(12.0, egui::FontFamily::Proportional),
                            pal.tx3,
                        ),
                    );
                }
                ui.fonts(|fonts| fonts.layout_job(job))
            }
            None => one_line(
                ui,
                RichText::new("—")
                    .size(14.0)
                    .family(weight::medium())
                    .color(pal.tx2),
                rect.width(),
            ),
        };
        let value_left = rect.right() - value.size().x;
        painter.galley(
            egui::pos2(value_left, row_mid - value.size().y / 2.0),
            value,
            pal.tx,
        );
        let label_left = rect.left() + Self::INDENT;
        let label = one_line(
            ui,
            secondary_text(self.label, pal.tx2),
            (value_left - 8.0 - label_left).max(0.0),
        );
        painter.galley(
            egui::pos2(label_left, row_mid - label.size().y / 2.0),
            label,
            pal.tx2,
        );
        y += Self::ROW;

        if let Some(fraction) = self.bar {
            y += Self::GAP;
            let track = egui::Rect::from_min_max(
                egui::pos2(label_left, y),
                egui::pos2(rect.right(), y + Self::BAR),
            );
            painter.rect_filled(track, 3.0, pal.over);
            let mut filled = track;
            filled.set_width(track.width() * fraction.clamp(0.0, 1.0));
            if filled.width() > 0.5 {
                painter.rect_filled(filled, 3.0, pal.ac_tx);
            }
            y += Self::BAR;
        }
        if let Some((text, ok)) = &self.line {
            y += Self::GAP;
            let color = if *ok { pal.ok } else { pal.tx3 };
            let galley = one_line(
                ui,
                small_text(text, color),
                (rect.right() - label_left).max(0.0),
            );
            painter.galley(egui::pos2(label_left, y), galley, color);
        }
    }
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
    // Exactly 32 × 32: the 18 px icon plus 7 px a side, whatever the
    // style's button padding, so rows of them can be measured.
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::splat(7.0);
        ui.add(widget)
    })
    .inner
    .on_hover_text(name)
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
        s if s < 60 => tr("just now").to_string(),
        s if s < 3600 => trf("{0} min ago", &[&(s / 60)]),
        s if s < 86_400 => trf("{0} h ago", &[&(s / 3600)]),
        s => trf("{0} d ago", &[&(s / 86_400)]),
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
