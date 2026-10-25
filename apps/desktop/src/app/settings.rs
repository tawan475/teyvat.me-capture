// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Settings (spec §5) and the teyvat.me destination page (§4.6), in the main
//! window: a side list at ≥ 720 px, tabs below. They replace the old App,
//! Tracker, Automation, Optimizer, Power tools and Bug report dialogs.

use egui::{Align, DragValue, Layout, RichText, Ui, vec2};

use super::frame::BREAKPOINT;
use super::{AddressChange, Page, TeyvatmeApp, ThemeChoice, TrackerSignIn, View};
use crate::TracingLevel;
use crate::signin::{self, AuthStatus, TrackerRequest, UploadScope, Whoami};
use crate::ui::icons;
use crate::ui::theme::style;
use crate::ui::widgets::{
    self, ButtonKind, CalloutKind, button, callout, card_frame, divider, icon_button, line,
    list_row, overline, secondary_text, small_text, strong_text, switch_row, title_text,
};

const PAGES: [(Page, &str); 5] = [
    (Page::App, "App"),
    (Page::Export, "Export"),
    (Page::Destinations, "Destinations"),
    (Page::Diagnostics, "Diagnostics"),
    (Page::About, "About"),
];

impl TeyvatmeApp {
    pub(super) fn settings(&mut self, ui: &mut Ui, page: Page) {
        let pal = widgets::p(ui);
        let wide = ui.available_width() >= BREAKPOINT;
        let mut chosen = page;
        if wide {
            egui::SidePanel::left("settings_nav")
                .exact_width(200.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(pal.bar)
                        .inner_margin(egui::Margin::same(10)),
                )
                .show_inside(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (option, label) in PAGES {
                        let selected = page == option
                            || (option == Page::Destinations && page == Page::Teyvatme);
                        let text = title_text(label, if selected { pal.tx } else { pal.tx2 });
                        let mut item = egui::Button::new(text)
                            .fill(if selected {
                                pal.over
                            } else {
                                egui::Color32::TRANSPARENT
                            })
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(egui::CornerRadius::same(6))
                            .min_size(vec2(ui.available_width(), 34.0));
                        if option == Page::About && self.update_put_off.is_some() {
                            item = item.right_text(RichText::new("●").color(pal.ac_tx));
                        }
                        if ui.add(item).clicked() {
                            chosen = option;
                        }
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(16)))
            .show_inside(ui, |ui| {
                if !wide {
                    let mut tab = if page == Page::Teyvatme {
                        Page::Destinations
                    } else {
                        page
                    };
                    egui::ScrollArea::horizontal()
                        .id_salt("settings_tabs")
                        .show(ui, |ui| {
                            widgets::segmented(ui, &mut tab, &PAGES);
                        });
                    if tab != page && !(tab == Page::Destinations && page == Page::Teyvatme) {
                        chosen = tab;
                    }
                    ui.add_space(8.0);
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(600.0);
                        ui.spacing_mut().item_spacing.y = 10.0;
                        match page {
                            Page::App => self.app_page(ui),
                            Page::Export => self.export_page(ui),
                            Page::Destinations => self.destinations_page(ui),
                            Page::Teyvatme => self.teyvatme_page(ui),
                            Page::Diagnostics => self.diagnostics_page(ui),
                            Page::About => self.about_page(ui),
                        }
                    });
            });
        if chosen != page {
            self.view = View::Settings(chosen);
        }
    }

