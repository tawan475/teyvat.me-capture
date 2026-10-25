// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What sits around the views. The window frame, title bar and its buttons
//! are the OS's own (native decorations on every platform); inside it is
//! only a slim toolbar: Back and the page title in Settings, Activity, a
//! pending update and Settings on the main screen.

use egui::{
    Align, Align2, Context, CornerRadius, Frame, Id, Layout, Margin, Order, RichText, Stroke, vec2,
};

use super::{TeyvatmeApp, View};
use crate::activity::Kind;
use crate::ui::icons;
use crate::ui::theme::{self, style, weight};
use crate::ui::widgets::{self, line, small_text};

/// Below this width the window is one column.
pub(super) const BREAKPOINT: f32 = 720.0;
const TOOLBAR_HEIGHT: f32 = 44.0;

impl TeyvatmeApp {
    /// The toolbar over the main screen and Settings; the first run has none.
    pub(super) fn toolbar(&mut self, ctx: &Context) {
        if matches!(self.view, View::FirstRun(_)) {
            return;
        }
        let pal = theme::palette(ctx);
        let mut activity_button = None;
        egui::TopBottomPanel::top("toolbar")
            .exact_height(TOOLBAR_HEIGHT)
            .show_separator_line(false)
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin {
                left: 10,
                right: 10,
                top: 6,
                bottom: 0,
            }))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let View::Settings(_) = self.view {
                        if widgets::icon_button(ui, icons::arrow_left(), "Back").clicked() {
                            self.leave_settings();
                        }
                        ui.label(
                            RichText::new("Settings")
                                .text_style(style::brand())
                                .color(pal.tx),
                        );
                        return;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if widgets::icon_button(ui, icons::settings(), "Settings").clicked() {
                            self.open_settings(super::Page::App);
                        }
                        let latest_failed = self
                            .activity
                            .latest()
                            .is_some_and(|event| event.kind == Kind::Error);
                        let hint = match self.activity.latest() {
                            Some(event) => format!("Activity · {}", event.line()),
                            None => "Activity".to_owned(),
                        };
                        let button = widgets::icon_button(ui, icons::history(), &hint);
                        if latest_failed {
                            // A dot on the button for a failure not looked at.
                            let dot = button.rect.right_top() + vec2(-9.0, 9.0);
                            ui.painter().circle_filled(dot, 3.5, pal.bad);
                        }
                        if button.clicked() {
                            self.activity_open = !self.activity_open;
                        }
                        activity_button = Some(button.rect);
                        if let Some(version) = self.update_put_off.clone() {
                            let pill = egui::Button::new(
                                small_text(format!("Update {version}"), pal.ac_on)
                                    .family(weight::semibold()),
                            )
                            .fill(pal.ac_con)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(13))
                            .min_size(vec2(0.0, 26.0));
                            if ui
                                .add(pill)
                                .on_hover_text("Installs and restarts Teyvat.me")
                                .clicked()
                            {
                                self.open_settings(super::Page::About);
                            }
                        }
                    });
                });
            });
        if self.view != View::Main {
            self.activity_open = false;
        }
        if let (true, Some(anchor)) = (self.activity_open, activity_button) {
            self.activity_popover(ctx, anchor);
        }
    }

    /// The last 50 events, under the Activity button.
    fn activity_popover(&mut self, ctx: &Context, anchor: egui::Rect) {
        let pal = theme::palette(ctx);
        let area = egui::Area::new(Id::new("activity_popover"))
            .order(Order::Foreground)
            .pivot(Align2::RIGHT_TOP)
            .fixed_pos(egui::pos2(anchor.right(), anchor.bottom() + 6.0))
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
        // A click anywhere else (the button itself toggles) closes it.
        let clicked_outside = ctx.input(|i| i.pointer.any_click())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos));
        if clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.activity_open = false;
        }
    }
}
