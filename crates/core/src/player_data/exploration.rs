// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Exploration, told apart by the game data's ids (`gi_exploration`).
//!
//! The messages are lists of small numbers whose shape a few dozen others
//! share, and their command ids and field numbers change every patch, so each
//! arrives as a [`RecordsMessage`] candidate and is believed only when its ids
//! are exactly what the game data lists, and its other values are in range
//! (7.1 names and ids for reference):
//!
//! | message (7.1 id) | recognised by |
//! |---|---|
//! | `GetSceneAreaRsp` (23366), the Statues of The Seven | records whose ids are distinct Statue cities, at least half of the game's, each with a level 1–10 |
//! | `CityReputationDataNotify` (1167) | records of exactly two numbers whose ids are distinct reputation cities, with a level 1–10 |
//! | `PlayerOfferingDataNotify` (7055) | records whose ids are distinct offerings, at least half of the game's, each with a level 1–100 |
//! | `GetAreaExplorePointRsp` (6978) | two packed lists of the same length: distinct exploration areas, and their points |
//! | `GetScenePointRsp` (26) | the account's UID and a scene the game data has map points for, beside a packed list of that scene's point ids |
//!
//! Each reply replaces what the last one of its kind said (every one is the
//! whole state of its part). Nothing here is live: the game sends them at
//! login and when the map is opened.

use std::collections::{BTreeMap, BTreeSet};

use teyvatme_game_data::Exploration;
use teyvatme_protocol::{Record, RecordsMessage};

use super::PlayerData;

/// Statue and reputation levels: 1–10.
const LEVELS: std::ops::RangeInclusive<u64> = 1..=10;

/// Offering levels: the Sacred Sakura and the Tree of Dreams go to 50; room left.
const OFFERING_LEVELS: std::ops::RangeInclusive<u64> = 1..=100;

/// Oculi given toward a Statue's next level, at most.
const MAX_OCULI: u64 = 1_000;

/// Exploration points one area holds, at most (4 digits in 7.1).
const MAX_EXPLORE_POINTS: u64 = 1_000_000;

/// What the exploration messages said, each part as its last reply did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ExplorationState {
    /// City → (level, oculi).
    pub statues: Option<BTreeMap<u32, (u32, u32)>>,
    /// City → level.
    pub reputation: Option<BTreeMap<u32, u32>>,
    /// Offering → level.
    pub offerings: Option<BTreeMap<u32, u32>>,
    /// Area → exploration points.
    pub explore: Option<BTreeMap<u32, u32>>,
    /// Scene → (unlocked points, revealed areas).
    pub scenes: BTreeMap<u32, (BTreeSet<u32>, BTreeSet<u32>)>,
}

/// Which part a message was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorationPart {
    Statues,
    Reputation,
    Offerings,
    Explore,
    ScenePoints(u32),
}

/// The record field whose values are distinct ids all in `known`, present in
/// every record, or `None`.
fn id_field(records: &[Record], known: &BTreeSet<u32>) -> Option<u32> {
    let first = records.first()?;
    first.varints.keys().copied().find(|tag| {
        let mut seen = BTreeSet::new();
        records.iter().all(|record| {
            record
                .varints
                .get(tag)
                .and_then(|v| u32::try_from(*v).ok())
                .is_some_and(|id| known.contains(&id) && seen.insert(id))
        })
    })
}

