// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Debug builds only: `TEYVATME_PREVIEW=<state>` draws a made-up state, for
//! screenshots of every screen without a game session. Placeholder data only
//! (UID 812345678). `TEYVATME_PREVIEW_THEME=light|dark` picks the theme,
//! `TEYVATME_PREVIEW_UPDATE` the updater's state. `full` is `decoding` with a
//! wish key, teyvat.me signed in (as `player01`) and auto-export on, as
//! board 12 draws it.

use std::time::{Duration, Instant};

use super::{Onboarding, Page, TeyvatmeApp, ThemeChoice, View};
use crate::AppState;
use crate::game_watch::{GameStatus, MissedLaunch};
use crate::player_data::ExportCounts;

impl TeyvatmeApp {
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
                self.preview_linked();
            }
            "toasts" => {
                state.game_status = GameStatus::Decoding;
                self.decoding_since = Some(chrono::Local::now());
                captured(&mut state);
                static RAISED: std::sync::Once = std::sync::Once::new();
                RAISED.call_once(|| {
                    self.toasts
                        .success("Saved teyvatme_2026-10-26_01-20.json")
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
                    .with_path(r"D:\Captures\Teyvatme\teyvatme_2026-10-25_21-04.json".into()),
            );
            self.activity
                .push(Event::ok(Topic::Destination, "Live sync · 12 changes"));
        }
    }
}
