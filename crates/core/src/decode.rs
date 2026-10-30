// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

//! The decode layer between captured frames and [`PlayerData`]: one frame
//! through the [`GameSniffer`] ([`decode_one_packet`]), the command matchers
//! over a decoded batch ([`classify_commands`]), and the one place their
//! results change the captured data ([`apply_commands`]), with what a
//! reconnect does to data already held ([`DataReplacement`]).
//!
//! The desktop monitor runs this on its sniffer thread, `--replay-export`
//! runs it over a recording, and the Android app runs it over its VPN's
//! copy of the game's traffic, so all three build the same state.

use std::time::{Duration, Instant};

use teyvatme_protocol::{
    CommandMatch, GameCommand, GamePacket, GameSniffer, KeyState, avatar_wardrobe,
    classify_command, classify_union, flat_message, matches_avatar_team_update,
    matches_records_message, matches_scene_request, matches_scene_team_update, player_nickname,
    union_commands,
};
use tokio::sync::mpsc;

use crate::player_data::{CollectionPart, PlayerData, RoutinePart};

/// What the sniffer thread sends back.
///
/// The [`GameSniffer`] itself never crosses the channel, so its session state
/// has to be reported alongside the packets rather than queried by the monitor.
/// `replay.rs` consumes the same events, from the same [`decode_one_packet`].
pub enum SnifferEvent {
    /// teyvatme-protocol concluded that the game connection restarted. Always
    /// sent *before* the packet that concluded it.
    SessionReset,
    /// A decoded packet, with the sniffer's key situation once it was decoded.
    Packet(GamePacket, KeyReport),
}

/// The sniffer's key situation, reported with every packet because the
/// [`GameSniffer`] itself never leaves its thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyReport {
    /// Only [`KeyState::Session`] means commands are the account's data.
    pub state: KeyState,
    /// The search for the current connection's session key gave up.
    pub recovery_failed: bool,
}

impl KeyReport {
    pub fn of(sniffer: &GameSniffer) -> Self {
        Self {
            state: sniffer.key_state(),
            recovery_failed: sniffer.key_recovery_failed(),
        }
    }
}

/// Captured data waiting to be replaced by a new connection's.
///
/// A session reset used to wipe the captured data on the spot. When the new
/// connection's key was then never recovered -- every in-game reconnect, until
/// recently -- the user was left with nothing at all: not the new data, and no
/// longer the snapshot that had been captured perfectly well before. Now the
/// old data stays, exportable, until the new connection delivers data of its
/// own, which replaces it then. An account switch therefore still replaces
/// the data, just at the moment the new account's arrives.
#[derive(Debug, Default)]
pub struct DataReplacement {
    pending: bool,
    /// Resin clocks that came while holding: the game sends its first one
    /// before the login data, so it would always be lost with the hold.
    held_resin: Vec<(u16, CommandMatch)>,
}

/// What to do with one classified command.
#[derive(Debug, PartialEq, Eq)]
pub enum DataVerdict {
    /// Fold it into the captured data, as always.
    Apply,
    /// First data of a new connection: drop the old connection's, then apply.
    ReplaceThenApply,
    /// A new connection began but this proves nothing about it yet: leave the
    /// captured data alone and skip it.
    Hold,
}

impl DataReplacement {
    /// A new connection began. Returns whether that is news: the copies of
    /// one handshake, or resets before any data arrived, collapse into one.
    pub fn connection_reset(&mut self) -> bool {
        let news = !std::mem::replace(&mut self.pending, true);
        if news {
            self.held_resin.clear();
        }
        news
    }

    /// The user cleared the data, so there is nothing left to replace.
    pub fn clear(&mut self) {
        self.pending = false;
        self.held_resin.clear();
    }

    /// Decide what a classified command does. `key` is what it decrypted
    /// under; `replaces` is whether it is real account data that may stand in
    /// for the old snapshot (a delete notify's shape is too common for that).
    pub fn verdict(&mut self, key: KeyState, replaces: bool) -> DataVerdict {
        if !self.pending {
            return DataVerdict::Apply;
        }
        if key == KeyState::Session && replaces {
            self.pending = false;
            return DataVerdict::ReplaceThenApply;
        }
        DataVerdict::Hold
    }

    /// [`verdict`](Self::verdict) for one decoded batch: it may replace the
    /// old snapshot when anything in it is real account data, which a delete
    /// notify on its own is not (its shape is too common).
    ///
    /// Resin clocks in a held batch are kept, and join the batch that
    /// replaces the data, after its own commands (the player properties they
    /// are checked against come in it).
    pub fn verdict_for_batch(
        &mut self,
        key: KeyState,
        classified: &mut Vec<(u16, CommandMatch)>,
    ) -> DataVerdict {
        let verdict = self.batch_verdict(key, classified);
        match verdict {
            DataVerdict::Hold => {
                // The caller drops a held batch, so its clocks are moved out.
                let (resin, rest): (Vec<_>, Vec<_>) = std::mem::take(classified)
                    .into_iter()
                    .partition(|(_, found)| matches!(found, CommandMatch::ResinClock(_)));
                *classified = rest;
                self.held_resin.extend(resin);
            }
            DataVerdict::ReplaceThenApply => classified.append(&mut self.held_resin),
            DataVerdict::Apply => {}
        }
        verdict
    }

