// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Endgame progress, recognised by shape: the Imaginarium Theater's season
//! state (`RoleCombatDataNotify` at every login, and the reply when the
//! Theater is opened; 7.1 ids 1982 and 2068), the Stygian Onslaught's
//! records (the reply when its menu is opened; 7.1 id 4056) and the Spiral
//! Abyss's record book (the reply when its Challenge Summary opens; 7.2 id
//! 23737).
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

use std::collections::{BTreeMap, BTreeSet};

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

/// Players in one co-op run (the game's own limit is four).
const MAX_STYGIAN_PLAYERS: usize = 4;

/// Character levels (100 since 6.x, with room).
const LEVELS: std::ops::RangeInclusive<u64> = 1..=120;

/// Constellations.
const MAX_CONSTELLATION: u64 = 6;

/// Profile picture ids (`ProfilePictureExcelConfigData` 7.2: 1 … 900004)
/// and frame ids (`ProfileFrameExcelConfigData`: 100000-100021).
const PICTURE_IDS: std::ops::RangeInclusive<u64> = 1..=99_999_999;

/// Frame ids, with room: what tells the frame from the picture when every
/// player of a message wears one (picture ids reach into this range too).
const FRAME_IDS: std::ops::RangeInclusive<u64> = 100_000..=100_999;

/// `ProfilePicture`'s picture field (the same in every client 6.3-7.2: 1
/// avatar, 2 costume, 3 picture, 4 frame); used only when the values can't
/// settle it.
const PICTURE_FIELD: u32 = 3;

/// A nickname's length in characters (the game allows 14; room for more).
const MAX_NICKNAME_CHARS: usize = 64;

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
    /// The players, as the record lists them (one in a solo run); empty when
    /// the list did not read (each entry must be a UID with one position).
    pub players: Vec<StygianPlayer>,
}

/// One player of a record, as the game's record screen shows them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StygianPlayer {
    pub uid: u64,
    /// The position, 1-4: the game's 1P, 2P, 3P, 4P.
    pub index: u32,
    /// The nickname (7.2 sends it in co-op records, not in solo ones).
    pub nickname: Option<String>,
    /// `ProfilePictureExcelConfigData.id` (settled across the message, see
    /// `settle_pictures`).
    pub picture: Option<u32>,
    /// The picture submessage's numbers by field, as sent: the picture and,
    /// when one is worn, the frame (7.2: `{3: 4801, 4: 100011}`).
    pub picture_numbers: BTreeMap<u32, u64>,
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
    /// The strongest single strike of the fight, when its figures came.
    pub strike: Option<Figure>,
    /// The highest total damage dealt by one character.
    pub damage: Option<Figure>,
}

/// A fight's figure: a value and, when the message leaves no doubt, the
/// avatar it belongs to.
///
/// The figures come as two avatar ids and two numbers in one submessage.
/// The numbers are told apart by value (one strike is never more than one
/// character's total damage), the avatars are not: when the two differ,
/// which goes with which is not on the wire in any form a reshuffled field
/// number cannot swap, so `by` is given only when both name one character.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Figure {
    pub value: u64,
    pub by: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StygianTeam {
    pub uid: u64,
    pub avatars: Vec<u32>,
    /// The same characters with what the record holds of each.
    pub members: Vec<StygianMember>,
}