/// The record field other than `id` present in every record with every value
/// in `range`, or `None` when there is not exactly one.
fn level_field(records: &[Record], id: u32, range: &std::ops::RangeInclusive<u64>) -> Option<u32> {
    let candidates: Vec<u32> = records
        .first()?
        .varints
        .keys()
        .copied()
        .filter(|tag| *tag != id)
        .filter(|tag| {
            records
                .iter()
                .all(|record| record.varints.get(tag).is_some_and(|v| range.contains(v)))
        })
        .collect();
    match candidates.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

fn statues(msg: &RecordsMessage, known: &Exploration) -> Option<BTreeMap<u32, (u32, u32)>> {
    let records = &msg.records;
    if records.len() * 2 < known.cities.len()
        || records
            .iter()
            .any(|r| r.varints.len() > 3 || !r.lists.is_empty())
    {
        return None;
    }
    let city = id_field(records, &known.cities)?;
    let level = level_field(records, city, &LEVELS)?;
    let mut out = BTreeMap::new();
    for record in records {
        // The third number, when the record has one, is the oculi given.
        let oculi = record
            .varints
            .iter()
            .find(|(tag, _)| **tag != city && **tag != level)
            .map_or(0, |(_, v)| *v);
        if oculi > MAX_OCULI {
            return None;
        }
        out.insert(
            record.varints[&city] as u32,
            (record.varints[&level] as u32, oculi as u32),
        );
    }
    Some(out)
}

fn reputation(msg: &RecordsMessage, known: &Exploration) -> Option<BTreeMap<u32, u32>> {
    let records = &msg.records;
    if records
        .iter()
        .any(|r| r.varints.len() != 2 || !r.lists.is_empty())
    {
        return None;
    }
    let city = id_field(records, &known.reputation_cities)?;
    let level = level_field(records, city, &LEVELS)?;
    Some(
        records
            .iter()
            .map(|r| (r.varints[&city] as u32, r.varints[&level] as u32))
            .collect(),
    )
}

fn offerings(msg: &RecordsMessage, known: &Exploration) -> Option<BTreeMap<u32, u32>> {
    let records = &msg.records;
    if records.len() * 2 < known.offerings.len() {
        return None;
    }
    let id = id_field(records, &known.offerings)?;
    // Beside the level, an offering record holds flags (0/1, absent when 0):
    // the level is the one field in every record, in range, that goes past 1.
    let level = records
        .first()?
        .varints
        .keys()
        .copied()
        .filter(|tag| *tag != id)
        .filter(|tag| {
            records.iter().all(|r| {
                r.varints
                    .get(tag)
                    .is_some_and(|v| OFFERING_LEVELS.contains(v))
            })
        })
        .filter(|tag| records.iter().any(|r| r.varints[tag] > 1))
        .collect::<Vec<_>>();
    let [level] = level.as_slice() else {
        return None;
    };
    // The levels' rewards taken: a list of levels no higher than the
    // offering's own, on the offerings already levelled. Natlan's tribal
    // chapters (ids 1-18 with a state and a start time) have none.
    let rewards_listed = records.iter().any(|r| {
        r.lists.values().any(|taken| {
            !taken.is_empty() && taken.iter().all(|l| (1..=r.varints[level]).contains(l))
        })
    });
    let timed = records
        .iter()
        .any(|r| r.varints.values().any(|v| *v >= 1_000_000_000));
    if !rewards_listed || timed {
        return None;
    }
    Some(
        records
            .iter()
            .map(|r| (r.varints[&id] as u32, r.varints[level] as u32))
            .collect(),
    )
}

fn explore(msg: &RecordsMessage, known: &Exploration) -> Option<BTreeMap<u32, u32>> {
    if !msg.records.is_empty() {
        return None;
    }
    let lists: Vec<&Vec<u64>> = msg.lists.values().collect();
    let [a, b] = lists.as_slice() else {
        return None;
    };
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let is_areas = |list: &[u64]| {
        let mut seen = BTreeSet::new();
        list.iter()
            .all(|v| u32::try_from(*v).is_ok_and(|id| known.areas.contains(&id) && seen.insert(id)))
    };
    let (areas, points) = match (is_areas(a), is_areas(b)) {
        (true, false) => (a, b),
        (false, true) => (b, a),
        _ => return None,
    };
    if points.iter().any(|p| *p > MAX_EXPLORE_POINTS) {
        return None;
    }
    Some(
        areas
            .iter()
            .zip(points.iter())
            .map(|(a, p)| (*a as u32, *p as u32))
            .collect(),
    )
}

fn scene_points(
    msg: &RecordsMessage,
    known: &Exploration,
    uid: Option<u32>,
) -> Option<(u32, BTreeSet<u32>, BTreeSet<u32>)> {
    // A reply to the client's own ask at login (7.1's carries no owner). In
    // someone else's world it is that world's map, so a message naming
    // another player is not believed.
    // A real reply always carries more than one list (unlocked, locked,
    // hidden points, revealed areas); the TCG level (a level and the rewards
    // 1..level taken) is a lone list that would read as a small scene's.
    if !msg.records.is_empty()
        || msg.lists.len() < 2
        || uid.is_some_and(|uid| {
            msg.varints
                .values()
                .any(|v| *v >= 100_000_000 && *v <= u64::from(u32::MAX) && *v != u64::from(uid))
        })
    {
        return None;
    }
    let scenes: Vec<u32> = msg
        .varints
        .values()
        .filter_map(|v| u32::try_from(*v).ok())
        .filter(|v| known.scene_points.contains_key(v))
        .collect();
    let [scene] = scenes.as_slice() else {
        return None;
    };
    let points_of_scene = &known.scene_points[scene];
    let within = |list: &[u64], set: &BTreeSet<u32>| {
        list.iter()
            .all(|v| u32::try_from(*v).is_ok_and(|id| set.contains(&id)))
    };
    // The unlocked points: the longest list of the scene's points, at least
    // twice the next (locked and hidden points are a handful).
    let mut point_lists: Vec<&Vec<u64>> = msg
        .lists
        .values()
        .filter(|list| within(list, points_of_scene))
        .collect();
    point_lists.sort_by_key(|list| std::cmp::Reverse(list.len()));
    let unlocked = *point_lists.first()?;
    if point_lists
        .get(1)
        .is_some_and(|next| next.len() * 2 > unlocked.len())
    {
        return None;
    }
    // The revealed areas: the one other list of exploration areas that is not
    // also a list of points.
    let areas: Vec<&Vec<u64>> = msg
        .lists
        .values()
        .filter(|list| !std::ptr::eq(*list, unlocked))
        .filter(|list| within(list, &known.areas) && !within(list, points_of_scene))
        .collect();
    let areas = match areas.as_slice() {
        [one] => one.iter().map(|v| *v as u32).collect(),
        _ => BTreeSet::new(),
    };
    Some((*scene, unlocked.iter().map(|v| *v as u32).collect(), areas))
}

impl PlayerData {
    /// Recognise a records message as one of the exploration parts and keep
    /// it, or `None` when it is none of them.
    pub fn apply_exploration(&mut self, msg: &RecordsMessage) -> Option<ExplorationPart> {
        let known = self.game_data.exploration().ok()?.clone();
        if !msg.records.is_empty() {
            if let Some(found) = statues(msg, &known) {
                self.exploration.statues = Some(found);
                return Some(ExplorationPart::Statues);
            }
            if let Some(found) = offerings(msg, &known) {
                self.exploration.offerings = Some(found);
                return Some(ExplorationPart::Offerings);
            }
            if let Some(found) = reputation(msg, &known) {
                self.exploration.reputation = Some(found);
                return Some(ExplorationPart::Reputation);
            }
            return None;
        }
        if let Some(found) = explore(msg, &known) {
            self.exploration.explore = Some(found);
            return Some(ExplorationPart::Explore);
        }
        if let Some((scene, points, areas)) = scene_points(msg, &known, self.account_uid()) {
            self.exploration.scenes.insert(scene, (points, areas));
            return Some(ExplorationPart::ScenePoints(scene));
        }
        None
    }
}
