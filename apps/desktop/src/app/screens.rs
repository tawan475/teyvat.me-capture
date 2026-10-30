// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What comes before the main window (spec §6): the startup states and the
//! first run, and the dialogs (§7).

use egui::{Align, Context, Id, Layout, Modal, RichText, Ui};
use teyvatme_core::FORMAT_NAME;

use super::{Onboarding, TeyvatmeApp, View};
use crate::game_watch::MissedLaunch;
use crate::i18n::{tr, trf};
use crate::ui::icons;
use crate::ui::theme::style;
use crate::ui::widgets::{
    self, ButtonKind, CalloutKind, button, callout, card_frame, divider, list_row, secondary_text,
    small_text, strong_text, switch_row, title_text,
};
use crate::{ConfirmationType, Message, State};

impl TeyvatmeApp {
    /// The startup states, centred.
    pub(super) fn startup_screen(&mut self, ui: &mut Ui, state: &State) {
        let pal = widgets::p(ui);
        centred(ui, |ui| match state {
            State::Starting => {
                ui.add(egui::Spinner::new().size(28.0).color(pal.ac_tx));
                ui.label(title_text(tr("Starting"), pal.tx));
            }
            State::CheckingForData => {
                ui.add(egui::Spinner::new().size(28.0).color(pal.ac_tx));
                ui.label(title_text(tr("Checking game data"), pal.tx));
            }
            State::WaitingForDownloadConfirmation(kind) => {
                disc(ui, icons::database());
                let title = match kind {
                    ConfirmationType::Initial => tr("Game data needed"),
                    ConfirmationType::Update => tr("New game data"),
                };
                ui.label(
                    RichText::new(title)
                        .text_style(style::dialog())
                        .color(pal.tx),
                );
                ui.label(secondary_text(
                    tr("Names for characters, weapons and items"),
                    pal.tx2,
                ));
                if button(
                    ui,
                    ButtonKind::Fill,
                    Some(icons::download()),
                    tr("Download"),
                    false,
                )
                .clicked()
                    && let Err(e) = self.ui_message_tx.send(Message::DownloadAcknowledged)
                {
                    tracing::error!("Unable to send UI message: {e}");
                }
            }
            State::Downloading => {
                ui.add(egui::Spinner::new().size(28.0).color(pal.ac_tx));
                ui.label(title_text(tr("Downloading game data"), pal.tx));
            }
            State::Main => {}
        });
    }