    fn batch_verdict(&mut self, key: KeyState, classified: &[(u16, CommandMatch)]) -> DataVerdict {
        // A delete notify, a changed item or property only alter what is
        // held, and the wish history messages are not account data at all.
        let replaces = classified.iter().any(|(_, found)| {
            found.is_account_data()
                && !matches!(
                    found,
                    CommandMatch::DeletedItems(_)
                        | CommandMatch::ChangedItems(_)
                        | CommandMatch::PropertyUpdate(_)
                        | CommandMatch::Nickname(_)
                        | CommandMatch::Profile(_)
                        | CommandMatch::SceneRequest(_)
                        | CommandMatch::Flat(_)
                        | CommandMatch::ResinClock(_)
                        | CommandMatch::CommissionProgress(_)
                        | CommandMatch::ResinSpent(_)
                        | CommandMatch::Records(_)
                        | CommandMatch::TeamUpdate(_)
                        | CommandMatch::Party(_)
                )
        });
        self.verdict(key, replaces)
    }
}

/// Which classes of captured data one batch of commands changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppliedData {
    /// The inventory: an item notify, or a delete notify that removed
    /// something actually held.
    pub items: bool,
    pub characters: bool,
    pub achievements: bool,
    /// Player properties. They stamp no timestamp of their own (see
    /// the desktop monitor's `handle_game_packet`) but are still new data.
    pub properties: bool,
    /// The Travel Accounts book. Like properties, no timestamp of its own:
    /// it rides along with whatever export the other classes trigger.
    pub anecdotes: bool,
    /// A part of the planner's routine (`gi_tasks`). No timestamp of its own
    /// either, like the anecdotes.
    pub routine: bool,
    /// Live updates applied (a character's talents, levels, constellations,
    /// gear, friendship, a new character; achievements moving), for live
    /// sync's change count: what they are is only known once they are
    /// applied.
    pub live_updates: usize,
}

impl AppliedData {
    pub fn any(&self) -> bool {
        self.items
            || self.characters
            || self.achievements
            || self.properties
            || self.anecdotes
            || self.routine
    }

    /// Everything either side changed.
    pub fn union(self, other: Self) -> Self {
        Self {
            items: self.items || other.items,
            characters: self.characters || other.characters,
            achievements: self.achievements || other.achievements,
            properties: self.properties || other.properties,
            anecdotes: self.anecdotes || other.anecdotes,
            routine: self.routine || other.routine,
            live_updates: self.live_updates + other.live_updates,
        }
    }
}

/// Run the command matchers over a decoded batch, keeping what they claimed.
///
/// One classification pass instead of an `else if` chain of matchers: the
/// chain could not notice that two of them claimed the same command, which is
/// exactly how a shape collision turns into silently missing data.
///
/// A command no matcher claims may be the game's bundle of small notifies,
/// whose player property updates (Mora, Primogems after a reward) are taken
/// out with their own command ids, or a character's live update, which only
/// the held data can recognise ([`CommandMatch::Flat`], decided in
/// [`apply_commands`]), or a list of small records that only the game data's
/// ids can (`CommandMatch::Records`: the Serenitea Pot's music).
pub fn classify_commands(commands: &[GameCommand]) -> Vec<(u16, CommandMatch)> {
    commands
        .iter()
        .flat_map(|command| {
            let _span = tracing::info_span!("command", id = command.command_id).entered();
            // The client's own requests carry no account data; of them only
            // the token request's client version is read, and the scene
            // requests' world owner (whose world the replies describe).
            if command.sent {
                return match classify_command(command) {
                    Some(found @ CommandMatch::ClientVersion(_)) => {
                        vec![(command.command_id, found)]
                    }
                    _ => matches_scene_request(&command.proto_data)
                        .map(|request| (command.command_id, CommandMatch::SceneRequest(request)))
                        .into_iter()
                        .collect(),
                };
            }
            match classify_command(command) {
                // The character message also holds the wardrobe.
                Some(found @ CommandMatch::Avatars(_)) => std::iter::once(found)
                    .chain(avatar_wardrobe(&command.proto_data).map(CommandMatch::Wardrobe))
                    .map(|found| (command.command_id, found))
                    .collect(),
                // The login's player data also holds the nickname.
                Some(found @ CommandMatch::Properties(_)) => std::iter::once(found)
                    .chain(player_nickname(&command.proto_data).map(CommandMatch::Nickname))
                    .map(|found| (command.command_id, found))
                    .collect(),
                Some(found) => vec![(command.command_id, found)],
                // Both readings: an edited team (a slot and its team) is
                // also the shape of the game's bundle of small notifies (an
                // id and a body). The team is believed against the held
                // characters; the bundle's commands are read as ever.
                None if union_commands(command).is_some() => {
                    let mut found = classify_union(command);
                    if let Some(teams) = matches_avatar_team_update(&command.proto_data) {
                        found.push((command.command_id, CommandMatch::TeamUpdate(teams)));
                    }
                    found
                }
                // Both readings: a lone packed list (the Serenitea Pot's
                // music) also passes as a nested message for the character
                // updates. Each is recognised in `apply_commands` against
                // the held data, which no message satisfies twice.
                None => flat_message(command)
                    .map(CommandMatch::Flat)
                    .into_iter()
                    .chain(matches_records_message(&command.proto_data).map(CommandMatch::Records))
                    .chain(
                        matches_avatar_team_update(&command.proto_data)
                            .map(CommandMatch::TeamUpdate),
                    )
                    .chain(matches_scene_team_update(&command.proto_data).map(CommandMatch::Party))
                    .map(|found| (command.command_id, found))
                    .collect(),
            }
        })
        .collect()
}

