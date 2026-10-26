// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! One frame: collect what the background answered, then draw the view.
//!
//! Cheap when idle: nothing here asks for a repaint by itself. The window
//! redraws when the background reports something (it calls
//! `request_repaint`), on input, and while something on screen animates
//! (a spinner, a flashing tile, a toast's timeout).

use std::sync::atomic::Ordering;

use egui::{Context, Frame};

use super::{HIDDEN_TO_TRAY, Onboarding, Page, StartWindow, TeyvatmeApp, View};
use crate::activity::{Event, Kind, Topic};
use crate::game_watch::{GameStatus, MissedLaunch};
use crate::i18n::{tr, trf};
use crate::ui::toast::Action;
use crate::{Message, State};

impl TeyvatmeApp {
    pub(super) fn run_frame(&mut self, ctx: &Context) {
        self.poll_background_results(ctx);
        #[cfg(windows)]
        self.release_settled_memory();

        // The title bar's own minimize button: on to the tray, when that is
        // where minimize goes.
        let minimized = ctx.input(|i| i.viewport().minimized) == Some(true);
        if minimized
            && !self.was_minimized
            && self.saved_state.minimize_to_tray == Some(true)
            && !HIDDEN_TO_TRAY.load(Ordering::Relaxed)
        {
            self.minimize(ctx, true);
        }
        self.was_minimized = minimized;
        let out_of_sight = HIDDEN_TO_TRAY.load(Ordering::Relaxed) || minimized;

        match std::mem::replace(&mut self.start_window, StartWindow::AsIs) {
            StartWindow::AsIs => {}
            StartWindow::Minimized => {
                let to_tray = self.saved_state.minimize_to_tray == Some(true);
                tracing::info!(to_tray, "starting minimized");
                self.minimize(ctx, to_tray);
            }
            StartWindow::Front => {
                tracing::info!("relaunched after an update: bringing the window to the front");
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }

        // Background reports always reach the log; toasts only while seen.
        self.drain_activity(!out_of_sight);

        // Nothing is drawn out of sight: eframe keeps running frames for a
        // minimized window, and anything animating would keep it at full rate.
        if out_of_sight {
            self.toasts.clear();
            let _ = self.saved_state_tx.send(self.saved_state.clone());
            return;
        }

        if self.applied_theme != Some(self.saved_state.theme) {
            ctx.set_theme(self.saved_state.theme.preference());
            // The native title bar follows the same choice.
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(
                self.saved_state.theme.system_theme(),
            ));
            self.applied_theme = Some(self.saved_state.theme);
        }

        let app_state = self.state_rx.borrow_and_update().clone();
        #[cfg(debug_assertions)]
        let app_state = self.preview(app_state);
        self.poll_game_status(&app_state);
        // Warns once per occurrence that the captured account isn't the one
        // uploads go to.
        self.note_uid_mismatch(app_state.player.as_ref().and_then(|player| player.uid));
        // Ctrl+Shift+P opens Diagnostics, as it opened Power tools.
        let diagnostics = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::P,
        );
        if ctx.input_mut(|i| i.consume_shortcut(&diagnostics)) {
            self.open_settings(Page::Diagnostics);
        }

        if matches!(app_state.state, State::Main)
            && !self.saved_state.onboarded
            && !matches!(self.view, View::FirstRun(_))
        {
            self.view = View::FirstRun(Onboarding::Welcome);
        }

        self.activity_anchor = None;

        // A new view (or settings page) fades in over a moment.
        if self.view != self.shown_view {
            self.shown_view = self.view;
            self.view_shown_at = std::time::Instant::now();
        }
        let fade = (self.view_shown_at.elapsed().as_secs_f32() / 0.16).min(1.0);
        if fade < 1.0 {
            ctx.request_repaint();
        }

