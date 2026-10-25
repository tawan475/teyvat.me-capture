// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The main window (spec §4): status hero, Captured, Wish history on the
//! left; Export (the main action) and Destinations (optional) on the right.
//! One column below the breakpoint. The app exports; it never displays the
//! account (that is the tracker's job).

use std::time::{Duration, Instant};

use chrono::Local;
use egui::{Align, Layout, RichText, Ui};

use super::frame::BREAKPOINT;
use super::{OptimizerExportTarget, Page, TeyvatmeApp, TrackerSignIn};
use crate::AppState;
use crate::game_watch::{GameStatus, MissedLaunch};
use crate::signin::{AuthStatus, TrackerRequest};
use crate::ui::icons;
use crate::ui::theme::Palette;
use crate::ui::widgets::{
    self, ButtonKind, Disc, HeroVariant, Step, button, card, divider, icon_button, line, list_row,
    secondary_text, small_text, strong_text, switch_row, title_text,
};

/// How long a tile glows after its class updates.
const TILE_FLASH: Duration = Duration::from_millis(600);

impl TeyvatmeApp {
    pub(super) fn home(&mut self, ui: &mut Ui, app_state: &AppState) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 12.0;
                        self.crash_banner(ui);
                        self.update_banner(ui);
                        let wide = ui.available_width() >= BREAKPOINT - 32.0;
                        if wide {
                            ui.columns(2, |columns| {
                                let [left, right] = columns else {
                                    unreachable!("two columns")
                                };
                                left.spacing_mut().item_spacing.y = 12.0;
                                right.spacing_mut().item_spacing.y = 12.0;
                                self.status_hero(left, app_state);
                                self.captured_card(left, app_state);
                                self.wish_card(left, app_state);
                                self.export_card(right, app_state);
                                self.destinations_card(right);
                            });
                        } else {
                            self.status_hero(ui, app_state);
                            self.captured_card(ui, app_state);
                            self.wish_card(ui, app_state);
                            self.export_card(ui, app_state);
                            self.destinations_card(ui);
                        }
                    });
            });
    }

    /// The update put off at start, until installed (from About).
    fn update_banner(&mut self, ui: &mut Ui) {
        let Some(version) = self.update_put_off.clone() else {
            return;
        };
        let pal = widgets::p(ui);
        let mut open = false;
        egui::Frame::new()
            .fill(pal.ac_con)
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(icons::image(icons::download(), 18.0, pal.ac_on));
                    line(ui, strong_text(format!("Update {version}"), pal.ac_on));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        open = button(ui, ButtonKind::Secondary, None, "Details", false)
                            .on_hover_text("Installs and restarts Teyvat.me")
                            .clicked();
                    });
                });
            });
        if open {
            self.open_settings(Page::About);
        }
    }

    /// "Crashed last time" / "Stopped unexpectedly", until dismissed.
    fn crash_banner(&mut self, ui: &mut Ui) {
        let Some(notice) = self.previous_run_notice.clone() else {
            return;
        };
        let pal = widgets::p(ui);
        let crashed = notice.to_lowercase().contains("crash");
        let (title, detail) = if crashed {
            ("Crashed last time", "Report and log kept")
        } else {
            ("Stopped unexpectedly", "No crash report · log kept")
        };
        let mut open_logs = false;
        let mut dismiss = false;
        egui::Frame::new()
            .fill(pal.warn_con)
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(icons::image(icons::triangle_alert(), 18.0, pal.warn_on));
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        line(ui, strong_text(title, pal.warn_on));
                        line(ui, secondary_text(detail, pal.warn_on));
                    })
                    .response
                    .on_hover_text(&notice);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        dismiss = icon_button(ui, icons::x(), "Dismiss").clicked();
                        open_logs =
                            button(ui, ButtonKind::Secondary, None, "Open log folder", false)
                                .clicked();
                    });
                });
            });
        if open_logs {
            std::thread::spawn(|| {
                let _ = crate::open_log_dir();
            });
        }
        if open_logs || dismiss {
            self.previous_run_notice = None;
        }
    }

    fn status_hero(&mut self, ui: &mut Ui, app_state: &AppState) {
        use Step::{Done, Failed, Now, Pending};
        let status = app_state.game_status;
        let ok_disc = Disc::Icon {
            icon: icons::circle_check,
            fill: |p: &Palette| p.ok,
            ink: |p: &Palette| p.ok_ink,
        };
        let bad = |icon: fn() -> egui::ImageSource<'static>| Disc::Icon {
            icon,
            fill: |p: &Palette| p.bad,
            ink: |p: &Palette| p.bad_ink,
        };
        let neutral = |icon: fn() -> egui::ImageSource<'static>| Disc::Icon {
            icon,
            fill: |p: &Palette| p.over,
            ink: |p: &Palette| p.tx2,
        };

        if !self.is_admin {
            let response = widgets::status_hero(
                ui,
                HeroVariant::Bad,
                bad(icons::shield_alert),
                "Capture needs administrator",
                "Started without admin rights",
                [Failed, Pending, Pending],
                |ui| {
                    if button(
                        ui,
                        ButtonKind::Fill,
                        Some(icons::shield_check()),
                        "Restart as administrator",
                        false,
                    )
                    .clicked()
                    {
                        self.restart_as_admin(ui.ctx());
                    }
                },
            );
            self.hero_actions(
                ui,
                response,
                "Windows lets only administrators read network packets",
            );
            return;
        }

        let decoding_since = self
            .decoding_since
            .map(|at| at.format("%H:%M").to_string())
            .unwrap_or_default();
        let since_start = self.started_at.elapsed();
        let response = match status {
            GameStatus::NotRunning => widgets::status_hero(
                ui,
                HeroVariant::Neutral,
                neutral(icons::gamepad),
                "Waiting for the game",
                "Genshin isn't running · start it now",
                [Done, Pending, Pending],
                |ui| {
                    widgets::chip(ui, icons::info(), "Open before the game")
                        .on_hover_text("The login is the only moment the session key can be read");
                },
            ),
            GameStatus::AwaitingLogin => widgets::status_hero(
                ui,
                HeroVariant::Neutral,
                neutral(icons::gamepad),
                "Game detected",
                "Already open · enter the world now",
                [Done, Now, Pending],
                |_| {},
            ),
            GameStatus::LaunchCaptured => widgets::status_hero(
                ui,
                HeroVariant::Neutral,
                Disc::Icon {
                    icon: icons::gamepad,
                    fill: |p: &Palette| p.fill,
                    ink: |_: &Palette| egui::Color32::WHITE,
                },
                "Game detected",
                "Launch captured · enter the world now",
                [Done, Now, Pending],
                |_| {},
            ),
            GameStatus::Decoding => widgets::status_hero(
                ui,
                HeroVariant::Ok,
                ok_disc,
                "Data decrypting",
                &format!("Session key found · {decoding_since}"),
                [Done, Done, Done],
                |_| {},
            ),
            GameStatus::CaptureOff => widgets::status_hero(
                ui,
                HeroVariant::Neutral,
                Disc::Spinner,
                "Starting capture",
                "Packet capture restarts itself within seconds",
                [Now, Pending, Pending],
                |ui| {
                    if since_start > Duration::from_secs(30) {
                        ui.horizontal(|ui| {
                            if button(
                                ui,
                                ButtonKind::Secondary,
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
                    }
                },
            ),
            GameStatus::LaunchMissed(cause) => {
                let (line_text, steps) = match cause {
                    MissedLaunch::AlreadyRunning => {
                        ("Genshin started before Teyvat.me", [Done, Failed, Pending])
                    }
                    MissedLaunch::CaptureStopped => (
                        "Capture was off when you logged in",
                        [Failed, Pending, Pending],
                    ),
                };
                widgets::status_hero(
                    ui,
                    HeroVariant::Bad,
                    bad(icons::triangle_alert),
                    "Launch missed",
                    line_text,
                    steps,
                    |ui| {
                        let pal = widgets::p(ui);
                        ui.label(strong_text(
                            "Back to the title screen and enter again",
                            pal.tx,
                        ));
                        ui.horizontal(|ui| {
                            let kill = ui.add_enabled_ui(self.game_kill_rx.is_none(), |ui| {
                                button(
                                    ui,
                                    ButtonKind::Danger,
                                    Some(icons::power()),
                                    "Close Genshin",
                                    false,
                                )
                                .on_hover_text(
                                    "Force-quits the game. Progress is safe; a domain run in \
                                         progress is lost.",
                                )
                            });
                            if kill.inner.clicked() {
                                self.kill_game();
                            }
                            ui.label(small_text("or restart Genshin", pal.tx3));
                        });
                    },
                )
            }
            GameStatus::KeyLost => widgets::status_hero(
                ui,
                HeroVariant::Bad,
                bad(icons::triangle_alert),
                "Login not read",
                "Session key not found",
                [Done, Done, Failed],
                |ui| {
                    let pal = widgets::p(ui);
                    ui.label(strong_text(
                        "Back to the title screen and enter again",
                        pal.tx,
                    ));
                    ui.label(small_text("Captured data is kept", pal.tx3));
                },
            ),
        };
        self.hero_actions(ui, response, status.tooltip());
    }

    /// History and Settings in the status card's top-right corner, and the
    /// card's tooltip everywhere else on it.
    fn hero_actions(&mut self, ui: &mut Ui, hero: egui::Response, tooltip: &str) {
        let pal = widgets::p(ui);
        let pad = widgets::HERO_PADDING;
        let corner = egui::Rect::from_min_size(
            egui::pos2(
                hero.rect.right() - pad - widgets::HERO_ACTIONS_WIDTH,
                hero.rect.top() + pad,
            ),
            egui::vec2(widgets::HERO_ACTIONS_WIDTH, 44.0),
        );
        let mut corner_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(corner)
                .layout(Layout::right_to_left(Align::Center)),
        );
        corner_ui.spacing_mut().item_spacing.x = 4.0;
        if icon_button(&mut corner_ui, icons::settings(), "Settings").clicked() {
            self.open_settings(Page::App);
        }
        let hint = match self.activity.latest() {
            Some(event) => format!("Activity · {}", event.line()),
            None => "Activity".to_owned(),
        };
        let history = icon_button(&mut corner_ui, icons::history(), &hint);
        if self
            .activity
            .latest()
            .is_some_and(|event| event.kind == crate::activity::Kind::Error)
        {
            // A dot on the button for a failure not looked at.
            let dot = history.rect.right_top() + egui::vec2(-8.0, 8.0);
            corner_ui.painter().circle_filled(dot, 3.5, pal.bad);
        }
        if history.clicked() {
            self.activity_open = !self.activity_open;
        }
        self.activity_anchor = Some(history.rect);
        if !ui.rect_contains_pointer(corner) {
            hero.on_hover_text(tooltip);
        }
    }

    fn captured_card(&mut self, ui: &mut Ui, app_state: &AppState) {
        let pal = widgets::p(ui);
        let (title, suffix) = match &app_state.player {
            Some(player) => {
                let mut parts = Vec::new();
                if let Some(ar) = player.ar {
                    parts.push(format!("AR {ar}"));
                }
                if let Some(server) = player.uid.and_then(server_of) {
                    parts.push(server.to_owned());
                }
                ("Captured", Some(parts.join(" · ")))
            }
            None if app_state.counts.is_none() => ("Captured", Some("Nothing yet".to_owned())),
            None => ("Captured", None),
        };
        let uid = app_state.player.as_ref().and_then(|player| player.uid);
        let time = app_state.captured_at.map(|at| {
            let ago = widgets::ago((Local::now() - at).num_seconds());
            (
                format!("{} · {ago}", at.format("%H:%M")),
                at.format("%Y-%m-%d %H:%M:%S").to_string(),
            )
        });
        let counts = app_state.counts;
        let updated = &app_state.updated;
        let flash = |at: Option<Instant>| {
            at.map_or(0.0, |at| {
                let elapsed = at.elapsed();
                if elapsed < TILE_FLASH {
                    1.0 - elapsed.as_secs_f32() / TILE_FLASH.as_secs_f32()
                } else {
                    0.0
                }
            })
        };
        let flashes = [
            flash(updated.characters_updated),
            flash(updated.items_updated),
            flash(updated.items_updated),
            flash(updated.items_updated),
            flash(updated.achievements_updated),
        ];
        if flashes.iter().any(|f| *f > 0.0) {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }

        let card = card(
            ui,
            title,
            suffix.as_deref().filter(|s| !s.is_empty()),
            |ui| {
                if let Some((time, full)) = &time {
                    ui.label(small_text(time, pal.tx3)).on_hover_text(full);
                }
            },
            |ui| {
                let gap = 8.0;
                let width = ui.available_width();
                let unit = (width - 5.0 * gap) / 6.0;
                let value =
                    |pick: fn(&crate::player_data::ExportCounts) -> usize| counts.map(|c| pick(&c));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    widgets::stat_tile(
                        ui,
                        unit * 2.0 + gap,
                        value(|c| c.characters),
                        "Characters",
                        flashes[0],
                    );
                    widgets::stat_tile(
                        ui,
                        unit * 2.0 + gap,
                        value(|c| c.weapons),
                        "Weapons",
                        flashes[1],
                    );
                    widgets::stat_tile(
                        ui,
                        unit * 2.0 + gap,
                        value(|c| c.artifacts),
                        "Artifacts",
                        flashes[2],
                    );
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    let half = (width - gap) / 2.0;
                    widgets::stat_tile(ui, half, value(|c| c.materials), "Materials", flashes[3]);
                    widgets::stat_tile(
                        ui,
                        half,
                        value(|c| c.achievements),
                        "Achievements",
                        flashes[4],
                    );
                });
            },
        );
        // The UID only on hover, so streams and screenshots don't show it.
        if let Some(uid) = uid {
            card.response.on_hover_text(format!("UID {uid}"));
        }
    }

    fn wish_card(&mut self, ui: &mut Ui, app_state: &AppState) {
        let pal = widgets::p(ui);
        let now = Instant::now();
        let url = self.wishes.wish_url(now);
        let uid_known = app_state
            .player
            .as_ref()
            .is_some_and(|player| player.uid.is_some());
        let status = self.wishes.status(now, app_state.game_status, uid_known);

        let Some(url) = url else {
            // Greyed: the key comes from the game's wish history page.
            let secondary = if self.wishes.key_expired(now) {
                "Key expired · open the wish history in game again"
            } else {
                "Open the wish history in game first"
            };
            egui::Frame::new()
                .fill(pal.over)
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(16, 4))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    list_row(
                        ui,
                        Some((icons::sparkles(), pal.tx3)),
                        title_text("Wish history", pal.tx2),
                        Some(secondary_text(secondary, pal.tx2)),
                        |_| {},
                    );
                })
                .response
                .on_hover_text(
                    "The wish URL needs the key the game sends when its wish history page opens, \
                     once per login.",
                );
            return;
        };

        let linked = crate::app::can_upload_to_tracker(&self.saved_state);
        let host = self.tracker_host();
        widgets::card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut copy = false;
            let mut open = None;
            list_row(
                ui,
                Some((icons::sparkles(), pal.ac_tx)),
                title_text("Wish history", pal.tx),
                Some(secondary_text("URL ready for paimon.moe, StarDB…", pal.tx2)),
                |ui| {
                    let more = icon_button(ui, icons::ellipsis(), "More");
                    egui::Popup::menu(&more).show(|ui| {
                        ui.set_min_width(180.0);
                        if ui.button("Open paimon.moe").clicked() {
                            open = Some("https://paimon.moe/wish/import");
                        }
                        if ui.button("Open StarDB").clicked() {
                            open = Some("https://stardb.gg/en/genshin/wish-import");
                        }
                    });
                    copy = button(
                        ui,
                        ButtonKind::Secondary,
                        Some(icons::copy()),
                        "Copy URL",
                        false,
                    )
                    .clicked();
                },
            )
            .0
            .on_hover_text(&status.tooltip);
            if copy || open.is_some() {
                ui.ctx().copy_text(url.clone());
                self.toasts.info("Wish URL copied");
            }
            if let Some(page) = open {
                ui.ctx().open_url(egui::OpenUrl::new_tab(page));
            }
            if linked {
                divider(ui);
                let sync = self.wishes.sync_line(now);
                let secondary = if self.saved_state.send_wishes {
                    sync.unwrap_or_else(|| "Sends new wishes as they appear".to_owned())
                } else {
                    "Off · the URL still works".to_owned()
                };
                let is_problem = secondary.contains("failed") || secondary.contains("expired");
                let color = if is_problem { pal.bad } else { pal.tx2 };
                switch_row(
                    ui,
                    &format!("Send wishes to {host}"),
                    Some(secondary_text(secondary, color)),
                    &mut self.saved_state.send_wishes,
                    true,
                );
            }
        });
    }

    fn export_card(&mut self, ui: &mut Ui, app_state: &AppState) {
        let pal = widgets::p(ui);
        let missing =
            crate::app::missing_export_data(&self.saved_state.export_settings, &app_state.updated);
        let nothing = app_state.counts.is_none();
        let busy = self.optimizer_export_rx.is_some();
        let folder = self.export_folder();
        let mut open_folder = false;
        let mut more_action = None;
        card(
            ui,
            "Export",
            None,
            |ui| {
                let more = icon_button(ui, icons::ellipsis(), "More exports");
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(220.0);
                    if ui.button("Copy achievements").clicked() {
                        more_action = Some(MoreExport::Achievements(None));
                    }
                    if ui.button("Achievements to StarDB").clicked() {
                        more_action =
                            Some(MoreExport::Achievements(Some("https://stardb.gg/import")));
                    }
                    if ui.button("Achievements to Seelie.me").clicked() {
                        more_action = Some(MoreExport::Achievements(Some(
                            "https://seelie.me/achievements",
                        )));
                    }
                    ui.separator();
                    if ui.button("Filters and auto-export").clicked() {
                        more_action = Some(MoreExport::Settings);
                    }
                });
                let open = ui.add_enabled_ui(folder.is_some(), |ui| {
                    icon_button(ui, icons::folder_open(), "Open export folder")
                        .on_disabled_hover_text("No export folder yet")
                });
                open_folder = open.inner.clicked();
            },
            |ui| {
                ui.horizontal(|ui| {
                    let enabled = !nothing && !busy;
                    let reason = if nothing {
                        "Nothing captured yet"
                    } else {
                        "An export is running"
                    };
                    let save = ui.add_enabled_ui(enabled, |ui| {
                        button(
                            ui,
                            ButtonKind::Fill,
                            Some(icons::download()),
                            "Save GOOD JSON…",
                            false,
                        )
                        .on_hover_text(
                            "GOOD JSON, read by Genshin Optimizer, teyvat.me and any GOOD reader",
                        )
                        .on_disabled_hover_text(reason)
                    });
                    if save.inner.clicked() {
                        if missing.is_empty() {
                            self.ask_where_to_save();
                        } else {
                            self.toasts
                                .error(crate::app::missing_export_data_toast(&missing));
                        }
                    }
                    let copy = ui.add_enabled_ui(enabled, |ui| {
                        button(
                            ui,
                            ButtonKind::Secondary,
                            Some(icons::copy()),
                            "Copy",
                            false,
                        )
                        .on_disabled_hover_text(reason)
                    });
                    if copy.inner.clicked() {
                        if missing.is_empty() {
                            self.genshin_optimizer_request_export(OptimizerExportTarget::Clipboard);
                        } else {
                            self.toasts
                                .error(crate::app::missing_export_data_toast(&missing));
                        }
                    }
                });
                divider(ui);
                let secondary = self.auto_export_line(&pal);
                let was_on = self.saved_state.save_result_to_file;
                switch_row(
                    ui,
                    "Auto-export",
                    Some(secondary),
                    &mut self.saved_state.save_result_to_file,
                    true,
                )
                .on_hover_text(
                    self.saved_state
                        .save_result_folder
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "Choose a folder".to_owned()),
                );
                if !was_on
                    && self.saved_state.save_result_to_file
                    && self.saved_state.save_result_folder.is_none()
                {
                    self.pick_export_folder();
                }
            },
        );
        if open_folder && let Some(folder) = folder {
            open_folder_in_explorer(folder);
        }
        match more_action {
            Some(MoreExport::Achievements(url)) => self.export_achievements(app_state, url),
            Some(MoreExport::Settings) => self.open_settings(Page::Export),
            None => {}
        }
    }

    /// The Auto-export row's second line.
    fn auto_export_line(&self, pal: &Palette) -> RichText {
        if !self.saved_state.save_result_to_file {
            return secondary_text("Off · choose a folder", pal.tx2);
        }
        if let Some(event) = self.activity.latest_of(crate::activity::Topic::Export) {
            let time = event.at.format("%H:%M");
            return match (&event.kind, &event.path) {
                (crate::activity::Kind::Ok, Some(path)) => secondary_text(
                    format!(
                        "✓ Saved {time} · {}",
                        path.file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    ),
                    pal.ok,
                ),
                (crate::activity::Kind::Error, _) => {
                    secondary_text(format!("Failed {time} · {}", event.text), pal.bad)
                }
                _ => self.folder_line(pal),
            };
        }
        self.folder_line(pal)
    }

    fn folder_line(&self, pal: &Palette) -> RichText {
        match &self.saved_state.save_result_folder {
            Some(path) => secondary_text(middle_ellipsis(&path.display().to_string(), 44), pal.tx2)
                .monospace(),
            None => secondary_text("Next to Teyvat.me", pal.tx2),
        }
    }

    fn destinations_card(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        let status = self.auth_status_rx.borrow().clone();
        let host = self.tracker_host();
        let mut action = None;
        card(
            ui,
            "Destinations",
            Some("Optional"),
            |_| {},
            |ui| {
                let (avatar, title, secondary, color) = match &status {
                    AuthStatus::SignedIn { whoami, error } => {
                        let user = whoami.as_ref().map_or("…", |w| w.user.as_str());
                        let line = self.destination_result_line();
                        let color = if error.is_some() || line.1 {
                            pal.bad
                        } else {
                            pal.tx2
                        };
                        (
                            Avatar::Initial(user.chars().next().unwrap_or('?')),
                            format!("{host} ↗ · {user}"),
                            line.0,
                            color,
                        )
                    }
                    AuthStatus::Waiting { .. } => (
                        Avatar::Waiting,
                        host.clone(),
                        "Waiting for the browser…".to_owned(),
                        pal.tx2,
                    ),
                    _ if self.saved_state.tracker_sign_in == TrackerSignIn::SignedOut
                        && self.tracker_account.is_some() =>
                    {
                        let name = self
                            .tracker_account
                            .as_ref()
                            .map(|a| a.name.clone())
                            .unwrap_or_default();
                        (
                            Avatar::Key,
                            format!("{host} ↗ · {name}"),
                            self.destination_result_line().0,
                            pal.tx2,
                        )
                    }
                    _ => (
                        Avatar::Cloud,
                        format!("{host} ↗"),
                        "Upload each capture · live sync".to_owned(),
                        pal.tx2,
                    ),
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    avatar.show(ui, &pal);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        match &status {
                            AuthStatus::SignedIn { .. } => {
                                if icon_button(ui, icons::settings(), "teyvat.me settings")
                                    .clicked()
                                {
                                    action = Some(DestAction::Page);
                                }
                                widgets::switch(ui, &mut self.saved_state.auto_export_to_tracker)
                                    .on_hover_text("Auto upload");
                            }
                            AuthStatus::Waiting { .. } => {
                                if button(ui, ButtonKind::Ghost, None, "Cancel", false).clicked() {
                                    action = Some(DestAction::Cancel);
                                }
                            }
                            _ => {
                                if icon_button(ui, icons::settings(), "teyvat.me settings")
                                    .clicked()
                                {
                                    action = Some(DestAction::Page);
                                }
                                if button(
                                    ui,
                                    ButtonKind::Secondary,
                                    Some(icons::log_in()),
                                    "Connect",
                                    false,
                                )
                                .on_hover_text(format!("Sign in to {host} in your browser"))
                                .clicked()
                                {
                                    action = Some(DestAction::Connect);
                                }
                            }
                        }
                        ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let name = ui.add(
                                egui::Label::new(title_text(title, pal.tx))
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            );
                            if name
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(format!("Open {host}"))
                                .clicked()
                            {
                                action = Some(DestAction::OpenSite);
                            }
                            line(ui, secondary_text(secondary, color));
                        });
                    });
                });
            },
        );
        match action {
            Some(DestAction::Connect) => self.send_tracker(TrackerRequest::SignIn),
            Some(DestAction::Cancel) => self.send_tracker(TrackerRequest::CancelSignIn),
            Some(DestAction::Page) => self.open_settings(Page::Teyvatme),
            Some(DestAction::OpenSite) => {
                let url = self.dashboard_url().unwrap_or_else(|| self.tracker_site());
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            None => {}
        }
    }

    /// The destination's last result: (line, is a problem).
    pub(super) fn destination_result_line(&self) -> (String, bool) {
        if let AuthStatus::SignedIn {
            whoami: Some(whoami),
            ..
        } = &*self.auth_status_rx.borrow()
            && whoami.scope == crate::signin::UploadScope::Account
            && whoami.account_id.is_none()
        {
            return ("Uploads: Not chosen".to_owned(), true);
        }
        match self.activity.latest_of(crate::activity::Topic::Destination) {
            Some(event) => {
                let mark = match event.kind {
                    crate::activity::Kind::Ok => "✓ ",
                    _ => "",
                };
                (
                    format!("{mark}{} · {}", event.text, event.at.format("%H:%M")),
                    event.kind == crate::activity::Kind::Error,
                )
            }
            None if self.saved_state.auto_export_to_tracker => {
                ("Uploads each capture".to_owned(), false)
            }
            None => ("Auto upload off".to_owned(), false),
        }
    }

    pub(super) fn ask_where_to_save(&mut self) {
        let now = Local::now();
        self.file_request = Some(super::FileRequest::SaveGood {
            name: format!("teyvatme_{}.json", now.format("%Y-%m-%d_%H-%M")),
            folder: self.export_folder(),
        });
    }

    pub(super) fn pick_export_folder(&mut self) {
        self.file_request = Some(super::FileRequest::ExportFolder);
    }

    /// The OS's own Save or Choose folder dialog, modal over the window. Like
    /// any native dialog it holds this thread until answered; capture and
    /// uploads run on their own threads meanwhile.
    pub(super) fn run_file_dialog(
        &mut self,
        ctx: &egui::Context,
        window: &eframe::Frame,
        request: super::FileRequest,
    ) {
        match request {
            super::FileRequest::SaveGood { name, folder } => {
                let mut dialog = rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title("Save GOOD JSON")
                    .add_filter("JSON", &["json"])
                    .set_file_name(name);
                if let Some(folder) = folder {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(mut path) = dialog.save_file() {
                    // A name typed without one gets the filter's extension.
                    if path.extension().is_none() {
                        path.set_extension("json");
                    }
                    self.last_save_folder = path.parent().map(|dir| dir.to_path_buf());
                    self.optimizer_save_path = Some(path);
                    self.genshin_optimizer_request_export(super::OptimizerExportTarget::File);
                }
            }
            super::FileRequest::ExportFolder => {
                let mut dialog = rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title("Auto-export folder");
                if let Some(folder) = &self.saved_state.save_result_folder {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(folder) = dialog.pick_folder() {
                    self.saved_state.save_result_folder = Some(folder);
                }
            }
        }
        ctx.request_repaint();
    }

    /// The auto-export folder, else the folder of the last manual save.
    fn export_folder(&self) -> Option<std::path::PathBuf> {
        self.saved_state
            .save_result_folder
            .clone()
            .or_else(|| self.last_save_folder.clone())
    }

    fn export_achievements(&mut self, app_state: &AppState, open: Option<&'static str>) {
        if app_state.updated.achievements_updated_time.is_none() {
            let toast = self
                .toasts
                .error("No achievements yet · they arrive as you enter the world");
            if let Some(url) = open {
                toast.action(crate::ui::toast::Action::OpenAnyway(url.to_owned()));
            }
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = self
            .ui_message_tx
            .send(crate::Message::ExportAchievements(tx));
        self.achievements_export_rx = Some(rx);
        self.pending_open_url = open.map(str::to_owned);
    }
}

enum MoreExport {
    Achievements(Option<&'static str>),
    Settings,
}

enum DestAction {
    Connect,
    Cancel,
    Page,
    OpenSite,
}

/// A destination row's 36 px avatar.
pub(super) enum Avatar {
    Cloud,
    Key,
    Waiting,
    Initial(char),
}

impl Avatar {
    pub(super) fn show(&self, ui: &mut Ui, pal: &Palette) {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(36.0), egui::Sense::hover());
        let painter = ui.painter();
        match self {
            Avatar::Initial(initial) => {
                painter.circle_filled(rect.center(), 18.0, pal.ac_con);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    initial.to_uppercase().to_string(),
                    egui::FontId::new(15.0, crate::ui::theme::weight::semibold()),
                    pal.ac_on,
                );
            }
            Avatar::Waiting => {
                painter.circle_filled(rect.center(), 18.0, pal.over);
                let spin = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(18.0));
                ui.put(spin, egui::Spinner::new().size(18.0).color(pal.ac_tx));
            }
            Avatar::Cloud | Avatar::Key => {
                painter.circle_filled(rect.center(), 18.0, pal.over);
                let icon = if matches!(self, Avatar::Key) {
                    icons::key_round()
                } else {
                    icons::cloud()
                };
                let icon_rect =
                    egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(18.0));
                icons::image(icon, 18.0, pal.tx2).paint_at(ui, icon_rect);
            }
        }
    }
}

