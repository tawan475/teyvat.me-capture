// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What floats over the views. The window frame, title bar and its buttons
//! are the OS's own (native decorations on every platform), and the views
//! start flush under them; History and Settings live in the status card's
//! corner, and the activity list opens under History.

use egui::{Align, Align2, Context, CornerRadius, Frame, Id, Layout, Margin, Order};

use super::{TeyvatmeApp, View};
use crate::activity::Kind;
use crate::ui::widgets::{self, line, small_text};
use crate::ui::{icons, theme};

/// Below this width the window is one column.
pub(super) const BREAKPOINT: f32 = 720.0;

impl TeyvatmeApp {
    /// The last 50 events, under the History button, while open.
    pub(super) fn activity_popover(&mut self, ctx: &Context) {
        let anchor = match (self.activity_open, self.view, self.activity_anchor) {
            (true, View::Main, Some(anchor)) => anchor,
            _ => {
                self.activity_open = false;
                return;
            }
        };
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
