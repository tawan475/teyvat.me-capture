// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Endgame progress, recognised by shape: the Imaginarium Theater's season
//! state (`RoleCombatDataNotify` at every login, and the reply when the
//! Theater is opened; 7.1 ids 1982 and 2068) and the Stygian Onslaught's
//! records (the reply when its menu is opened; 7.1 id 4056).
//!
//! Both come back as *candidates*: their field numbers and most of their
//! names are reshuffled every version, so what can be told here is the
//! shape -- a month-long schedule window, records stamped with a
//! millisecond time and a seven-digit schedule id -- and the caller settles
//! the rest against the game data's season tables (which season the window
//! is, which rows are its difficulties) and the account (whose team a record
//! holds). Other players' UIDs are read only to tell them from the account's
//! and are never exported.
//!
//! Run over every command of six recorded 7.1 sessions (3.4 million
//! messages, unions unwrapped) and the users' logs: each matcher claimed
//! only the message it is for.

use std::collections::BTreeMap;

use protobuf::UnknownValueRef::{Fixed32, Fixed64, LengthDelimited, Varint};

use crate::wire::{self, Value};

/// Plausible unix seconds for a season's bounds (2020-09-15 … 2096).
const SEASON_TIMES: std::ops::RangeInclusive<u64> = 1_600_000_000..=4_000_000_000;

/// A Theater season is a calendar month: `end + 1 - start` in days.
const SEASON_DAYS: std::ops::RangeInclusive<u64> = 28..=31;

/// The most acts a difficulty has (Visionary and Lunar Mode: 10, with room).
pub const MAX_THEATER_ACTS: u64 = 12;

/// Plausible unix milliseconds for a record's time (the mode exists since
/// 2025-06-25).
const RECORD_TIMES_MS: std::ops::RangeInclusive<u64> = 1_750_000_000_000..=4_000_000_000_000;

/// Stygian schedule ids are seven digits (7.1: 5 269 001-5 269 013).
const STYGIAN_SCHEDULE_IDS: std::ops::RangeInclusive<u64> = 1_000_000..=99_999_999;

/// Stygian difficulties (7.1: six rows).
const STYGIAN_DIFFICULTIES: std::ops::RangeInclusive<u64> = 1..=20;

/// Fights per record (three bosses; room for more).
const MAX_STYGIAN_FIGHTS: usize = 6;

/// A fight's time in seconds (the game's own limit is minutes).
const MAX_FIGHT_SECONDS: u64 = 36_000;

/// Records a message may hold (one per season played, solo and co-op).
const MAX_STYGIAN_RECORDS: usize = 200;

/// Player UIDs: nine or ten digits.
const UIDS: std::ops::RangeInclusive<u64> = 100_000_000..=9_999_999_999;

/// Avatar ids (`10000002`-`10000999`).
const AVATAR_IDS: std::ops::RangeInclusive<u64> = 10_000_000..=10_009_999;

/// The Imaginarium Theater's season state, as its message holds it: the
/// season's window and the small numbers and lists beside it. Which season
/// and difficulty they are is settled against the game data by the caller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TheaterDetail {
    /// The season's opening, unix seconds (04:00 server time on the first).
    pub start: u64,
    /// The season's last second.
    pub end: u64,
    /// Every lone varint under 100,000 (the season id and the chosen
    /// difficulty's row id are among them).
    pub small: Vec<u64>,
    /// Every packed list of numbers (the acts cleared are one of them).
    pub lists: Vec<Vec<u64>>,
}

impl TheaterDetail {
    /// The acts cleared this season: the longest list that is exactly
    /// `1..=n` (the game lists the cleared acts, and the acts whose rewards
    /// were claimed, as such lists; the cleared ones are never fewer).
    pub fn acts_cleared(&self) -> u32 {
        self.lists
            .iter()
            .filter(|list| {
                !list.is_empty()
                    && list.len() as u64 <= MAX_THEATER_ACTS
                    && list.iter().enumerate().all(|(i, v)| *v == i as u64 + 1)
            })
            .map(|list| list.len() as u32)
            .max()
            .unwrap_or(0)
    }
}