/// Fold classified commands into the captured data.
///
/// The one place a decoded command changes [`PlayerData`]: the live monitor
/// and `--replay-export` both apply commands through here, so a replay builds
/// exactly the state the app would have.
pub fn apply_commands(
    player_data: &mut PlayerData,
    classified: Vec<(u16, CommandMatch)>,
) -> AppliedData {
    let mut applied = AppliedData::default();
    for (command_id, found) in classified {
        let _span = tracing::info_span!("command", id = command_id).entered();
        match found {
            CommandMatch::Items(items) => {
                tracing::info!("Found item packet with {} items", items.len());
                player_data.process_items(&items);
                applied.items = true;
            }
            CommandMatch::Properties(properties) => {
                tracing::info!("Found properties packet: {:?}", properties);
                player_data.process_properties(&properties);
                applied.properties = true;
            }
            CommandMatch::PropertyUpdate(properties) => {
                tracing::info!("Found property update: {:?}", properties);
                player_data.process_properties(&properties);
                applied.properties = true;
            }
            CommandMatch::Nickname(nickname) => {
                tracing::info!(chars = nickname.chars().count(), "Found the nickname");
                player_data.set_nickname(nickname);
                applied.properties = true;
            }
            CommandMatch::SceneRequest(request) => {
                // Changes nothing exported: it says whose world the next
                // replies describe.
                player_data.note_scene_request(command_id, request);
            }
            CommandMatch::Profile(profile) => {
                tracing::info!(
                    texts = profile.texts.len(),
                    name_card = ?profile.name_card,
                    birthday = profile.birthday.is_some(),
                    picture = profile.picture.is_some(),
                    "Found a profile card"
                );
                player_data.process_profile(profile);
                applied.properties = true;
            }
            CommandMatch::Flat(flat) => {
                // Most are other traffic: only the held characters can say.
                if let Some(change) = player_data.apply_avatar_change(&flat) {
                    tracing::info!(?change, "Found character update");
                    if change.is_equip() {
                        applied.items = true;
                    } else {
                        applied.characters = true;
                    }
                    applied.live_updates += 1;
                }
            }
            CommandMatch::Avatars(avatars) => {
                tracing::info!("Found avatar packet with {} avatars", avatars.len());
                player_data.process_characters(&avatars);
                applied.characters = true;
            }
            CommandMatch::Achievements(achievements) => {
                tracing::info!(
                    "Found achievement packet with {} achievements",
                    achievements.len()
                );
                player_data.process_achievements(&achievements);
                applied.achievements = true;
            }
            CommandMatch::AchievementUpdates(updates) => {
                let changed = player_data.apply_achievement_updates(&updates);
                if changed > 0 {
                    tracing::info!(changed, of = updates.len(), "Found achievement update");
                    applied.achievements = true;
                    applied.live_updates += changed;
                }
            }
            CommandMatch::Fetters(fetters) => {
                let changes = player_data.apply_fetters(&fetters);
                if !changes.is_empty() {
                    tracing::info!(?changes, "Found friendship update");
                    applied.characters = true;
                    applied.live_updates += changes.len();
                }
            }
            CommandMatch::Anecdotes(anecdotes) => {
                tracing::info!(
                    "Found anecdote packet with {} anecdotes",
                    anecdotes.anecdotes.len()
                );
                player_data.process_anecdotes(&anecdotes);
                applied.anecdotes = true;
            }
            CommandMatch::Commissions(found) => {
                tracing::info!(total = found.total, finished = ?found.finished, "Found commissions");
                player_data.process_routine(RoutinePart::Commissions(found));
                applied.routine = true;
            }
            CommandMatch::CommissionProgress(found) => {
                // Most are other traffic: only the learned reward id can say.
                if player_data.apply_commission_progress(command_id, &found) {
                    tracing::info!(?found, "Found commission progress");
                    applied.routine = true;
                    applied.live_updates += 1;
                }
            }
            CommandMatch::ResinSpent(found) => {
                tracing::debug!(
                    spent = found.spent,
                    day = found.day,
                    "Found the resin spent today"
                );
                player_data.process_routine(RoutinePart::ResinSpent(found));
                applied.routine = true;
            }
            CommandMatch::Abyss(found) => {
                tracing::info!(
                    stars = found.stars,
                    max_stars = found.max_stars,
                    "Found abyss period"
                );
                player_data.process_routine(RoutinePart::Abyss(found));
                applied.routine = true;
            }
            CommandMatch::Expeditions(found) => {
                let out = found.iter().filter(|e| e.is_out()).count();
                tracing::info!(out, "Found expeditions");
                player_data.process_routine(RoutinePart::Expeditions(found));
                applied.routine = true;
            }
            CommandMatch::HomeResources(found) => {
                tracing::info!("Found realm currency");
                player_data.process_routine(RoutinePart::Home(found));
                applied.routine = true;
            }
            CommandMatch::Welkin(days) => {
                tracing::info!("Found Welkin Moon days");
                player_data.process_routine(RoutinePart::Welkin(days));
                applied.routine = true;
            }
            CommandMatch::Quests(found) => {
                tracing::info!(
                    finished = found.finished.len(),
                    active = found.active.len(),
                    "Found the parent quests"
                );
                player_data.process_collection(CollectionPart::Quests(found));
                applied.routine = true;
            }
            CommandMatch::Archive(found) => {
                tracing::info!(tabs = found.len(), "Found the Archive");
                player_data.process_collection(CollectionPart::Archive(found));
                applied.routine = true;
            }
            CommandMatch::Defeats(found) => {
                tracing::info!(count = found.len(), "Found the enemies defeated");
                player_data.process_collection(CollectionPart::Defeats(found));
                applied.routine = true;
            }
            CommandMatch::Handbook(found) => {
                tracing::info!(
                    chapters = found.chapters.len(),
                    "Found the Adventurer Handbook"
                );
                player_data.process_collection(CollectionPart::Handbook(found));
                applied.routine = true;
            }
            CommandMatch::Tcg(found) => {
                tracing::info!(cards = found.cards.len(), "Found the TCG cards");
                player_data.process_collection(CollectionPart::Tcg(found));
                applied.routine = true;
            }
            CommandMatch::Aranara(found) => {
                tracing::info!("Found the Aranara collections");
                player_data.process_collection(CollectionPart::Aranara(found));
                applied.routine = true;
            }
            CommandMatch::WeaponSkins(found) => {
                tracing::info!(count = found.len(), "Found the weapon skins");
                player_data.process_collection(CollectionPart::WeaponSkins(found));
                applied.routine = true;
            }
            CommandMatch::MusicGame(found) => {
                tracing::info!(songs = found.len(), "Found the music game");
                player_data.process_collection(CollectionPart::MusicGame(found));
                applied.routine = true;
            }
            CommandMatch::BeyondCosmetics(found) => {
                tracing::info!(count = found.len(), "Found the Miliastra cosmetics");
                player_data.process_collection(CollectionPart::BeyondCosmetics(found));
                applied.routine = true;
            }
            CommandMatch::SignIns(found) => {
                tracing::info!(count = found.len(), "Found the login events");
                player_data.process_collection(CollectionPart::SignIns(found));
                applied.routine = true;
            }
            CommandMatch::TcgChallenges(found) => {
                tracing::info!(levels = found.levels.len(), "Found the TCG challenges");
                player_data.process_collection(CollectionPart::TcgChallenges(found));
                applied.routine = true;
            }
            CommandMatch::TcgTavern(found) => {
                tracing::info!(characters = found.len(), "Found the TCG Tavern");
                player_data.process_collection(CollectionPart::TcgTavern(found));
                applied.routine = true;
            }
            CommandMatch::Comfort(found) => {
                tracing::info!(realms = found.len(), "Found the Serenitea Pot's comfort");
                player_data.process_collection(CollectionPart::Comfort(found));
                applied.routine = true;
            }
            CommandMatch::Events(found) => {
                tracing::info!(count = found.len(), "Found the events running");
                player_data.process_collection(CollectionPart::Events(found));
                applied.routine = true;
            }
            CommandMatch::Bosses(found) => {
                tracing::info!(count = found.respawns.len(), "Found the bosses' respawns");
                player_data.process_routine(RoutinePart::Bosses(found));
                applied.routine = true;
            }
            CommandMatch::Hangouts(found) => {
                tracing::info!(chapters = found.len(), "Found the Hangout Events");
                player_data.process_collection(CollectionPart::Hangouts(found));
                applied.routine = true;
            }
            CommandMatch::TeamUpdate(teams) => {
                // Only the held characters can say (never log the names).
                let changed = player_data.apply_team_update(&teams);
                if changed > 0 {
                    tracing::info!(changed, "Found a saved team edited");
                    applied.routine = true;
                    applied.live_updates += changed;
                }
            }
            CommandMatch::Party(party) => {
                if let Some(slot) = player_data.apply_party(&party) {
                    tracing::info!(slot, "Found the team in use");
                    applied.routine = true;
                    applied.live_updates += 1;
                }
            }
            CommandMatch::Wardrobe(found) => {
                tracing::info!("Found the wardrobe");
                player_data.process_collection(CollectionPart::Wardrobe(found));
                applied.routine = true;
            }
            CommandMatch::Records(records) => {
                // Most are other traffic: only the game data's ids can say.
                if let Some(count) = player_data.apply_collections(&records) {
                    tracing::info!(count, "Found the Serenitea Pot's Radiant Spincrystals");
                    applied.routine = true;
                } else if let Some(count) = player_data.apply_recipes(&records) {
                    tracing::info!(count, "Found the recipes");
                    applied.routine = true;
                } else if let Some(list) = player_data.apply_lone_list(&records) {
                    tracing::info!(?list, "Found a lone list");
                    applied.routine = true;
                } else if let Some(count) = player_data.apply_tribal_chapters(&records) {
                    tracing::info!(count, "Found the tribal chapters");
                    applied.routine = true;
                } else if let Some(part) = player_data.apply_exploration(&records) {
                    tracing::info!(?part, "Found exploration");
                    applied.routine = true;
                }
            }
            CommandMatch::ResinClock(clock) => {
                tracing::debug!(?clock, "resin clock candidate");
                player_data.process_routine(RoutinePart::Resin(clock));
                applied.routine = true;
            }
            CommandMatch::GachaInfo(links) => {
                // The wish history reads the links; the path is the account's.
                if let Some(path) = links.iter().find_map(|link| link.epitomized) {
                    tracing::info!(?path, "Found the Epitomized Path");
                    player_data.process_routine(RoutinePart::WishPath(path));
                    applied.routine = true;
                }
            }
            CommandMatch::ChangedItems(items) => {
                // Candidates as well: only the captured account's are taken.
                let changed = player_data.change_items(&items);
                if changed > 0 {
                    tracing::info!(
                        "Item change packet updated {changed} of {} items",
                        items.len()
                    );
                    applied.items = true;
                } else {
                    tracing::debug!(
                        "Item change packet held none of the account's items ({} items)",
                        items.len()
                    );
                }
            }
            CommandMatch::DeletedItems(guids) => {
                // The matcher reports candidates -- its shape is shared with
                // a couple of dozen other messages -- so the inventory
                // intersection is the real test. Removing nothing means this
                // was one of those others, and must not count as an inventory
                // change: that would tell the UI and the automation gate the
                // inventory just changed.
                let removed = player_data.remove_items(&guids);
                if removed > 0 {
                    tracing::info!(
                        "Item delete packet removed {removed} of {} guids",
                        guids.len()
                    );
                    applied.items = true;
                } else {
                    tracing::debug!(
                        "Item delete packet held none of the captured inventory ({} guids)",
                        guids.len()
                    );
                }
            }
            // `CommandMatch` is non-exhaustive.
            _ => {}
        }
    }
    applied
}

