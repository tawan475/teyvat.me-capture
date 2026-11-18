// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Which parts of the export go to the tracker.
//!
//! Every upload (a snapshot, the live journal's base, every live batch) is
//! made from one export; [`Sharing::strip`] takes the parts the user
//! switched off out of that export before it is keyed, hashed or sent, so a
//! part that is off never leaves the device: not in a snapshot, and not in
//! a batch (the delta is computed from the stripped export, so no path under
//! an off part exists).
//!
//! The upload names what is off in `gi_off` (contract with the tracker,
//! agreed 2026-11-18): the [`Part::name`]s, in [`Part::ALL`]'s order, `[]`
//! when everything is shared; absent from an older build. An off GOOD
//! section's key is left out of the upload, never sent empty
//! ([`crate::good::Good::upload_json`]), so "off" is never read as "the
//! account has none". Files saved to a folder are not uploads: they never
//! carry `gi_off` and follow the Export page's own filters.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::good::Good;

/// One switch. The order is the contract's (`gi_off` lists names in it)
/// and the settings page's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Part {
    Characters,
    Weapons,
    Artifacts,
    Materials,
    /// `gi_player`: nickname, signature, birthday, name card, picture.
    Profile,
    /// `gi_player`: UID, AR, World Level, resin and the rest; with it,
    /// `gi_tasks.resin` and `gi_tasks.resinSpent`.
    Numbers,
    CharacterExtras,
    /// `gi_achievements`, `gi_achievement_times`, `gi_achievement_progress`.
    Achievements,
    Anecdotes,
    Tasks,
    Endgame,
    Exploration,
    Collections,
    Wardrobe,
    /// `gi_teapot`; with it, `gi_tasks.realm`.
    Teapot,
    Events,
    Miliastra,
}

impl Part {
    pub const ALL: [Part; 17] = [
        Part::Characters,
        Part::Weapons,
        Part::Artifacts,
        Part::Materials,
        Part::Profile,
        Part::Numbers,
        Part::CharacterExtras,
        Part::Achievements,
        Part::Anecdotes,
        Part::Tasks,
        Part::Endgame,
        Part::Exploration,
        Part::Collections,
        Part::Wardrobe,
        Part::Teapot,
        Part::Events,
        Part::Miliastra,
    ];

    /// The four Genshin Optimizer sections ("Builds only" keeps these and
    /// the account numbers).
    pub const BUILDS: [Part; 4] = [
        Part::Characters,
        Part::Weapons,
        Part::Artifacts,
        Part::Materials,
    ];

    /// The name in `gi_off` (contract: never renamed, only appended).
    pub fn name(self) -> &'static str {
        match self {
            Part::Characters => "characters",
            Part::Weapons => "weapons",
            Part::Artifacts => "artifacts",
            Part::Materials => "materials",
            Part::Profile => "gi_player.profile",
            Part::Numbers => "gi_player.numbers",
            Part::CharacterExtras => "gi_characters",
            Part::Achievements => "gi_achievements",
            Part::Anecdotes => "gi_anecdotes",
            Part::Tasks => "gi_tasks",
            Part::Endgame => "gi_endgame",
            Part::Exploration => "gi_exploration",
            Part::Collections => "gi_collections",
            Part::Wardrobe => "gi_wardrobe",
            Part::Teapot => "gi_teapot",
            Part::Events => "gi_events",
            Part::Miliastra => "gi_miliastra",
        }
    }

    pub fn from_name(name: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|part| part.name() == name)
    }
}

/// The parts switched off (the saved setting). Everything is shared until
/// something is switched off, which is what uploads always did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartsOff(BTreeSet<Part>);

impl PartsOff {
    pub fn is_off(&self, part: Part) -> bool {
        self.0.contains(&part)
    }

    pub fn is_shared(&self, part: Part) -> bool {
        !self.is_off(part)
    }

    pub fn set_shared(&mut self, part: Part, shared: bool) {
        if shared {
            self.0.remove(&part);
        } else {
            self.0.insert(part);
        }
    }

    /// How many of the 17 parts are shared.
    pub fn shared_count(&self) -> usize {
        Part::ALL.len() - self.0.len()
    }

    pub fn all_shared(&self) -> bool {
        self.0.is_empty()
    }