/// One Stygian Onslaught record: a season's run at a difficulty, as the
/// game keeps it (solo, or a co-op run with the players of that team).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StygianRecord {
    /// `LeyLineChallengeExcelConfigData.scheduleId`.
    pub schedule: u32,
    pub difficulty: u32,
    /// When it was set, unix milliseconds.
    pub at_ms: u64,
    /// The fights, in the order sent (the game numbers them from 1).
    pub fights: Vec<StygianFight>,
}

/// One boss of a record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StygianFight {
    /// Seconds.
    pub time: u32,
    pub finished: bool,
    /// Each player's team: their UID and the avatar ids. In a solo run one
    /// team, the account's own.
    pub teams: Vec<StygianTeam>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StygianTeam {
    pub uid: u64,
    pub avatars: Vec<u32>,
}

fn as_varint(value: &Value<'_>) -> Option<u64> {
    match value {
        Varint(v) => Some(*v),
        _ => None,
    }
}

fn as_bytes<'a>(value: &Value<'a>) -> Option<&'a [u8]> {
    match value {
        LengthDelimited(b) => Some(b),
        _ => None,
    }
}

/// `bytes` as packed varints, or `None` if they are not exactly that.
fn packed(bytes: &[u8]) -> Option<Vec<u64>> {
    let mut out = Vec::new();
    let mut value = 0u64;
    let mut shift = 0u32;
    for &byte in bytes {
        if shift >= 64 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            out.push(value);
            value = 0;
            shift = 0;
        } else {
            shift += 7;
        }
    }
    (shift == 0 && !out.is_empty()).then_some(out)
}

/// Whether `start`/`end` bound one calendar month: `end` is the last second
/// before the next opening, `start` on the hour.
fn is_month_window(start: u64, end: u64) -> bool {
    if !SEASON_TIMES.contains(&start) || !SEASON_TIMES.contains(&end) || end <= start {
        return false;
    }
    let seconds = end + 1 - start;
    start.is_multiple_of(3600)
        && seconds.is_multiple_of(86_400)
        && SEASON_DAYS.contains(&(seconds / 86_400))
}

/// Read `bytes` as the Theater's season detail, or `None` if it is not one:
/// two lone timestamps bounding one calendar month, a few small numbers and
/// lists, nothing fixed-width.
fn theater_detail(bytes: &[u8]) -> Option<TheaterDetail> {
    let fields = wire::fields(bytes)?;
    let mut times = Vec::new();
    let mut small = Vec::new();
    let mut lists = Vec::new();
    for values in fields.values() {
        match values.as_slice() {
            [Varint(v)] if SEASON_TIMES.contains(v) => times.push(*v),
            [Varint(v)] if *v < 100_000 => small.push(*v),
            [Varint(_)] => {}
            [LengthDelimited(one)] => {
                if let Some(list) = packed(one) {
                    lists.push(list);
                }
            }
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {}
            values if values.iter().all(|v| matches!(v, Varint(_))) => {
                // An unpacked repeated numeric field.
                lists.push(values.iter().filter_map(as_varint).collect());
            }
            _ => return None,
        }
    }
    // The two times bounding a month; a third timestamp-sized number (a
    // first-clear time among the lone fields) would make the pick ambiguous.
    let mut windows = Vec::new();
    for (i, start) in times.iter().enumerate() {
        for (j, end) in times.iter().enumerate() {
            if i != j && is_month_window(*start, *end) {
                windows.push((*start, *end));
            }
        }
    }
    let [(start, end)] = windows.as_slice() else {
        return None;
    };
    if small.is_empty() {
        return None;
    }
    Some(TheaterDetail {
        start: *start,
        end: *end,
        small,
        lists,
    })
}