/// Whether a classified batch holds the game sending the whole inventory (a
/// login, or a world change), given the items held before it (live sync's
/// snapshots; see [`crate::live::is_full_send`]).
pub fn full_send_in(classified: &[(u16, CommandMatch)], items_held: usize) -> bool {
    classified.iter().any(|(_, found)| {
        matches!(found, CommandMatch::Items(items) if crate::live::is_full_send(items.len(), items_held))
    })
}

/// How many account entries a classified batch touches: live sync's cheap
/// estimate of what changed (the diff decides what is sent).
pub fn changes_in(classified: &[(u16, CommandMatch)]) -> usize {
    classified
        .iter()
        .map(|(_, found)| match found {
            CommandMatch::Items(items) => items.len(),
            CommandMatch::Avatars(avatars) => avatars.len(),
            CommandMatch::Properties(properties) | CommandMatch::PropertyUpdate(properties) => {
                properties.len()
            }
            CommandMatch::Achievements(achievements) => achievements.len(),
            CommandMatch::DeletedItems(guids) => guids.len(),
            CommandMatch::ChangedItems(items) => items.len(),
            CommandMatch::Anecdotes(_) => 1,
            _ => 0,
        })
        .sum()
}

/// How often the sniffer thread logs what it has been doing.
pub const STATS_INTERVAL: Duration = Duration::from_secs(60);

