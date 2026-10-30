// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Settings (spec §5) and the teyvat.me destination page (§4.6), in the main
//! window: a side list at ≥ 720 px, tabs below. They replace the old App,
//! Tracker, Automation, Optimizer, Power tools and Bug report dialogs.

use egui::{Align, DragValue, Layout, RichText, Ui, vec2};
use teyvatme_core::FORMAT_NAME;

use super::frame::BREAKPOINT;
use super::{AddressChange, Page, TeyvatmeApp, ThemeChoice, TrackerSignIn};
use crate::TracingLevel;
use crate::clock::ClockChoice;
use crate::i18n::{Lang, tr, trf};
use crate::signin::{self, AuthStatus, TrackerRequest, UploadScope, Whoami};
use crate::ui::icons;
use crate::ui::theme::style;
use crate::ui::widgets::{
    self, ButtonKind, CalloutKind, button, callout, card_frame, divider, icon_button, line,
    list_row, overline, secondary_text, small_text, strong_text, switch_row, title_text,
};

fn pages() -> [(Page, &'static str); 5] {
    [
        (Page::App, tr("App")),
        (Page::Export, tr("Export")),
        (Page::Destinations, tr("Tracker")),
        (Page::Diagnostics, tr("Diagnostics")),
        (Page::About, tr("About")),
    ]
}

fn page_icon(page: Page) -> egui::ImageSource<'static> {
    match page {
        Page::App => icons::settings(),
        Page::Export => icons::download(),
        Page::Destinations | Page::Teyvatme => icons::cloud(),
        Page::Diagnostics => icons::bug(),
        Page::About => icons::info(),
    }
}

/// Back and the tr("Settings") title; true when Back was clicked.
fn settings_heading(ui: &mut Ui) -> bool {
    let pal = widgets::p(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let back = icon_button(ui, icons::arrow_left(), tr("Back")).clicked();
        ui.label(
            RichText::new(tr("Settings"))
                .text_style(style::brand())
                .color(pal.tx),
        );
        back
    })
    .inner
}

