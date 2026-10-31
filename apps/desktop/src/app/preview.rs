// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Debug builds only: `TEYVATME_PREVIEW=<state>` draws a made-up state, for
//! screenshots of every screen without a game session. Placeholder data only
//! (UID 812345678). `TEYVATME_PREVIEW_THEME=light|dark` picks the theme,
//! `TEYVATME_PREVIEW_UPDATE` the updater's state. `full` is `decoding` with a
//! wish key, teyvat.me signed in (as `player01`) and auto-export on, as
//! board 12 draws it; `done` is `full` once the login's data has settled.
//! `TEYVATME_PREVIEW_SHOT=<file.png>` saves the window's own pixels there
//! once it has settled, then closes it; `TEYVATME_PREVIEW_OPEN=language`
//! clicks the language picker 2 s in (an input event inside the app, not the
//! system's mouse); `TEYVATME_PREVIEW_TRAY=1` goes to the tray 3 s in and
//! comes back 12 s in, through the tray's own code.

use std::time::{Duration, Instant};

use super::{Onboarding, Page, TeyvatmeApp, ThemeChoice, View};
use crate::AppState;
use crate::game_watch::{GameStatus, MissedLaunch};
use crate::player_data::ExportCounts;

impl TeyvatmeApp {
    /// `TEYVATME_PREVIEW_TRAY`: into the tray at 3 s, back at 12 s.
    pub(super) fn preview_tray(&mut self, ctx: &egui::Context) {
        static STEP: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        if std::env::var_os("TEYVATME_PREVIEW_TRAY").is_none() {
            return;
        }
        let at = self.started_at.elapsed();
        let step = STEP.load(std::sync::atomic::Ordering::Relaxed);
        if step == 0 && at > Duration::from_secs(3) {
            STEP.store(1, std::sync::atomic::Ordering::Relaxed);
            tracing::debug!("preview: into the tray");
            self.minimize(ctx, true);
        } else if step == 1 && at > Duration::from_secs(12) {
            STEP.store(2, std::sync::atomic::Ordering::Relaxed);
            tracing::debug!("preview: back from the tray");
            super::restore_window(ctx);
        }
        if step < 2 {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    /// `TEYVATME_PREVIEW_OPEN=language`: one click on the picker, 2 s in.
    pub(super) fn preview_click(&mut self, raw_input: &mut egui::RawInput) {
        static CLICKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if std::env::var("TEYVATME_PREVIEW_OPEN").as_deref() != Ok("language")
            || self.started_at.elapsed() < Duration::from_secs(2)
        {
            return;
        }
        let Some(pos) = self.preview_click_at else {
            return;
        };
        if CLICKED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        raw_input
            .events
            .extend([egui::Event::PointerMoved(pos), button(true), button(false)]);
    }

    /// `TEYVATME_PREVIEW_SHOT`: ask for the window's pixels 4 s in, save
    /// them, close.
    pub(super) fn preview_shot(&mut self, ctx: &egui::Context) {
        let Ok(path) = std::env::var("TEYVATME_PREVIEW_SHOT") else {
            return;
        };
        let shot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            let [width, height] = image.size;
            let bytes = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(png) = image::RgbaImage::from_raw(width as u32, height as u32, bytes)
                && let Err(e) = png.save(&path)
            {
                tracing::warn!("preview shot not saved: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if self.started_at.elapsed() > Duration::from_secs(4)
            && !ASKED.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint_after(Duration::from_millis(250));
    }

    /// The state to draw: `real`, or the preview's.
    pub(super) fn preview(&mut self, real: AppState) -> AppState {
        let Ok(name) = std::env::var("TEYVATME_PREVIEW") else {
            return real;
        };
        let mut state = real;
        // `TEYVATME_PREVIEW_UPDATE=available|ready|installing|uptodate` stands in
        // for the updater's answer.
        if let Ok(update) = std::env::var("TEYVATME_PREVIEW_UPDATE") {
            use crate::update::UpdateStatus;
            let next = "1.9.9".to_string();
            let status = match update.as_str() {
                "available" => UpdateStatus::Available(next),
                "ready" => UpdateStatus::Ready(next),
                "installing" => UpdateStatus::Installing(next),
                "manual" => UpdateStatus::Manual(next),
                "failed" => UpdateStatus::Failed("connection timed out".to_string()),
                _ => UpdateStatus::UpToDate,
            };
            if *self.update_status.borrow() != status {
                // A receiver keeps its last value once the sender is gone.
                self.update_status = tokio::sync::watch::channel(status).1;
            }
        }
        state.state = crate::State::Main;
        state.capturing = true;
        self.is_admin = true;
        self.saved_state.onboarded = true;
        match std::env::var("TEYVATME_PREVIEW_THEME").as_deref() {
            Ok("dark") => self.saved_state.theme = ThemeChoice::Dark,
            Ok("light") => self.saved_state.theme = ThemeChoice::Light,
            _ => {}
        }
        let captured = |state: &mut AppState| {
            state.counts = Some(ExportCounts {
                characters: 103,
                artifacts: 1_780,
                weapons: 1_122,
                materials: 1_547,
                achievements: 1_840,
            });
            state.player = Some(crate::good::GiPlayer {
                uid: Some(812_345_678),
                ar: Some(60),
                wl: Some(9),
                ..Default::default()
            });
            state.anecdotes = Some(86);
            let now = chrono::Utc::now().timestamp() as u32;
            state.tasks = Some(crate::good::GiTasks {
                resin: Some(crate::good::GiResin {
                    value: 142,
                    next_at: now + 300,
                }),
                realm: Some(crate::good::GiRealm {
                    value: 1_820,
                    limit: 2_400,
                }),
                commissions: Some(crate::good::GiCommissions {
                    finished: 4,
                    total: 4,
                    claimed: None,
                }),
                expeditions: Some(
                    [40, 95, 180, 300, 600]
                        .map(|minutes| crate::good::GiExpedition {
                            done_at: now + minutes * 60,
                            ..Default::default()
                        })
                        .to_vec(),
                ),
                weekly_bosses: Some(crate::good::GiWeeklyBosses {
                    left: 2,
                    used: Some(1),
                    limit: 3,
                    cost: 30,
                    full_cost: 60,
                }),
                abyss: Some(crate::good::GiAbyss {
                    start: now - 6 * 86_400,
                    end: now + 9 * 86_400 - 3_600,
                    stars: 27,
                    max_stars: 36,
                }),
                ..Default::default()
            });
            state.tasks_at = Some(chrono::Local::now() - chrono::TimeDelta::minutes(3));
            state.captured_at = Some(chrono::Local::now() - chrono::TimeDelta::minutes(2));
            state.updated.items_updated = Some(Instant::now() - Duration::from_secs(120));
            state.updated.characters_updated = state.updated.items_updated;
            state.updated.achievements_updated = state.updated.items_updated;
            state.updated.achievements_updated_time = state.captured_at;
        };
        match name.as_str() {
            "waiting" => state.game_status = GameStatus::NotRunning,
            "login" => state.game_status = GameStatus::LaunchCaptured,
            "starting" => {
                state.capturing = false;
                state.game_status = GameStatus::CaptureOff;
            }
            "decoding" => {
                state.game_status = GameStatus::Decoding;
                self.decoding_since = Some(chrono::Local::now() - chrono::TimeDelta::minutes(3));
                captured(&mut state);
            }
            "full" => {
                state.game_status = GameStatus::Decoding;
                self.decoding_since = Some(chrono::Local::now() - chrono::TimeDelta::minutes(3));
                captured(&mut state);
                linked_state(&mut state);
                self.preview_linked();
            }
            "done" => {
                state.game_status = GameStatus::Decoding;
                state.capture_done = true;
                self.decoding_since = Some(chrono::Local::now() - chrono::TimeDelta::minutes(3));
                captured(&mut state);
                linked_state(&mut state);
                self.preview_linked();
            }
            "toasts" => {
                state.game_status = GameStatus::Decoding;
                self.decoding_since = Some(chrono::Local::now());
                captured(&mut state);
                static RAISED: std::sync::Once = std::sync::Once::new();
                RAISED.call_once(|| {
                    self.toasts
                        .success("Saved leyline-2026-10-26_01-20.json")
                        .action(crate::ui::toast::Action::Show(std::env::temp_dir()))
                        .saved();
                    self.toasts
                        .error("teyvat.me upload failed · HTTP 503")
                        .action(crate::ui::toast::Action::RetryUpload);
                });
            }
            "missed" => state.game_status = GameStatus::LaunchMissed(MissedLaunch::AlreadyRunning),
            "keylost" => {
                state.game_status = GameStatus::KeyLost;
                captured(&mut state);
            }
            "noadmin" => {
                self.is_admin = false;
                state.capturing = false;
            }
            "welcome" => {
                self.saved_state.onboarded = false;
                self.view = View::FirstRun(Onboarding::Welcome);
            }
            "setup" => {
                self.saved_state.onboarded = false;
                self.view = View::FirstRun(Onboarding::SetUp);
            }
            page if page.starts_with("settings-") => {
                state.game_status = GameStatus::Decoding;
                captured(&mut state);
                linked_state(&mut state);
                self.view = View::Settings(match &page["settings-".len()..] {
                    "export" => Page::Export,
                    "destinations" => Page::Destinations,
                    "teyvatme" => Page::Teyvatme,
                    "diagnostics" => Page::Diagnostics,
                    "about" => Page::About,
                    _ => Page::App,
                });
            }
            _ => {}
        }
        state
    }

    /// A wish key, teyvat.me signed in and auto-export on, for `full`.
    fn preview_linked(&mut self) {
        use crate::activity::{Event, Topic};
        use crate::signin::{AuthStatus, UploadScope, Whoami};
        self.wishes
            .preview_key(Instant::now() - Duration::from_secs(60));
        if !matches!(*self.auth_status_rx.borrow(), AuthStatus::SignedIn { .. }) {
            let whoami = Whoami {
                scope: UploadScope::Uid,
                user: "player01".to_owned(),
                account_id: None,
                account_name: None,
                uid: None,
                server: None,
                dashboard_url: Some("https://teyvat.me/app".to_owned()),
                config_url: None,
            };
            self.auth_status_rx = tokio::sync::watch::channel(AuthStatus::SignedIn {
                whoami: Some(whoami),
                error: None,
            })
            .1;
            self.saved_state.tracker_sign_in = super::TrackerSignIn::SignedIn;
            self.saved_state.auto_export_to_tracker = true;
            self.saved_state.save_result_to_file = true;
            self.saved_state.save_result_folder = Some(r"D:\Captures\Teyvatme".into());
            self.activity.push(
                Event::ok(Topic::Export, "Auto-exported")
                    .with_path(r"D:\Captures\Teyvatme\leyline-2026-10-25_21-04-37.json".into()),
            );
            self.activity
                .push(Event::ok(Topic::Destination, "Live sync · 12 changes"));
        }
    }
}

/// What a linked tracker gives `full`, `done` and the settings previews:
/// this device's alerts.
fn linked_state(state: &mut AppState) {
    state.tracker_alerts = Some(teyvatme_core::link::Alerts {
        resin: Some(160),
        resin_full: true,
    });
}
