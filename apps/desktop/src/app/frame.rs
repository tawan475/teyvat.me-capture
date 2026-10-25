// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The window's frame (spec §2): the 40 px title bar, the frameless resize
//! edges, and the 26 px footer with the activity line.

use egui::viewport::ResizeDirection;
use egui::{
    Align, Color32, Context, CornerRadius, CursorIcon, Frame, Id, Layout, Margin, Order,
    PointerButton, RichText, Sense, Stroke, Vec2, ViewportCommand, vec2,
};

use super::{TeyvatmeApp, View};
use crate::activity::Kind;
use crate::ui::icons;
use crate::ui::theme::{self, style, weight};
use crate::ui::widgets::{self, line, small_text};

pub(super) const TITLE_BAR_HEIGHT: f32 = 40.0;
pub(super) const FOOTER_HEIGHT: f32 = 26.0;
/// Below this width the window is one column.
pub(super) const BREAKPOINT: f32 = 720.0;
const RESIZE_EDGE: f32 = 6.0;

impl TeyvatmeApp {
    pub(super) fn title_bar(&mut self, ctx: &Context) {
        let pal = theme::palette(ctx);
        egui::TopBottomPanel::top("title_bar")
            .exact_height(TITLE_BAR_HEIGHT)
            .frame(Frame::new().fill(pal.bar).inner_margin(Margin {
                left: 14,
                right: 4,
                top: 0,
                bottom: 0,
            }))
            .show(ctx, |ui| {
                let narrow = ctx.screen_rect().width() < BREAKPOINT;
                ui.horizontal_centered(|ui| {
                    match self.view {
                        View::Main | View::FirstRun(_) => widgets::brand(ui, !narrow),
                        View::Settings(_) => {
                            if widgets::icon_button(ui, icons::arrow_left(), "Back").clicked() {
                                self.leave_settings();
                            }
                            ui.label(
                                RichText::new("Settings")
                                    .text_style(style::brand())
                                    .color(pal.tx),
                            );
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if window_button(ui, icons::x(), "Close", true).clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        if window_button(ui, icons::minus(), "Minimize", false).clicked() {
                            match self.saved_state.minimize_to_tray {
                                Some(to_tray) => self.minimize(ctx, to_tray),
                                None => self.minimize_modal_open = true,
                            }
                        }
                        if self.view == View::Main
                            && widgets::icon_button(ui, icons::settings(), "Settings").clicked()
                        {
                            self.open_settings(super::Page::App);
                        }

                        // What is left is the drag area.
                        let drag = ui.interact(
                            ui.available_rect_before_wrap(),
                            Id::new("title_drag"),
                            Sense::click_and_drag(),
                        );
                        if drag.drag_started_by(PointerButton::Primary) {
                            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                        }
                        if drag.double_clicked() {
                            let maximized = ctx.input(|i| i.viewport().maximized).unwrap_or(false);
                            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                        }
                    });
                });
            });
    }