/// One page in the side list: icon and name, a soft fill and an accent bar
/// when selected (as the OS's own settings do), a dot for news.
fn nav_item(
    ui: &mut Ui,
    icon: egui::ImageSource<'static>,
    label: &str,
    selected: bool,
    badge: bool,
) -> egui::Response {
    let pal = widgets::p(ui);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 36.0), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    if selected || response.hovered() {
        let fill = if selected {
            pal.over
        } else {
            pal.over.gamma_multiply(0.6)
        };
        painter.rect_filled(rect, egui::CornerRadius::same(6), fill);
    }
    if selected {
        let bar = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 1.5, rect.center().y),
            vec2(3.0, 16.0),
        );
        painter.rect_filled(bar, egui::CornerRadius::same(2), pal.ac_tx);
    }
    let ink = if selected { pal.tx } else { pal.tx2 };
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 22.0, rect.center().y),
        egui::Vec2::splat(16.0),
    );
    icons::image(icon, 16.0, ink).paint_at(ui, icon_rect);
    let text = egui::WidgetText::from(title_text(label, ink)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        rect.width() - 64.0,
        egui::TextStyle::Body,
    );
    let text_pos = egui::pos2(rect.left() + 40.0, rect.center().y - text.size().y / 2.0);
    ui.painter().galley(text_pos, text, ink);
    if badge {
        ui.painter().circle_filled(
            egui::pos2(rect.right() - 14.0, rect.center().y),
            3.5,
            pal.ac_tx,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    response
}

impl TeyvatmeApp {
    pub(super) fn settings(&mut self, ui: &mut Ui, page: Page) {
        let pal = widgets::p(ui);
        let wide = ui.available_width() >= BREAKPOINT;
        let mut chosen = page;
        let mut back = false;
        if wide {
            egui::SidePanel::left("settings_nav")
                .exact_width(208.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(pal.bar).inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 8,
                    bottom: 8,
                }))
                .show_inside(ui, |ui| {
                    back = settings_heading(ui);
                    ui.add_space(6.0);
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (option, label) in pages() {
                        let selected = page == option
                            || (option == Page::Destinations && page == Page::Teyvatme);
                        let badge = option == Page::About
                            && self.update_status.borrow().pending().is_some();
                        if nav_item(ui, page_icon(option), label, selected, badge).clicked() {
                            chosen = option;
                        }
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::same(16)))
            .show_inside(ui, |ui| {
                if !wide {
                    back = settings_heading(ui);
                    ui.add_space(4.0);
                    let mut tab = if page == Page::Teyvatme {
                        Page::Destinations
                    } else {
                        page
                    };
                    egui::ScrollArea::horizontal()
                        .id_salt("settings_tabs")
                        .show(ui, |ui| {
                            widgets::segmented(ui, &mut tab, &pages());
                        });
                    if tab != page && !(tab == Page::Destinations && page == Page::Teyvatme) {
                        chosen = tab;
                    }
                    ui.add_space(8.0);
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // At most 600 px; `set_max_width` alone would also
                        // widen it past a narrower window.
                        ui.set_max_width(ui.available_width().min(600.0));
                        ui.spacing_mut().item_spacing.y = 10.0;
                        match page {
                            Page::App => self.app_page(ui),
                            Page::Export => self.export_page(ui),
                            // The tab is the tracker page itself (no list
                            // row to click through to it).
                            Page::Destinations | Page::Teyvatme => self.teyvatme_page(ui),
                            Page::Diagnostics => self.diagnostics_page(ui),
                            Page::About => self.about_page(ui),
                        }
                    });
            });
        if back {
            self.leave_settings();
        } else if chosen != page {
            // Through open_settings, which readies the tracker page.
            self.open_settings(chosen);
        }
    }

    fn app_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, tr("Startup"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.startup
                .settings_rows(ui, &mut self.saved_state.start_on_startup);
        });

        overline(ui, tr("Window"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            list_row(
                ui,
                None,
                title_text(tr("Minimize to"), pal.tx),
                Some(secondary_text(
                    if cfg!(target_os = "linux") {
                        tr("The tray needs a desktop with a system tray")
                    } else {
                        tr("Where the minimize button sends the window")
                    },
                    pal.tx2,
                )),
                |ui| {
                    // Unset (older settings, a fresh install) is the taskbar:
                    // the title bar's button can't stop to ask.
                    let mut to_tray = self.saved_state.minimize_to_tray == Some(true);
                    widgets::segmented(
                        ui,
                        &mut to_tray,
                        &[(true, tr("Tray")), (false, tr("Taskbar"))],
                    );
                    if to_tray != (self.saved_state.minimize_to_tray == Some(true)) {
                        self.saved_state.minimize_to_tray = Some(to_tray);
                    }
                },
            );
            divider(ui);
            list_row(ui, None, title_text(tr("Theme"), pal.tx), None, |ui| {
                widgets::segmented(
                    ui,
                    &mut self.saved_state.theme,
                    &[
                        (ThemeChoice::System, tr("System")),
                        (ThemeChoice::Light, tr("Light")),
                        (ThemeChoice::Dark, tr("Dark")),
                    ],
                );
            });
            divider(ui);
            let clock = self.saved_state.clock;
            list_row(ui, None, title_text(tr("Clock"), pal.tx), None, |ui| {
                // The system choice says what it is now, like Language does.
                let system = if crate::clock::system_is_twelve_hour() {
                    tr("System · 12-hour")
                } else {
                    tr("System · 24-hour")
                };
                widgets::segmented(
                    ui,
                    &mut self.saved_state.clock,
                    &[
                        (ClockChoice::System, system),
                        (ClockChoice::TwelveHour, tr("12-hour")),
                        (ClockChoice::TwentyFourHour, tr("24-hour")),
                    ],
                );
            });
            if self.saved_state.clock != clock {
                crate::clock::set(self.saved_state.clock);
            }
            divider(ui);
            let previous = self.saved_state.language;
            list_row(ui, None, title_text(tr("Language"), pal.tx), None, |ui| {
                let name = |choice: Option<Lang>| match choice {
                    Some(lang) => lang.native_name().to_owned(),
                    None => trf("System · {0}", &[&crate::i18n::system().native_name()]),
                };
                let combo = egui::ComboBox::from_id_salt("language")
                    .selected_text(name(self.saved_state.language))
                    .height(420.0)
                    .show_ui(ui, |ui| {
                        // Each name in its own script, once its font is in.
                        let installed = ui.ctx().fonts(|fonts| fonts.families());
                        ui.selectable_value(&mut self.saved_state.language, None, name(None));
                        for lang in Lang::ALL {
                            let mut text = RichText::new(name(Some(lang)));
                            if let Some(family) = crate::ui::theme::name_family(&installed, lang) {
                                text = text.family(family);
                            }
                            ui.selectable_value(&mut self.saved_state.language, Some(lang), text);
                        }
                    });
                #[cfg(debug_assertions)]
                {
                    self.preview_click_at = Some(combo.response.rect.center());
                }
                let open = combo.inner.is_some();
                if open != self.language_picker_open {
                    self.language_picker_open = open;
                    crate::ui::theme::set_picker_fonts(ui.ctx(), crate::i18n::current(), open);
                }
            });
            if self.saved_state.language != previous {
                let lang = crate::i18n::set(self.saved_state.language);
                crate::ui::theme::set_picker_fonts(ui.ctx(), lang, self.language_picker_open);
            }
        });

        // Windows only: nothing shows notifications elsewhere yet.
        if cfg!(windows) {
            overline(ui, tr("Notifications"));
            card_frame(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                switch_row(
                    ui,
                    tr("Resin alert"),
                    Some(secondary_text(
                        tr("A Windows notification when your resin reaches this"),
                        pal.tx2,
                    )),
                    &mut self.saved_state.resin_alert,
                    true,
                );
                divider(ui);
                let on = self.saved_state.resin_alert;
                list_row(ui, None, title_text(tr("Alert at"), pal.tx), None, |ui| {
                    if button(ui, ButtonKind::Ghost, None, tr("Test"), false).clicked() {
                        let title = trf("Resin {0}/{1}", &[&self.saved_state.resin_alert_at, &200]);
                        let text = tr("This is how the resin alert looks").to_owned();
                        std::thread::spawn(move || {
                            if let Err(e) = crate::notify::show(&title, &text) {
                                tracing::warn!("the test notification could not be shown: {e:#}");
                            }
                        });
                    }
                    ui.add_enabled(
                        on,
                        DragValue::new(&mut self.saved_state.resin_alert_at)
                            .range(1..=200)
                            .suffix(" / 200"),
                    );
                });
            });
        }
    }

    fn export_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, tr("Auto-export"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let was_on = self.saved_state.save_result_to_file;
            switch_row(
                ui,
                tr("Save each capture to a folder"),
                Some(secondary_text(
                    trf("{0} JSON, one file per capture", &[&FORMAT_NAME]),
                    pal.tx2,
                )),
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
                    .unwrap_or_else(|| tr("Next to Teyvat.me").to_owned());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if self.saved_state.save_result_folder.is_some()
                        && button(ui, ButtonKind::Ghost, None, tr("Clear"), false).clicked()
                    {
                        self.saved_state.save_result_folder = None;
                    }
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

        overline(ui, tr("Filters"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let settings = &mut self.saved_state.export_settings;
            switch_row(
                ui,
                tr("Characters"),
                None,
                &mut settings.include_characters,
                true,
            );
            if settings.include_characters {
                steppers(
                    ui,
                    &mut [
                        (tr("Min level"), &mut settings.min_character_level, 1, 90),
                        (tr("Min ascension"), &mut settings.min_character_ascension, 0, 6),
                        (
                            tr("Min constellation"),
                            &mut settings.min_character_constellation,
                            0,
                            6,
                        ),
                    ],
                );
            }
            divider(ui);
            switch_row(ui, tr("Artifacts"), None, &mut settings.include_artifacts, true);
            if settings.include_artifacts {
                steppers(
                    ui,
                    &mut [
                        (tr("Min level"), &mut settings.min_artifact_level, 0, 20),
                        (tr("Min rarity"), &mut settings.min_artifact_rarity, 1, 5),
                    ],
                );
            }
            divider(ui);
            switch_row(ui, tr("Weapons"), None, &mut settings.include_weapons, true);
            if settings.include_weapons {
                steppers(
                    ui,
                    &mut [
                        (tr("Min level"), &mut settings.min_weapon_level, 1, 90),
                        (tr("Min refinement"), &mut settings.min_weapon_refinement, 1, 5),
                        (tr("Min ascension"), &mut settings.min_weapon_ascension, 0, 6),
                        (tr("Min rarity"), &mut settings.min_weapon_rarity, 1, 5),
                    ],
                );
            }
            divider(ui);
            switch_row(ui, tr("Materials"), None, &mut settings.include_materials, true);
            divider(ui);
            switch_row(
                ui,
                tr("Fill the 4th line of 5★ 3-liners"),
                Some(secondary_text(tr("For Genshin Optimizer"), pal.tx2)),
                &mut settings.fake_initialize_4th_line,
                true,
            )
            .on_hover_text(
                tr("Genshin Optimizer still treats 5★ 3-liners like before 6.0, where the new stat is hidden. This activates that last line and sets the artifact to level 4, as if it had been levelled. The last line should be the unlockable 4th one."),
            );
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
                    && icon_button(ui, icons::external_link(), tr("Open dashboard")).clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
                if icon_button(ui, icons::refresh(), tr("Refresh")).clicked() {
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
                &trf("Captured UID {0} is not this account's", &[&captured]),
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
                &trf("Not connected: nothing goes to {0}", &[&host]),
            );
        }

        overline(ui, tr("Uploads"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            switch_row(
                ui,
                tr("Auto upload"),
                Some(secondary_text(
                    trf("Each capture goes to {0}", &[&host]),
                    pal.tx2,
                )),
                &mut self.saved_state.auto_export_to_tracker,
                true,
            );
            divider(ui);
            let auto = self.saved_state.auto_export_to_tracker;
            switch_row(
                ui,
                tr("Live sync"),
                Some(secondary_text(tr("Changes as you play"), pal.tx2)),
                &mut self.saved_state.live_sync,
                auto,
            )
            .on_hover_text(
                tr("Between snapshots, send what changed as you play, so the tracker stays current without logging out. Off: every change uploads the whole account."),
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
                    TrackerSignIn::Unknown => Some(tr("Still reading the sign-in")),
                    _ if !self.saved_state.tracker_import_key.is_empty() => {
                        Some(tr("Import key not verified"))
                    }
                    _ => Some(tr("Not connected to teyvat.me")),
                }
            } else if !missing.is_empty() {
                Some(tr("Nothing captured yet"))
            } else if self.optimizer_export_rx.is_some() || self.tracker_upload_rx.is_some() {
                Some(tr("An upload is running"))
            } else {
                None
            };
            let mut upload = false;
            list_row(
                ui,
                None,
                title_text(tr("Last result"), pal.tx),
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
                            tr("Upload now"),
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

        overline(ui, tr("Advanced"));
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
                    tr("Waiting for the browser…").to_owned(),
                    tr("Finish signing in on the page that opened").to_owned(),
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
                            tr("Uploads: Not chosen").to_owned()
                        } else {
                            trf("Uploads: {0}", &[&target])
                        },
                        if unset { pal.bad } else { pal.tx2 },
                    )
                }
                AuthStatus::SignedOut { .. } => match &self.tracker_account {
                    Some(account) => (
                        super::home::Avatar::Key,
                        account.name.clone(),
                        trf("UID {0} · {1}", &[&account.uid, &account.server]),
                        pal.tx2,
                    ),
                    None if !self.saved_state.tracker_import_key.is_empty()
                        && self.tracker_verify_rx.is_some() =>
                    {
                        (
                            super::home::Avatar::Waiting,
                            tr("Verifying key…").to_owned(),
                            String::new(),
                            pal.tx2,
                        )
                    }
                    None if !self.saved_state.tracker_import_key.is_empty() => (
                        super::home::Avatar::Key,
                        tr("Import key not verified").to_owned(),
                        tr("Refused or unreachable").to_owned(),
                        pal.bad,
                    ),
                    None => (
                        super::home::Avatar::Cloud,
                        tr("Not connected").to_owned(),
                        tr("You sign in in your browser").to_owned(),
                        pal.tx2,
                    ),
                },
            };
            avatar.show(ui, &pal);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                match status {
                    AuthStatus::SignedIn { whoami, error } => {
                        if icon_button(ui, icons::log_out(), tr("Sign out")).clicked() {
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
                            if button(
                                ui,
                                kind,
                                Some(icons::external_link()),
                                tr("Configure"),
                                false,
                            )
                            .on_hover_text(trf("Choose where uploads go, on {0}", &[&host]))
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
                        if button(ui, ButtonKind::Ghost, None, tr("Cancel"), false).clicked() {
                            action = Some(Account::Cancel);
                        }
                        if let Some(link) = link
                            && button(
                                ui,
                                ButtonKind::Secondary,
                                Some(icons::copy()),
                                tr("Copy link"),
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
                        let connect = button(ui, kind, Some(icons::log_in()), tr("Connect"), false);
                        let connect = if self.tracker_account.is_some() {
                            connect.on_hover_text(tr("Signing in is safer than a stored key"))
                        } else {
                            connect.on_hover_text(trf("Sign in to {0} in your browser", &[&host]))
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
                        #[rustfmt::skip]
                        let hint = tr("Where this PC's uploads go. Change it on the tracker.");
                        line(ui, secondary_text(secondary, secondary_color)).on_hover_text(hint);
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
                self.toasts.info(tr("Sign-in link copied"));
            }
            None => {}
        }
    }

    /// The tracker address and the import key.
    fn advanced(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        ui.label(title_text(tr("Tracker address"), pal.tx)).on_hover_text(
            tr("The tracker's API address. A sign-in belongs to one address: changing it signs out."),
        );
        let change =
            super::address_change(&self.saved_state.tracker_api_url, &self.tracker_url_edit);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let apply = ui.add_enabled_ui(matches!(change, AddressChange::To { .. }), |ui| {
                    button(ui, ButtonKind::Fill, None, tr("Apply"), false)
                });
                if apply.inner.clicked()
                    && let AddressChange::To { base, .. } = &change
                {
                    self.pending_address = Some(base.clone());
                }
                if icon_button(ui, icons::rotate_ccw(), tr("Default address")).clicked() {
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
            ui.label(small_text(tr("Not an http(s) address"), pal.bad));
        } else {
            ui.label(small_text(
                trf("Default {0}", &[&super::default_tracker_url()]),
                pal.tx3,
            ));
        }
        divider(ui);

        #[rustfmt::skip]
        let key_hint = tr("Generate one on the tracker's dashboard and paste it here.");
        ui.label(title_text(tr("Import key"), pal.tx))
            .on_hover_text(key_hint);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(
                    ui,
                    icons::trash(),
                    tr("Remove the stored key from this machine"),
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
            Some(small_text(tr("Verifying key…"), pal.tx2))
        } else if self.saved_state.tracker_verified {
            self.tracker_account.as_ref().map(|account| {
                small_text(
                    trf("✓ Valid: {0} (UID {1})", &[&account.name, &account.uid]),
                    pal.ok,
                )
            })
        } else if !self.saved_state.tracker_import_key.is_empty() {
            Some(small_text(tr("✗ Key invalid or unreachable"), pal.bad))
        } else {
            None
        };
        if let Some(result) = result {
            ui.label(result);
        }
        callout(
            ui,
            CalloutKind::Warn,
            tr("Plain text, never expires. Signing in is safer."),
        );
        ui.label(small_text(tr("Used only while signed out"), pal.tx3));
    }

    fn diagnostics_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        overline(ui, tr("Logs"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let previous = self.saved_state.tracing_level;
            list_row(
                ui,
                None,
                title_text(tr("Logging level"), pal.tx),
                None,
                |ui| {
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
                },
            );
            if previous != self.saved_state.tracing_level {
                self.tracing_reload_handle
                    .set_filter(self.saved_state.tracing_level.get_filter());
            }
            if self.saved_state.tracing_level == TracingLevel::VerboseTrace {
                callout(
                    ui,
                    CalloutKind::Warn,
                    tr("Writes every packet, your account included, to the log"),
                );
            }
            divider(ui);
            if switch_row(
                ui,
                tr("Log raw packets"),
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
            list_row(ui, None, title_text(tr("Log folder"), pal.tx), None, |ui| {
                open = button(
                    ui,
                    ButtonKind::Secondary,
                    Some(icons::folder_open()),
                    tr("Open"),
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

        overline(ui, tr("Report"));
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut new_issue = false;
            list_row(
                ui,
                Some((icons::bug(), pal.tx2)),
                title_text(tr("Report a bug"), pal.tx),
                Some(secondary_text(
                    tr("New issue on GitHub, attach latest.log"),
                    pal.tx2,
                )),
                |ui| {
                    new_issue = button(
                        ui,
                        ButtonKind::Secondary,
                        Some(icons::external_link()),
                        tr("New issue"),
                        false,
                    )
                    .on_hover_text(check_log_hint())
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

    /// Close and start the installed update (`main` relaunches the
    /// executable once the window is gone and the single-instance lock free).
    pub(super) fn restart_to_update(&mut self, ui: &Ui) {
        self.restart_requested
            .store(true, std::sync::atomic::Ordering::SeqCst);
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Settings › About's updates: the version running, "Check for updates",
    /// and what was found -- installed in the background while capture goes
    /// on, then one restart (or the next start) to use it.
    fn updates_card(&mut self, ui: &mut Ui) {
        use crate::update::{UpdateCommand, UpdateStatus};
        let pal = widgets::p(ui);
        let status = self.update_status.borrow().clone();
        let highlight = matches!(status, UpdateStatus::Available(_) | UpdateStatus::Ready(_));
        let (fill, ink, ink2) = if highlight {
            (pal.ac_con, pal.ac_on, pal.ac_on)
        } else {
            (pal.card, pal.tx, pal.tx2)
        };
        let line = match &status {
            UpdateStatus::Idle => trf("You have {0}", &[&env!("CARGO_PKG_VERSION")]),
            UpdateStatus::Checking => tr("Checking for updates").to_string(),
            UpdateStatus::UpToDate => {
                trf("{0} · the newest version", &[&env!("CARGO_PKG_VERSION")])
            }
            UpdateStatus::Available(v) => trf(
                "{0} is out · you have {1}",
                &[v, &env!("CARGO_PKG_VERSION")],
            ),
            UpdateStatus::Manual(v) => trf("{0} is out · download it on this platform", &[v]),
            UpdateStatus::Installing(v) => trf("Installing {0} · capture keeps running", &[v]),
            UpdateStatus::Ready(v) => trf(
                "{0} installed · restart to use it, or it starts next time",
                &[v],
            ),
            UpdateStatus::Failed(e) => trf("Couldn't update: {0}", &[e]),
        };
        let mut command = None;
        let mut restart = false;
        let mut open_releases = false;
        egui::Frame::new()
            .fill(fill)
            .stroke(if highlight {
                egui::Stroke::NONE
            } else {
                egui::Stroke::new(1.0_f32, pal.line)
            })
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                list_row(
                    ui,
                    Some((icons::download(), ink)),
                    strong_text(tr("Updates"), ink),
                    Some(secondary_text(line, ink2)),
                    |ui| match &status {
                        UpdateStatus::Checking | UpdateStatus::Installing(_) => {
                            ui.add(egui::Spinner::new().size(18.0).color(ink));
                        }
                        UpdateStatus::Available(_) => {
                            if button(
                                ui,
                                ButtonKind::Fill,
                                Some(icons::download()),
                                tr("Update now"),
                                false,
                            )
                            .on_hover_text(tr(
                                "Downloads and installs it while capture keeps running",
                            ))
                            .clicked()
                            {
                                command = Some(UpdateCommand::Install);
                            }
                            if button(
                                ui,
                                ButtonKind::Ghost,
                                Some(icons::external_link()),
                                tr("Release notes"),
                                false,
                            )
                            .clicked()
                            {
                                open_releases = true;
                            }
                        }
                        UpdateStatus::Ready(_) => {
                            restart = button(ui, ButtonKind::Fill, None, tr("Restart now"), false)
                                .clicked();
                        }
                        UpdateStatus::Manual(_) => {
                            open_releases = button(
                                ui,
                                ButtonKind::Fill,
                                Some(icons::external_link()),
                                tr("Download"),
                                false,
                            )
                            .clicked();
                        }
                        UpdateStatus::Idle | UpdateStatus::UpToDate | UpdateStatus::Failed(_) => {
                            if button(
                                ui,
                                ButtonKind::Secondary,
                                None,
                                tr("Check for updates"),
                                false,
                            )
                            .clicked()
                            {
                                command = Some(UpdateCommand::Check { manual: true });
                            }
                        }
                    },
                );
            });
        if let Some(command) = command
            && let Err(e) = self.update_tx.send(command)
        {
            tracing::error!("Unable to send update command: {e}");
        }
        if open_releases {
            ui.ctx().open_url(egui::OpenUrl::new_tab(
                "https://github.com/tawan475/teyvat.me-capture/releases",
            ));
        }
        if restart {
            self.restart_to_update(ui);
        }
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
                let short = &sha[..sha.len().min(8)];
                let version = trf("{0} · game data {1}", &[&env!("CARGO_PKG_VERSION"), &short]);
                ui.label(small_text(version, pal.tx3))
                    .on_hover_text(trf("Game data: dump {0}", &[&sha]));
            });
        });

        self.updates_card(ui);

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (icon, title, secondary, url) in [
                (
                    icons::github(),
                    tr("Source code"),
                    "tawan475/teyvat.me-capture",
                    "https://github.com/tawan475/teyvat.me-capture",
                ),
                (
                    icons::globe(),
                    "teyvat.me",
                    tr("The tracker website"),
                    "https://teyvat.me",
                ),
                (
                    icons::discord(),
                    "Discord",
                    tr("Help, news and feedback"),
                    crate::DISCORD_URL,
                ),
                (
                    icons::heart(),
                    tr("Thanks and licences"),
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
                    .interact(row.0.rect, row.0.id.with(url), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });
        ui.label(small_text(
            tr("Unofficial fan project, not affiliated with HoYoverse."),
            pal.tx3,
        ));
    }
}

/// The New issue button's tooltip.
#[rustfmt::skip]
fn check_log_hint() -> &'static str {
    tr("Check the log first: at Verbose trace it holds your account's game data")
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
        TracingLevel::Default => tr("Default"),
        TracingLevel::VerboseInfo => tr("Verbose info"),
        TracingLevel::VerboseDebug => tr("Verbose debug"),
        TracingLevel::VerboseTrace => tr("Verbose trace"),
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
    signin::host_of(url).unwrap_or_else(|| tr("the tracker").to_owned())
}