    /// The parts that are off, in the contract's order.
    pub fn off(&self) -> impl Iterator<Item = Part> + '_ {
        self.0.iter().copied()
    }

    /// The "Builds only" preset: the four build sections and the account
    /// numbers, nothing else.
    pub fn builds_only() -> Self {
        Self(
            Part::ALL
                .into_iter()
                .filter(|part| !Part::BUILDS.contains(part) && *part != Part::Numbers)
                .collect(),
        )
    }

    /// `gi_off`'s value.
    pub fn names(&self) -> Vec<String> {
        self.off().map(|part| part.name().to_string()).collect()
    }

    /// From `gi_off`-style names; a name this build does not know is
    /// skipped (the Android app hands its setting over this way).
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self(names.into_iter().filter_map(Part::from_name).collect())
    }
}

/// What an upload leaves out, and whether the UID must go regardless.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sharing {
    pub off: PartsOff,
    /// Uploads are routed to an account by the file's UID (signed in with
    /// "Auto (by UID)", or a user import key): the UID goes even with the
    /// account numbers off, or the upload could not be placed. With one
    /// chosen account it stays home with the rest.
    pub routes_by_uid: bool,
}

/// Everything shared, and the UID kept: what uploads did before the switches.
impl Default for Sharing {
    fn default() -> Self {
        Self {
            off: PartsOff::default(),
            routes_by_uid: true,
        }
    }
}

