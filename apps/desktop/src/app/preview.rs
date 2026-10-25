// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Debug builds only: `TEYVATME_PREVIEW=<state>` draws a made-up state, for
//! screenshots of every screen without a game session. Placeholder data only
//! (UID 812345678). `TEYVATME_PREVIEW_THEME=light|dark` picks the theme.

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
                ..Default::default()
            });
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
            "update" => state.state = crate::State::WaitingForUpdateConfirmation("1.0.1".into()),
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
}