        let pal = crate::ui::theme::palette(ctx);
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.bg))
            .show(ctx, |ui| {
                ui.multiply_opacity(0.35 + 0.65 * (1.0 - (1.0 - fade).powi(3)));
                self.draw_view(ui, &app_state);
            });

        self.quick_bar(ctx);
        self.activity_popover(ctx);
        self.launch_missed_dialog(ctx);
        self.address_dialog(ctx);

        // Narrow, they would cover the floating buttons: start above them.
        let inset = if self.view == View::Main && ctx.screen_rect().width() < 560.0 {
            super::frame::QUICK_BAR_HEIGHT - 8.0
        } else {
            0.0
        };
        if let Some(action) = self.toasts.show(ctx, inset) {
            self.handle_toast_action(ctx, action);
        }
        for (kind, text) in self.toasts.take_raised() {
            let topic = Topic::App;
            self.activity.push(match kind {
                Kind::Ok => Event::ok(topic, text),
                Kind::Info => Event::info(topic, text),
                Kind::Warn => Event::warn(topic, text),
                Kind::Error => Event::error(topic, text),
            });
        }

        let _ = self.saved_state_tx.send(self.saved_state.clone());
    }

    /// The view for this state: the main screen, Settings or the first run
    /// once started; the startup screens before (About and Diagnostics open
    /// from them too).
    fn draw_view(&mut self, ui: &mut egui::Ui, app_state: &crate::AppState) {
        match (&app_state.state, self.view) {
            (State::Main, View::Main) => self.home(ui, app_state),
            (State::Main, View::Settings(page)) => self.settings(ui, page),
            (State::Main, View::FirstRun(step)) => self.first_run(ui, step),
            (_, View::Settings(page)) if matches!(page, Page::About | Page::Diagnostics) => {
                self.settings(ui, page)
            }
            (state, _) => self.startup_screen(ui, state),
        }
    }

    /// Move the background's reports into the activity log, and toast the
    /// ones meant for the user. Out of sight only the newest success and the
    /// newest problem are kept for the return.
    fn drain_activity(&mut self, visible: bool) {
        while let Ok(event) = self.toast_rx.try_recv() {
            if event.toast {
                if event.kind == Kind::Error || event.kind == Kind::Warn {
                    self.held_problem = Some(event.clone());
                } else {
                    self.held_notice = Some(event.clone());
                }
            }
            self.activity.push(event);
        }
        if !visible {
            return;
        }
        for event in [self.held_notice.take(), self.held_problem.take()]
            .into_iter()
            .flatten()
        {
            let toast = self.toasts.quiet(event.kind, event.text);
            if let Some(path) = event.path {
                toast.action(Action::Show(path)).saved();
            }
        }
    }

    /// The launch-missed dialog's once-per-cause rule, the game kill's
    /// answer, and when the session started decrypting.
    fn poll_game_status(&mut self, app_state: &crate::AppState) {
        match app_state.game_status {
            GameStatus::LaunchMissed(cause @ MissedLaunch::AlreadyRunning) => {
                if self.game_missed_modal_shown_for != Some(cause) {
                    self.game_missed_modal_shown_for = Some(cause);
                    self.game_missed_modal_open = true;
                }
            }
            GameStatus::LaunchMissed(MissedLaunch::CaptureStopped) => {}
            _ => self.game_missed_modal_shown_for = None,
        }
        if app_state.game_status == GameStatus::Decoding
            && self.last_game_status != GameStatus::Decoding
        {
            self.decoding_since = Some(chrono::Local::now());
            self.toasts
                .success(tr("Session key found · reading your data"));
        }
        if app_state.game_status == GameStatus::LaunchCaptured
            && self.last_game_status == GameStatus::NotRunning
        {
            self.toasts.info(tr("Genshin detected · enter the world"));
        }
        self.last_game_status = app_state.game_status;

        if let Some(rx) = &mut self.game_kill_rx {
            match rx.try_recv() {
                Ok(0) => {
                    self.game_kill_rx = None;
                    self.toasts.info(tr("No running Genshin to close"));
                }
                Ok(killed) => {
                    self.game_kill_rx = None;
                    self.toasts.success(if killed == 1 {
                        tr("Genshin closed. Start it again to capture.").to_owned()
                    } else {
                        closed_many(killed)
                    });
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.game_kill_rx = None;
                    self.toasts.error(tr("Can't reach the capture backend"));
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
    }

    pub(super) fn kill_game(&mut self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if let Err(e) = self.ui_message_tx.send(Message::KillGame(tx)) {
            tracing::error!("Unable to send kill game message: {e}");
            self.toasts.error(tr("Can't reach the capture backend"));
        } else {
            self.game_kill_rx = Some(rx);
            self.toasts.info(tr("Closing Genshin…"));
        }
    }

    /// Relaunch elevated (`runas`), with the same arguments minus
    /// `--no-admin`, then close this copy.
    pub(super) fn restart_as_admin(&mut self, ctx: &Context) {
        #[cfg(windows)]
        {
            // `main` relaunches once the window is closed and the locks are
            // free; see `admin::ELEVATE_ON_RESTART`.
            crate::admin::ELEVATE_ON_RESTART.store(true, Ordering::SeqCst);
            self.restart_requested.store(true, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        #[cfg(not(windows))]
        let _ = ctx;
    }

    fn handle_toast_action(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::Show(path) => crate::autostart::reveal_file(&path),
            Action::OpenAnyway(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Action::RetryUpload => {
                self.genshin_optimizer_request_export(super::OptimizerExportTarget::TrackerManual)
            }
        }
    }

    pub(super) fn open_settings(&mut self, page: Page) {
        if page == Page::Teyvatme || page == Page::Destinations {
            self.tracker_url_edit = self.saved_state.tracker_api_url.clone();
            if self.saved_state.tracker_sign_in != super::TrackerSignIn::SignedIn {
                self.request_tracker_verify();
            }
        }
        self.view = View::Settings(page);
    }

    pub(super) fn leave_settings(&mut self) {
        // Leaving the destination page re-verifies an edited key.
        if self.view == View::Settings(Page::Teyvatme)
            && self.saved_state.tracker_sign_in != super::TrackerSignIn::SignedIn
            && !self.saved_state.tracker_import_key.is_empty()
            && !self.saved_state.tracker_verified
        {
            self.request_tracker_verify();
        }
        self.view = View::Main;
    }

    /// The tracker's host, as the UI names it (`teyvat.me`).
    pub(super) fn tracker_host(&self) -> String {
        crate::signin::host_of(&self.saved_state.tracker_api_url)
            .unwrap_or_else(|| tr("the tracker").to_owned())
    }

    /// The tracker's website: the API address without its `/api`.
    pub(super) fn tracker_site(&self) -> String {
        let url = self.saved_state.tracker_api_url.trim_end_matches('/');
        url.strip_suffix("/api").unwrap_or(url).to_owned()
    }

    /// The signed-in user's dashboard, or the key's account's.
    pub(super) fn dashboard_url(&self) -> Option<String> {
        match &*self.auth_status_rx.borrow() {
            crate::signin::AuthStatus::SignedIn { whoami, .. } => {
                whoami.as_ref().and_then(|w| w.dashboard_url.clone())
            }
            _ => self
                .tracker_account
                .as_ref()
                .and_then(|account| account.dashboard_url.clone()),
        }
    }
}

/// The toast after closing more than one Genshin process.
#[rustfmt::skip]
fn closed_many(killed: impl std::fmt::Display) -> String {
    trf("Closed {0} Genshin processes. Start it again to capture.", &[&killed])
}