impl Sharing {
    /// Take the parts that are off out of `good`, and say so in `gi_off`.
    /// Only for an export on its way to the tracker.
    pub fn strip(&self, good: &mut Good) {
        let off = |part| self.off.is_off(part);
        if off(Part::Characters) {
            good.characters.clear();
        }
        if off(Part::Weapons) {
            good.weapons.clear();
        }
        if off(Part::Artifacts) {
            good.artifacts.clear();
        }
        if off(Part::Materials) {
            good.materials.clear();
        }
        if let Some(player) = &mut good.gi_player {
            if off(Part::Profile) {
                player.nickname = None;
                player.signature = None;
                player.birthday = None;
                player.name_card = None;
                player.profile_picture = None;
            }
            if off(Part::Numbers) {
                let uid = player.uid.filter(|_| self.routes_by_uid);
                let profile = std::mem::take(player);
                *player = crate::good::GiPlayer {
                    uid,
                    nickname: profile.nickname,
                    signature: profile.signature,
                    birthday: profile.birthday,
                    name_card: profile.name_card,
                    profile_picture: profile.profile_picture,
                    ..Default::default()
                };
            }
            if *player == crate::good::GiPlayer::default() {
                good.gi_player = None;
            }
        }
        if off(Part::CharacterExtras) {
            good.gi_characters = None;
        }
        if off(Part::Achievements) {
            good.gi_achievements = None;
            good.gi_achievement_times = None;
            good.gi_achievement_progress = None;
        }
        if off(Part::Anecdotes) {
            good.gi_anecdotes = None;
        }
        if off(Part::Tasks) {
            good.gi_tasks = None;
        }
        if let Some(tasks) = &mut good.gi_tasks {
            if off(Part::Numbers) {
                tasks.resin = None;
                tasks.resin_spent = None;
            }
            if off(Part::Teapot) {
                tasks.realm = None;
            }
        }
        if off(Part::Endgame) {
            good.gi_endgame = None;
        }
        if off(Part::Exploration) {
            good.gi_exploration = None;
        }
        if off(Part::Collections) {
            good.gi_collections = None;
        }
        if off(Part::Wardrobe) {
            good.gi_wardrobe = None;
        }
        if off(Part::Teapot) {
            good.gi_teapot = None;
        }
        if off(Part::Events) {
            good.gi_events = None;
        }
        if off(Part::Miliastra) {
            good.gi_miliastra = None;
        }
        good.gi_off = Some(self.off.names());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_the_contracts_in_its_order() {
        let names: Vec<_> = Part::ALL.iter().map(|part| part.name()).collect();
        assert_eq!(
            names,
            [
                "characters",
                "weapons",
                "artifacts",
                "materials",
                "gi_player.profile",
                "gi_player.numbers",
                "gi_characters",
                "gi_achievements",
                "gi_anecdotes",
                "gi_tasks",
                "gi_endgame",
                "gi_exploration",
                "gi_collections",
                "gi_wardrobe",
                "gi_teapot",
                "gi_events",
                "gi_miliastra",
            ]
        );
        for part in Part::ALL {
            assert_eq!(Part::from_name(part.name()), Some(part));
        }
        assert_eq!(Part::from_name("gi_future"), None);
        // The enum's order is the list's (`gi_off` is written sorted by it).
        let mut sorted = Part::ALL;
        sorted.sort();
        assert_eq!(sorted, Part::ALL);
    }

    #[test]
    fn everything_is_shared_until_something_is_switched_off() {
        let mut off = PartsOff::default();
        assert!(off.all_shared());
        assert_eq!(off.shared_count(), 17);
        assert!(off.names().is_empty());
        off.set_shared(Part::Teapot, false);
        off.set_shared(Part::Profile, false);
        assert_eq!(off.shared_count(), 15);
        // In the contract's order, whatever order they were switched in.
        assert_eq!(off.names(), ["gi_player.profile", "gi_teapot"]);
        off.set_shared(Part::Teapot, true);
        assert_eq!(off.names(), ["gi_player.profile"]);
        // The saved form reads back, and unknown names are skipped.
        let saved = serde_json::to_string(&off).unwrap();
        assert_eq!(serde_json::from_str::<PartsOff>(&saved).unwrap(), off);
        assert_eq!(
            PartsOff::from_names(["gi_teapot", "gi_future", "weapons"]).names(),
            ["weapons", "gi_teapot"]
        );
    }

    #[test]
    fn builds_only_keeps_the_builds_and_the_account_numbers() {
        let off = PartsOff::builds_only();
        for part in Part::ALL {
            let kept = Part::BUILDS.contains(&part) || part == Part::Numbers;
            assert_eq!(off.is_shared(part), kept, "{part:?}");
        }
        assert_eq!(off.shared_count(), 5);
    }

    use crate::good::tests::full_good;

    fn upload(sharing: &Sharing) -> serde_json::Value {
        let mut good = full_good();
        sharing.strip(&mut good);
        serde_json::from_str(&good.upload_json().unwrap()).unwrap()
    }

    fn sharing(off: &[Part], routes_by_uid: bool) -> Sharing {
        let mut parts = PartsOff::default();
        for part in off {
            parts.set_shared(*part, false);
        }
        Sharing {
            off: parts,
            routes_by_uid,
        }
    }

    /// The top-level keys each part owns in the upload.
    fn keys(part: Part) -> &'static [&'static str] {
        match part {
            Part::Characters => &["characters"],
            Part::Weapons => &["weapons"],
            Part::Artifacts => &["artifacts"],
            Part::Materials => &["materials"],
            Part::Profile | Part::Numbers => &[],
            Part::CharacterExtras => &["gi_characters"],
            Part::Achievements => &[
                "gi_achievements",
                "gi_achievement_times",
                "gi_achievement_progress",
            ],
            Part::Anecdotes => &["gi_anecdotes"],
            Part::Tasks => &["gi_tasks"],
            Part::Endgame => &["gi_endgame"],
            Part::Exploration => &["gi_exploration"],
            Part::Collections => &["gi_collections"],
            Part::Wardrobe => &["gi_wardrobe"],
            Part::Teapot => &["gi_teapot"],
            Part::Events => &["gi_events"],
            Part::Miliastra => &["gi_miliastra"],
        }
    }

    #[test]
    fn with_everything_shared_the_upload_is_the_export_plus_an_empty_list() {
        let full = serde_json::to_value(full_good()).unwrap();
        let mut sent = upload(&Sharing::default());
        assert_eq!(sent["gi_off"], serde_json::json!([]));
        sent.as_object_mut().unwrap().remove("gi_off");
        assert_eq!(sent, full);
        // The fixture has every part, or the tests below prove nothing.
        for part in Part::ALL {
            for key in keys(part) {
                assert!(full.get(key).is_some(), "the fixture lacks {key}");
            }
        }
    }