/// What the sniffer thread did, for one INFO line a minute.
///
/// The log of 2026-10-04 had nothing to show for the hour and three quarters
/// in which nothing decoded: no line said frames were still arriving, that
/// the key was missing, or which conversation each direction was stuck on.
/// This is that line.
#[derive(Debug, Default)]
pub struct SnifferStats {
    /// Frames this interval.
    frames: u64,
    /// Frames the interval before, so going quiet is reported once.
    previous_frames: u64,
    /// Deepest the queue to this thread got this interval.
    max_queue: usize,
    /// Commands decoded since the last session reset.
    commands_since_reset: u64,
    /// When commands last decoded.
    last_decode: Option<Instant>,
    /// The process-wide duplicate count at the last report.
    duplicates_reported: u64,
}

/// One stats line.
#[derive(Debug)]
pub struct StatsLine {
    pub frames: u64,
    pub duplicates: u64,
    pub max_queue: usize,
    pub commands_since_reset: u64,
    pub last_decode_age: Option<Duration>,
}

impl SnifferStats {
    /// A frame was taken off the queue, which then held `queued` (this one
    /// included).
    pub fn note_frame(&mut self, queued: usize) {
        self.frames += 1;
        self.max_queue = self.max_queue.max(queued);
    }

    pub fn note_commands(&mut self, count: usize, now: Instant) {
        if count > 0 {
            self.commands_since_reset += count as u64;
            self.last_decode = Some(now);
        }
    }

    pub fn note_reset(&mut self) {
        self.commands_since_reset = 0;
    }

    /// The line for the interval ending `now`, given the process-wide count of
    /// dropped duplicates. `None` while capture is idle: a line is due when
    /// frames arrived this interval, or the interval before (so the drop to
    /// zero is reported once).
    pub fn report(&mut self, duplicates_total: u64, now: Instant) -> Option<StatsLine> {
        let due = self.frames > 0 || self.previous_frames > 0;
        let line = due.then(|| StatsLine {
            frames: self.frames,
            duplicates: duplicates_total.saturating_sub(self.duplicates_reported),
            max_queue: self.max_queue,
            commands_since_reset: self.commands_since_reset,
            last_decode_age: self.last_decode.map(|at| now.saturating_duration_since(at)),
        });
        self.previous_frames = self.frames;
        self.frames = 0;
        self.max_queue = 0;
        self.duplicates_reported = duplicates_total;
        line
    }
}

/// Decode one captured frame and forward everything it produced.
///
/// Returns `false` once the monitor is gone and the thread should stop.
///
/// The session reset is sent *before* the decoded packet, and that ordering is
/// the point of this function existing separately: one `receive_packet` call can
/// both conclude the connection restarted and return the first commands of the
/// new connection, and those commands must not land in the old account's maps.
///
/// `session_generation` is read rather than
/// [`ConnectionPacket::HandshakeRequested`] because the handshake datagram is
/// unauthenticated -- any local process can forge one with 20 bytes on a game
/// port -- while the generation moves only when teyvatme-protocol has itself
/// corroborated the restart (a new KCP conversation, or the live session key
/// going dead). Latching on the raw event instead handed anyone a one-packet
/// "erase this user's capture" button, one layer above the library check that
/// exists to prevent exactly that.
pub fn decode_one_packet(
    sniffer: &mut GameSniffer,
    last_generation: &mut u64,
    packet: Vec<u8>,
    decoded_tx: &mpsc::UnboundedSender<SnifferEvent>,
    stats: &mut SnifferStats,
) -> bool {
    let decoded = sniffer.receive_packet(packet);

    let generation = sniffer.session_generation();
    if generation != *last_generation {
        *last_generation = generation;
        stats.note_reset();
        if decoded_tx.send(SnifferEvent::SessionReset).is_err() {
            return false;
        }
    }

    if let Some(GamePacket::Commands(commands)) = &decoded {
        stats.note_commands(commands.len(), Instant::now());
    }

    match decoded {
        Some(decoded) => decoded_tx
            .send(SnifferEvent::Packet(decoded, KeyReport::of(sniffer)))
            .is_ok(),
        None => true,
    }
}