    /// The footer: capture state, the newest activity (click: the last 50),
    /// an update pill, the version.
    pub(super) fn footer(&mut self, ctx: &Context, capturing: bool) {
        let pal = theme::palette(ctx);
        let response = egui::TopBottomPanel::bottom("footer")
            .exact_height(FOOTER_HEIGHT)
            .frame(
                Frame::new()
                    .fill(pal.bar)
                    .inner_margin(Margin::symmetric(12, 0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    // Capture state.
                    const STARTING: std::time::Duration = std::time::Duration::from_secs(30);
                    let since = self.started_at.elapsed();
                    let starting = !capturing && since < STARTING;
                    if starting {
                        // One more frame when "starting" turns into "off".
                        ctx.request_repaint_after(STARTING - since);
                    }
                    let (dot, label) = if capturing {
                        (pal.ok, "Capture on")
                    } else if starting {
                        (pal.tx3, "Capture starting")
                    } else {
                        (pal.bad, "Capture off")
                    };
                    let backend = if cfg!(windows) { "pktmon" } else { "pcap" };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 4.0, dot);
                        ui.label(small_text(label, pal.tx2));
                    })
                    .response
                    .on_hover_text(format!(
                        "{backend} · restarts itself within seconds if it stops"
                    ));
                    let (sep, _) = ui.allocate_exact_size(vec2(1.0, 14.0), Sense::hover());
                    ui.painter().rect_filled(sep, 0.0, pal.line);

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let version = ui.label(small_text(env!("CARGO_PKG_VERSION"), pal.tx3));
                        version.on_hover_text(format!(
                            "Game data: dump {}",
                            crate::monitor::embedded_game_data_sha()
                                .map(|sha| &sha[..sha.len().min(8)])
                                .unwrap_or("unknown")
                        ));
                        if cfg!(debug_assertions) {
                            ui.label(small_text("debug", pal.warn_on).family(weight::medium()));
                        }
                        if let Some(version) = self.update_put_off.clone() {
                            let pill = egui::Button::new(
                                small_text(format!("Update {version}"), pal.ac_on)
                                    .family(weight::semibold()),
                            )
                            .fill(pal.ac_con)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(9))
                            .min_size(vec2(0.0, 18.0));
                            if ui.add(pill).clicked() {
                                self.open_settings(super::Page::About);
                            }
                        }
                        // The activity line takes what is left.
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            let (text, color) = match self.activity.latest() {
                                Some(event) => (
                                    event.line(),
                                    if event.kind == Kind::Error {
                                        pal.bad
                                    } else {
                                        pal.tx2
                                    },
                                ),
                                None => ("No activity yet".to_owned(), pal.tx3),
                            };
                            let label = ui
                                .add(
                                    egui::Label::new(small_text(text, color))
                                        .truncate()
                                        .sense(Sense::click()),
                                )
                                .on_hover_cursor(CursorIcon::PointingHand)
                                .on_hover_text("Activity");
                            if label.clicked() {
                                self.activity_open = !self.activity_open;
                            }
                        });
                    });
                });
            });
        if self.activity_open {
            self.activity_popover(ctx, response.response.rect.top());
        }
    }

    fn activity_popover(&mut self, ctx: &Context, footer_top: f32) {
        let pal = theme::palette(ctx);
        let area = egui::Area::new(Id::new("activity_popover"))
            .order(Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(12.0, footer_top - 6.0))
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .corner_radius(CornerRadius::same(theme::radius::MENU))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width((ctx.screen_rect().width() - 48.0).min(460.0));
                        ui.horizontal(|ui| {
                            ui.label(widgets::strong_text("Activity", pal.tx));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::button(
                                    ui,
                                    widgets::ButtonKind::Ghost,
                                    Some(icons::folder_open()),
                                    "Open log folder",
                                    false,
                                )
                                .clicked()
                                {
                                    std::thread::spawn(|| {
                                        let _ = crate::open_log_dir();
                                    });
                                }
                            });
                        });
                        widgets::divider(ui);
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                if self.activity.is_empty() {
                                    ui.label(small_text("Nothing yet", pal.tx3));
                                }
                                for event in self.activity.iter() {
                                    ui.horizontal(|ui| {
                                        let (icon, color) = match event.kind {
                                            Kind::Ok => (icons::check(), pal.ok),
                                            Kind::Info => (icons::info(), pal.tx2),
                                            Kind::Warn => (icons::triangle_alert(), pal.warn_on),
                                            Kind::Error => (icons::circle_x(), pal.bad),
                                        };
                                        ui.label(small_text(
                                            event.at.format("%H:%M").to_string(),
                                            pal.tx3,
                                        ));
                                        ui.add(icons::image(icon, 14.0, color));
                                        line(ui, small_text(&event.text, pal.tx2))
                                            .on_hover_text(&event.text);
                                    });
                                }
                            });
                    });
            });
        // A click anywhere else closes it.
        let clicked_outside = ctx.input(|i| i.pointer.any_click())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|pos| !area.response.rect.contains(pos));
        if clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.activity_open = false;
        }
    }
}

/// A title bar button: 40 × 40, the close one red on hover.
fn window_button(
    ui: &mut egui::Ui,
    icon: egui::ImageSource<'static>,
    name: &str,
    close: bool,
) -> egui::Response {
    let pal = widgets::p(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    let response = response.on_hover_text(name);
    let hovered = response.hovered();
    if hovered {
        let fill = if close { pal.close_hover } else { pal.over };
        ui.painter()
            .rect_filled(rect.shrink(4.0), CornerRadius::same(8), fill);
    }
    let ink = if hovered && close {
        Color32::WHITE
    } else {
        pal.tx2
    };
    let icon_rect = egui::Rect::from_center_size(rect.center(), Vec2::splat(16.0));
    icons::image(icon, 16.0, ink).paint_at(ui, icon_rect);
    response
}

/// The frameless window's resize edges: 6 px zones that hand the resize to
/// the OS.
pub(super) fn resize_edges(ctx: &Context) {
    if ctx.input(|i| i.viewport().maximized).unwrap_or(false) {
        return;
    }
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let rect = ctx.screen_rect();
    let left = pos.x - rect.left() < RESIZE_EDGE;
    let right = rect.right() - pos.x < RESIZE_EDGE;
    let top = pos.y - rect.top() < RESIZE_EDGE;
    let bottom = rect.bottom() - pos.y < RESIZE_EDGE;
    let (direction, cursor) = match (left, right, top, bottom) {
        (true, _, true, _) => (ResizeDirection::NorthWest, CursorIcon::ResizeNorthWest),
        (_, true, true, _) => (ResizeDirection::NorthEast, CursorIcon::ResizeNorthEast),
        (true, _, _, true) => (ResizeDirection::SouthWest, CursorIcon::ResizeSouthWest),
        (_, true, _, true) => (ResizeDirection::SouthEast, CursorIcon::ResizeSouthEast),
        (true, ..) => (ResizeDirection::West, CursorIcon::ResizeWest),
        (_, true, ..) => (ResizeDirection::East, CursorIcon::ResizeEast),
        (_, _, true, _) => (ResizeDirection::North, CursorIcon::ResizeNorth),
        (_, _, _, true) => (ResizeDirection::South, CursorIcon::ResizeSouth),
        _ => return,
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
    }
}