/// Recover the Theater's season state from a `RoleCombatDataNotify` payload
/// (or the reply to opening the Theater), or `None` if it is not one: one
/// submessage reading as the season detail beside at most other submessages
/// and one lone number (a reply's return code).
///
/// Which season the window is, and which of the small numbers is the
/// difficulty chosen, the caller settles against the game data's seasons.
pub fn matches_theater_notify(data: &[u8]) -> Option<TheaterDetail> {
    let fields = wire::fields(data)?;
    if fields.is_empty() {
        return None;
    }
    let mut found = None;
    let mut lone_numbers = 0;
    for values in fields.values() {
        match values.as_slice() {
            [Varint(_)] => lone_numbers += 1,
            [LengthDelimited(bytes)] => {
                if let Some(detail) = theater_detail(bytes)
                    && found.replace(detail).is_some()
                {
                    return None;
                }
            }
            _ => return None,
        }
    }
    (lone_numbers <= 1).then_some(found).flatten()
}

/// Read a team: a player's UID beside a list of avatars (each a submessage
/// with an avatar id among its numbers), nothing else varying.
fn stygian_team(bytes: &[u8]) -> Option<StygianTeam> {
    let fields = wire::fields(bytes)?;
    let mut uid = None;
    let mut avatars = None;
    for values in fields.values() {
        match values.as_slice() {
            [Varint(v)] if UIDS.contains(v) => {
                if uid.replace(*v).is_some() {
                    return None;
                }
            }
            [Varint(_)] => {}
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {
                let ids: Option<Vec<u32>> = values
                    .iter()
                    .map(|v| {
                        let avatar = wire::fields(as_bytes(v)?)?;
                        let mut id = None;
                        for inner in avatar.values() {
                            if let [Varint(n)] = inner.as_slice()
                                && AVATAR_IDS.contains(n)
                                && id.replace(*n as u32).is_some()
                            {
                                return None;
                            }
                        }
                        id
                    })
                    .collect();
                match ids {
                    Some(ids) if !ids.is_empty() => {
                        if avatars.replace(ids).is_some() {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    Some(StygianTeam {
        uid: uid?,
        avatars: avatars?,
    })
}

/// A fight's lone numbers by tag, and its teams.
struct FightRead {
    numbers: BTreeMap<u32, u64>,
    teams: Vec<StygianTeam>,
}

fn stygian_fight(bytes: &[u8]) -> Option<FightRead> {
    let fields = wire::fields(bytes)?;
    let mut numbers = BTreeMap::new();
    let mut teams = None;
    for (tag, values) in &fields {
        match values.as_slice() {
            [Varint(v)] => {
                numbers.insert(*tag, *v);
            }
            [Fixed32(_) | Fixed64(_)] => return None,
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {
                // The teams, or the equipment guids / the boss' figures:
                // the one list whose every entry reads as a team is the teams.
                let read: Option<Vec<StygianTeam>> =
                    values.iter().map(|v| stygian_team(as_bytes(v)?)).collect();
                if let Some(read) = read
                    && teams.replace(read).is_some()
                {
                    return None;
                }
            }
            values if values.iter().all(|v| matches!(v, Varint(_))) => {}
            _ => return None,
        }
    }
    Some(FightRead {
        numbers,
        teams: teams?,
    })
}

/// Read a record: one millisecond time (fixed-width), a seven-digit
/// schedule id and a small difficulty among its lone numbers, and one list
/// of fights whose numbers are told apart across the fights: the one that
/// counts 1, 2, 3… is the index, the one that is 0 or 1 is "finished", and
/// the one left is the time.
fn stygian_record(bytes: &[u8]) -> Option<StygianRecord> {
    let fields = wire::fields(bytes)?;
    let mut at_ms = None;
    let mut schedule = None;
    let mut small = Vec::new();
    let mut fights: Option<Vec<FightRead>> = None;
    for values in fields.values() {
        match values.as_slice() {
            [Fixed64(ms)] if RECORD_TIMES_MS.contains(ms) => {
                if at_ms.replace(*ms).is_some() {
                    return None;
                }
            }
            [Fixed64(_) | Fixed32(_)] => return None,
            [Varint(v)] if STYGIAN_SCHEDULE_IDS.contains(v) => {
                if schedule.replace(*v as u32).is_some() {
                    return None;
                }
            }
            [Varint(v)] if STYGIAN_DIFFICULTIES.contains(v) => small.push(*v as u32),
            [Varint(_)] => {}
            values
                if values.iter().all(|v| matches!(v, LengthDelimited(_)))
                    && values.len() <= MAX_STYGIAN_FIGHTS =>
            {
                let read: Option<Vec<FightRead>> =
                    values.iter().map(|v| stygian_fight(as_bytes(v)?)).collect();
                if let Some(read) = read
                    && fights.replace(read).is_some()
                {
                    return None;
                }
            }
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {}
            _ => return None,
        }
    }
    // The difficulty is the small number beside a flag (a bool, 1 when set,
    // which an unset flag leaves out): one small number, or two of which
    // one is 1.
    small.sort_unstable();
    let difficulty = match small.as_slice() {
        [one] => *one,
        [1, other] => *other,
        _ => return None,
    };
    let fights = fights?;
    // Tell the fights' numbers apart by their values across the fights. A
    // number that is 0 is left off the wire, so a tag any fight carries is a
    // column, 0 where absent (an unfinished fight has no time and no flag).
    let mut tags: Vec<u32> = fights
        .iter()
        .flat_map(|f| f.numbers.keys().copied())
        .collect();
    tags.sort_unstable();
    tags.dedup();
    let column = |tag: u32| -> Vec<u64> {
        fights
            .iter()
            .map(|f| f.numbers.get(&tag).copied().unwrap_or(0))
            .collect()
    };
    let index_tag = tags.iter().copied().find(|tag| {
        column(*tag)
            .iter()
            .enumerate()
            .all(|(i, v)| *v == i as u64 + 1)
    });
    let finished_tag = tags
        .iter()
        .copied()
        .filter(|tag| Some(*tag) != index_tag)
        .filter(|tag| column(*tag).iter().all(|v| *v <= 1))
        .collect::<Vec<_>>();
    let time_tag = tags
        .iter()
        .copied()
        .filter(|tag| Some(*tag) != index_tag && !finished_tag.contains(tag))
        .filter(|tag| column(*tag).iter().all(|v| *v <= MAX_FIGHT_SECONDS))
        .collect::<Vec<_>>();
    // A fight that was not finished has no time, and the index may be absent
    // from a single-fight record: what is needed is one "finished" column
    // and at most one time column.
    let [finished_tag] = finished_tag.as_slice() else {
        return None;
    };
    let time_tag = match time_tag.as_slice() {
        [] => None,
        [tag] => Some(*tag),
        _ => return None,
    };
    let fights = fights
        .into_iter()
        .map(|f| StygianFight {
            time: time_tag.map_or(0, |tag| f.numbers.get(&tag).copied().unwrap_or(0) as u32),
            finished: f.numbers.get(finished_tag) == Some(&1),
            teams: f.teams,
        })
        .collect();
    Some(StygianRecord {
        schedule: schedule?,
        difficulty,
        at_ms: at_ms?,
        fights,
    })
}

/// Recover the Stygian Onslaught records from the reply to opening its
/// menu, or `None` if it is not one: one submessage holding lists of
/// records (solo runs, co-op runs) beside other lists and numbers, under a
/// top message of that submessage and at most a return code.
///
/// The caller keeps the records of seasons the game data knows and the
/// account's own team in each.
pub fn matches_stygian_records(data: &[u8]) -> Option<Vec<StygianRecord>> {
    let top = wire::fields(data)?;
    let mut body = None;
    let mut lone_numbers = 0;
    for values in top.values() {
        match values.as_slice() {
            [Varint(_)] => lone_numbers += 1,
            [LengthDelimited(bytes)] => {
                if body.replace(*bytes).is_some() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if lone_numbers > 1 {
        return None;
    }
    let fields = wire::fields(body?)?;
    let mut records = Vec::new();
    for values in fields.values() {
        if !values.iter().all(|v| matches!(v, LengthDelimited(_))) {
            continue;
        }
        let read: Option<Vec<StygianRecord>> = values
            .iter()
            .map(|v| stygian_record(as_bytes(v)?))
            .collect();
        if let Some(read) = read {
            records.extend(read);
        }
    }
    (!records.is_empty() && records.len() <= MAX_STYGIAN_RECORDS).then_some(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut value: u64, out: &mut Vec<u8>) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn field_varint(tag: u32, value: u64, out: &mut Vec<u8>) {
        varint(u64::from(tag << 3), out);
        varint(value, out);
    }

    fn field_bytes(tag: u32, bytes: &[u8], out: &mut Vec<u8>) {
        varint(u64::from(tag << 3 | 2), out);
        varint(bytes.len() as u64, out);
        out.extend_from_slice(bytes);
    }

    fn field_fixed64(tag: u32, value: u64, out: &mut Vec<u8>) {
        varint(u64::from(tag << 3 | 1), out);
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn packed_list(tag: u32, values: &[u64], out: &mut Vec<u8>) {
        let mut inner = Vec::new();
        for v in values {
            varint(*v, &mut inner);
        }
        field_bytes(tag, &inner, out);
    }

    fn pair(key: u64, value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        field_varint(1, key, &mut out);
        field_varint(2, value, &mut out);
        out
    }

    /// The 7.1 layout of `_combat_detail` for a season cleared to act 10 at
    /// difficulty row 129, with the season window of 2026-11 (Asia).
    fn theater_detail_bytes(acts: &[u64], rewards: &[u64]) -> Vec<u8> {
        let mut d = Vec::new();
        // last_schedule_monthly_brief {schedule_id 30, acts 10}
        let mut brief = Vec::new();
        field_varint(1, 30, &mut brief);
        field_varint(11, 10, &mut brief);
        field_bytes(1, &brief, &mut d);
        field_varint(4, 1_796_068_799, &mut d); // next_schedule_change_time
        packed_list(7, &[10000130, 10000126, 10000125], &mut d);
        if !rewards.is_empty() {
            packed_list(8, rewards, &mut d);
        }
        field_varint(9, 31, &mut d); // cur_schedule_id
        field_varint(11, 10, &mut d);
        field_varint(15, 25, &mut d);
        for key in 1..=10u64 {
            field_bytes(45, &pair(key, 1_719_968_007), &mut d);
        }
        field_varint(121, 1_793_476_800, &mut d); // schedule_start_time
        field_varint(244, 10, &mut d);
        field_varint(433, 2, &mut d);
        field_varint(473, 5, &mut d);
        // The milestone lists come with the acts, once the season was played.
        if !acts.is_empty() {
            packed_list(546, &[3, 6, 9, 12], &mut d);
            packed_list(911, &[1, 2], &mut d);
            packed_list(1010, acts, &mut d);
        }
        field_varint(1171, 2, &mut d);
        field_varint(1212, 10, &mut d);
        field_varint(1224, 12, &mut d);
        packed_list(1249, &[3, 4, 5], &mut d);
        field_varint(1386, 3, &mut d);
        field_varint(1435, 2, &mut d);
        for key in [2u64, 3, 4, 5, 6, 8] {
            field_bytes(1774, &pair(key, 1), &mut d);
        }
        field_varint(1885, 1, &mut d);
        field_varint(1889, 1, &mut d);
        field_varint(2032, 129, &mut d); // the difficulty chosen
        d
    }

    #[test]
    fn the_login_notify_gives_the_season_window_and_the_acts() {
        let mut data = Vec::new();
        field_bytes(
            2,
            &theater_detail_bytes(&[1, 2, 3, 4, 5, 6, 7], &[1, 2, 3]),
            &mut data,
        );
        let detail = matches_theater_notify(&data).unwrap();
        assert_eq!(detail.start, 1_793_476_800);
        assert_eq!(detail.end, 1_796_068_799);
        assert!(detail.small.contains(&31) && detail.small.contains(&129));
        // The cleared acts (7), not the rewards claimed (3).
        assert_eq!(detail.acts_cleared(), 7);
    }

    #[test]
    fn a_season_not_entered_yet_has_no_acts() {
        let mut data = Vec::new();
        field_bytes(2, &theater_detail_bytes(&[], &[]), &mut data);
        assert_eq!(matches_theater_notify(&data).unwrap().acts_cleared(), 0);
    }

    #[test]
    fn the_reply_to_opening_the_theater_is_read_too() {
        // `_combat_detail` on 4, `_battle_data` on 5 (a submessage that is not
        // a detail), retcode on 7.
        let mut data = Vec::new();
        let mut battle = Vec::new();
        field_bytes(1, &[], &mut battle);
        field_varint(5, 3, &mut battle);
        field_bytes(
            4,
            &theater_detail_bytes(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[]),
            &mut data,
        );
        field_bytes(5, &battle, &mut data);
        field_varint(7, 0, &mut data);
        assert_eq!(matches_theater_notify(&data).unwrap().acts_cleared(), 10);
    }

    #[test]
    fn a_half_month_window_is_not_a_theater_season() {
        // The Spiral Abyss' brief: start and end a fortnight apart.
        let mut d = Vec::new();
        field_varint(1, 1_793_476_800, &mut d);
        field_varint(2, 1_793_476_800 + 15 * 86_400 - 1, &mut d);
        field_varint(3, 12, &mut d);
        let mut data = Vec::new();
        field_bytes(1, &d, &mut data);
        assert_eq!(matches_theater_notify(&data), None);
        // Two month windows (this season's and the next's): ambiguous, refused.
        let mut d = Vec::new();
        field_varint(1, 1_793_476_800, &mut d);
        field_varint(2, 1_796_068_799, &mut d);
        field_varint(3, 1_796_068_800, &mut d);
        field_varint(4, 1_798_747_199, &mut d);
        field_varint(5, 31, &mut d);
        let mut data = Vec::new();
        field_bytes(1, &d, &mut data);
        assert_eq!(matches_theater_notify(&data), None);
    }

    fn avatar_info(id: u64) -> Vec<u8> {
        let mut out = Vec::new();
        field_varint(1, id, &mut out);
        field_varint(2, 90, &mut out);
        field_varint(3, 6, &mut out);
        field_varint(4, 1, &mut out);
        out
    }

    fn team(uid: u64, avatars: &[u64]) -> Vec<u8> {
        let mut out = Vec::new();
        field_varint(1, uid, &mut out);
        for id in avatars {
            field_bytes(2, &avatar_info(*id), &mut out);
        }
        out
    }

    /// A fight as the game sends it: a 0 time or an unset flag is left off.
    fn fight(index: u64, time: u64, finished: bool, teams: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        if time > 0 {
            field_varint(1, time, &mut out);
        }
        packed_list(
            3,
            &[3_436_616_016_015_148_319, 3_436_616_016_015_382_416],
            &mut out,
        );
        if finished {
            field_varint(9, 1, &mut out);
        }
        let mut boss = Vec::new();
        field_varint(1, 10000096, &mut boss);
        field_varint(2, 367_183, &mut boss);
        field_bytes(10, &boss, &mut out);
        field_varint(14, index, &mut out);
        for t in teams {
            field_bytes(15, t, &mut out);
        }
        out
    }

    fn record(
        schedule: u64,
        difficulty: u64,
        at_ms: u64,
        fights: &[Vec<u8>],
        uids: &[u64],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        for uid in uids {
            let mut player = Vec::new();
            field_varint(1, 1, &mut player);
            field_varint(2, *uid, &mut player);
            field_bytes(3, &[], &mut player); // profile_picture
            field_bytes(2, &player, &mut out);
        }
        field_varint(4, schedule, &mut out);
        for f in fights {
            field_bytes(7, f, &mut out);
        }
        field_fixed64(10, at_ms, &mut out);
        field_varint(11, difficulty, &mut out);
        // A flag the game sets on some records (7.1: `AJHCBEKBNJO`), which
        // reads like a second small number.
        if difficulty.is_multiple_of(2) {
            field_varint(14, 1, &mut out);
        }
        out
    }

    const ME: u64 = 812_345_678;
    const FRIEND: u64 = 823_456_789;

    fn records_message(solo: &[Vec<u8>], coop: &[Vec<u8>]) -> Vec<u8> {
        let mut body = Vec::new();
        for r in coop {
            field_bytes(2, r, &mut body);
        }
        field_varint(3, 18, &mut body);
        field_varint(4, 5_269_012, &mut body);
        for r in solo {
            field_bytes(5, r, &mut body);
        }
        field_varint(6, 6, &mut body);
        let mut best = Vec::new();
        field_varint(1, 5_269_012, &mut best);
        field_varint(2, 3, &mut best);
        field_bytes(15, &best, &mut body);
        let mut data = Vec::new();
        field_bytes(5, &body, &mut data);
        field_varint(14, 0, &mut data);
        data
    }

    #[test]
    fn solo_and_coop_records_are_read_with_their_teams() {
        let solo = record(
            5_269_012,
            6,
            1_790_972_494_088,
            &[
                fight(1, 51, true, &[team(ME, &[10000096, 10000032, 10000088])]),
                fight(2, 64, true, &[team(ME, &[10000030, 10000046])]),
                fight(3, 0, false, &[team(ME, &[10000002])]),
            ],
            &[ME],
        );
        let coop = record(
            5_269_011,
            4,
            1_787_523_203_301,
            &[fight(
                1,
                40,
                true,
                &[team(ME, &[10000096]), team(FRIEND, &[10000030])],
            )],
            &[ME, FRIEND],
        );
        let found = matches_stygian_records(&records_message(&[solo], &[coop])).unwrap();
        assert_eq!(found.len(), 2);
        let coop = &found[0];
        assert_eq!(
            (coop.schedule, coop.difficulty, coop.at_ms),
            (5_269_011, 4, 1_787_523_203_301)
        );
        assert_eq!(coop.fights[0].teams.len(), 2);
        let solo = &found[1];
        assert_eq!(solo.schedule, 5_269_012);
        assert_eq!(solo.fights.len(), 3);
        assert_eq!((solo.fights[0].time, solo.fights[0].finished), (51, true));
        assert_eq!((solo.fights[2].time, solo.fights[2].finished), (0, false));
        assert_eq!(solo.fights[1].teams[0].uid, ME);
        assert_eq!(solo.fights[1].teams[0].avatars, [10000030, 10000046]);
    }

    #[test]
    fn a_record_without_a_time_or_with_two_is_refused() {
        let mut no_time = Vec::new();
        field_varint(4, 5_269_012, &mut no_time);
        field_bytes(
            7,
            &fight(1, 51, true, &[team(ME, &[10000096])]),
            &mut no_time,
        );
        field_varint(11, 6, &mut no_time);
        assert_eq!(
            matches_stygian_records(&records_message(&[no_time], &[])),
            None
        );
        let mut two = record(
            5_269_012,
            6,
            1_790_972_494_088,
            &[fight(1, 51, true, &[team(ME, &[10000096])])],
            &[ME],
        );
        field_fixed64(12, 1_790_972_494_089, &mut two);
        assert_eq!(matches_stygian_records(&records_message(&[two], &[])), None);
    }

    #[test]
    fn a_message_of_other_lists_is_not_records() {
        let mut body = Vec::new();
        packed_list(1, &[1, 2, 3], &mut body);
        field_bytes(2, &pair(5_269_012, 3), &mut body);
        let mut data = Vec::new();
        field_bytes(5, &body, &mut data);
        assert_eq!(matches_stygian_records(&data), None);
        assert_eq!(matches_stygian_records(&[]), None);
        assert_eq!(matches_theater_notify(&[]), None);
    }
}