    /// Welcome, then Set up. No account step anywhere.
    pub(super) fn first_run(&mut self, ui: &mut Ui, step: Onboarding) {
        let pal = widgets::p(ui);
        centred(ui, |ui| {
            ui.set_max_width((ui.available_width() - 32.0).min(460.0));
            match step {
                Onboarding::Welcome => {
                    widgets::star(ui, 44.0);
                    ui.label(
                        RichText::new("Teyvat.me Capture")
                            .text_style(style::display())
                            .color(pal.tx),
                    );
                    ui.label(secondary_text(
                        trf("Your Genshin account, read from the game's own traffic and saved as {0} JSON", &[&FORMAT_NAME]),
                        pal.tx2,
                    ));
                    card_frame(ui).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        list_row(
                            ui,
                            Some((icons::shield_check(), pal.tx2)),
                            title_text(tr("Runs as administrator"), pal.tx),
                            Some(secondary_text(
                                tr("Windows lets only admins read network packets"),
                                pal.tx2,
                            )),
                            |_| {},
                        )
                        .0
                        .on_hover_text(tr("Windows asks every time Teyvat.me starts"));
                        divider(ui);
                        list_row(
                            ui,
                            Some((icons::eye(), pal.tx2)),
                            title_text(tr("Read only"), pal.tx),
                            Some(secondary_text(tr("Nothing is sent to the game"), pal.tx2)),
                            |_| {},
                        );
                        divider(ui);
                        list_row(
                            ui,
                            Some((icons::power(), pal.tx2)),
                            title_text(tr("Open before the game"), pal.tx),
                            Some(secondary_text(tr("It must see Genshin's login"), pal.tx2)),
                            |_| {},
                        );
                    });
                    if button(ui, ButtonKind::Fill, None, tr("Get started"), true).clicked() {
                        self.view = View::FirstRun(Onboarding::SetUp);
                        if self.saved_state.save_result_folder.is_none() {
                            self.saved_state.save_result_folder = default_export_folder();
                        }
                        self.saved_state.save_result_to_file = true;
                    }
                    ui.label(small_text(
                        tr("Unofficial fan project, not affiliated with HoYoverse."),
                        pal.tx3,
                    ));
                }
                Onboarding::SetUp => {
                    ui.label(
                        RichText::new(tr("Before you play"))
                            .text_style(style::about())
                            .color(pal.tx),
                    );
                    card_frame(ui).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let path = self
                            .saved_state
                            .save_result_folder
                            .as_ref()
                            .map(|path| path.display().to_string());
                        let folder = path
                            .clone()
                            .unwrap_or_else(|| tr("Next to Teyvat.me").to_owned());
                        let mut where_to =
                            secondary_text(super::home::middle_ellipsis(&folder, 40), pal.tx2);
                        if path.is_some() {
                            where_to = where_to.monospace();
                        }
                        switch_row(
                            ui,
                            tr("Auto-export"),
                            Some(where_to),
                            &mut self.saved_state.save_result_to_file,
                            true,
                        )
                        .on_hover_text(folder);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(
                                    ui,
                                    ButtonKind::Secondary,
                                    Some(icons::folder_open()),
                                    tr("Choose"),
                                    false,
                                )
                                .clicked()
                                {
                                    self.pick_export_folder();
                                }
                            });
                        });
                        divider(ui);
                        self.startup
                            .settings_rows(ui, &mut self.saved_state.start_on_startup);
                    });
                    ui.horizontal(|ui| {
                        if button(ui, ButtonKind::Ghost, None, tr("Back"), false).clicked() {
                            self.view = View::FirstRun(Onboarding::Welcome);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if button(ui, ButtonKind::Fill, None, tr("Done"), true).clicked() {
                                self.saved_state.onboarded = true;
                                self.view = View::Main;
                            }
                        });
                    });
                }
            }
        });
    }

    /// "Genshin was already in the world", once per cause.
    pub(super) fn launch_missed_dialog(&mut self, ctx: &Context) {
        if !self.game_missed_modal_open {
            return;
        }
        let pal = crate::ui::theme::palette(ctx);
        let cause = self.game_missed_modal_shown_for;
        let mut close = false;
        let modal = dialog(ctx, "launch_missed").show(ctx, |ui| {
            ui.set_width(dialog_width(ctx));
            ui.horizontal(|ui| {
                ui.add(icons::image(icons::triangle_alert(), 22.0, pal.bad));
                ui.label(
                    RichText::new(match cause {
                        Some(MissedLaunch::CaptureStopped) => {
                            tr("Genshin ran while capture was off")
                        }
                        _ => tr("Genshin was already in the world"),
                    })
                    .text_style(style::dialog())
                    .color(pal.tx),
                );
            });
            ui.label(secondary_text(
                tr("Teyvat.me started after you entered the world, so this session can't be read."),
                pal.tx2,
            ));
            ui.label(strong_text(
                tr("Back to the title screen and enter again, or restart Genshin."),
                pal.tx,
            ));
            ui.horizontal(|ui| {
                let kill = ui.add_enabled_ui(self.game_kill_rx.is_none(), |ui| {
                    button(
                        ui,
                        ButtonKind::Danger,
                        Some(icons::power()),
                        tr("Close Genshin"),
                        false,
                    )
                    .on_hover_text(force_quit_hint())
                });
                if kill.inner.clicked() {
                    self.kill_game();
                    close = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, ButtonKind::Fill, None, tr("OK"), false)
                        .on_hover_text(stays_red_hint())
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
        });
        if close || modal.should_close() {
            self.game_missed_modal_open = false;
        }
    }

    /// "Change tracker?": every upload carries the whole account.
    pub(super) fn address_dialog(&mut self, ctx: &Context) {
        let Some(base) = self.pending_address.clone() else {
            return;
        };
        let pal = crate::ui::theme::palette(ctx);
        let host = crate::signin::host_of(&base).unwrap_or_else(|| base.clone());
        let mut answer = None;
        let modal = dialog(ctx, "address_check").show(ctx, |ui| {
            ui.set_width(dialog_width(ctx));
            ui.label(
                RichText::new(tr("Change tracker?"))
                    .text_style(style::dialog())
                    .color(pal.tx),
            );
            #[rustfmt::skip]
            let goes_to = trf("Uploads will go to {0}. Every upload carries the whole account.", &[&host]);
            ui.label(secondary_text(goes_to, pal.tx2));
            if self.saved_state.tracker_sign_in == super::TrackerSignIn::SignedIn {
                callout(
                    ui,
                    CalloutKind::Warn,
                    &trf("You will be signed out of {0}", &[&self.tracker_host()]),
                );
            }
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, ButtonKind::Fill, None, tr("Continue"), false).clicked() {
                        answer = Some(true);
                    }
                    if button(ui, ButtonKind::Secondary, None, tr("Cancel"), false).clicked() {
                        answer = Some(false);
                    }
                });
            });
        });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                self.pending_address = None;
                self.apply_tracker_address(base);
            }
            Some(false) => self.pending_address = None,
            None => {}
        }
    }
}

