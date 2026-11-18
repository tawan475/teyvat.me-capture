// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Settings › Sharing: which parts of the export go to the tracker
//! (`teyvatme_core::sharing`). Three presets, a summary of what the tracker
//! receives, and one switch per part in folded groups. A change applies at
//! once; the monitor starts a new live journal for it once the switches
//! were left alone (`LiveSync::note_sharing_changed`).

use egui::{Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use teyvatme_core::sharing::{Part, PartsOff};

use super::{TeyvatmeApp, TrackerSignIn};
use crate::i18n::{tr, trf};
use crate::ui::icons;
use crate::ui::theme::{style, weight};
use crate::ui::widgets::{
    self, CalloutKind, callout, card_frame, divider, secondary_text, small_text, strong_text,
    switch_row,
};

/// A part's name on the page.
pub(super) fn part_name(part: Part) -> &'static str {
    match part {
        Part::Characters => tr("Characters"),
        Part::Weapons => tr("Weapons"),
        Part::Artifacts => tr("Artifacts"),
        Part::Materials => tr("Materials"),
        Part::Profile => tr("Profile"),
        Part::Numbers => tr("Account numbers"),
        Part::Achievements => tr("Achievements"),
        Part::Anecdotes => tr("Anecdotes"),
        Part::Tasks => tr("Planner tasks"),
        Part::Endgame => tr("Endgame"),
        Part::Exploration => tr("Exploration"),
        Part::Collections => tr("Collections"),
        Part::Wardrobe => tr("Wardrobe"),
        Part::Teapot => tr("Serenitea Pot"),
        Part::Events => tr("Events"),
        Part::Miliastra => tr("Miliastra Wonderland"),
    }
}

/// What a part holds, in a line.
fn part_holds(part: Part) -> &'static str {
    match part {
        Part::Characters => tr("Level, ascension, constellation, talents"),
        Part::Weapons => tr("Level, ascension, refinement, who holds it"),
        Part::Artifacts => tr("Every piece: its set, stats, level, who wears it"),
        Part::Materials => tr("The bag's counts"),
        Part::Profile => tr("Nickname, signature, birthday, name card, picture"),
        Part::Numbers => tr("UID, AR, World Level, resin, story keys, max stamina"),
        Part::Achievements => tr("Which are done, when, and how far the rest are"),
        Part::Anecdotes => tr("Travel Accounts: each anecdote and the stamps"),
        Part::Tasks => tr("Commissions, expeditions, weekly bosses, timers"),
        Part::Endgame => tr("Theater, Stygian Onslaught and Abyss records"),
        Part::Exploration => tr("Statues, offerings, exploration, reputation"),
        Part::Collections => tr("Archive, quests, recipes, cards, music, furnishings"),
        Part::Wardrobe => tr("Outfits, gliders, saved teams"),
        Part::Teapot => tr("Comfort of each realm, realm currency"),
        Part::Events => tr("Events running at login and their progress"),
        Part::Miliastra => tr("Cosmetics owned"),
    }
}

/// The page's groups, in order.
const GROUPS: [&[Part]; 4] = [
    &Part::BUILDS,
    &[Part::Profile, Part::Numbers],
    &[
        Part::Achievements,
        Part::Anecdotes,
        Part::Tasks,
        Part::Endgame,
        Part::Exploration,
        Part::Collections,
    ],
    &[Part::Wardrobe, Part::Teapot, Part::Events, Part::Miliastra],
];

