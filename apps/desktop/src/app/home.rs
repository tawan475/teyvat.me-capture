// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The main window, layout G (spec §4): the status strip with the app
//! buttons, then the Traveler | Today pair, the Export | Wish history pair,
//! and the Tracker row at the window's foot. The two cards of a pair end on
//! one line. One column below the breakpoint. The app exports; it shows
//! only the account line, six counts and today's routine (the rest is the
//! tracker's job).

use std::time::{Duration, Instant};

use chrono::Local;
use egui::{Align, Id, Layout, Margin, Response, RichText, Ui, vec2};
use teyvatme_core::FORMAT_NAME;

use super::frame::BREAKPOINT;
use super::{OptimizerExportTarget, Page, TeyvatmeApp, TrackerSignIn, today};
use crate::AppState;
use crate::game_watch::{GameStatus, MissedLaunch};
use crate::i18n::{tr, trf};
use crate::signin::{AuthStatus, TrackerRequest};
use crate::ui::icons;
use crate::ui::theme::Palette;
use crate::ui::widgets::{
    self, ButtonKind, Disc, HeroVariant, MiniStat, STAT_TILE_HEIGHT, Step, Strip, button, card,
    divider, icon_button, line, secondary_text, small_text, strong_text, switch_row, title_text,
};
use crate::update::UpdateStatus;

/// How long a tile glows after its class updates.
const TILE_FLASH: Duration = Duration::from_millis(600);
/// Space between cards and columns.
const GAP: f32 = 12.0;

/// What the strip's second line asks for (the bad states).
#[derive(Clone, Copy, PartialEq, Eq)]
enum StripExtra {
    Admin,
    Logs,
    Missed,
    KeyLost,
}

/// What a click in the strip's second line asked for.
enum StripAction {
    RestartAsAdmin,
    OpenLogs,
    CloseGenshin,
}

