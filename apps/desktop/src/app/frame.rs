// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What floats over the views. The window frame, title bar and its buttons
//! are the OS's own (native decorations on every platform), and the views
//! start flush under them; History, theme and Settings float in a pill at
//! the bottom left, and the activity list opens above History.

use egui::{Align, Align2, Context, CornerRadius, Frame, Id, Layout, Margin, Order};

use super::{TeyvatmeApp, View};
use crate::activity::Kind;
use crate::i18n::{tr, trf};
use crate::ui::widgets::{self, line, small_text};
use crate::ui::{icons, theme};

/// Below this width the window is one column.
pub(super) const BREAKPOINT: f32 = 720.0;
/// The floating buttons' pill and its margin, which the main screen keeps
/// free under its last card.
pub(super) const QUICK_BAR_HEIGHT: f32 = 56.0;

impl TeyvatmeApp {
    /// History, theme, Settings and Discord: a small pill floating at the bottom left
    /// of the main screen, on its own.
    pub(super) fn quick_bar(&mut self, ctx: &Context) {
        if self.view != View::Main {
            return;
        }
        let pal = theme::palette(ctx);
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("quick_bar"))
            .order(Order::Middle)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(screen.left() + 16.0, screen.bottom() - 12.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(pal.card)
                    .stroke(egui::Stroke::new(1.0_f32, pal.line))
                    .corner_radius(CornerRadius::same(20))
                    .inner_margin(Margin::same(4))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 2],
                        blur: 10,
                        spread: 0,
                        color: pal.card_shadow,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let hint = match self.activity.latest() {
                                Some(event) => trf("Activity · {0}", &[&event.line()]),
                                None => tr("Activity").to_owned(),
                            };
                            let history = widgets::icon_button(ui, icons::history(), &hint);
                            if self
                                .activity
                                .latest()
                                .is_some_and(|event| event.kind == Kind::Error)
                            {
                                // A dot for a failure not looked at.
                                let dot = history.rect.right_top() + egui::vec2(-8.0, 8.0);
                                ui.painter().circle_filled(dot, 3.5, pal.bad);
                            }
                            if history.clicked() {
                                self.activity_open = !self.activity_open;
                            }
                            self.activity_anchor = Some(history.rect);
                            // Light ↔ dark in one click; Settings → App has System.
                            let dark = ui.visuals().dark_mode;
                            let (icon, name) = if dark {
                                (icons::sun(), tr("Light theme"))
                            } else {
                                (icons::moon(), tr("Dark theme"))
                            };
                            if widgets::icon_button(ui, icon, name).clicked() {
                                self.saved_state.theme = if dark {
                                    super::ThemeChoice::Light
                                } else {
                                    super::ThemeChoice::Dark
                                };
                            }
                            if widgets::icon_button(ui, icons::settings(), tr("Settings")).clicked()
                            {
                                self.open_settings(super::Page::App);
                            }
                            if widgets::icon_button(ui, icons::discord(), "Discord").clicked() {
                                ui.ctx()
                                    .open_url(egui::OpenUrl::new_tab(crate::DISCORD_URL));
                            }
                        });
                    });
            });
    }

    /// The last 50 events, above the History button, while open.
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
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(egui::pos2(anchor.left() - 4.0, anchor.top() - 10.0))
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .corner_radius(CornerRadius::same(theme::radius::MENU))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width((ctx.screen_rect().width() - 48.0).min(460.0));
                        ui.horizontal(|ui| {
                            ui.label(widgets::strong_text(tr("Activity"), pal.tx));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if widgets::button(
                                    ui,
                                    widgets::ButtonKind::Ghost,
                                    Some(icons::folder_open()),
                                    tr("Open log folder"),
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
                        // Full width: shrunk to its content, the list cut its
                        // lines at the first frame's width.
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .max_height(320.0)
                            .show(ui, |ui| {
                                if self.activity.is_empty() {
                                    ui.label(small_text(tr("Nothing yet"), pal.tx3));
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