/// The region a UID's first digit names.
pub(super) fn server_of(uid: u32) -> Option<&'static str> {
    let mut first = uid;
    while first >= 10 {
        first /= 10;
    }
    match (uid >= 100_000_000, first) {
        (true, 6) => Some("America"),
        (true, 7) => Some("Europe"),
        (true, 8) => Some("Asia"),
        (true, 9) => Some("TW · HK · MO"),
        (true, 1 | 2 | 3 | 5) => Some("China"),
        _ => None,
    }
}

/// `D:\Captures\…\Teyvatme`: cut in the middle to `max` characters.
pub(super) fn middle_ellipsis(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_owned();
    }
    let keep = max.saturating_sub(1);
    let head = keep / 2;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

fn open_folder_in_explorer(folder: std::path::PathBuf) {
    std::thread::spawn(move || {
        if let Err(e) = open::that(&folder) {
            tracing::error!("could not open {folder:?}: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uid_names_its_region() {
        assert_eq!(server_of(812_345_678), Some("Asia"));
        assert_eq!(server_of(612_345_678), Some("America"));
        assert_eq!(server_of(712_345_678), Some("Europe"));
        assert_eq!(server_of(912_345_678), Some("TW · HK · MO"));
        assert_eq!(server_of(12_345), None);
    }

    #[test]
    fn long_paths_are_cut_in_the_middle() {
        assert_eq!(middle_ellipsis("short", 10), "short");
        let cut = middle_ellipsis(r"D:\Captures\Teyvatme\very\long\folder\name", 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(cut.starts_with(r"D:\Captu") && cut.ends_with("er\\name"));
    }
}