impl TeyvatmeApp {
    pub(super) fn home(&mut self, ui: &mut Ui, app_state: &AppState) {
        let viewport = ui.available_height();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let content_top = ui.cursor().top();
                egui::Frame::new()
                    .inner_margin(Margin::same(16))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = GAP;
                        self.crash_banner(ui);
                        self.update_banner(ui);
                        let wide = ui.available_width() >= BREAKPOINT - 32.0;
                        self.status_strip(ui, app_state, !wide);
                        self.pair(
                            ui,
                            "traveler-today",
                            wide,
                            |app, ui, stretch| app.traveler_card(ui, app_state, stretch),
                            |app, ui, stretch| app.today_card(ui, app_state, stretch),
                        );
                        self.pair(
                            ui,
                            "export-wishes",
                            wide,
                            |app, ui, stretch| app.export_card(ui, app_state, stretch),
                            |app, ui, stretch| app.wish_card(ui, app_state, stretch),
                        );
                        // The Tracker row at the window's foot: the space
                        // left, if any, goes above it.
                        let id = Id::new("tracker-row-height");
                        let height = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(58.0);
                        let bottom = content_top + viewport - 16.0;
                        let room = (bottom - height - ui.cursor().top()).floor() - 1.0;
                        if room > 0.0 {
                            ui.add_space(room);
                        }
                        let row = self.tracker_row(ui);
                        if (row.rect.height() - height).abs() > 0.5 {
                            ui.data_mut(|d| d.insert_temp(id, row.rect.height()));
                            ui.ctx().request_discard("tracker row height");
                        }
                    });
            });
    }

    /// Two cards side by side whose frames end on one line: each pass
    /// measures what each card needs, and the next gives both the taller
    /// one's height, the shorter card spreading its content into it
    /// (`stretch`). One under the other when narrow.
    fn pair(
        &mut self,
        ui: &mut Ui,
        name: &str,
        wide: bool,
        left: impl FnOnce(&mut Self, &mut Ui, f32) -> Response,
        right: impl FnOnce(&mut Self, &mut Ui, f32) -> Response,
    ) {
        if !wide {
            left(self, ui, 0.0);
            right(self, ui, 0.0);
            return;
        }
        let id = Id::new("pair").with(name);
        let natural: [f32; 2] = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let target = natural[0].max(natural[1]);
        let stretch = [target - natural[0], target - natural[1]];
        let mut measured = natural;
        ui.spacing_mut().item_spacing.x = GAP;
        ui.columns(2, |columns| {
            let [l, r] = columns else {
                unreachable!("two columns")
            };
            for column in [&mut *l, &mut *r] {
                column.spacing_mut().item_spacing = vec2(8.0, GAP);
            }
            measured[0] = left(self, l, stretch[0]).rect.height() - stretch[0];
            measured[1] = right(self, r, stretch[1]).rect.height() - stretch[1];
        });
        ui.spacing_mut().item_spacing.x = 8.0;
        if measured
            .iter()
            .zip(natural)
            .any(|(now, before)| (now - before).abs() > 0.5)
        {
            ui.data_mut(|d| d.insert_temp(id, measured));
            ui.ctx().request_discard("card pair heights");
        }
    }

    /// A newer version found, or installed and waiting for a restart.
    fn update_banner(&mut self, ui: &mut Ui) {
        let status = self.update_status.borrow().clone();
        let (title, ready, version) = match &status {
            UpdateStatus::Available(v) | UpdateStatus::Installing(v) => {
                (trf("Update {0}", &[v]), false, v.clone())
            }
            UpdateStatus::Ready(v) => (
                trf("{0} installed · restart to use it", &[v]),
                true,
                v.clone(),
            ),
            _ => return,
        };
        let pal = widgets::p(ui);
        let mut open = false;
        let mut restart = false;
        egui::Frame::new()
            .fill(pal.ac_con)
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(Margin {
                left: 14,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add(icons::image(icons::download(), 18.0, pal.ac_on));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let outlined = |ui: &mut Ui, text: &str| {
                            ui.add(
                                egui::Button::new(
                                    RichText::new(text)
                                        .text_style(crate::ui::theme::style::secondary())
                                        .family(crate::ui::theme::weight::semibold())
                                        .color(pal.ac_on),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0_f32, pal.ac_on))
                                .corner_radius(egui::CornerRadius::same(8))
                                .min_size(vec2(0.0, 32.0)),
                            )
                        };
                        if ready {
                            restart = outlined(ui, tr("Restart now")).clicked();
                        } else {
                            open = outlined(ui, tr("Details")).clicked();
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            line(ui, strong_text(title, pal.ac_on)).on_hover_text(trf(
                                "Teyvat.me Capture {0} is ready to install · you have {1}",
                                &[&version, &env!("CARGO_PKG_VERSION")],
                            ));
                        });
                    });
                });
            });
        if open {
            self.open_settings(Page::About);
        }
        if restart {
            self.restart_to_update(ui);
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
            (tr("Crashed last time"), tr("Report and log kept"))
        } else {
            (tr("Stopped unexpectedly"), tr("No crash report · log kept"))
        };
        let mut open_logs = false;
        let mut dismiss = false;
        egui::Frame::new()
            .fill(pal.warn_con)
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(icons::image(icons::triangle_alert(), 18.0, pal.warn_on));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        dismiss = icon_button(ui, icons::x(), tr("Dismiss")).clicked();
                        open_logs = button(
                            ui,
                            ButtonKind::Secondary,
                            None,
                            tr("Open log folder"),
                            false,
                        )
                        .clicked();
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                line(ui, strong_text(title, pal.warn_on));
                                line(ui, secondary_text(detail, pal.warn_on));
                            })
                            .response
                            .on_hover_text(&notice);
                        });
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

    /// The status strip across the window, with the app buttons at its end.
    fn status_strip(&mut self, ui: &mut Ui, app_state: &AppState, narrow: bool) {
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
        let decoding_since = self
            .decoding_since
            .map(|at| at.format(crate::clock::hm()).to_string())
            .unwrap_or_default();
        let session_line = trf("Login detected · {0}", &[&decoding_since]);

        let title = strip_title(self.is_admin, status, app_state.capture_done);
        let (variant, disc, subtitle, steps, extra): (_, _, &str, _, _) = if !self.is_admin {
            (
                HeroVariant::Bad,
                bad(icons::shield_alert),
                tr("Started without admin rights"),
                [Failed, Pending, Pending],
                Some(StripExtra::Admin),
            )
        } else {
            match status {
                GameStatus::NotRunning => (
                    HeroVariant::Neutral,
                    neutral(icons::gamepad),
                    tr("Genshin isn't running · start it now"),
                    [Done, Pending, Pending],
                    None,
                ),
                GameStatus::AwaitingLogin => (
                    HeroVariant::Neutral,
                    neutral(icons::gamepad),
                    tr("Already open · enter the world now"),
                    [Done, Now, Pending],
                    None,
                ),
                GameStatus::LaunchCaptured => (
                    HeroVariant::Neutral,
                    Disc::Icon {
                        icon: icons::gamepad,
                        fill: |p: &Palette| p.fill,
                        ink: |_: &Palette| egui::Color32::WHITE,
                    },
                    tr("Launch captured · enter the world now"),
                    [Done, Now, Pending],
                    None,
                ),
                GameStatus::Decoding => (
                    HeroVariant::Ok,
                    ok_disc,
                    session_line.as_str(),
                    [Done, Done, Done],
                    None,
                ),
                GameStatus::CaptureOff => (
                    HeroVariant::Neutral,
                    Disc::Spinner,
                    tr("Packet capture restarts itself within seconds"),
                    [Now, Pending, Pending],
                    (self.started_at.elapsed() > Duration::from_secs(30))
                        .then_some(StripExtra::Logs),
                ),
                GameStatus::LaunchMissed(cause) => {
                    let (line_text, steps) = match cause {
                        MissedLaunch::AlreadyRunning => (
                            tr("Genshin started before Teyvat.me"),
                            [Done, Failed, Pending],
                        ),
                        MissedLaunch::CaptureStopped => (
                            tr("Capture was off when you logged in"),
                            [Failed, Pending, Pending],
                        ),
                    };
                    (
                        HeroVariant::Bad,
                        bad(icons::triangle_alert),
                        line_text,
                        steps,
                        Some(StripExtra::Missed),
                    )
                }
                GameStatus::KeyLost => (
                    HeroVariant::Bad,
                    bad(icons::triangle_alert),
                    tr("Session key not found"),
                    [Done, Done, Failed],
                    Some(StripExtra::KeyLost),
                ),
            }
        };
        let tooltip = if self.is_admin {
            if status == GameStatus::Decoding && app_state.capture_done {
                tr("Everything this login sent has arrived. Changes as you play keep coming in.")
            } else {
                status.tooltip()
            }
        } else {
            tr("Windows lets only administrators read network packets")
        };

        let pal = widgets::p(ui);
        let killing = self.game_kill_rx.is_some();
        let mut action = None;
        let mut second_line = |ui: &mut Ui| match extra {
            Some(StripExtra::Admin) => {
                if button(
                    ui,
                    ButtonKind::Fill,
                    Some(icons::shield_check()),
                    tr("Restart as administrator"),
                    false,
                )
                .clicked()
                {
                    action = Some(StripAction::RestartAsAdmin);
                }
            }
            Some(StripExtra::Logs) => {
                ui.horizontal(|ui| {
                    if button(
                        ui,
                        ButtonKind::Secondary,
                        Some(icons::folder_open()),
                        tr("Open log folder"),
                        false,
                    )
                    .clicked()
                    {
                        action = Some(StripAction::OpenLogs);
                    }
                    ui.label(small_text(tr("after 30 s"), pal.tx3));
                });
            }
            Some(StripExtra::Missed) => {
                line(
                    ui,
                    strong_text(tr("Back to the title screen and enter again"), pal.tx),
                );
                ui.horizontal(|ui| {
                    let kill = ui.add_enabled_ui(!killing, |ui| {
                        button(
                            ui,
                            ButtonKind::Danger,
                            Some(icons::power()),
                            tr("Close Genshin"),
                            false,
                        )
                        .on_hover_text(tr(
                            "Force-quits the game. Progress is safe; a domain run in progress is lost.",
                        ))
                    });
                    if kill.inner.clicked() {
                        action = Some(StripAction::CloseGenshin);
                    }
                    ui.label(small_text(tr("or restart Genshin"), pal.tx3));
                });
            }
            Some(StripExtra::KeyLost) => {
                line(
                    ui,
                    strong_text(tr("Back to the title screen and enter again"), pal.tx),
                );
                ui.label(small_text(tr("Captured data is kept"), pal.tx3));
            }
            None => {}
        };
        widgets::status_strip(
            ui,
            Strip {
                variant,
                disc,
                title,
                subtitle,
                steps,
                tooltip,
            },
            narrow,
            |ui| self.app_buttons(ui),
            extra
                .is_some()
                .then_some(&mut second_line as &mut dyn FnMut(&mut Ui)),
        );
        match action {
            Some(StripAction::RestartAsAdmin) => self.restart_as_admin(ui.ctx()),
            Some(StripAction::OpenLogs) => {
                std::thread::spawn(|| {
                    let _ = crate::open_log_dir();
                });
            }
            Some(StripAction::CloseGenshin) => self.kill_game(),
            None => {}
        }
    }

    /// The account line and six counts (spec §4.2).
    fn traveler_card(&mut self, ui: &mut Ui, app_state: &AppState, stretch: f32) -> Response {
        let pal = widgets::p(ui);
        let suffix = match &app_state.player {
            Some(player) => {
                let mut parts = Vec::new();
                if let Some(ar) = player.ar {
                    parts.push(trf("AR {0}", &[&ar]));
                }
                if let Some(wl) = player.wl {
                    parts.push(trf("WL {0}", &[&wl]));
                }
                if let Some(server) = player.uid.and_then(server_of) {
                    parts.push(server.to_owned());
                }
                Some(parts.join(" · "))
            }
            None if app_state.counts.is_none() => Some(tr("Nothing yet").to_owned()),
            None => None,
        };
        let uid = app_state.player.as_ref().and_then(|player| player.uid);
        let time = app_state.captured_at.map(|at| {
            let ago = widgets::ago((Local::now() - at).num_seconds());
            (
                format!("{} · {ago}", at.format(crate::clock::hm())),
                at.format(&format!("%Y-%m-%d {}", crate::clock::hms()))
                    .to_string(),
            )
        });
        if app_state.captured_at.is_some() {
            // "just now" turns into "1 min ago" by itself.
            ui.ctx().request_repaint_after(Duration::from_secs(30));
        }
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
        let value = |pick: fn(&crate::player_data::ExportCounts) -> usize| counts.map(|c| pick(&c));
        let tiles = [
            (
                value(|c| c.characters),
                tr("Characters"),
                flash(updated.characters_updated),
            ),
            (
                value(|c| c.weapons),
                tr("Weapons"),
                flash(updated.items_updated),
            ),
            (
                value(|c| c.artifacts),
                tr("Artifacts"),
                flash(updated.items_updated),
            ),
            (
                value(|c| c.materials),
                tr("Materials"),
                flash(updated.items_updated),
            ),
            (
                value(|c| c.achievements),
                tr("Achievements"),
                flash(updated.achievements_updated),
            ),
            (app_state.anecdotes, tr("Anecdotes"), 0.0),
        ];
        if tiles.iter().any(|(_, _, f)| *f > 0.0) {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }

        let card = card(
            ui,
            tr("Captured"),
            suffix.as_deref().filter(|s| !s.is_empty()),
            |ui| {
                if let Some((time, full)) = &time {
                    ui.label(small_text(time, pal.tx3)).on_hover_text(full);
                }
            },
            |ui| {
                let gap = 8.0;
                let width = ui.available_width();
                let size = vec2(
                    ((width - 2.0 * gap) / 3.0).floor(),
                    STAT_TILE_HEIGHT + stretch / 2.0,
                );
                for row in tiles.chunks(3) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (value, label, flash) in row {
                            widgets::stat_tile(ui, size, *value, label, *flash);
                        }
                    });
                }
            },
        );
        // The UID only on hover, so streams and screenshots don't show it.
        let response = card.response;
        match uid {
            Some(uid) => response.on_hover_text(trf("UID {0}", &[&uid])),
            None => response,
        }
    }

    /// Today's routine at login (spec §4.2b): Resin and realm currency side
    /// by side with their bars, the four counters under them.
    fn today_card(&mut self, ui: &mut Ui, app_state: &AppState, stretch: f32) -> Response {
        let pal = widgets::p(ui);
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        let login_resin = app_state.player.as_ref().and_then(|player| player.resin);
        let figures = today::figures(app_state.tasks.as_ref(), login_resin, now);
        if app_state.tasks.is_some() {
            // Resin's clock and the expeditions' times move on by themselves.
            ui.ctx().request_repaint_after(Duration::from_secs(30));
        }
        let [resin, realm, commissions, expeditions, bosses, abyss] = figures;
        let stat = |icon, label, figure: today::Figure, tooltip| MiniStat {
            icon,
            label,
            value: figure.value,
            bar: figure.bar,
            line: figure.line,
            tooltip,
        };
        let stats = [
            stat(
                icons::droplet(),
                tr("Resin"),
                resin,
                tr("Original Resin · one every 8 minutes · counted on from the login"),
            ),
            stat(
                icons::house(),
                tr("Realm currency"),
                realm,
                tr("Serenitea Pot · the jar at login"),
            ),
            stat(
                icons::list_checks(),
                tr("Commissions"),
                commissions,
                tr("Daily Commissions at login"),
            ),
            stat(
                icons::hourglass(),
                tr("Expeditions"),
                expeditions,
                tr("Who is out and when the first is back"),
            ),
            stat(
                icons::swords(),
                tr("Weekly bosses"),
                bosses,
                tr("Half-price weekly bosses left this week"),
            ),
            stat(
                icons::star(),
                tr("Spiral Abyss"),
                abyss,
                tr("Stars this period"),
            ),
        ];
        let at = app_state
            .tasks_at
            .map(|at| at.format(crate::clock::hm()).to_string());
        card(
            ui,
            tr("Today"),
            None,
            |ui| {
                if let Some(at) = &at {
                    ui.label(small_text(trf("at login {0}", &[at]), pal.tx3))
                        .on_hover_text(tr(
                            "The game's state at login; Resin and realm currency count on by themselves",
                        ));
                }
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let width = ui.available_width();
                let columns = if width >= 360.0 { 2 } else { 1 };
                let gap = 20.0;
                let cell = (width - gap * (columns - 1) as f32) / columns as f32;
                let rows: Vec<&[MiniStat]> = stats.chunks(columns).collect();
                let extra = stretch / rows.len() as f32;
                for row in rows {
                    let natural = row.iter().map(MiniStat::height).fold(0.0, f32::max);
                    let (rect, _) = ui.allocate_exact_size(
                        vec2(width, natural + 8.0 + extra),
                        egui::Sense::hover(),
                    );
                    for (i, stat) in row.iter().enumerate() {
                        let left = rect.left() + i as f32 * (cell + gap);
                        let cell_rect = egui::Rect::from_min_max(
                            egui::pos2(left, rect.top()),
                            egui::pos2(left + cell, rect.bottom()),
                        );
                        stat.show(ui, cell_rect, natural);
                    }
                }
            },
        )
        .response
    }

    fn wish_card(&mut self, ui: &mut Ui, app_state: &AppState, stretch: f32) -> Response {
        let pal = widgets::p(ui);
        let now = Instant::now();
        let url = self.wishes.wish_url(now);
        let uid_known = app_state
            .player
            .as_ref()
            .is_some_and(|player| player.uid.is_some());
        let status = self.wishes.status(now, app_state.game_status, uid_known);
        let linked = crate::app::can_upload_to_tracker(&self.saved_state);
        let host = self.tracker_host();

        // The whole history as a UIGF file, read here and saved locally.
        let save = self.wishes.save.state();
        match &save {
            crate::wish_history::SaveState::Done { path, records } => {
                self.toasts
                    .success(trf("Saved {0} wishes", &[&records]))
                    .action(crate::ui::toast::Action::Show(path.clone()))
                    .saved();
                self.wishes.save.clear();
            }
            crate::wish_history::SaveState::Failed(why) => {
                self.toasts
                    .error(trf("Wish history not saved · {0}", &[&why]));
                self.wishes.save.clear();
            }
            _ => {}
        }
        let reading = self.wishes.save.reading();

        let no_key = if self.wishes.key_expired(now) {
            tr("Key expired · open the wish history in game again")
        } else {
            tr("Open the wish history in game first")
        };
        let header = if reading {
            save.line().unwrap_or_default()
        } else {
            match self.wishes.key_received(now) {
                Some(at) if url.is_some() => {
                    trf("URL ready · {0}", &[&at.format(crate::clock::hm())])
                }
                _ if url.is_some() => tr("URL ready").to_owned(),
                _ => no_key.to_owned(),
            }
        };
        let header_tip = if url.is_some() {
            status.tooltip.clone()
        } else {
            tr("The wish URL needs the key the game sends when its wish history page opens, once per login.")
                .to_owned()
        };

        let mut copy = false;
        let mut open = None;
        let mut open_menu = None;
        let mut save_now = false;
        let mut cancel_save = false;
        let ready = url.is_some();
        let response = card(
            ui,
            tr("Wish history"),
            None,
            |ui| {
                let more = icon_button(ui, icons::ellipsis(), tr("More"));
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(200.0);
                    ui.add_enabled_ui(ready, |ui| {
                        if ui.button(tr("Open paimon.moe")).clicked() {
                            open_menu = Some("https://paimon.moe/wish/import");
                        }
                        if ui.button(tr("Open StarDB")).clicked() {
                            open_menu = Some("https://stardb.gg/en/genshin/wish-import");
                        }
                    });
                    ui.separator();
                    if reading {
                        cancel_save = ui.button(tr("Stop saving")).clicked();
                    } else {
                        let save = ui
                            .add_enabled_ui(ready, |ui| ui.button(tr("Save history…")))
                            .inner
                            .on_hover_text(tr(
                                "Reads every banner from the game's wish API (1 s a page, a few minutes for a long history) and saves it as UIGF JSON",
                            ))
                            .on_disabled_hover_text(no_key);
                        save_now = save.clicked();
                    }
                });
                if reading {
                    ui.add(egui::Spinner::new().size(14.0).color(pal.ac_tx));
                }
                ui.add(egui::Label::new(small_text(&header, pal.tx3)).truncate())
                    .on_hover_text(&header_tip);
            },
            |ui| {
                ui.add_space(stretch / 2.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(ready, |ui| {
                        copy = button(
                            ui,
                            ButtonKind::Secondary,
                            Some(icons::copy()),
                            tr("Copy URL"),
                            false,
                        )
                        .on_hover_text(tr("Copy wish URL, for paimon.moe, StarDB or any wish importer"))
                        .on_disabled_hover_text(no_key)
                        .clicked();
                        if button(ui, ButtonKind::Ghost, None, "paimon.moe", false)
                            .on_hover_text(tr("Copies the URL, then opens paimon.moe"))
                            .on_disabled_hover_text(no_key)
                            .clicked()
                        {
                            open = Some("https://paimon.moe/wish/import");
                        }
                        if button(ui, ButtonKind::Ghost, None, "StarDB", false)
                            .on_hover_text(tr("Copies the URL, then opens StarDB"))
                            .on_disabled_hover_text(no_key)
                            .clicked()
                        {
                            open = Some("https://stardb.gg/en/genshin/wish-import");
                        }
                    });
                });
                ui.add_space(stretch - stretch / 2.0);
                divider(ui);
                let title = trf("Send to {0}", &[&host]);
                if linked {
                    let sync = self.wishes.sync_line(now);
                    let secondary = if self.saved_state.send_wishes {
                        sync.unwrap_or_else(|| tr("Sends new wishes as they appear").to_owned())
                    } else {
                        tr("Off · the URL still works").to_owned()
                    };
                    let is_problem = self.saved_state.send_wishes && self.wishes.sync_problem(now);
                    let color = if is_problem { pal.bad } else { pal.tx2 };
                    switch_row(
                        ui,
                        &title,
                        Some(secondary_text(secondary, color)),
                        &mut self.saved_state.send_wishes,
                        true,
                    )
                    .on_hover_text(tr(
                        "Only wishes the tracker doesn't have yet are read, 1 s per page",
                    ));
                } else {
                    let mut off = false;
                    switch_row(
                        ui,
                        &title,
                        Some(secondary_text(
                            trf("Off · connect {0} to send them", &[&host]),
                            pal.tx2,
                        )),
                        &mut off,
                        false,
                    );
                }
            },
        )
        .response;

        let open = open.or(open_menu);
        if (copy || open.is_some())
            && let Some(url) = &url
        {
            ui.ctx().copy_text(url.clone());
            self.toasts.info(tr("Wish URL copied"));
            if let Some(page) = open {
                ui.ctx().open_url(egui::OpenUrl::new_tab(page));
            }
        }
        if cancel_save {
            self.wishes.save.cancel();
        }
        if save_now {
            self.file_request = Some(super::FileRequest::SaveWishes {
                name: format!(
                    "teyvatme_wishes_{}.json",
                    Local::now().format("%Y-%m-%d_%H-%M")
                ),
            });
        }
        response
    }

    fn export_card(&mut self, ui: &mut Ui, app_state: &AppState, stretch: f32) -> Response {
        let pal = widgets::p(ui);
        let missing =
            crate::app::missing_export_data(&self.saved_state.export_settings, &app_state.updated);
        let nothing = app_state.counts.is_none();
        let busy = self.optimizer_export_rx.is_some();
        let folder = self.export_folder();
        let mut open_folder = false;
        let mut more_action = None;
        let response = card(
            ui,
            tr("Export"),
            None,
            |ui| {
                let more = icon_button(ui, icons::ellipsis(), tr("More exports"));
                egui::Popup::menu(&more).show(|ui| {
                    ui.set_min_width(220.0);
                    if ui.button(tr("Copy achievements")).clicked() {
                        more_action = Some(MoreExport::Achievements(None));
                    }
                    if ui.button(tr("Achievements to StarDB")).clicked() {
                        more_action =
                            Some(MoreExport::Achievements(Some("https://stardb.gg/import")));
                    }
                    if ui.button(tr("Achievements to Seelie.me")).clicked() {
                        more_action = Some(MoreExport::Achievements(Some(
                            "https://seelie.me/achievements",
                        )));
                    }
                    ui.separator();
                    if ui.button(tr("Filters and auto-export")).clicked() {
                        more_action = Some(MoreExport::Settings);
                    }
                });
                let open = ui.add_enabled_ui(folder.is_some(), |ui| {
                    icon_button(ui, icons::folder_open(), tr("Open export folder"))
                        .on_disabled_hover_text(tr("No export folder yet"))
                });
                open_folder = open.inner.clicked();
            },
            |ui| {
                ui.add_space(stretch / 2.0);
                ui.horizontal(|ui| {
                    let enabled = !nothing && !busy;
                    let reason = if nothing {
                        tr("Nothing captured yet")
                    } else {
                        tr("An export is running")
                    };
                    let save = ui.add_enabled_ui(enabled, |ui| {
                        button(
                            ui,
                            ButtonKind::Fill,
                            Some(icons::download()),
                            &trf("Save {0} JSON…", &[&FORMAT_NAME]),
                            false,
                        )
                        .on_hover_text(good_json_hint())
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
                            tr("Copy"),
                            false,
                        )
                        .on_hover_text(trf("{0} JSON to the clipboard", &[&FORMAT_NAME]))
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
                ui.add_space(stretch - stretch / 2.0);
                divider(ui);
                let secondary = self.auto_export_line(&pal);
                let was_on = self.saved_state.save_result_to_file;
                switch_row(
                    ui,
                    tr("Auto-export"),
                    Some(secondary),
                    &mut self.saved_state.save_result_to_file,
                    true,
                )
                .on_hover_text(
                    self.saved_state
                        .save_result_folder
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| tr("Choose a folder").to_owned()),
                );
                if !was_on
                    && self.saved_state.save_result_to_file
                    && self.saved_state.save_result_folder.is_none()
                {
                    self.pick_export_folder();
                }
            },
        )
        .response;
        if open_folder && let Some(folder) = folder {
            open_folder_in_explorer(folder);
        }
        match more_action {
            Some(MoreExport::Achievements(url)) => self.export_achievements(app_state, url),
            Some(MoreExport::Settings) => self.open_settings(Page::Export),
            None => {}
        }
        response
    }

    /// The Auto-export row's second line.
    fn auto_export_line(&self, pal: &Palette) -> RichText {
        if !self.saved_state.save_result_to_file {
            return secondary_text(tr("Off · choose a folder"), pal.tx2);
        }
        if let Some(event) = self.activity.latest_of(crate::activity::Topic::Export) {
            let time = event.at.format(crate::clock::hm());
            return match (&event.kind, &event.path) {
                (crate::activity::Kind::Ok, Some(path)) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default();
                    secondary_text(trf("✓ Saved {0} · {1}", &[&time, &name]), pal.ok)
                }
                (crate::activity::Kind::Error, _) => {
                    secondary_text(trf("Failed {0} · {1}", &[&time, &event.text]), pal.bad)
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
            None => secondary_text(tr("Next to Teyvat.me"), pal.tx2),
        }
    }

    /// The Tracker row across the window's foot (spec §4.5): one slim card,
    /// a row per destination (teyvat.me today).
    fn tracker_row(&mut self, ui: &mut Ui) -> Response {
        let pal = widgets::p(ui);
        let status = self.auth_status_rx.borrow().clone();
        let host = self.tracker_host();
        let key_linked = self.saved_state.tracker_sign_in == TrackerSignIn::SignedOut
            && self.tracker_account.is_some();
        let mut action = None;
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
                tr("Waiting for the browser…").to_owned(),
                pal.tx2,
            ),
            _ if key_linked => {
                let name = self
                    .tracker_account
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                let line = self.destination_result_line();
                (
                    Avatar::Key,
                    format!("{host} ↗ · {name}"),
                    line.0,
                    if line.1 { pal.bad } else { pal.tx2 },
                )
            }
            _ => (
                Avatar::Cloud,
                format!("{host} ↗"),
                tr("Upload each capture · live sync").to_owned(),
                pal.tx2,
            ),
        };
        let connected = matches!(status, AuthStatus::SignedIn { .. }) || key_linked;
        let response = widgets::card_frame(ui)
            .inner_margin(Margin::symmetric(16, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 44.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        avatar.show(ui, &pal);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            match &status {
                                AuthStatus::Waiting { .. } => {
                                    if button(ui, ButtonKind::Ghost, None, tr("Cancel"), false)
                                        .clicked()
                                    {
                                        action = Some(DestAction::Cancel);
                                    }
                                }
                                _ => {
                                    let gear = trf("{0} settings", &[&host]);
                                    if icon_button(ui, icons::settings(), &gear).clicked() {
                                        action = Some(DestAction::Page);
                                    }
                                    if connected {
                                        let upload = trf("Upload each capture to {0}", &[&host]);
                                        widgets::switch(
                                            ui,
                                            &mut self.saved_state.auto_export_to_tracker,
                                        )
                                        .on_hover_text(&upload);
                                        ui.label(small_text(tr("Tracker"), pal.tx3))
                                            .on_hover_text(&upload);
                                    } else if button(
                                        ui,
                                        ButtonKind::Secondary,
                                        Some(icons::log_in()),
                                        tr("Connect"),
                                        false,
                                    )
                                    .on_hover_text(trf("Sign in to {0} in your browser", &[&host]))
                                    .clicked()
                                    {
                                        action = Some(DestAction::Connect);
                                    }
                                }
                            }
                            ui.add_space(6.0);
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 1.0;
                                    let name = ui.add(
                                        egui::Label::new(title_text(&title, pal.tx))
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                    );
                                    if name
                                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                                        .on_hover_text(trf("Open {0}", &[&host]))
                                        .clicked()
                                    {
                                        action = Some(DestAction::OpenSite);
                                    }
                                    line(ui, secondary_text(&secondary, color));
                                });
                            });
                        });
                    },
                );
            })
            .response;
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
        response
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
            return (tr("Uploads: Not chosen").to_owned(), true);
        }
        match self.activity.latest_of(crate::activity::Topic::Destination) {
            Some(event) => {
                let mark = match event.kind {
                    crate::activity::Kind::Ok => "✓ ",
                    _ => "",
                };
                (
                    format!(
                        "{mark}{} · {}",
                        event.text,
                        event.at.format(crate::clock::hm())
                    ),
                    event.kind == crate::activity::Kind::Error,
                )
            }
            None if self.saved_state.auto_export_to_tracker => {
                (tr("Uploads each capture").to_owned(), false)
            }
            None => (tr("Auto upload off").to_owned(), false),
        }
    }

    pub(super) fn ask_where_to_save(&mut self) {
        let now = Local::now();
        self.file_request = Some(super::FileRequest::SaveGood {
            name: teyvatme_core::export_file_name(now.format("%Y-%m-%d_%H-%M")),
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
                    .set_title(trf("Save {0} JSON", &[&FORMAT_NAME]))
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
            super::FileRequest::SaveWishes { name } => {
                let mut dialog = rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(tr("Save wish history"))
                    .add_filter("UIGF JSON", &["json"])
                    .set_file_name(name);
                if let Some(folder) = self.export_folder() {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(mut path) = dialog.save_file() {
                    if path.extension().is_none() {
                        path.set_extension("json");
                    }
                    match self.wishes.key(Instant::now()) {
                        Some(key) => self.wishes.save.start(key, path, ctx.clone()),
                        None => {
                            #[rustfmt::skip]
                            let expired = tr("The wish key expired · open the wish history in game again");
                            self.toasts.error(expired);
                        }
                    }
                }
            }
            super::FileRequest::ExportFolder => {
                let mut dialog = rfd::FileDialog::new()
                    .set_parent(window)
                    .set_title(tr("Auto-export folder"));
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
            #[rustfmt::skip]
            let none_yet = tr("No achievements yet · they arrive as you enter the world");
            let toast = self.toasts.error(none_yet);
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

/// The status strip's title, also the tray icon's tooltip.
pub(super) fn strip_title(is_admin: bool, status: GameStatus, done: bool) -> &'static str {
    if !is_admin {
        return tr("Capture needs administrator");
    }
    match status {
        GameStatus::NotRunning => tr("Waiting for the game"),
        GameStatus::AwaitingLogin | GameStatus::LaunchCaptured => tr("Game detected"),
        GameStatus::Decoding if done => tr("Capture done"),
        GameStatus::Decoding => tr("Data decrypting"),
        GameStatus::CaptureOff => tr("Starting capture"),
        GameStatus::LaunchMissed(_) => tr("Launch missed"),
        GameStatus::KeyLost => tr("Login not read"),
    }
}

/// The Save button's tooltip: the file is GOOD, so any GOOD reader loads it.
#[rustfmt::skip]
fn good_json_hint() -> String {
    trf("{0} JSON, read by Genshin Optimizer, teyvat.me and any GOOD reader", &[&FORMAT_NAME])
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
        (true, 6) => Some(tr("America")),
        (true, 7) => Some(tr("Europe")),
        (true, 8) => Some(tr("Asia")),
        (true, 9) => Some("TW · HK · MO"),
        (true, 1 | 2 | 3 | 5) => Some(tr("China")),
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