fn group_name(index: usize) -> &'static str {
    match index {
        0 => tr("Builds · Genshin Optimizer data"),
        1 => tr("Account"),
        2 => tr("Progress"),
        _ => tr("Home and extras"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preset {
    Everything,
    Builds,
    Custom,
}

/// The preset a set of switches is: one of the two fixed ones when it
/// equals theirs (unless Custom was picked), else Custom.
fn preset_of(off: &PartsOff, custom_picked: bool) -> Preset {
    if custom_picked {
        Preset::Custom
    } else if off.all_shared() {
        Preset::Everything
    } else if *off == PartsOff::builds_only() {
        Preset::Builds
    } else {
        Preset::Custom
    }
}

/// The names of the parts that are off, comma separated.
fn off_names(off: &PartsOff) -> String {
    off.off().map(part_name).collect::<Vec<_>>().join(", ")
}

impl TeyvatmeApp {
    /// Whether uploads are placed by UID now (the UID then always goes).
    fn sharing_routes_by_uid(&self) -> bool {
        super::routes_by_uid(&self.saved_state, &self.auth_status_rx.borrow())
    }

    /// The Tracker page's Sharing row: its line (the row cuts it short when
    /// it is long) and the same again for the hover.
    pub(super) fn sharing_summary_line(&self) -> (String, String) {
        let off = &self.saved_state.sharing_off;
        let total = Part::ALL.len();
        if off.all_shared() {
            let line = trf("Everything · all {0} parts", &[&total]);
            return (line.clone(), line);
        }
        let full = trf(
            "{0} of {1} parts · {2} not shared",
            &[&off.shared_count(), &total, &off_names(off)],
        );
        (full.clone(), full)
    }

    /// A switch changed (or a preset was picked): remember when, for the
    /// note. The monitor reads the saved state and does the rest.
    fn sharing_changed(&mut self) {
        self.sharing_changed_at = Some(chrono::Local::now());
    }

    pub(super) fn sharing_page(&mut self, ui: &mut Ui) {
        let pal = widgets::p(ui);
        let total = Part::ALL.len();
        let can_upload = super::can_upload_to_tracker(&self.saved_state);
        let routes_by_uid = self.sharing_routes_by_uid();

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(
                RichText::new(tr("Sharing"))
                    .text_style(style::about())
                    .color(pal.tx),
            );
            ui.add(
                egui::Label::new(secondary_text(
                    tr("What this PC sends to teyvat.me. Files saved to a folder follow Export."),
                    pal.tx2,
                ))
                .wrap(),
            );
        });

        // Nothing leaves now: the switches still work, and say from when.
        if !can_upload {
            let text = match self.saved_state.tracker_sign_in {
                TrackerSignIn::Unknown => None,
                _ => Some(tr(
                    "Not signed in: nothing leaves this PC. These choices apply from the first upload.",
                )),
            };
            if let Some(text) = text {
                callout(ui, CalloutKind::Info, text);
            }
        } else if !self.saved_state.auto_export_to_tracker {
            callout(
                ui,
                CalloutKind::Info,
                tr("Auto upload is off: these choices apply to Upload now."),
            );
        }

        // Presets: one click for most people.
        let shown = preset_of(&self.saved_state.sharing_off, self.sharing_custom_picked);
        let mut picked = shown;
        widgets::segmented(
            ui,
            &mut picked,
            &[
                (Preset::Everything, tr("Everything")),
                (Preset::Builds, tr("Builds only")),
                (Preset::Custom, tr("Custom")),
            ],
        );
        if picked != shown {
            // Custom keeps its own set while a fixed preset is chosen.
            if shown == Preset::Custom {
                self.saved_state.sharing_custom = Some(self.saved_state.sharing_off.clone());
            }
            let before = self.saved_state.sharing_off.clone();
            match picked {
                Preset::Everything => self.saved_state.sharing_off = PartsOff::default(),
                Preset::Builds => self.saved_state.sharing_off = PartsOff::builds_only(),
                Preset::Custom => {
                    if let Some(custom) = self.saved_state.sharing_custom.clone() {
                        self.saved_state.sharing_off = custom;
                    }
                }
            }
            self.sharing_custom_picked = picked == Preset::Custom;
            if self.saved_state.sharing_off != before {
                self.sharing_changed();
            }
        }

        // What the tracker receives: the count and the parts left out, the
        // same list every snapshot carries.
        let off = self.saved_state.sharing_off.clone();
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(secondary_text(
                if can_upload {
                    tr("teyvat.me receives")
                } else {
                    tr("teyvat.me will receive")
                },
                pal.tx2,
            ));
            ui.label(
                RichText::new(if off.all_shared() {
                    trf("All {0} parts", &[&total])
                } else {
                    trf("{0} of {1} parts", &[&off.shared_count(), &total])
                })
                .text_style(style::about())
                .family(weight::semibold())
                .color(pal.tx),
            );
            if !off.all_shared() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for part in off.off() {
                        // The UID still goes while uploads are placed by it.
                        let text = if part == Part::Numbers && can_upload && routes_by_uid {
                            trf("{0} · UID kept", &[&part_name(part)])
                        } else {
                            part_name(part).to_string()
                        };
                        off_chip(ui, &text);
                    }
                });
            }
            ui.add(
                egui::Label::new(small_text(
                    if off.all_shared() {
                        tr("teyvat.me is told with every snapshot that nothing is left out.")
                    } else {
                        tr(
                            "Not shared, and teyvat.me is told so with every snapshot: its pages say “Not shared by Capture”.",
                        )
                    },
                    pal.tx3,
                ))
                .wrap(),
            );
        });

        if let Some(at) = self.sharing_changed_at {
            let time = at.format(crate::clock::hm()).to_string();
            callout(
                ui,
                CalloutKind::Info,
                &trf(
                    "Changed {0} · the next upload is a full snapshot and starts a new live journal",
                    &[&time],
                ),
            );
        }

        for (index, parts) in GROUPS.iter().enumerate() {
            self.sharing_group(ui, index, parts, can_upload && routes_by_uid);
        }
    }

    /// One folded group: its name, how many of its parts are shared (and
    /// which are not), and its switches when open.
    fn sharing_group(&mut self, ui: &mut Ui, index: usize, parts: &[Part], uid_kept: bool) {
        let pal = widgets::p(ui);
        let bit = 1u8 << index;
        let open = self.saved_state.sharing_open & bit != 0;
        let off: Vec<Part> = parts
            .iter()
            .copied()
            .filter(|part| self.saved_state.sharing_off.is_off(*part))
            .collect();
        let mut line = trf(
            "{0} of {1} shared",
            &[&(parts.len() - off.len()), &parts.len()],
        );
        if !off.is_empty() {
            let names = off
                .iter()
                .map(|p| part_name(*p))
                .collect::<Vec<_>>()
                .join(", ");
            line = trf("{0} · {1} not shared", &[&line, &names]);
        }
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            // The header is the fold's button: the whole row.
            let header = ui
                .scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.set_min_height(40.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(icons::image(
                                if open {
                                    icons::chevron_down()
                                } else {
                                    icons::chevron_right()
                                },
                                18.0,
                                pal.tx2,
                            ));
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add_space(2.0);
                                widgets::line(ui, strong_text(group_name(index), pal.tx));
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    if !off.is_empty() {
                                        ui.add(icons::image(icons::cloud_off(), 14.0, pal.tx));
                                    }
                                    // Bold when something is left out: told
                                    // by the icon and the weight, not colour.
                                    let text = if off.is_empty() {
                                        secondary_text(&line, pal.tx2)
                                    } else {
                                        secondary_text(&line, pal.tx).family(weight::semibold())
                                    };
                                    widgets::line(ui, text).on_hover_text(&line);
                                });
                            });
                        });
                    });
                })
                .response
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            header.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::CollapsingHeader,
                    true,
                    open,
                    group_name(index),
                )
            });
            if header.clicked() {
                self.saved_state.sharing_open ^= bit;
            }
            if !open {
                return;
            }
            for part in parts {
                divider(ui);
                let mut shared = self.saved_state.sharing_off.is_shared(*part);
                let was = shared;
                switch_row(
                    ui,
                    part_name(*part),
                    Some(secondary_text(part_holds(*part), pal.tx2)),
                    &mut shared,
                    true,
                );
                if !shared {
                    let (text, lock) = match (*part, uid_kept) {
                        (Part::Numbers, true) => (
                            tr("Not shared, except the UID"),
                            Some(tr(
                                "Uploads go to the account by UID, so the UID always goes",
                            )),
                        ),
                        (Part::Numbers, false) => (
                            tr("Not shared, the UID included"),
                            Some(tr(
                                "teyvat.me can't warn when a capture's UID isn't this account's",
                            )),
                        ),
                        _ => (tr("Not shared"), None),
                    };
                    note_line(ui, icons::cloud_off(), text, true);
                    if let Some(lock) = lock {
                        let icon = if uid_kept {
                            icons::lock()
                        } else {
                            icons::info()
                        };
                        note_line(ui, icon, lock, false);
                    }
                }
                if shared != was {
                    self.saved_state.sharing_off.set_shared(*part, shared);
                    // Touching a switch is the Custom preset.
                    self.sharing_custom_picked = true;
                    self.saved_state.sharing_custom = Some(self.saved_state.sharing_off.clone());
                    self.sharing_changed();
                }
            }
        });
    }
}