/// The Close Genshin button's tooltip.
#[rustfmt::skip]
pub(super) fn force_quit_hint() -> &'static str {
    tr("Force-quits the game. Progress is safe; a domain run in progress is lost.")
}

/// The launch-missed dialog's OK tooltip.
#[rustfmt::skip]
fn stays_red_hint() -> &'static str {
    tr("The status stays red until a session Teyvat.me watched starts")
}

/// A dialog over the scrim.
pub(super) fn dialog(ctx: &Context, id: &str) -> Modal {
    let pal = crate::ui::theme::palette(ctx);
    Modal::new(Id::new(id)).backdrop_color(pal.scrim).frame(
        egui::Frame::popup(&ctx.style())
            .fill(pal.card)
            .inner_margin(egui::Margin::same(20)),
    )
}

fn dialog_width(ctx: &Context) -> f32 {
    (ctx.screen_rect().width() - 72.0).min(380.0)
}

/// A 44 px accent disc with an icon.
fn disc(ui: &mut Ui, icon: egui::ImageSource<'static>) {
    let pal = widgets::p(ui);
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(44.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 22.0, pal.ac_con);
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(22.0));
    icons::image(icon, 22.0, pal.ac_on).paint_at(ui, icon_rect);
}

/// Content centred in the available space.
fn centred(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        let height = ui.available_height();
        ui.add_space((height * 0.18).max(16.0));
        ui.spacing_mut().item_spacing.y = 12.0;
        content(ui);
    });
}

/// `Documents\Teyvat.me`, where first run suggests auto-exports go.
fn default_export_folder() -> Option<std::path::PathBuf> {
    documents_dir().map(|dir| dir.join("Teyvat.me"))
}

/// The user's Documents folder, wherever Windows has it (OneDrive included).
#[cfg(windows)]
fn documents_dir() -> Option<std::path::PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    // SAFETY: the returned buffer is freed with CoTaskMemFree below.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(path.0 as *const _));
        text.map(std::path::PathBuf::from)
    }
}

#[cfg(not(windows))]
fn documents_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join("Documents"))
}