/// Frame builders for tests, here and in Teyvat.me's (with the
/// `test-support` feature).
#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    /// An Ethernet II / IPv4 / UDP frame around `payload`.
    ///
    /// Both checksums are left zero: `SlicedPacket::from_ethernet`, which is
    /// what teyvatme-protocol parses with, does not verify them, and a zero UDP
    /// checksum means "not computed" over IPv4 anyway.
    pub fn udp_frame(src_port: u16, dest_port: u16, payload: &[u8]) -> Vec<u8> {
        let udp_len = (8 + payload.len()) as u16;
        let total_len = 20 + udp_len;

        let mut frame = Vec::with_capacity(14 + total_len as usize);
        frame.extend_from_slice(&[1, 2, 3, 4, 5, 6]); // destination mac
        frame.extend_from_slice(&[7, 8, 9, 10, 11, 12]); // source mac
        frame.extend_from_slice(&0x0800u16.to_be_bytes()); // ethertype: ipv4

        frame.push(0x45); // version 4, 5 * 4 = 20 byte header
        frame.push(0); // dscp/ecn
        frame.extend_from_slice(&total_len.to_be_bytes());
        frame.extend_from_slice(&0u16.to_be_bytes()); // identification
        frame.extend_from_slice(&0x4000u16.to_be_bytes()); // don't fragment
        frame.push(64); // ttl
        frame.push(17); // protocol: udp
        frame.extend_from_slice(&0u16.to_be_bytes()); // header checksum
        frame.extend_from_slice(&[10, 0, 0, 2]); // source ip
        frame.extend_from_slice(&[10, 0, 0, 1]); // destination ip

        frame.extend_from_slice(&src_port.to_be_bytes());
        frame.extend_from_slice(&dest_port.to_be_bytes());
        frame.extend_from_slice(&udp_len.to_be_bytes());
        frame.extend_from_slice(&0u16.to_be_bytes()); // checksum
        frame.extend_from_slice(payload);
        frame
    }

    /// A connection-management datagram: 20 bytes led by a 4-byte big-endian
    /// code. This is the whole forgery -- nothing about it is authenticated, and
    /// a datagram sent *to* a game port counts just like one from it.
    pub fn connection_frame(code: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 20];
        payload[..4].copy_from_slice(&code.to_be_bytes());
        udp_frame(50000, 22102, &payload)
    }

    pub fn handshake_frame() -> Vec<u8> {
        connection_frame(0xFF)
    }

    /// One segment in the game's KCP framing: `conv(4) extra(4) cmd(1) frg(1)
    /// wnd(2) ts(4) sn(4) una(4) len(4) extra(4) content`.
    pub fn segment_frame(conv: u32, content: &[u8]) -> Vec<u8> {
        let mut segment = Vec::new();
        segment.extend_from_slice(&conv.to_le_bytes());
        segment.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        segment.push(81); // cmd: push
        segment.push(0); // frg
        segment.extend_from_slice(&128u16.to_le_bytes()); // wnd
        segment.extend_from_slice(&0u32.to_le_bytes()); // ts
        segment.extend_from_slice(&0u32.to_le_bytes()); // sn
        segment.extend_from_slice(&0u32.to_le_bytes()); // una
        segment.extend_from_slice(&(content.len() as u32).to_le_bytes());
        segment.extend_from_slice(&0xFEED_FACEu32.to_le_bytes());
        segment.extend_from_slice(content);
        udp_frame(22102, 50000, &segment)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use teyvatme_game_data::AnimeGameData;
    use teyvatme_protocol::ConnectionPacket;

    use super::test_support::*;
    use super::*;

    #[test]
    fn an_edited_team_is_read_though_it_looks_like_a_bundle() {
        // 7.1's AvatarTeamUpdateNotify: {2: {1: slot, 2: {12: packed guids}}},
        // which is also the bundle's shape (an id and a body per entry).
        fn varint(mut value: u64, out: &mut Vec<u8>) {
            while value >= 0x80 {
                out.push((value as u8) | 0x80);
                value >>= 7;
            }
            out.push(value as u8);
        }
        fn bytes(tag: u64, payload: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            varint((tag << 3) | 2, &mut out);
            varint(payload.len() as u64, &mut out);
            out.extend_from_slice(payload);
            out
        }
        let mut guids = Vec::new();
        for n in 1..=4 {
            varint((812_345_678u64 << 32) + n, &mut guids);
        }
        let entry = [vec![0x08, 5], bytes(2, &bytes(12, &guids))].concat();
        let payload = bytes(2, &entry);
        let command = GameCommand {
            command_id: 25632,
            header_len: 0,
            data_len: payload.len() as u32,
            proto_header: Vec::new(),
            proto_data: payload,
            sent: false,
        };
        let found = classify_commands(&[command]);
        assert!(
            found
                .iter()
                .any(|(_, found)| matches!(found, CommandMatch::TeamUpdate(teams) if teams.contains_key(&5))),
            "{:?}",
            found.iter().map(|(_, f)| f.kind()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_session_reset_reaches_the_monitor_before_the_packet_that_caused_it() {
        // One `receive_packet` can both conclude the connection restarted and
        // return the first commands of the new connection. If the reset arrived
        // second the monitor would wipe the new account's data instead of the
        // old account's.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut sniffer = GameSniffer::new();
        let mut generation = sniffer.session_generation();

        assert!(decode_one_packet(
            &mut sniffer,
            &mut generation,
            handshake_frame(),
            &tx,
            &mut SnifferStats::default()
        ));

        assert!(matches!(rx.try_recv(), Ok(SnifferEvent::SessionReset)));
        assert!(matches!(
            rx.try_recv(),
            Ok(SnifferEvent::Packet(
                GamePacket::Connection(ConnectionPacket::HandshakeRequested),
                _
            ))
        ));
        assert!(rx.try_recv().is_err());

        // The generation only moved once, so the reset is announced once.
        assert!(decode_one_packet(
            &mut sniffer,
            &mut generation,
            segment_frame(7, &[0u8; 40]),
            &tx,
            &mut SnifferStats::default()
        ));
        assert!(matches!(rx.try_recv(), Ok(SnifferEvent::Packet(..))));
        assert!(
            rx.try_recv().is_err(),
            "one reset per session, not per packet"
        );
    }

    #[test]
    fn an_unauthenticated_connection_packet_does_not_ask_the_monitor_to_wipe_anything() {
        // The monitor used to latch a data reset on `Disconnected` -- as
        // forgeable as any other 20-byte datagram on a game port -- and then
        // spend the latch on the next `Commands`, which an ACK of the *live*
        // session satisfies with an empty vec. One datagram wiped a live
        // capture. Only a reset teyvatme-protocol has corroborated counts now,
        // and a real disconnect is always followed by the handshake that
        // corroborates one.
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut sniffer = GameSniffer::new();
        let mut generation = sniffer.session_generation();

        // 404 is `Disconnected`; any other code is `HandshakeEstablished`.
        for code in [404, 1] {
            assert!(decode_one_packet(
                &mut sniffer,
                &mut generation,
                connection_frame(code),
                &tx,
                &mut SnifferStats::default()
            ));
            assert!(matches!(
                rx.try_recv(),
                Ok(SnifferEvent::Packet(GamePacket::Connection(_), _))
            ));
        }

        assert!(
            rx.try_recv().is_err(),
            "no reset without a corroborated reconnect"
        );
    }

    #[test]
    fn a_kcp_segment_that_restarts_nothing_announces_no_reset() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut sniffer = GameSniffer::new();
        let mut generation = sniffer.session_generation();

        assert!(decode_one_packet(
            &mut sniffer,
            &mut generation,
            segment_frame(7, &[0u8; 40]),
            &tx,
            &mut SnifferStats::default()
        ));

        assert!(matches!(rx.try_recv(), Ok(SnifferEvent::Packet(..))));
        assert!(rx.try_recv().is_err());
    }

    // -- the sniffer's stats line ----------------------------------------------

    #[test]
    fn an_idle_sniffer_reports_nothing() {
        let mut stats = SnifferStats::default();
        assert!(stats.report(0, Instant::now()).is_none());
    }

    #[test]
    fn a_report_covers_its_own_interval() {
        let mut stats = SnifferStats::default();
        let start = Instant::now();
        for queued in [1, 5, 3] {
            stats.note_frame(queued);
        }
        stats.note_commands(4, start);
        stats.note_commands(0, start);

        let line = stats.report(80, start).expect("frames arrived");
        assert_eq!(line.frames, 3);
        assert_eq!(line.duplicates, 80);
        assert_eq!(line.max_queue, 5);
        assert_eq!(line.commands_since_reset, 4);
        assert_eq!(line.last_decode_age, Some(Duration::ZERO));

        // The next interval counts afresh, except what spans intervals.
        stats.note_frame(1);
        let later = start + Duration::from_secs(60);
        let line = stats.report(100, later).expect("a frame arrived");
        assert_eq!(line.frames, 1);
        assert_eq!(line.duplicates, 20, "a delta, not the running total");
        assert_eq!(line.max_queue, 1);
        assert_eq!(line.commands_since_reset, 4);
        assert_eq!(line.last_decode_age, Some(Duration::from_secs(60)));
    }

    #[test]
    fn capture_going_quiet_is_reported_once() {
        let mut stats = SnifferStats::default();
        let now = Instant::now();
        stats.note_frame(1);
        assert!(stats.report(0, now).is_some());

        let quiet = stats.report(0, now).expect("the drop to zero is news");
        assert_eq!(quiet.frames, 0);
        assert!(stats.report(0, now).is_none(), "and then it stays quiet");
    }

    #[test]
    fn a_reset_restarts_the_command_count() {
        let mut stats = SnifferStats::default();
        let now = Instant::now();
        stats.note_frame(1);
        stats.note_commands(7, now);
        stats.note_reset();
        stats.note_commands(2, now);
        assert_eq!(stats.report(0, now).unwrap().commands_since_reset, 2);
    }

    // -- keeping the last good data across a reset -----------------------------

    #[test]
    fn data_keeps_flowing_in_when_no_reset_is_pending() {
        let mut replacement = DataReplacement::default();
        for key in [KeyState::None, KeyState::Dispatch, KeyState::Session] {
            assert_eq!(replacement.verdict(key, true), DataVerdict::Apply);
            assert_eq!(replacement.verdict(key, false), DataVerdict::Apply);
        }
    }

    #[test]
    fn a_reset_keeps_the_old_data_until_the_new_connection_delivers() {
        // A reconnect whose key is never recovered must not cost the snapshot
        // that was already captured: it stays exportable until real data from
        // the new connection takes its place.
        let mut replacement = DataReplacement::default();
        assert!(replacement.connection_reset());

        // Nothing decrypted under the session key: the old data stays.
        assert_eq!(
            replacement.verdict(KeyState::Dispatch, true),
            DataVerdict::Hold
        );
        assert_eq!(replacement.verdict(KeyState::None, true), DataVerdict::Hold);

        // The new connection's own data replaces it, once.
        assert_eq!(
            replacement.verdict(KeyState::Session, true),
            DataVerdict::ReplaceThenApply
        );
        assert_eq!(
            replacement.verdict(KeyState::Session, true),
            DataVerdict::Apply
        );
    }

    #[test]
    fn a_delete_notify_does_not_replace_the_snapshot() {
        // Its shape is shared with two dozen other messages, so it is held
        // rather than allowed to throw the old snapshot away on its own.
        let mut replacement = DataReplacement::default();
        replacement.connection_reset();

        assert_eq!(
            replacement.verdict(KeyState::Session, false),
            DataVerdict::Hold
        );
        assert_eq!(
            replacement.verdict(KeyState::Session, true),
            DataVerdict::ReplaceThenApply
        );
    }

    #[test]
    fn a_property_update_does_not_replace_the_snapshot() {
        // A Mora update can come before the new connection's login data; it
        // must not throw the old snapshot away for one currency.
        let mut replacement = DataReplacement::default();
        replacement.connection_reset();
        let mut update = vec![(
            21436,
            CommandMatch::PropertyUpdate(HashMap::from([(10016, 5)])),
        )];
        assert_eq!(
            replacement.verdict_for_batch(KeyState::Session, &mut update),
            DataVerdict::Hold
        );
        let mut page = vec![(
            1,
            CommandMatch::Properties((10_010..=10_016).map(|k| (k, 3)).collect()),
        )];
        assert_eq!(
            replacement.verdict_for_batch(KeyState::Session, &mut page),
            DataVerdict::ReplaceThenApply
        );
        assert_eq!(
            replacement.verdict_for_batch(KeyState::Session, &mut update),
            DataVerdict::Apply
        );
    }

    #[test]
    fn a_resin_clock_held_before_the_login_data_joins_it() {
        let clock = || {
            CommandMatch::ResinClock(teyvatme_protocol::ResinClock {
                next_at: 1_791_000_300,
                values: vec![124, 2],
            })
        };
        let mut replacement = DataReplacement::default();
        replacement.connection_reset();
        let mut first = vec![(8226, clock())];
        assert_eq!(
            replacement.verdict_for_batch(KeyState::Session, &mut first),
            DataVerdict::Hold
        );
        let mut login = vec![(
            1,
            CommandMatch::Properties((10_010..=10_016).map(|k| (k, 3)).collect()),
        )];
        assert_eq!(
            replacement.verdict_for_batch(KeyState::Session, &mut login),
            DataVerdict::ReplaceThenApply
        );
        // After the properties it is checked against.
        assert!(matches!(
            login.as_slice(),
            [
                (1, CommandMatch::Properties(_)),
                (8226, CommandMatch::ResinClock(_))
            ]
        ));
        // A connection that never delivered data takes its clocks with it.
        replacement.connection_reset();
        let mut stale = vec![(8226, clock())];
        replacement.verdict_for_batch(KeyState::Session, &mut stale);
        replacement.clear();
        replacement.connection_reset();
        let mut next = vec![(
            1,
            CommandMatch::Properties((10_010..=10_016).map(|k| (k, 3)).collect()),
        )];
        replacement.verdict_for_batch(KeyState::Session, &mut next);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn a_property_update_changes_mora() {
        let mut data = PlayerData::new(AnimeGameData::new());
        let update = || {
            vec![(
                21436,
                CommandMatch::PropertyUpdate(HashMap::from([(10016, 141_000)])),
            )]
        };
        assert_eq!(changes_in(&update()), 1);
        assert!(apply_commands(&mut data, update()).properties);
    }

    #[test]
    fn duplicate_resets_collapse_into_one() {
        // A handshake used to arrive eight times (pktmon's copies) and each
        // copy wiped the data and logged it.
        let mut replacement = DataReplacement::default();
        assert!(replacement.connection_reset(), "the first one is news");
        for _ in 0..8 {
            assert!(!replacement.connection_reset());
        }
        assert_eq!(
            replacement.verdict(KeyState::Session, true),
            DataVerdict::ReplaceThenApply
        );
        assert_eq!(
            replacement.verdict(KeyState::Session, true),
            DataVerdict::Apply
        );
        // And the next connection is news again.
        assert!(replacement.connection_reset());
    }

    #[test]
    fn routine_parts_are_applied_without_stamping_a_data_class() {
        let mut data = PlayerData::new(AnimeGameData::new());
        let applied = apply_commands(
            &mut data,
            vec![
                (
                    1,
                    CommandMatch::Commissions(teyvatme_protocol::DailyCommissions {
                        total: 4,
                        finished: Some(2),
                    }),
                ),
                (2, CommandMatch::Welkin(30)),
            ],
        );
        assert_eq!(
            applied,
            AppliedData {
                routine: true,
                ..AppliedData::default()
            }
        );
        assert!(applied.any());
        assert!(!data.is_empty());
    }

    #[test]
    fn a_failed_reconnect_leaves_the_snapshot_exportable() {
        // End to end over the monitor's own pieces: a captured inventory, a
        // reconnect, and only dispatch-key traffic after it.
        let mut data = PlayerData::new(AnimeGameData::new());
        let mut replacement = DataReplacement::default();
        let mut item = teyvatme_protocol::r#gen::protos::Item::new();
        item.item_id = 104003;
        item.guid = 7;
        item.mut_material().count = 12;
        data.process_items(std::slice::from_ref(&item));

        replacement.connection_reset();
        // What `handle_game_packet` does with a held verdict: nothing.
        assert_eq!(
            replacement.verdict(KeyState::Dispatch, true),
            DataVerdict::Hold
        );
        assert!(!data.is_empty(), "the snapshot is still there to export");
    }
}