/// A line under a switch row: an icon and a short text.
fn note_line(ui: &mut Ui, icon: egui::ImageSource<'static>, text: &str, strong: bool) {
    let pal = widgets::p(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add(icons::image(
            icon,
            14.0,
            if strong { pal.tx } else { pal.tx2 },
        ));
        let text = if strong {
            small_text(text, pal.tx).family(weight::semibold())
        } else {
            small_text(text, pal.tx2)
        };
        ui.add(egui::Label::new(text).wrap());
    });
}

/// A part that is left out, as a chip: the cloud-off icon and its name.
fn off_chip(ui: &mut Ui, text: &str) {
    let pal = widgets::p(ui);
    egui::Frame::new()
        .stroke(Stroke::new(1.0_f32, pal.tx3))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add(icons::image(icons::cloud_off(), 13.0, pal.tx));
                ui.label(small_text(text, pal.tx).family(weight::medium()));
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_groups_hold_every_part_once_in_the_contracts_order() {
        let listed: Vec<Part> = GROUPS
            .iter()
            .flat_map(|group| group.iter().copied())
            .collect();
        assert_eq!(listed, Part::ALL);
    }

    #[test]
    fn a_set_of_switches_reads_as_its_preset() {
        let mut off = PartsOff::default();
        assert_eq!(preset_of(&off, false), Preset::Everything);
        assert_eq!(preset_of(&PartsOff::builds_only(), false), Preset::Builds);
        off.set_shared(Part::Teapot, false);
        assert_eq!(preset_of(&off, false), Preset::Custom);
        // Custom stays picked while its set happens to equal a preset's.
        assert_eq!(preset_of(&PartsOff::default(), true), Preset::Custom);
    }
}