    #[test]
    fn an_off_part_never_reaches_the_upload() {
        let full = serde_json::to_value(full_good()).unwrap();
        for part in Part::ALL {
            let sent = upload(&sharing(&[part], true));
            assert_eq!(sent["gi_off"], serde_json::json!([part.name()]));
            for key in keys(part) {
                // Left out, not sent empty.
                assert!(sent.get(key).is_none(), "{part:?} off still sends {key}");
            }
            // Every other part's keys are untouched (the two halves of
            // gi_player and the tasks they reach into are checked below).
            for other in Part::ALL {
                if other == part {
                    continue;
                }
                for key in keys(other) {
                    if *key == "gi_tasks" && matches!(part, Part::Numbers | Part::Teapot) {
                        continue;
                    }
                    assert_eq!(sent[*key], full[*key], "{part:?} off changed {key}");
                }
            }
        }
    }

    #[test]
    fn the_profile_and_the_numbers_are_two_halves_of_gi_player() {
        let full = serde_json::to_value(full_good()).unwrap();
        let profile = [
            "nickname",
            "signature",
            "birthday",
            "nameCard",
            "profilePicture",
        ];
        let numbers = [
            "uid",
            "ar",
            "arExp",
            "wl",
            "wlLimit",
            "resin",
            "storyKeys",
            "maxStamina",
            "gameData",
        ];
        for key in profile.iter().chain(&numbers) {
            assert!(
                full["gi_player"].get(key).is_some(),
                "the fixture lacks gi_player.{key}"
            );
        }

        let sent = upload(&sharing(&[Part::Profile], true));
        for key in profile {
            assert!(sent["gi_player"].get(key).is_none(), "{key}");
        }
        for key in numbers {
            assert_eq!(sent["gi_player"][key], full["gi_player"][key]);
        }

        // The numbers off while uploads go by UID: the UID alone still goes.
        let sent = upload(&sharing(&[Part::Numbers], true));
        for key in profile {
            assert_eq!(sent["gi_player"][key], full["gi_player"][key]);
        }
        for key in &numbers[1..] {
            assert!(sent["gi_player"].get(key).is_none(), "{key}");
        }
        assert_eq!(sent["gi_player"]["uid"], full["gi_player"]["uid"]);
        let sent = upload(&sharing(&[Part::Profile, Part::Numbers], true));
        assert_eq!(
            sent["gi_player"],
            serde_json::json!({ "uid": full["gi_player"]["uid"] })
        );

        // To one chosen account the UID stays home too, and with both
        // halves off nothing of gi_player is left to send.
        let sent = upload(&sharing(&[Part::Numbers], false));
        assert!(sent["gi_player"].get("uid").is_none());
        let sent = upload(&sharing(&[Part::Profile, Part::Numbers], false));
        assert!(sent.get("gi_player").is_none());
    }

    #[test]
    fn resin_goes_with_the_numbers_and_the_realm_currency_with_the_pot() {
        let full = serde_json::to_value(full_good()).unwrap();
        for key in ["resin", "realm", "wishPath", "commissions"] {
            assert!(
                full["gi_tasks"].get(key).is_some(),
                "the fixture lacks gi_tasks.{key}"
            );
        }
        let sent = upload(&sharing(&[Part::Numbers], true));
        assert!(sent["gi_tasks"].get("resin").is_none());
        assert!(sent["gi_tasks"].get("resinSpent").is_none());
        assert_eq!(sent["gi_tasks"]["realm"], full["gi_tasks"]["realm"]);

        let sent = upload(&sharing(&[Part::Teapot], true));
        assert!(sent["gi_tasks"].get("realm").is_none());
        assert_eq!(sent["gi_tasks"]["resin"], full["gi_tasks"]["resin"]);
        // The Epitomized Path stays with the planner tasks.
        assert_eq!(sent["gi_tasks"]["wishPath"], full["gi_tasks"]["wishPath"]);
        assert_eq!(
            sent["gi_tasks"]["commissions"],
            full["gi_tasks"]["commissions"]
        );
    }