/// A character of a record's team as it was at the time of the record.
///
/// Each avatar entry is the id and a few lone numbers (7.2: level, costume,
/// constellation, element), whose field numbers are reshuffled every
/// version. They are told apart across every character of the message
/// (`settle_members`): the level is the number every character has, 1-120
/// and above 8 for some; the constellation the number of 0-6 that some
/// characters leave out (a 0 is left off the wire, and C0 characters are in
/// nearly every list). The element is 1-8 and always there too, which is
/// why a list without a single C0 character gives no constellations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StygianMember {
    pub id: u32,
    pub level: Option<u32>,
    pub constellation: Option<u32>,
    /// The entry's other lone numbers by field, as sent.
    pub numbers: BTreeMap<u32, u64>,
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
                let read: Option<Vec<StygianMember>> = values
                    .iter()
                    .map(|v| {
                        let avatar = wire::fields(as_bytes(v)?)?;
                        let mut id = None;
                        let mut numbers = BTreeMap::new();
                        for (tag, inner) in &avatar {
                            if let [Varint(n)] = inner.as_slice() {
                                if AVATAR_IDS.contains(n) {
                                    if id.replace(*n as u32).is_some() {
                                        return None;
                                    }
                                } else {
                                    numbers.insert(*tag, *n);
                                }
                            }
                        }
                        Some(StygianMember {
                            id: id?,
                            numbers,
                            ..Default::default()
                        })
                    })
                    .collect();
                match read {
                    Some(read) if !read.is_empty() => {
                        if avatars.replace(read).is_some() {
                            return None;
                        }
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    let members = avatars?;
    Some(StygianTeam {
        uid: uid?,
        avatars: members.iter().map(|m| m.id).collect(),
        members,
    })
}

/// `bytes` as a profile picture: a submessage of one to three lone numbers
/// of id size (7.2: `{3: 1001}`, or `{3: 4801, 4: 100011}` with a frame).
/// Which is the picture is settled across the message (`settle_pictures`).
fn picture(bytes: &[u8]) -> Option<BTreeMap<u32, u64>> {
    let fields = wire::fields(bytes)?;
    let mut numbers = BTreeMap::new();
    for (tag, values) in &fields {
        match values.as_slice() {
            [Varint(v)] if PICTURE_IDS.contains(v) => {
                numbers.insert(*tag, *v);
            }
            _ => return None,
        }
    }
    (1..=3).contains(&numbers.len()).then_some(numbers)
}

/// Settle which field of the players' picture submessages is the picture:
/// the one every player has (a frame is optional: the default one is left
/// out); when every player also wears a frame, the field whose values are
/// all frame-sized beside one that is not. When both are frame-sized (a
/// quest picture, 100010-100300, beside a frame) the picture is the
/// `ProfilePicture` field the game has used since 6.3, 3 (`PICTURE_FIELD`).
/// Otherwise no pictures are read.
fn settle_pictures(records: &mut [StygianRecord]) {
    let maps = || {
        records
            .iter()
            .flat_map(|r| &r.players)
            .map(|p| &p.picture_numbers)
            .filter(|m| !m.is_empty())
    };
    let tags: BTreeSet<u32> = maps().flat_map(|m| m.keys().copied()).collect();
    let everywhere: Vec<u32> = tags
        .iter()
        .copied()
        .filter(|t| maps().all(|m| m.contains_key(t)))
        .collect();
    let frame_sized = |t: &u32| maps().all(|m| FRAME_IDS.contains(&m[t]));
    let tag = match everywhere.as_slice() {
        [one] => Some(*one),
        [a, b] => match (frame_sized(a), frame_sized(b)) {
            (true, false) => Some(*b),
            (false, true) => Some(*a),
            (true, true) => everywhere.contains(&PICTURE_FIELD).then_some(PICTURE_FIELD),
            (false, false) => None,
        },
        _ => None,
    };
    for player in records.iter_mut().flat_map(|r| &mut r.players) {
        player.picture = tag
            .and_then(|t| player.picture_numbers.get(&t))
            .map(|v| *v as u32);
    }
}

/// `bytes` as a nickname: printable text of a nickname's length.
fn nickname(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let chars = text.chars().count();
    ((1..=MAX_NICKNAME_CHARS).contains(&chars) && !text.chars().any(char::is_control))
        .then(|| text.to_string())
}

/// Read a record's player: a UID, one position of 1-4, and optionally a
/// nickname (text) and a profile picture (a submessage with its id). Text
/// that also reads as a picture submessage is told apart by the other one;
/// two of either, and it gives neither.
fn stygian_player(bytes: &[u8]) -> Option<StygianPlayer> {
    let fields = wire::fields(bytes)?;
    let mut uid = None;
    let mut positions = Vec::new();
    let mut texts = Vec::new();
    let mut pictures = Vec::new();
    let mut either = Vec::new();
    for values in fields.values() {
        match values.as_slice() {
            [Varint(v)] if UIDS.contains(v) => {
                if uid.replace(*v).is_some() {
                    return None;
                }
            }
            [Varint(v)] if (1..=MAX_STYGIAN_PLAYERS as u64).contains(v) => {
                positions.push(*v as u32)
            }
            [Varint(_)] => {}
            [LengthDelimited(b)] => match (nickname(b), picture(b)) {
                (Some(text), Some(id)) => either.push((text, id)),
                (Some(text), None) => texts.push(text),
                (None, Some(id)) => pictures.push(id),
                (None, None) => {}
            },
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {}
            _ => return None,
        }
    }
    let [index] = positions.as_slice() else {
        return None;
    };
    if let [(text, numbers)] = either.as_slice() {
        match (texts.len(), pictures.len()) {
            (0, 1) => texts.push(text.clone()),
            (1, 0) => pictures.push(numbers.clone()),
            _ => {}
        }
    }
    Some(StygianPlayer {
        uid: uid?,
        index: *index,
        nickname: only_one(texts),
        picture: None,
        picture_numbers: only_one(pictures).unwrap_or_default(),
    })
}

/// The list's one entry, or `None` for none or several.
fn only_one<T>(list: Vec<T>) -> Option<T> {
    let mut list = list.into_iter();
    let first = list.next()?;
    list.next().is_none().then_some(first)
}

/// A record's player list: every entry a player, UIDs and positions unique.
fn stygian_players(values: &[Value<'_>]) -> Option<Vec<StygianPlayer>> {
    if values.len() > MAX_STYGIAN_PLAYERS {
        return None;
    }
    let players: Vec<StygianPlayer> = values
        .iter()
        .map(|v| stygian_player(as_bytes(v)?))
        .collect::<Option<_>>()?;
    let uids: BTreeSet<u64> = players.iter().map(|p| p.uid).collect();
    let positions: BTreeSet<u32> = players.iter().map(|p| p.index).collect();
    (uids.len() == players.len() && positions.len() == players.len()).then_some(players)
}

/// Settle each character's level and constellation across every character
/// of the message (see `StygianMember`); a field that does not stand out
/// leaves that value unread for all.
fn settle_members(records: &mut [StygianRecord]) {
    let members = || {
        records
            .iter()
            .flat_map(|r| &r.fights)
            .flat_map(|f| &f.teams)
            .flat_map(|t| &t.members)
    };
    let value = |m: &StygianMember, tag: u32| m.numbers.get(&tag).copied().unwrap_or(0);
    let tags: BTreeSet<u32> = members().flat_map(|m| m.numbers.keys().copied()).collect();
    let everywhere = |tag: u32| members().all(|m| m.numbers.contains_key(&tag));
    let level = only_one(
        tags.iter()
            .copied()
            .filter(|t| everywhere(*t))
            .filter(|t| members().all(|m| LEVELS.contains(&value(m, *t))))
            .filter(|t| members().any(|m| value(m, *t) > 8))
            .collect(),
    );
    let constellation = only_one(
        tags.iter()
            .copied()
            .filter(|t| Some(*t) != level && !everywhere(*t))
            .filter(|t| members().all(|m| value(m, *t) <= MAX_CONSTELLATION))
            .collect(),
    );
    for member in records
        .iter_mut()
        .flat_map(|r| &mut r.fights)
        .flat_map(|f| &mut f.teams)
        .flat_map(|t| &mut t.members)
    {
        member.level = level.map(|t| value(member, t) as u32);
        member.constellation = constellation.map(|t| value(member, t) as u32);
    }
}

/// A fight's lone numbers by tag, its teams and its figures.
struct FightRead {
    numbers: BTreeMap<u32, u64>,
    teams: Vec<StygianTeam>,
    strike: Option<Figure>,
    damage: Option<Figure>,
}

/// Read a fight's figures: a submessage of exactly four lone numbers, two
/// avatar ids and two values, or `None` if `bytes` is not that (the
/// equipment guid list and a team are the other submessages of a fight).
fn fight_figures(bytes: &[u8]) -> Option<(Figure, Figure)> {
    let fields = wire::fields(bytes)?;
    let mut avatars = Vec::new();
    let mut values = Vec::new();
    for field in fields.values() {
        match field.as_slice() {
            [Varint(v)] if AVATAR_IDS.contains(v) => avatars.push(*v as u32),
            [Varint(v)] if *v >= 1 => values.push(*v),
            _ => return None,
        }
    }
    if avatars.len() != 2 || values.len() != 2 {
        return None;
    }
    values.sort_unstable();
    let by = (avatars[0] == avatars[1]).then_some(avatars[0]);
    Some((
        Figure {
            value: values[0],
            by,
        },
        Figure {
            value: values[1],
            by,
        },
    ))
}

fn stygian_fight(bytes: &[u8]) -> Option<FightRead> {
    let fields = wire::fields(bytes)?;
    let mut numbers = BTreeMap::new();
    let mut teams = None;
    let mut figures = None;
    for (tag, values) in &fields {
        match values.as_slice() {
            [Varint(v)] => {
                numbers.insert(*tag, *v);
            }
            [Fixed32(_) | Fixed64(_)] => return None,
            [LengthDelimited(one)] if fight_figures(one).is_some() => {
                if figures.replace(fight_figures(one)?).is_some() {
                    return None;
                }
            }
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {
                // The teams, or the equipment guids: the one list whose every
                // entry reads as a team is the teams.
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
    let (strike, damage) = figures.unzip();
    Some(FightRead {
        numbers,
        teams: teams?,
        strike,
        damage,
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
    let mut players: Option<Vec<StygianPlayer>> = None;
    let mut players_twice = false;
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
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {
                let read: Option<Vec<FightRead>> = (values.len() <= MAX_STYGIAN_FIGHTS)
                    .then(|| values.iter().map(|v| stygian_fight(as_bytes(v)?)).collect())
                    .flatten();
                if let Some(read) = read {
                    if fights.replace(read).is_some() {
                        return None;
                    }
                } else if let Some(read) = stygian_players(values) {
                    players_twice |= players.replace(read).is_some();
                }
            }
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
            strike: f.strike,
            damage: f.damage,
        })
        .collect();
    Some(StygianRecord {
        schedule: schedule?,
        difficulty,
        at_ms: at_ms?,
        fights,
        players: if players_twice {
            Vec::new()
        } else {
            players.unwrap_or_default()
        },
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
    if records.is_empty() || records.len() > MAX_STYGIAN_RECORDS {
        return None;
    }
    settle_members(&mut records);
    settle_pictures(&mut records);
    Some(records)
}

/// Spiral Abyss schedule ids (`TowerScheduleExcelConfigData`: 124 at 7.2;
/// the handbook keeps the last two years, so nothing below 100 is listed
/// any more), above any star count so the period is told from the stars.
const ABYSS_SCHEDULE_IDS: std::ops::RangeInclusive<u64> = 61..=100_000;
/// The Abyssal Moon Spire's floors (9-12: the record book counts only
/// those), with room above.
const ABYSS_FLOORS: std::ops::RangeInclusive<u64> = 9..=20;
/// Chambers of a floor (three), with room.
const ABYSS_CHAMBERS: std::ops::RangeInclusive<u64> = 1..=5;
/// Stars of a period (36 at a full clear), with room.
const ABYSS_STARS: std::ops::RangeInclusive<u64> = 0..=60;
/// Periods a message may hold (24 for a player of six years).
const MAX_ABYSS_PERIODS: usize = 200;
/// Figures a period carries (five at 7.2) and characters it lists as picked
/// (four), with room.
const MAX_ABYSS_FIGURES: usize = 16;
const MAX_ABYSS_PICKED: usize = 32;

/// One period of the Spiral Abyss's record book (the Challenge Summary),
/// as the game keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AbyssRecord {
    /// `TowerScheduleExcelConfigData.scheduleId`.
    pub schedule: u32,
    /// The deepest descent: its floor and chamber.
    pub floor: u32,
    pub chamber: u32,
    /// Stars earned that period.
    pub stars: u32,
    /// The figures by field number, each a character and a value; a figure
    /// the game sent empty (no damage taken) is left out. Which figure a
    /// field is lives on the wire only as its number, reshuffled every
    /// version, so the caller labels them against the layouts it knows.
    pub figures: BTreeMap<u32, (u32, u64)>,
    /// The characters the game lists as picked that period, with how often.
    pub picked: Vec<(u32, u32)>,
}

/// The stats' pairs by field: each pair its two numbers by field, or `None`
/// for an empty one.
type AbyssPairs = BTreeMap<u32, Vec<Option<BTreeMap<u32, u64>>>>;

/// One period as read, before the message settles what its fields are: the
/// summary's lone numbers by field and the stats' pairs by field.
struct AbyssRecordRead {
    summary: BTreeMap<u32, u64>,
    pairs: AbyssPairs,
}

/// Read `data` as the Spiral Abyss's record book, or `None` if it is not
/// one: a list of periods (and a lone number or two beside it), each
/// period a few lone numbers, one submessage of lone numbers (the deepest
/// floor and chamber, the stars and the period) and one whose every field
/// is a pair of a character and a number (the five figures, and the picked
/// characters with their count, which repeat on one field). What each field
/// is gets settled across the message: the period is the only number above
/// any star count and differs per entry; the chamber, floor and stars by
/// their ranges and the game's own rule that a floor opens with six stars
/// on the one before and a chamber with the one before cleared.
pub fn matches_abyss_records(data: &[u8]) -> Option<Vec<AbyssRecord>> {
    let top = wire::fields(data)?;
    let mut periods = None;
    let mut lone_numbers = 0;
    for values in top.values() {
        match values.as_slice() {
            [Varint(_)] => lone_numbers += 1,
            values if values.iter().all(|v| matches!(v, LengthDelimited(_))) => {
                let read: Option<Vec<AbyssRecordRead>> = (values.len() <= MAX_ABYSS_PERIODS)
                    .then(|| values.iter().map(|v| abyss_record(as_bytes(v)?)).collect())
                    .flatten();
                if periods.replace(read?).is_some() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if lone_numbers > 2 {
        return None;
    }
    settle_abyss(periods?)
}

fn abyss_record(bytes: &[u8]) -> Option<AbyssRecordRead> {
    let fields = wire::fields(bytes)?;
    let mut summary = None;
    let mut pairs = None;
    for values in fields.values() {
        match values.as_slice() {
            [Varint(_)] => {}
            [LengthDelimited(sub)] => {
                let inner = wire::fields(sub)?;
                if inner.values().all(|v| matches!(v.as_slice(), [Varint(_)])) {
                    if !(3..=8).contains(&inner.len()) {
                        return None;
                    }
                    let read = inner
                        .iter()
                        .map(|(tag, v)| (*tag, as_varint(&v[0]).unwrap_or(0)))
                        .collect();
                    if summary.replace(read).is_some() {
                        return None;
                    }
                } else {
                    let read = abyss_pairs(&inner)?;
                    if pairs.replace(read).is_some() {
                        return None;
                    }
                }
            }
            _ => return None,
        }
    }
    Some(AbyssRecordRead {
        summary: summary?,
        pairs: pairs?,
    })
}

/// The stats submessage: every field one or more submessages of exactly two
/// lone numbers, or empty.
fn abyss_pairs(fields: &wire::Fields<'_>) -> Option<AbyssPairs> {
    let mut out = BTreeMap::new();
    let mut count = 0;
    for (tag, values) in fields {
        let mut list = Vec::new();
        for value in values {
            let bytes = as_bytes(value)?;
            if bytes.is_empty() {
                list.push(None);
                continue;
            }
            let pair = wire::fields(bytes)?;
            if pair.len() != 2 {
                return None;
            }
            let read: Option<BTreeMap<u32, u64>> = pair
                .iter()
                .map(|(t, v)| match v.as_slice() {
                    [Varint(n)] => Some((*t, *n)),
                    _ => None,
                })
                .collect();
            list.push(Some(read?));
        }
        count += list.len();
        out.insert(*tag, list);
    }
    (!out.is_empty() && count <= MAX_ABYSS_FIGURES + MAX_ABYSS_PICKED).then_some(out)
}

/// Settle the fields across the periods (see [`matches_abyss_records`]).
fn settle_abyss(reads: Vec<AbyssRecordRead>) -> Option<Vec<AbyssRecord>> {
    if reads.is_empty() {
        return None;
    }
    // The summary's four roles: one assignment of its fields must fit every
    // period, and only one.
    let tags: BTreeSet<u32> = reads
        .iter()
        .flat_map(|r| r.summary.keys().copied())
        .collect();
    if !(3..=4).contains(&tags.len()) {
        return None;
    }
    let value = |r: &AbyssRecordRead, tag: u32| r.summary.get(&tag).copied().unwrap_or(0);
    let all = |tag: u32, range: &std::ops::RangeInclusive<u64>| {
        reads.iter().all(|r| range.contains(&value(r, tag)))
    };
    let mut found = None;
    for &schedule in &tags {
        let mut seen = BTreeSet::new();
        if !all(schedule, &ABYSS_SCHEDULE_IDS)
            || !reads.iter().all(|r| seen.insert(value(r, schedule)))
        {
            continue;
        }
        for &floor in &tags {
            if floor == schedule || !all(floor, &ABYSS_FLOORS) {
                continue;
            }
            for &chamber in &tags {
                if chamber == schedule || chamber == floor || !all(chamber, &ABYSS_CHAMBERS) {
                    continue;
                }
                // The stars: the field left, or none when the summary has
                // three (0 stars are off the wire).
                let stars = tags
                    .iter()
                    .copied()
                    .find(|t| ![schedule, floor, chamber].contains(t));
                let fits = reads.iter().all(|r| {
                    let stars = stars.map_or(0, |t| value(r, t));
                    let floors_above_nine = value(r, floor) - 9;
                    ABYSS_STARS.contains(&stars)
                        && stars >= 6 * floors_above_nine + value(r, chamber) - 1
                        && stars <= 9 * floors_above_nine + 3 * value(r, chamber)
                });
                if fits && found.replace((schedule, floor, chamber, stars)).is_some() {
                    return None;
                }
            }
        }
    }
    let (schedule, floor, chamber, stars) = found?;

    // The pairs: the field that repeats within a period is the picked list,
    // the rest the figures; and in a pair the field that is a character id
    // in every pair of the message is the character.
    let mut picked_tag = None;
    let mut by_tag: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    for r in &reads {
        for (tag, list) in &r.pairs {
            if list.len() > 1 && picked_tag.replace(*tag).is_some_and(|t| t != *tag) {
                return None;
            }
        }
        for pair in r.pairs.values().flatten().flatten() {
            for (tag, n) in pair {
                let entry = by_tag.entry(*tag).or_default();
                entry.0 += 1;
                entry.1 += usize::from(AVATAR_IDS.contains(n));
            }
        }
    }
    let picked_tag = picked_tag?;
    let avatar_tags: Vec<u32> = by_tag
        .iter()
        .filter(|(_, (seen, avatars))| seen == avatars)
        .map(|(tag, _)| *tag)
        .collect();
    let [avatar_tag] = avatar_tags.as_slice() else {
        return None;
    };
    let read_pair = |pair: &BTreeMap<u32, u64>| -> Option<(u32, u64)> {
        let avatar = *pair.get(avatar_tag)?;
        let (_, number) = pair.iter().find(|(t, _)| *t != avatar_tag)?;
        Some((avatar as u32, *number))
    };
    let mut out = Vec::with_capacity(reads.len());
    for r in &reads {
        let mut figures = BTreeMap::new();
        let mut picked = Vec::new();
        for (tag, list) in &r.pairs {
            if *tag == picked_tag {
                for pair in list.iter().flatten() {
                    let (avatar, times) = read_pair(pair)?;
                    picked.push((avatar, u32::try_from(times).ok()?));
                }
            } else {
                let [one] = list.as_slice() else {
                    return None;
                };
                if let Some(pair) = one {
                    figures.insert(*tag, read_pair(pair)?);
                }
            }
        }
        if figures.len() > MAX_ABYSS_FIGURES || picked.len() > MAX_ABYSS_PICKED {
            return None;
        }
        out.push(AbyssRecord {
            schedule: value(r, schedule) as u32,
            floor: value(r, floor) as u32,
            chamber: value(r, chamber) as u32,
            stars: stars.map_or(0, |t| value(r, t)) as u32,
            figures,
            picked,
        });
    }
    Some(out)
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
        // The figures: strike by one avatar, total damage by another (the
        // same one in the first fight of a record).
        let mut boss = Vec::new();
        field_varint(4, 10000096, &mut boss);
        field_varint(8, 367_183, &mut boss);
        field_varint(10, 15_616_746, &mut boss);
        field_varint(12, if index == 1 { 10000096 } else { 10000032 }, &mut boss);
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
        // Figures: the smaller number is the strike; the avatar only when
        // both figures name the same one.
        assert_eq!(
            solo.fights[0].strike,
            Some(Figure {
                value: 367_183,
                by: Some(10000096)
            })
        );
        assert_eq!(
            solo.fights[0].damage,
            Some(Figure {
                value: 15_616_746,
                by: Some(10000096)
            })
        );
        assert_eq!(
            solo.fights[1].damage,
            Some(Figure {
                value: 15_616_746,
                by: None
            })
        );
        assert_eq!(solo.fights[1].teams[0].uid, ME);
        assert_eq!(solo.fights[1].teams[0].avatars, [10000030, 10000046]);
    }

    /// A 7.2 avatar entry: id, level, costume (when worn), constellation
    /// (left off at C0), element.
    fn avatar_72(id: u64, level: u64, constellation: u64, element: u64) -> Vec<u8> {
        let mut out = Vec::new();
        field_varint(1, id, &mut out);
        field_varint(2, level, &mut out);
        if id == 10000032 {
            field_varint(3, 203_201, &mut out);
        }
        if constellation > 0 {
            field_varint(4, constellation, &mut out);
        }
        field_varint(5, element, &mut out);
        out
    }

    fn team_72(uid: u64, avatars: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for a in avatars {
            field_bytes(6, a, &mut out);
        }
        field_varint(13, uid, &mut out);
        out
    }

    /// A 7.2 player entry: picture, nickname (co-op only), UID, position.
    fn player_72(uid: u64, index: u64, nickname: Option<&str>, picture: u64) -> Vec<u8> {
        let mut out = Vec::new();
        let mut pic = Vec::new();
        field_varint(3, picture, &mut pic);
        // A worn frame (7.2: 100011…); picture ids reach that size too.
        if picture >= 100_000 {
            field_varint(4, 100_011, &mut pic);
        }
        field_bytes(5, &pic, &mut out);
        if let Some(name) = nickname {
            field_bytes(11, name.as_bytes(), &mut out);
        }
        field_varint(13, uid, &mut out);
        field_varint(14, index, &mut out);
        out
    }

    fn fight_72(index: u64, time: u64, teams: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        field_varint(1, 1, &mut out);
        field_varint(3, time, &mut out);
        let mut figures = Vec::new();
        field_varint(3, 10000106, &mut figures);
        field_varint(8, 10000106, &mut figures);
        field_varint(10, 11_561_469, &mut figures);
        field_varint(12, 912_553, &mut figures);
        field_bytes(4, &figures, &mut out);
        for t in teams {
            field_bytes(9, t, &mut out);
        }
        field_varint(15, index, &mut out);
        out
    }

    fn record_72(players: &[Vec<u8>], fights: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        field_fixed64(6, 1_790_972_494_088, &mut out);
        for p in players {
            field_bytes(7, p, &mut out);
        }
        for f in fights {
            field_bytes(10, f, &mut out);
        }
        field_varint(11, 5_269_012, &mut out);
        field_varint(15, 6, &mut out);
        out
    }

    const THIRD: u64 = 834_567_890;

    #[test]
    fn coop_records_name_the_players_and_their_characters() {
        let coop = record_72(
            &[
                player_72(FRIEND, 2, Some("Lumine"), 2),
                player_72(ME, 1, Some("Traveler"), 100_230),
                player_72(THIRD, 3, Some("08"), 4000),
            ],
            &[
                fight_72(
                    1,
                    51,
                    &[
                        team_72(
                            ME,
                            &[avatar_72(10000052, 90, 3, 4), avatar_72(10000032, 80, 0, 1)],
                        ),
                        team_72(FRIEND, &[avatar_72(10000106, 100, 6, 1)]),
                        team_72(THIRD, &[avatar_72(10000103, 90, 0, 8)]),
                    ],
                ),
                fight_72(
                    2,
                    69,
                    &[
                        team_72(
                            ME,
                            &[avatar_72(10000060, 90, 6, 2), avatar_72(10000032, 80, 0, 1)],
                        ),
                        team_72(FRIEND, &[avatar_72(10000111, 90, 6, 4)]),
                        team_72(THIRD, &[avatar_72(10000073, 90, 0, 3)]),
                    ],
                ),
            ],
        );
        let solo = record_72(
            &[player_72(ME, 1, None, 1001)],
            &[fight_72(
                1,
                40,
                &[team_72(
                    ME,
                    &[
                        avatar_72(10000052, 90, 3, 4),
                        avatar_72(10000106, 90, 1, 1),
                        avatar_72(10000103, 90, 0, 8),
                        avatar_72(10000107, 90, 0, 5),
                    ],
                )],
            )],
        );
        let mut body = Vec::new();
        field_bytes(3, &coop, &mut body);
        field_bytes(13, &solo, &mut body);
        let mut data = Vec::new();
        field_bytes(9, &body, &mut data);
        let found = matches_stygian_records(&data).unwrap();
        assert_eq!(found.len(), 2);
        let coop = &found[0];
        let seen = |p: &StygianPlayer| (p.uid, p.index, p.nickname.clone(), p.picture);
        assert_eq!(
            coop.players.iter().map(seen).collect::<Vec<_>>(),
            [
                (FRIEND, 2, Some("Lumine".to_string()), Some(2)),
                // The picture beside a frame: the field every player has.
                (ME, 1, Some("Traveler".to_string()), Some(100_230)),
                // "08" also reads as a one-number submessage: told apart by
                // the real picture beside it.
                (THIRD, 3, Some("08".to_string()), Some(4000)),
            ]
        );
        let mine = &coop.fights[0].teams[0];
        assert_eq!(mine.avatars, [10000052, 10000032]);
        let read = |m: &StygianMember| (m.id, m.level, m.constellation);
        assert_eq!(
            mine.members.iter().map(read).collect::<Vec<_>>(),
            [(10000052, Some(90), Some(3)), (10000032, Some(80), Some(0))]
        );
        assert_eq!(
            read(&coop.fights[0].teams[1].members[0]),
            (10000106, Some(100), Some(6))
        );
        // The solo record: one player, no nickname; read with the same fields.
        let solo = &found[1];
        assert_eq!(
            solo.players.iter().map(seen).collect::<Vec<_>>(),
            [(ME, 1, None, Some(1001))]
        );
        assert_eq!(
            solo.fights[0].teams[0]
                .members
                .iter()
                .map(read)
                .collect::<Vec<_>>(),
            [
                (10000052, Some(90), Some(3)),
                (10000106, Some(90), Some(1)),
                (10000103, Some(90), Some(0)),
                (10000107, Some(90), Some(0)),
            ]
        );
    }

    #[test]
    fn a_quest_picture_beside_a_frame_is_still_the_picture() {
        // Every player wears a frame and a picture of frame size (a quest
        // one): the values can't tell them apart, the picture field can.
        let solo = record_72(
            &[player_72(ME, 1, None, 100_230)],
            &[fight_72(
                1,
                40,
                &[team_72(ME, &[avatar_72(10000052, 90, 0, 4)])],
            )],
        );
        let mut body = Vec::new();
        field_bytes(13, &solo, &mut body);
        let mut data = Vec::new();
        field_bytes(9, &body, &mut data);
        let found = matches_stygian_records(&data).unwrap();
        assert_eq!(found[0].players[0].picture, Some(100_230));
    }

    #[test]
    fn without_a_c0_character_constellations_are_not_guessed() {
        // Every character C1+: the constellation field is always there, like
        // the element, so neither stands out.
        let solo = record_72(
            &[player_72(ME, 1, None, 1001)],
            &[fight_72(
                1,
                40,
                &[team_72(
                    ME,
                    &[avatar_72(10000052, 90, 3, 4), avatar_72(10000106, 90, 1, 1)],
                )],
            )],
        );
        let mut body = Vec::new();
        field_bytes(13, &solo, &mut body);
        let mut data = Vec::new();
        field_bytes(9, &body, &mut data);
        let found = matches_stygian_records(&data).unwrap();
        let member = &found[0].fights[0].teams[0].members[0];
        assert_eq!((member.level, member.constellation), (Some(90), None));
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

    /// A period of the record book in the 7.2 layout: the summary on field
    /// 15 (floor 7, stars 8, period 11, chamber 14), the stats on field 9
    /// with each pair {5: character, 15: number}, the picked list on 13.
    fn abyss_record_bytes(
        period: u64,
        floor: u64,
        chamber: u64,
        stars: u64,
        figures: &[(u32, Option<(u64, u64)>)],
        picked: &[(u64, u64)],
    ) -> Vec<u8> {
        let abyss_pair = |avatar: u64, number: u64| {
            let mut out = Vec::new();
            field_varint(5, avatar, &mut out);
            field_varint(15, number, &mut out);
            out
        };
        let mut entry = Vec::new();
        field_varint(2, 1, &mut entry);
        field_varint(4, 10, &mut entry);
        field_varint(6, 1, &mut entry);
        let mut stats = Vec::new();
        for (tag, figure) in figures {
            match figure {
                Some((avatar, number)) => {
                    field_bytes(*tag, &abyss_pair(*avatar, *number), &mut stats)
                }
                None => field_bytes(*tag, &[], &mut stats),
            }
        }
        for (avatar, times) in picked {
            field_bytes(13, &abyss_pair(*avatar, *times), &mut stats);
        }
        field_bytes(9, &stats, &mut entry);
        let mut summary = Vec::new();
        field_varint(7, floor, &mut summary);
        if stars > 0 {
            field_varint(8, stars, &mut summary);
        }
        field_varint(11, period, &mut summary);
        field_varint(14, chamber, &mut summary);
        field_bytes(15, &summary, &mut entry);
        entry
    }

    fn abyss_message(periods: &[Vec<u8>]) -> Vec<u8> {
        let mut data = Vec::new();
        for p in periods {
            field_bytes(8, p, &mut data);
        }
        data
    }

    #[test]
    fn the_abyss_record_book_is_read_and_its_fields_settled() {
        let full = abyss_record_bytes(
            123,
            12,
            3,
            36,
            &[
                (1, Some((10000125, 24))),
                (6, Some((10000047, 17_911))),
                (10, Some((10000106, 6_424_192))),
                (11, Some((10000106, 111))),
                (12, Some((10000058, 85))),
            ],
            &[
                (10000103, 17),
                (10000107, 17),
                (10000116, 17),
                (10000125, 17),
            ],
        );
        // An empty figure (no damage taken) is left out.
        let quiet = abyss_record_bytes(
            122,
            12,
            3,
            36,
            &[
                (1, Some((10000125, 13))),
                (6, None),
                (10, Some((10000106, 5_800_611))),
                (11, Some((10000104, 14))),
                (12, Some((10000104, 10))),
            ],
            &[(10000103, 6), (10000104, 6)],
        );
        let read = matches_abyss_records(&abyss_message(&[quiet, full])).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(
            read[1],
            AbyssRecord {
                schedule: 123,
                floor: 12,
                chamber: 3,
                stars: 36,
                figures: BTreeMap::from([
                    (1, (10000125, 24)),
                    (6, (10000047, 17_911)),
                    (10, (10000106, 6_424_192)),
                    (11, (10000106, 111)),
                    (12, (10000058, 85)),
                ]),
                picked: vec![
                    (10000103, 17),
                    (10000107, 17),
                    (10000116, 17),
                    (10000125, 17)
                ],
            }
        );
        assert_eq!(read[0].schedule, 122);
        assert_eq!(read[0].figures.len(), 4);
        assert!(!read[0].figures.contains_key(&6));
        assert_eq!(read[0].picked, vec![(10000103, 6), (10000104, 6)]);

        // One period alone settles too, the game's own unlock rule telling
        // the chamber from the stars: floor 9, chamber 1, 3 stars.
        let one = abyss_record_bytes(
            124,
            9,
            1,
            3,
            &[(1, Some((10000125, 2)))],
            &[(10000103, 1), (10000107, 1)],
        );
        let read = matches_abyss_records(&abyss_message(&[one])).unwrap();
        assert_eq!((read[0].floor, read[0].chamber, read[0].stars), (9, 1, 3));
        // A summary the rule cannot settle (chamber 3 of floor 9 with no
        // star is impossible, and so is any other reading) is not the book.
        let odd = abyss_record_bytes(
            124,
            9,
            3,
            0,
            &[(1, Some((10000125, 2)))],
            &[(10000103, 1), (10000107, 1)],
        );
        assert_eq!(matches_abyss_records(&abyss_message(&[odd])), None);
    }

    #[test]
    fn the_other_endgame_messages_are_not_the_abyss_record_book() {
        let record = record(
            5_269_012,
            6,
            1_790_972_494_088,
            &[fight(1, 51, true, &[team(ME, &[10000096])])],
            &[ME],
        );
        assert_eq!(
            matches_abyss_records(&records_message(&[record], &[])),
            None
        );
        assert_eq!(
            matches_abyss_records(&theater_detail_bytes(&[1, 2, 3], &[3])),
            None
        );
        assert_eq!(matches_abyss_records(&[]), None);
        // Two lists of periods, or a figure field that repeats, are not it.
        let one = abyss_record_bytes(
            124,
            9,
            1,
            3,
            &[(1, Some((10000125, 2)))],
            &[(10000103, 1), (10000107, 1)],
        );
        let mut data = abyss_message(std::slice::from_ref(&one));
        field_bytes(9, &one, &mut data);
        assert_eq!(matches_abyss_records(&data), None);
        let twice = abyss_record_bytes(
            124,
            9,
            1,
            3,
            &[(1, Some((10000125, 2))), (1, Some((10000125, 3)))],
            &[(10000103, 1), (10000107, 1)],
        );
        assert_eq!(matches_abyss_records(&abyss_message(&[twice])), None);
    }
}