    fn app_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, "Startup");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.startup
                .settings_rows(ui, &mut self.saved_state.start_on_startup);
        });

        overline(ui, "Window");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            list_row(
                ui,
                None,
                title_text("Minimize to", pal.tx),
                Some(secondary_text(
                    if cfg!(target_os = "linux") {
                        "The tray needs a desktop with a system tray"
                    } else {
                        "Where the minimize button sends the window"
                    },
                    pal.tx2,
                )),
                |ui| {
                    // Unset (older settings, a fresh install) is the taskbar:
                    // the title bar's button can't stop to ask.
                    let mut to_tray = self.saved_state.minimize_to_tray == Some(true);
                    widgets::segmented(ui, &mut to_tray, &[(true, "Tray"), (false, "Taskbar")]);
                    if to_tray != (self.saved_state.minimize_to_tray == Some(true)) {
                        self.saved_state.minimize_to_tray = Some(to_tray);
                    }
                },
            );
            divider(ui);
            list_row(ui, None, title_text("Theme", pal.tx), None, |ui| {
                widgets::segmented(
                    ui,
                    &mut self.saved_state.theme,
                    &[
                        (ThemeChoice::System, "System"),
                        (ThemeChoice::Light, "Light"),
                        (ThemeChoice::Dark, "Dark"),
                    ],
                );
            });
        });
    }

    fn export_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, "Auto-export");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let was_on = self.saved_state.save_result_to_file;
            switch_row(
                ui,
                "Save each capture to a folder",
                Some(secondary_text("GOOD JSON, one file per capture", pal.tx2)),
                &mut self.saved_state.save_result_to_file,
                true,
            );
            if !was_on
                && self.saved_state.save_result_to_file
                && self.saved_state.save_result_folder.is_none()
            {
                self.pick_export_folder();
            }
            ui.horizontal(|ui| {
                let path = self
                    .saved_state
                    .save_result_folder
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "Next to Teyvat.me".to_owned());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.saved_state.save_result_folder.is_some()
                        && button(ui, ButtonKind::Ghost, None, "Clear", false).clicked()
                    {
                        self.saved_state.save_result_folder = None;
                    }
                    if button(
                        ui,
                        ButtonKind::Secondary,
                        Some(icons::folder_open()),
                        "Choose",
                        false,
                    )
                    .clicked()
                    {
                        self.pick_export_folder();
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let mut text = RichText::new(path.clone()).color(pal.tx2);
                        if self.saved_state.save_result_folder.is_some() {
                            text = text.monospace();
                        }
                        line(ui, text).on_hover_text(path);
                    });
                });
            });
        });

        overline(ui, "Filters").on_hover_text("Destination uploads always include everything");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let settings = &mut self.saved_state.export_settings;
            switch_row(
                ui,
                "Characters",
                None,
                &mut settings.include_characters,
                true,
            );
            if settings.include_characters {
                steppers(
                    ui,
                    &mut [
                        ("Min level", &mut settings.min_character_level, 1, 90),
                        ("Min ascension", &mut settings.min_character_ascension, 0, 6),
                        (
                            "Min constellation",
                            &mut settings.min_character_constellation,
                            0,
                            6,
                        ),
                    ],
                );
            }
            divider(ui);
            switch_row(ui, "Artifacts", None, &mut settings.include_artifacts, true);
            if settings.include_artifacts {
                steppers(
                    ui,
                    &mut [
                        ("Min level", &mut settings.min_artifact_level, 0, 20),
                        ("Min rarity", &mut settings.min_artifact_rarity, 0, 6),
                    ],
                );
            }
            divider(ui);
            switch_row(ui, "Weapons", None, &mut settings.include_weapons, true);
            if settings.include_weapons {
                steppers(
                    ui,
                    &mut [
                        ("Min level", &mut settings.min_weapon_level, 1, 90),
                        ("Min refinement", &mut settings.min_weapon_refinement, 1, 5),
                        ("Min ascension", &mut settings.min_weapon_ascension, 0, 6),
                        ("Min rarity", &mut settings.min_weapon_rarity, 1, 5),
                    ],
                );
            }
            divider(ui);
            switch_row(ui, "Materials", None, &mut settings.include_materials, true);
            divider(ui);
            switch_row(
                ui,
                "Fill the 4th line of 5★ 3-liners",
                Some(secondary_text("For Genshin Optimizer", pal.tx2)),
                &mut settings.fake_initialize_4th_line,
                true,
            )
            .on_hover_text(
                "Genshin Optimizer still treats 5★ 3-liners like before 6.0, where the new stat \
                 is hidden. This activates that last line and sets the artifact to level 4, as if \
                 it had been levelled. The last line should be the unlockable 4th one.",
            );
        });
    }

    fn destinations_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, "Destinations");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let host = self.tracker_host();
            let (line, problem) = self.destination_result_line();
            let row = list_row(
                ui,
                Some((icons::cloud(), pal.tx2)),
                title_text(&host, pal.tx),
                Some(secondary_text(
                    line,
                    if problem { pal.bad } else { pal.tx2 },
                )),
                |ui| {
                    ui.add(icons::image(icons::chevron_right(), 18.0, pal.tx3));
                },
            );
            let open = ui.interact(row.0.rect, row.0.id.with("open"), egui::Sense::click());
            if open
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.view = View::Settings(Page::Teyvatme);
            }
        });
    }

    fn teyvatme_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        let status = self.auth_status_rx.borrow().clone();
        let host = self.tracker_host();
        let signed_in = self.saved_state.tracker_sign_in == TrackerSignIn::SignedIn;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&host)
                    .text_style(style::dialog())
                    .color(pal.tx),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(url) = self.dashboard_url()
                    && icon_button(ui, icons::external_link(), "Open dashboard").clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
                if icon_button(ui, icons::refresh(), "Refresh").clicked() {
                    if signed_in {
                        self.whoami_checked_at = Some(std::time::Instant::now());
                        self.send_tracker(TrackerRequest::Refresh);
                    } else {
                        self.request_tracker_verify();
                    }
                }
            });
        });

        if let AuthStatus::SignedOut {
            notice: Some(notice),
        } = &status
        {
            callout(ui, CalloutKind::Error, notice).on_hover_text(notice);
        }

        // The account row.
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.account_row(ui, &status, &host);
        });

        let captured_uid = self
            .state_rx
            .borrow()
            .player
            .as_ref()
            .and_then(|player| player.uid);
        if let Some(account) = self.upload_account()
            && let Some(captured) = super::uid_mismatch(&account, captured_uid)
        {
            callout(
                ui,
                CalloutKind::Warn,
                &format!("Captured UID {captured} is not this account's"),
            )
            .on_hover_text(super::uid_mismatch_message(&account, captured));
        }
        if self.saved_state.auto_export_to_tracker
            && self.saved_state.tracker_sign_in != TrackerSignIn::Unknown
            && !super::want_tracker_upload(&self.saved_state)
        {
            callout(
                ui,
                CalloutKind::Warn,
                &format!("Not connected: nothing goes to {host}"),
            );
        }

        overline(ui, "Uploads");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            switch_row(
                ui,
                "Auto upload",
                Some(secondary_text(
                    format!("Each capture goes to {host}"),
                    pal.tx2,
                )),
                &mut self.saved_state.auto_export_to_tracker,
                true,
            );
            divider(ui);
            let auto = self.saved_state.auto_export_to_tracker;
            switch_row(
                ui,
                "Live sync",
                Some(secondary_text("Changes as you play", pal.tx2)),
                &mut self.saved_state.live_sync,
                auto,
            )
            .on_hover_text(
                "Between snapshots, send what changed as you play, so the tracker stays current \
                 without logging out. Off: every change uploads the whole account.",
            );
            divider(ui);
            let (line_text, problem) = self.destination_result_line();
            let missing = super::missing_export_data(
                &super::OptimizerExportTarget::TrackerManual
                    .export_settings(&self.saved_state.export_settings),
                &self.state_rx.borrow().updated,
            );
            let reason = if !super::can_upload_to_tracker(&self.saved_state) {
                match self.saved_state.tracker_sign_in {
                    TrackerSignIn::Unknown => Some("Still reading the sign-in"),
                    _ if !self.saved_state.tracker_import_key.is_empty() => {
                        Some("Import key not verified")
                    }
                    _ => Some("Not connected to teyvat.me"),
                }
            } else if !missing.is_empty() {
                Some("Nothing captured yet")
            } else if self.optimizer_export_rx.is_some() || self.tracker_upload_rx.is_some() {
                Some("An upload is running")
            } else {
                None
            };
            let mut upload = false;
            list_row(
                ui,
                None,
                title_text("Last result", pal.tx),
                Some(secondary_text(
                    line_text,
                    if problem { pal.bad } else { pal.tx2 },
                )),
                |ui| {
                    let response = ui.add_enabled_ui(reason.is_none(), |ui| {
                        button(
                            ui,
                            ButtonKind::Secondary,
                            Some(icons::cloud_upload()),
                            "Upload now",
                            false,
                        )
                        .on_disabled_hover_text(reason.unwrap_or_default())
                    });
                    upload = response.inner.clicked();
                },
            );
            if upload {
                self.genshin_optimizer_request_export(super::OptimizerExportTarget::TrackerManual);
            }
        });

        overline(ui, "Advanced");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.advanced(ui);
        });
    }

    fn account_row(&mut self, ui: &mut Ui, status: &AuthStatus, host: &str) {
        let pal = widgets::p(ui);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (avatar, title, secondary, secondary_color) = match status {
                AuthStatus::Loading => (
                    super::home::Avatar::Cloud,
                    "…".to_owned(),
                    String::new(),
                    pal.tx3,
                ),
                AuthStatus::Waiting { .. } => (
                    super::home::Avatar::Waiting,
                    "Waiting for the browser…".to_owned(),
                    "Finish signing in on the page that opened".to_owned(),
                    pal.tx2,
                ),
                AuthStatus::SignedIn { whoami, .. } => {
                    let user = whoami.as_ref().map_or("…".to_owned(), |w| w.user.clone());
                    let target = whoami
                        .as_ref()
                        .map_or_else(|| "…".to_owned(), Whoami::target_label);
                    let unset = whoami
                        .as_ref()
                        .is_some_and(|w| w.scope == UploadScope::Account && w.account_id.is_none());
                    (
                        super::home::Avatar::Initial(user.chars().next().unwrap_or('?')),
                        user,
                        if unset {
                            "Uploads: Not chosen".to_owned()
                        } else {
                            format!("Uploads: {target}")
                        },
                        if unset { pal.bad } else { pal.tx2 },
                    )
                }
                AuthStatus::SignedOut { .. } => match &self.tracker_account {
                    Some(account) => (
                        super::home::Avatar::Key,
                        account.name.clone(),
                        format!("UID {} · {}", account.uid, account.server),
                        pal.tx2,
                    ),
                    None if !self.saved_state.tracker_import_key.is_empty()
                        && self.tracker_verify_rx.is_some() =>
                    {
                        (
                            super::home::Avatar::Waiting,
                            "Verifying key…".to_owned(),
                            String::new(),
                            pal.tx2,
                        )
                    }
                    None if !self.saved_state.tracker_import_key.is_empty() => (
                        super::home::Avatar::Key,
                        "Import key not verified".to_owned(),
                        "Refused or unreachable".to_owned(),
                        pal.bad,
                    ),
                    None => (
                        super::home::Avatar::Cloud,
                        "Not connected".to_owned(),
                        "You sign in in your browser".to_owned(),
                        pal.tx2,
                    ),
                },
            };
            avatar.show(ui, &pal);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                match status {
                    AuthStatus::SignedIn { whoami, error } => {
                        if icon_button(ui, icons::log_out(), "Sign out").clicked() {
                            action = Some(Account::SignOut);
                        }
                        if let Some(url) = whoami.as_ref().and_then(|w| w.config_url.clone()) {
                            let unset = whoami.as_ref().is_some_and(|w| {
                                w.scope == UploadScope::Account && w.account_id.is_none()
                            });
                            let kind = if unset {
                                ButtonKind::Fill
                            } else {
                                ButtonKind::Secondary
                            };
                            if button(ui, kind, Some(icons::external_link()), "Configure", false)
                                .on_hover_text(format!("Choose where uploads go, on {host}"))
                                .clicked()
                            {
                                action = Some(Account::Open(url));
                            }
                        }
                        if let Some(error) = error {
                            ui.add(icons::image(icons::triangle_alert(), 16.0, pal.warn_on))
                                .on_hover_text(error);
                        }
                    }
                    AuthStatus::Waiting { link } => {
                        if button(ui, ButtonKind::Ghost, None, "Cancel", false).clicked() {
                            action = Some(Account::Cancel);
                        }
                        if let Some(link) = link
                            && button(
                                ui,
                                ButtonKind::Secondary,
                                Some(icons::copy()),
                                "Copy link",
                                false,
                            )
                            .clicked()
                        {
                            action = Some(Account::CopyLink(link.clone()));
                        }
                    }
                    AuthStatus::Loading => {}
                    AuthStatus::SignedOut { .. } => {
                        let kind = if self.tracker_account.is_some() {
                            ButtonKind::Secondary
                        } else {
                            ButtonKind::Fill
                        };
                        let connect = button(ui, kind, Some(icons::log_in()), "Connect", false);
                        let connect = if self.tracker_account.is_some() {
                            connect.on_hover_text("Signing in is safer than a stored key")
                        } else {
                            connect.on_hover_text(format!("Sign in to {host} in your browser"))
                        };
                        if connect.clicked() {
                            action = Some(Account::Connect);
                        }
                    }
                }
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    line(ui, title_text(title, pal.tx));
                    if !secondary.is_empty() {
                        line(ui, secondary_text(secondary, secondary_color))
                            .on_hover_text("Where this PC's uploads go. Change it on the tracker.");
                    }
                });
            });
        });
        match action {
            Some(Account::Connect) => self.send_tracker(TrackerRequest::SignIn),
            Some(Account::Cancel) => self.send_tracker(TrackerRequest::CancelSignIn),
            Some(Account::SignOut) => self.send_tracker(TrackerRequest::SignOut),
            Some(Account::Open(url)) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            Some(Account::CopyLink(link)) => {
                ui.ctx().copy_text(link);
                self.toasts.info("Sign-in link copied");
            }
            None => {}
        }
    }

    /// The tracker address and the import key.
    fn advanced(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        ui.label(title_text("Tracker address", pal.tx)).on_hover_text(
            "The tracker's API address. A sign-in belongs to one address: changing it signs out.",
        );
        let change =
            super::address_change(&self.saved_state.tracker_api_url, &self.tracker_url_edit);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let apply = ui.add_enabled_ui(matches!(change, AddressChange::To { .. }), |ui| {
                    button(ui, ButtonKind::Fill, None, "Apply", false)
                });
                if apply.inner.clicked()
                    && let AddressChange::To { base, .. } = &change
                {
                    self.pending_address = Some(base.clone());
                }
                if icon_button(ui, icons::rotate_ccw(), "Default address").clicked() {
                    self.tracker_url_edit = super::default_tracker_url();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.tracker_url_edit)
                        .font(egui::TextStyle::Monospace)
                        .margin(egui::Margin::symmetric(10, 8))
                        .min_size(egui::vec2(0.0, 34.0))
                        .vertical_align(Align::Center)
                        .hint_text(super::default_tracker_url())
                        .desired_width(ui.available_width()),
                );
            });
        });
        if change == AddressChange::Invalid {
            ui.label(small_text("Not an http(s) address", pal.bad));
        } else {
            ui.label(small_text(
                format!("Default {}", super::default_tracker_url()),
                pal.tx3,
            ));
        }
        divider(ui);

        ui.label(title_text("Import key", pal.tx))
            .on_hover_text("Generate one on the tracker's dashboard and paste it here.");
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(
                    ui,
                    icons::trash(),
                    "Remove the stored key from this machine",
                )
                .clicked()
                {
                    self.saved_state.tracker_import_key.clear();
                    self.tracker_account = None;
                    self.saved_state.tracker_verified = false;
                    self.tracker_verify_rx = None;
                }
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.saved_state.tracker_import_key)
                        .password(true)
                        .font(egui::TextStyle::Monospace)
                        .margin(egui::Margin::symmetric(10, 8))
                        .min_size(egui::vec2(0.0, 34.0))
                        .vertical_align(Align::Center)
                        .desired_width(ui.available_width()),
                );
                if field.lost_focus() {
                    self.request_tracker_verify();
                }
            });
        });
        let result = if self.tracker_verify_rx.is_some() {
            Some(small_text("Verifying key…", pal.tx2))
        } else if self.saved_state.tracker_verified {
            self.tracker_account.as_ref().map(|account| {
                small_text(
                    format!("✓ Valid: {} (UID {})", account.name, account.uid),
                    pal.ok,
                )
            })
        } else if !self.saved_state.tracker_import_key.is_empty() {
            Some(small_text("✗ Key invalid or unreachable", pal.bad))
        } else {
            None
        };
        if let Some(result) = result {
            ui.label(result);
        }
        callout(
            ui,
            CalloutKind::Warn,
            "Plain text, never expires. Signing in is safer.",
        );
        ui.label(small_text("Used only while signed out", pal.tx3));
    }

    fn diagnostics_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, "Logs");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let previous = self.saved_state.tracing_level;
            list_row(ui, None, title_text("Logging level", pal.tx), None, |ui| {
                egui::ComboBox::from_id_salt("logging_level")
                    .selected_text(level_name(self.saved_state.tracing_level))
                    .show_ui(ui, |ui| {
                        for level in [
                            TracingLevel::Default,
                            TracingLevel::VerboseInfo,
                            TracingLevel::VerboseDebug,
                            TracingLevel::VerboseTrace,
                        ] {
                            ui.selectable_value(
                                &mut self.saved_state.tracing_level,
                                level,
                                level_name(level),
                            );
                        }
                    });
            });
            if previous != self.saved_state.tracing_level {
                self.tracing_reload_handle
                    .set_filter(self.saved_state.tracing_level.get_filter());
            }
            if self.saved_state.tracing_level == TracingLevel::VerboseTrace {
                callout(
                    ui,
                    CalloutKind::Warn,
                    "Writes every packet, your account included, to the log",
                );
            }
            divider(ui);
            if switch_row(
                ui,
                "Log raw packets",
                None,
                &mut self.saved_state.log_raw_packets,
                true,
            )
            .changed()
            {
                let _ = self.log_packets_tx.send(self.saved_state.log_raw_packets);
            }
            divider(ui);
            let mut open = false;
            list_row(ui, None, title_text("Log folder", pal.tx), None, |ui| {
                open = button(
                    ui,
                    ButtonKind::Secondary,
                    Some(icons::folder_open()),
                    "Open",
                    false,
                )
                .clicked();
            });
            if open {
                std::thread::spawn(|| {
                    let _ = crate::open_log_dir();
                });
            }
        });

        overline(ui, "Report");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut new_issue = false;
            list_row(
                ui,
                Some((icons::bug(), pal.tx2)),
                title_text("Report a bug", pal.tx),
                Some(secondary_text(
                    "New issue on GitHub, attach latest.log",
                    pal.tx2,
                )),
                |ui| {
                    new_issue = button(
                        ui,
                        ButtonKind::Secondary,
                        Some(icons::external_link()),
                        "New issue",
                        false,
                    )
                    .on_hover_text(
                        "Check the log first: at Verbose trace it holds your account's game data",
                    )
                    .clicked();
                },
            );
            if new_issue {
                ui.ctx().open_url(egui::OpenUrl::new_tab(
                    "https://github.com/tawan475/teyvat.me-capture/issues/new",
                ));
            }
        });
    }

    fn about_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        ui.horizontal(|ui| {
            widgets::star(ui, 40.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new("Teyvat.me Capture")
                        .text_style(style::about())
                        .color(pal.tx),
                );
                let sha = crate::monitor::embedded_game_data_sha().unwrap_or("unknown");
                ui.label(small_text(
                    format!(
                        "{} · game data {}",
                        env!("CARGO_PKG_VERSION"),
                        &sha[..sha.len().min(8)]
                    ),
                    pal.tx3,
                ))
                .on_hover_text(format!("Game data: dump {sha}"));
            });
        });

        if let Some(version) = self.update_put_off.clone() {
            egui::Frame::new()
                .fill(pal.ac_con)
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut install = false;
                    list_row(
                        ui,
                        Some((icons::download(), pal.ac_on)),
                        strong_text("Update available", pal.ac_on),
                        Some(secondary_text(version, pal.ac_on)),
                        |ui| {
                            install = button(ui, ButtonKind::Fill, None, "Install", false)
                                .on_hover_text("Restarts Teyvat.me, which then installs it")
                                .clicked();
                            if button(
                                ui,
                                ButtonKind::Ghost,
                                Some(icons::external_link()),
                                "Release notes",
                                false,
                            )
                            .clicked()
                            {
                                ui.ctx().open_url(egui::OpenUrl::new_tab(
                                    "https://github.com/tawan475/teyvat.me-capture/releases",
                                ));
                            }
                        },
                    );
                    if install {
                        self.restart_requested
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
        }

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (icon, title, secondary, url) in [
                (
                    icons::github(),
                    "Source code",
                    "tawan475/teyvat.me-capture",
                    "https://github.com/tawan475/teyvat.me-capture",
                ),
                (
                    icons::globe(),
                    "teyvat.me",
                    "The tracker website, an optional destination",
                    "https://teyvat.me",
                ),
                (
                    icons::heart(),
                    "Thanks and licences",
                    "irminsul, auto-artifactarium, anime-game-data, Outfit, Lucide",
                    "https://github.com/tawan475/teyvat.me-capture/blob/main/THIRD_PARTY_NOTICES.md",
                ),
            ] {
                let row = list_row(
                    ui,
                    Some((icon, pal.tx2)),
                    title_text(title, pal.tx),
                    Some(secondary_text(secondary, pal.tx2)),
                    |ui| {
                        ui.add(icons::image(icons::external_link(), 16.0, pal.tx3));
                    },
                );
                if ui
                    .interact(row.0.rect, row.0.id.with(title), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });
        ui.label(small_text(
            "Unofficial fan project, not affiliated with HoYoverse.",
            pal.tx3,
        ));
    }
}

enum Account {
    Connect,
    Cancel,
    SignOut,
    Open(String),
    CopyLink(String),
}

fn level_name(level: TracingLevel) -> &'static str {
    match level {
        TracingLevel::Default => "Default",
        TracingLevel::VerboseInfo => "Verbose info",
        TracingLevel::VerboseDebug => "Verbose debug",
        TracingLevel::VerboseTrace => "Verbose trace",
    }
}

/// Indented steppers for a filter group.
fn steppers(ui: &mut Ui, rows: &mut [(&str, &mut u32, u32, u32)]) {
    let pal = widgets::p(ui);
    for (label, value, min, max) in rows.iter_mut() {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(small_text(*label, pal.tx2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(DragValue::new(*value).range(*min..=*max));
            });
        });
    }
}

#[allow(dead_code)]
fn host_or_default(url: &str) -> String {
    signin::host_of(url).unwrap_or_else(|| "the tracker".to_owned())
}