    #[test]
    fn a_file_never_carries_the_list_and_always_has_its_sections() {
        // Not stripped: a file saved to a folder.
        let file = serde_json::to_value(full_good()).unwrap();
        assert!(file.get("gi_off").is_none());
        // An upload with a section off does not leak into what is
        // serialised next (a file, a parse and re-serialise).
        let mut good = full_good();
        sharing(&[Part::Characters, Part::Materials], true).strip(&mut good);
        let sent: serde_json::Value = serde_json::from_str(&good.upload_json().unwrap()).unwrap();
        assert!(sent.get("characters").is_none() && sent.get("materials").is_none());
        assert!(sent.get("weapons").is_some() && sent.get("artifacts").is_some());
        let plain = serde_json::to_value(full_good()).unwrap();
        assert!(plain.get("characters").is_some() && plain.get("materials").is_some());
        // An upload without the keys reads back (the manual upload parses
        // what it was handed).
        let back: Good = serde_json::from_value(sent).unwrap();
        assert!(back.characters.is_empty());
        // With no GOOD section off the upload is the plain serialisation.
        let mut good = full_good();
        sharing(&[Part::Endgame], true).strip(&mut good);
        assert_eq!(
            good.upload_json().unwrap(),
            serde_json::to_string(&good).unwrap()
        );
    }

    /// The live journal's root paths each part owns (`delta.rs` keys some
    /// parts under names of its own).
    fn roots(part: Part) -> &'static [&'static str] {
        match part {
            Part::Characters => &["characters"],
            Part::Weapons => &["weapons"],
            Part::Artifacts => &["artifacts"],
            Part::Materials => &["materials"],
            Part::CharacterExtras => &["gi_characters"],
            Part::Achievements => &["achievements"],
            Part::Anecdotes => &["anecdotes", "gi_anecdotes"],
            Part::Tasks => &["gi_tasks"],
            Part::Wardrobe => &["gi_wardrobe"],
            _ => &[],
        }
    }

    #[test]
    fn no_live_path_exists_under_an_off_part() {
        use crate::delta::Keyed;
        let paths = |sharing: &Sharing| {
            let mut good = full_good();
            sharing.strip(&mut good);
            Keyed::of(&good, |_| None)
                .entries
                .into_keys()
                .collect::<BTreeSet<_>>()
        };
        let under = |paths: &BTreeSet<crate::delta::Path>, root: &str| {
            paths
                .iter()
                .any(|path| path.first().is_some_and(|p| p == root))
        };
        let all = paths(&Sharing::default());
        // Every root the journal has belongs to a part here (or is
        // gi_player, checked below): a new one must be given a switch.
        let known: BTreeSet<&str> = Part::ALL
            .iter()
            .flat_map(|part| roots(*part).iter().copied())
            .chain(["gi_player"])
            .collect();
        for path in &all {
            let root = path.first().map(String::as_str).unwrap_or_default();
            assert!(
                known.contains(root),
                "no switch covers the journal's {root}"
            );
        }
        for part in Part::ALL {
            let sent = paths(&sharing(&[part], true));
            // Nothing is added by switching a part off.
            assert!(sent.is_subset(&all), "{part:?}");
            for root in roots(part) {
                assert!(under(&all, root), "nothing is keyed under {root}");
                assert!(!under(&sent, root), "{part:?} off still journals {root}");
            }
        }
        // gi_player is two parts: each half's fields leave with it.
        let field = |paths: &BTreeSet<crate::delta::Path>, name: &str| {
            paths.iter().any(|path| {
                path.first().is_some_and(|p| p == "gi_player")
                    && path.get(1).is_some_and(|p| p == name)
            })
        };
        assert!(field(&all, "nickname") && field(&all, "ar"));
        let sent = paths(&sharing(&[Part::Profile], true));
        assert!(!field(&sent, "nickname") && field(&sent, "ar"));
        let sent = paths(&sharing(&[Part::Numbers], true));
        assert!(field(&sent, "nickname") && !field(&sent, "ar") && !field(&sent, "resin"));
        // Resin and the realm currency leave gi_tasks with their parts.
        let task = |paths: &BTreeSet<crate::delta::Path>, name: &str| {
            paths.iter().any(|path| {
                path.first().is_some_and(|p| p == "gi_tasks")
                    && path.get(1).is_some_and(|p| p == name)
            })
        };
        assert!(task(&all, "resin") && task(&all, "realm"));
        assert!(!task(&paths(&sharing(&[Part::Numbers], true)), "resin"));
        assert!(!task(&paths(&sharing(&[Part::Teapot], true)), "realm"));
    }
}
