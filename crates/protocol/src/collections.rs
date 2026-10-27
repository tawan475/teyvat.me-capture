// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What the player has collected, recognised by shape: the Archive, enemies
//! defeated, the Adventurer Handbook, Genius Invokation TCG cards and decks,
//! Aranara collections and weapon skins.
//!
//! Each is one message of the login bundle (`CodexDataFullNotify`,
//! `PlayerInvestigationAllInfoNotify`, `GCGDSDataNotify`,
//! `AranaraCollectionDataNotify`, `_AvatarWeaponSkinDataNotify`) or a reply
//! the client asks for right after login (`QueryCodexMonsterBeKilledNumRsp`).
//! Like everything else this library reads, their command ids and field
//! numbers are reshuffled by every game version, so nothing here names one:
//! each message is recognised by what its values are, and its fields are told
//! apart the same way. What cannot be told apart is left out, not guessed.
//!
//! Every matcher here was run over every command of four recorded 7.1
//! sessions (about 48,000 messages, unions unwrapped) and matched only the
//! message it is for.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use protobuf::Message;
use protobuf::UnknownValueRef::{self, Fixed32, Fixed64, LengthDelimited, Varint};

use crate::r#gen::protos::Unk;

/// One field value, borrowed from the parsed message.
type Value<'a> = UnknownValueRef<'a>;

fn parse(bytes: &[u8]) -> Option<Unk> {
    Unk::parse_from_bytes(bytes).ok()
}

/// A parsed message's fields grouped by tag, in wire order within each tag.
fn fields_by_tag(msg: &Unk) -> BTreeMap<u32, Vec<Value<'_>>> {
    let mut fields: BTreeMap<u32, Vec<Value<'_>>> = BTreeMap::new();
    for (tag, value) in msg.unknown_fields().iter() {
        fields.entry(tag).or_default().push(value);
    }
    fields
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
    (shift == 0).then_some(out)
}

/// A repeated numeric field's values, packed (one length-delimited value) or
/// not (one varint per value).
fn numbers(values: &[Value<'_>]) -> Option<Vec<u64>> {
    match values {
        [LengthDelimited(bytes)] => packed(bytes),
        _ => values
            .iter()
            .map(|value| match value {
                Varint(v) => Some(*v),
                _ => None,
            })
            .collect(),
    }
}

fn single_varint(values: &[Value<'_>]) -> Option<u64> {
    match values {
        [Varint(v)] => Some(*v),
        _ => None,
    }
}

fn all_distinct(values: &[u64]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

/// A submessage's lone varints and number lists, by tag; `None` if it does
/// not parse. Every length-delimited field's raw values are in `other` too
/// (strings, submessages, map entries); a lone one that reads as packed
/// numbers is also in `lists`.
#[derive(Default)]
struct Entry {
    varints: BTreeMap<u32, u64>,
    lists: BTreeMap<u32, Vec<u64>>,
    other: BTreeMap<u32, Vec<Vec<u8>>>,
    /// Whether it holds fixed-width numbers.
    fixed: bool,
}

fn entry(bytes: &[u8]) -> Option<Entry> {
    let msg = parse(bytes)?;
    let mut out = Entry::default();
    for (tag, values) in fields_by_tag(&msg) {
        match values.first()? {
            Varint(_) if values.len() == 1 => {
                out.varints.insert(tag, single_varint(&values)?);
            }
            Varint(_) => {
                out.lists.insert(tag, numbers(&values)?);
            }
            LengthDelimited(_) => {
                let raw: Vec<Vec<u8>> = values
                    .iter()
                    .map(|value| match value {
                        LengthDelimited(inner) => Some(inner.to_vec()),
                        _ => None,
                    })
                    .collect::<Option<_>>()?;
                if let [one] = raw.as_slice()
                    && let Some(list) = packed(one)
                {
                    out.lists.insert(tag, list);
                }
                out.other.insert(tag, raw);
            }
            Fixed32(_) | Fixed64(_) => out.fixed = true,
        }
    }
    Some(out)
}

/// The entries of every length-delimited value of one tag, or `None` if one
/// is not an entry.
fn entries(values: &[Value<'_>]) -> Option<Vec<Entry>> {
    values
        .iter()
        .map(|value| match value {
            LengthDelimited(bytes) => entry(bytes),
            _ => None,
        })
        .collect()
}

/// The one tag of `entries` present in every entry whose values pass `ok`,
/// if exactly one does.
fn only_tag(entries: &[Entry], skip: &[u32], ok: impl Fn(&[u64]) -> bool) -> Option<u32> {
    let mut common: BTreeSet<u32> = entries.first()?.varints.keys().copied().collect();
    for entry in entries {
        common.retain(|tag| entry.varints.contains_key(tag));
    }
    let found: Vec<u32> = common
        .into_iter()
        .filter(|tag| !skip.contains(tag))
        .filter(|tag| {
            let values: Vec<u64> = entries.iter().map(|entry| entry.varints[tag]).collect();
            ok(&values)
        })
        .collect();
    match found.as_slice() {
        [tag] => Some(*tag),
        _ => None,
    }
}

// ------------------------------------------------------------------ archive

/// The Archive's tabs. Each tab's codex ids live in their own block
/// (`10 010 002` is a quest, `30 211 101` the weapon 11101...), which is how
/// an id list is told to be one tab's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArchiveTab {
    Quests,
    LivingBeings,
    Artifacts,
    Weapons,
    Materials,
    Books,
    Tutorials,
    Viewpoints,
}

impl ArchiveTab {
    pub const ALL: [ArchiveTab; 8] = [
        Self::Quests,
        Self::LivingBeings,
        Self::Artifacts,
        Self::Weapons,
        Self::Materials,
        Self::Books,
        Self::Tutorials,
        Self::Viewpoints,
    ];

    /// The tab's id block (7.1: quests 10 010 002-11 080 004, living beings
    /// 20 010 101-29 150 101, artifact sets 30 102 004-30 109 405, weapons
    /// 30 211 101-30 215 516, materials 40 100 001-, books 50 001 001-,
    /// tutorials 60 100 001-, viewpoints 70 100 001-).
    pub fn ids(self) -> RangeInclusive<u64> {
        match self {
            Self::Quests => 10_000_000..=19_999_999,
            Self::LivingBeings => 20_000_000..=29_999_999,
            Self::Artifacts => 30_100_000..=30_199_999,
            Self::Weapons => 30_200_000..=30_299_999,
            Self::Materials => 40_000_000..=49_999_999,
            Self::Books => 50_000_000..=59_999_999,
            Self::Tutorials => 60_000_000..=69_999_999,
            Self::Viewpoints => 70_000_000..=79_999_999,
        }
    }

    fn of(ids: &[u64]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|tab| ids.iter().all(|id| tab.ids().contains(id)))
    }
}

/// The most ids believed in one tab (7.1: ~1,000 materials).
const MAX_ARCHIVE_TAB: usize = 20_000;

/// The fewest tabs a full Archive holds (7.1 sends all 8).
const MIN_ARCHIVE_TABS: usize = 5;

/// Recover the Archive from a `CodexDataFullNotify` payload, or `None` if it
/// is not one: one repeated field of entries, each with exactly one list of
/// ids of one tab's block ([`ArchiveTab::ids`]), the tabs all different and at
/// least `MIN_ARCHIVE_TABS` of them. Each tab's ids come back ascending.
pub fn matches_codex_notify(data: &[u8]) -> Option<BTreeMap<ArchiveTab, Vec<u32>>> {
    let msg = parse(data)?;
    for values in fields_by_tag(&msg).values() {
        if values.len() < MIN_ARCHIVE_TABS {
            continue;
        }
        let Some(list) = entries(values) else {
            continue;
        };
        let mut archive = BTreeMap::new();
        let ok = list.iter().all(|entry| {
            let tabbed: Vec<(ArchiveTab, &Vec<u64>)> = entry
                .lists
                .values()
                .filter(|ids| !ids.is_empty())
                .filter_map(|ids| Some((ArchiveTab::of(ids)?, ids)))
                .collect();
            let [(tab, ids)] = tabbed.as_slice() else {
                return false;
            };
            if ids.len() > MAX_ARCHIVE_TAB || !all_distinct(ids) {
                return false;
            }
            let mut ids: Vec<u32> = ids.iter().map(|id| *id as u32).collect();
            ids.sort_unstable();
            archive.insert(*tab, ids).is_none()
        });
        if ok && archive.len() >= MIN_ARCHIVE_TABS {
            return Some(archive);
        }
    }
    None
}

// ------------------------------------------------------------------ defeats

/// The most Living Beings believed (7.1: 538).
const MAX_LIVING_BEINGS: usize = 10_000;

/// One Living Being's counts (`QueryCodexMonsterBeKilledNumRsp`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefeatCount {
    pub defeated: u32,
    pub captured: u32,
}

/// Recover enemies defeated and creatures captured, by Living Beings codex id,
/// from a `QueryCodexMonsterBeKilledNumRsp` payload, or `None` if it is not
/// one: three number lists of one length, one of them unique Living Beings
/// ids ([`ArchiveTab::LivingBeings`]), beside at most a result code.
///
/// The two counts are told apart by what can be captured: only creatures
/// (crystalflies, animals), so the captured list has fewer non-zero counts.
/// When both have as many, they cannot be told apart and nothing is read.
pub fn matches_codex_kill_count(data: &[u8]) -> Option<BTreeMap<u32, DefeatCount>> {
    let msg = parse(data)?;
    let mut lists = Vec::new();
    for values in fields_by_tag(&msg).values() {
        match values.as_slice() {
            [Varint(_)] => {}
            values => lists.push(numbers(values)?),
        }
    }
    let [a, b, c] = lists.as_slice() else {
        return None;
    };
    let len = a.len();
    if len == 0 || len > MAX_LIVING_BEINGS || b.len() != len || c.len() != len {
        return None;
    }
    let is_ids = |list: &[u64]| {
        all_distinct(list)
            && list
                .iter()
                .all(|id| ArchiveTab::LivingBeings.ids().contains(id))
    };
    let (ids, x, y) = match (is_ids(a), is_ids(b), is_ids(c)) {
        (true, false, false) => (a, b, c),
        (false, true, false) => (b, a, c),
        (false, false, true) => (c, a, b),
        _ => return None,
    };
    if x.iter().chain(y).any(|count| *count > u64::from(u32::MAX)) {
        return None;
    }
    let nonzero = |list: &[u64]| list.iter().filter(|n| **n > 0).count();
    let (defeated, captured) = match nonzero(x).cmp(&nonzero(y)) {
        std::cmp::Ordering::Greater => (x, y),
        std::cmp::Ordering::Less => (y, x),
        std::cmp::Ordering::Equal => return None,
    };
    Some(
        ids.iter()
            .zip(defeated.iter().zip(captured))
            .filter(|(_, (d, c))| **d > 0 || **c > 0)
            .map(|(id, (d, c))| {
                (
                    *id as u32,
                    DefeatCount {
                        defeated: *d as u32,
                        captured: *c as u32,
                    },
                )
            })
            .collect(),
    )
}

// ----------------------------------------------------------------- handbook

/// `Investigation.State` / `InvestigationTarget.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandbookState {
    InProgress,
    /// Done, its reward not taken yet.
    Complete,
    RewardTaken,
}

impl HandbookState {
    fn from(value: u64) -> Option<Self> {
        match value {
            1 => Some(Self::InProgress),
            2 => Some(Self::Complete),
            3 => Some(Self::RewardTaken),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandbookProgress {
    pub progress: u32,
    pub total: u32,
    pub state: HandbookState,
}

/// One target of a Handbook chapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandbookTarget {
    pub chapter: u32,
    pub progress: HandbookProgress,
}

/// The Adventurer Handbook (`PlayerInvestigationAllInfoNotify`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Handbook {
    /// By chapter id.
    pub chapters: BTreeMap<u32, HandbookProgress>,
    /// By the target's quest id.
    pub targets: BTreeMap<u32, HandbookTarget>,
}

/// Handbook chapter ids (7.1: 10 001-50 001).
const HANDBOOK_CHAPTER_IDS: RangeInclusive<u64> = 1_000..=999_999;

/// The most chapters and targets believed (7.1: 22 and 244).
const MAX_HANDBOOK_CHAPTERS: usize = 500;
const MAX_HANDBOOK_TARGETS: usize = 10_000;

/// Read `entries`' progress, total and state, besides the tags in `skip`:
/// the state is the one tag on every entry whose values are all 1-3; of the
/// other tags, the total is on every entry and never below the progress,
/// which is left off the wire at 0 (so it may be on every entry too, or not).
fn handbook_progress(entries: &[Entry], skip: &[u32]) -> Option<Vec<HandbookProgress>> {
    let state_tag = only_tag(entries, skip, |values| {
        values.iter().all(|v| HandbookState::from(*v).is_some())
    })?;
    let mut known = skip.to_vec();
    known.push(state_tag);
    let mut tags: BTreeSet<u32> = BTreeSet::new();
    for entry in entries {
        if !entry.lists.is_empty() || !entry.other.is_empty() || entry.fixed {
            return None;
        }
        tags.extend(entry.varints.keys().filter(|tag| !known.contains(tag)));
    }
    let on_all = |tag: &u32| entries.iter().all(|entry| entry.varints.contains_key(tag));
    let at = |entry: &Entry, tag: Option<u32>| {
        tag.and_then(|t| entry.varints.get(&t).copied())
            .unwrap_or(0)
    };
    let (total_tag, progress_tag) = match tags.iter().copied().collect::<Vec<_>>().as_slice() {
        [total] if on_all(total) => (*total, None),
        [a, b] => {
            let a_total = on_all(a) && entries.iter().all(|e| at(e, Some(*a)) >= at(e, Some(*b)));
            let b_total = on_all(b) && entries.iter().all(|e| at(e, Some(*b)) >= at(e, Some(*a)));
            match (a_total, b_total) {
                // Equal everywhere: either reading gives the same numbers.
                (true, _) => (*a, Some(*b)),
                (false, true) => (*b, Some(*a)),
                (false, false) => return None,
            }
        }
        _ => return None,
    };
    entries
        .iter()
        .map(|entry| {
            let total = entry.varints[&total_tag];
            let progress = at(entry, progress_tag);
            if total == 0 || progress > total || total > u64::from(u32::MAX) {
                return None;
            }
            Some(HandbookProgress {
                progress: progress as u32,
                total: total as u32,
                state: HandbookState::from(entry.varints[&state_tag])?,
            })
        })
        .collect()
}

/// Recover the Adventurer Handbook from a `PlayerInvestigationAllInfoNotify`
/// payload, or `None` if it is not one: two repeated fields of varint-only
/// entries, the chapters (a unique chapter id each) and their targets (each
/// naming one of those chapters and a unique quest id), every entry with a
/// state (1-3), a total and a progress no larger than it.
pub fn matches_investigation_notify(data: &[u8]) -> Option<Handbook> {
    let msg = parse(data)?;
    let fields = fields_by_tag(&msg);
    let mut lists = Vec::new();
    for values in fields.values() {
        match values.first()? {
            LengthDelimited(_) => lists.push(entries(values)?),
            Varint(_) if values.len() == 1 => {}
            _ => return None,
        }
    }
    let [a, b] = lists.as_slice() else {
        return None;
    };
    // The chapters are the shorter list; each target names one of them.
    let (chapters, targets) = if a.len() < b.len() { (a, b) } else { (b, a) };
    if chapters.is_empty()
        || chapters.len() > MAX_HANDBOOK_CHAPTERS
        || targets.len() > MAX_HANDBOOK_TARGETS
    {
        return None;
    }
    let chapter_tag = only_tag(chapters, &[], |values| {
        all_distinct(values) && values.iter().all(|v| HANDBOOK_CHAPTER_IDS.contains(v))
    })?;
    let chapter_ids: BTreeSet<u64> = chapters.iter().map(|c| c.varints[&chapter_tag]).collect();
    let target_chapter_tag = only_tag(targets, &[], |values| {
        values.iter().all(|v| chapter_ids.contains(v))
    })?;
    let quest_tag = only_tag(targets, &[target_chapter_tag], |values| {
        all_distinct(values) && values.iter().all(|v| (1_000..=99_999_999).contains(v))
    })?;
    let chapter_progress = handbook_progress(chapters, &[chapter_tag])?;
    let target_progress = handbook_progress(targets, &[target_chapter_tag, quest_tag])?;
    Some(Handbook {
        chapters: chapters
            .iter()
            .zip(chapter_progress)
            .map(|(c, p)| (c.varints[&chapter_tag] as u32, p))
            .collect(),
        targets: targets
            .iter()
            .zip(target_progress)
            .map(|(t, progress)| {
                (
                    t.varints[&quest_tag] as u32,
                    HandbookTarget {
                        chapter: t.varints[&target_chapter_tag] as u32,
                        progress,
                    },
                )
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------- tcg

/// Genius Invokation TCG card ids: character cards 1101-9999, action cards
/// six digits (7.1: 211 011-333 008).
const TCG_CARD_IDS: RangeInclusive<u64> = 1_000..=999_999;

/// Character card ids (7.1: 1101-2301 in decks).
const TCG_CHARACTER_IDS: RangeInclusive<u64> = 1_000..=9_999;

/// Action card ids (7.1: 211 011-333 008).
const TCG_ACTION_IDS: RangeInclusive<u64> = 100_000..=999_999;

/// Cards and characters one deck holds.
const TCG_DECK_CARDS: usize = 30;
const TCG_DECK_CHARACTERS: usize = 3;

/// The most of one card held, and the most cards believed (7.1: 104 kinds).
const MAX_TCG_COPIES: u64 = 99;
const MAX_TCG_CARDS: usize = 5_000;

/// The most proficiency a card can have (7.1: 64 seen).
const MAX_TCG_PROFICIENCY: u64 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TcgCard {
    pub count: u32,
    /// 0 when the card has none (or the game sent none).
    pub proficiency: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TcgDeck {
    /// The player's own text.
    pub name: String,
    pub characters: Vec<u32>,
    pub cards: Vec<u32>,
    pub valid: bool,
}

/// The TCG collection (`GCGDSDataNotify`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TcgCollection {
    /// By card id.
    pub cards: BTreeMap<u32, TcgCard>,
    /// By deck id.
    pub decks: BTreeMap<u32, TcgDeck>,
    pub current_deck: Option<u32>,
}

fn tcg_deck(entry: &Entry) -> Option<TcgDeck> {
    let find = |len: usize, ids: &RangeInclusive<u64>| {
        entry
            .lists
            .iter()
            .find(|(_, list)| list.len() == len && list.iter().all(|v| ids.contains(v)))
            .map(|(tag, list)| (*tag, list.iter().map(|v| *v as u32).collect::<Vec<u32>>()))
    };
    let (characters_tag, characters) = find(TCG_DECK_CHARACTERS, &TCG_CHARACTER_IDS)?;
    let cards = find(TCG_DECK_CARDS, &TCG_CARD_IDS);
    let cards_tag = cards.as_ref().map(|(tag, _)| *tag);
    // The name: the other length-delimited field, as text (absent when empty).
    let names: Vec<String> = entry
        .other
        .iter()
        .filter(|(tag, _)| **tag != characters_tag && Some(**tag) != cards_tag)
        .flat_map(|(_, values)| values)
        .map(|bytes| String::from_utf8(bytes.clone()).ok())
        .collect::<Option<_>>()?;
    if names.len() > 1 || entry.varints.values().any(|v| *v > 1_000) {
        return None;
    }
    Some(TcgDeck {
        name: names.into_iter().next().unwrap_or_default(),
        characters,
        cards: cards.map(|(_, cards)| cards).unwrap_or_default(),
        valid: false,
    })
}

/// A list of decks by id, or `None` if one is not a deck. Each holds a small
/// slot id and a valid flag (0/1), proto3 leaving either off at 0: across
/// decks the id is the one tag with distinct values; a lone deck's id is its
/// largest number, and it is valid when a 1 is there besides.
fn tcg_decks(list: &[Entry]) -> Option<BTreeMap<u32, TcgDeck>> {
    let decks: Vec<TcgDeck> = list.iter().map(tcg_deck).collect::<Option<_>>()?;
    let ids: Vec<(u32, bool)> = match list {
        [] => return None,
        [one] => {
            let id = *one.varints.values().max()?;
            let ones = one.varints.values().filter(|v| **v == 1).count();
            vec![(id as u32, ones > usize::from(id == 1))]
        }
        _ => {
            let id_tag = only_tag(list, &[], |values| {
                all_distinct(values) && values.iter().all(|v| (1..=1_000).contains(v))
            })?;
            list.iter()
                .map(|deck| {
                    let id = deck.varints[&id_tag] as u32;
                    let valid = deck
                        .varints
                        .iter()
                        .any(|(tag, v)| *tag != id_tag && *v == 1);
                    (id, valid)
                })
                .collect()
        }
    };
    let out: BTreeMap<u32, TcgDeck> = ids
        .into_iter()
        .zip(decks)
        .map(|((id, valid), deck)| (id, TcgDeck { valid, ..deck }))
        .collect();
    (out.len() == list.len()).then_some(out)
}

/// Recover the TCG collection from a `GCGDSDataNotify` payload, or `None` if
/// it is not one: a repeated field of card entries (a unique card id each, a
/// count 1-`MAX_TCG_COPIES`, maybe a proficiency; character and action
/// cards both) beside a repeated field of at least one deck (each with a list
/// of `TCG_DECK_CHARACTERS` character cards and usually `TCG_DECK_CARDS`
/// cards) and a few number lists. A player without a deck is not told apart.
///
/// A card's count and proficiency are both small: the count is the tag on
/// every entry (proto3 leaves a 0 off, and a held card is held at least
/// once), the proficiency the other.
pub fn matches_tcg_notify(data: &[u8]) -> Option<TcgCollection> {
    let msg = parse(data)?;
    let fields = fields_by_tag(&msg);
    let mut cards = None;
    let mut decks = None;
    let mut varints = Vec::new();
    for values in fields.values() {
        match values.first()? {
            Varint(_) if values.len() == 1 => varints.push(single_varint(values)?),
            Varint(_) => {}
            LengthDelimited(_) => {
                // Card and deck lists; the plain number lists beside them
                // (card backs, playmats) read as neither.
                let Some(list) = entries(values) else {
                    continue;
                };
                // Two card lists or two deck lists is another message.
                let twice = match tcg_cards(&list) {
                    Some(found) => cards.replace(found).is_some(),
                    None => tcg_decks(&list).is_some_and(|found| decks.replace(found).is_some()),
                };
                if twice {
                    return None;
                }
            }
            Fixed32(_) | Fixed64(_) => return None,
        }
    }
    // Character cards and action cards both, and a deck: other messages of
    // id-and-count entries (wood exchanges, Pot markers, tips) have neither.
    let cards = cards?;
    let decks = decks.filter(|decks| !decks.is_empty())?;
    let character = cards
        .keys()
        .any(|id| TCG_CHARACTER_IDS.contains(&u64::from(*id)));
    let action = cards
        .keys()
        .any(|id| TCG_ACTION_IDS.contains(&u64::from(*id)));
    if !character || !action {
        return None;
    }
    let current_deck = varints
        .iter()
        .find(|v| decks.contains_key(&(**v as u32)))
        .map(|v| *v as u32);
    Some(TcgCollection {
        cards,
        decks,
        current_deck,
    })
}

/// The copies count's tag: the one varint (besides the id) on nearly every
/// card, 1 to [`MAX_TCG_COPIES`] wherever it is sent. Nearly, not every: a
/// card held 0 times comes without it (a player's 7.1 list: 183 of 184), while
/// the face (1-2) and the proficiency are on few cards.
fn tcg_count_tag(list: &[Entry], id_tag: u32) -> Option<u32> {
    let tags: BTreeSet<u32> = list
        .iter()
        .flat_map(|e| e.varints.keys().copied())
        .collect();
    let found: Vec<u32> = tags
        .into_iter()
        .filter(|tag| *tag != id_tag)
        .filter(|tag| {
            let values: Vec<u64> = list
                .iter()
                .filter_map(|e| e.varints.get(tag).copied())
                .collect();
            values.len() * 10 >= list.len() * 9
                && values.iter().all(|v| (1..=MAX_TCG_COPIES).contains(v))
        })
        .collect();
    match found.as_slice() {
        [tag] => Some(*tag),
        _ => None,
    }
}

fn tcg_cards(list: &[Entry]) -> Option<BTreeMap<u32, TcgCard>> {
    if list.len() < 2 || list.len() > MAX_TCG_CARDS {
        return None;
    }
    let id_tag = only_tag(list, &[], |values| {
        all_distinct(values) && values.iter().all(|v| TCG_CARD_IDS.contains(v))
    })?;
    let count_tag = tcg_count_tag(list, id_tag)?;
    // Every other varint: the face (1-2) and the proficiency. The face is
    // there on few cards; the proficiency is the one tag with a value above 2.
    let mut proficiency_tag = None;
    for entry in list {
        for (tag, value) in &entry.varints {
            if *tag == id_tag || *tag == count_tag {
                continue;
            }
            if *value > MAX_TCG_PROFICIENCY {
                return None;
            }
            if *value > 2 && *proficiency_tag.get_or_insert(*tag) != *tag {
                return None;
            }
        }
    }
    Some(
        list.iter()
            // A card with no copies (its count left out, as protobuf leaves
            // out a 0) is not owned.
            .filter(|entry| entry.varints.contains_key(&count_tag))
            .map(|entry| {
                (
                    entry.varints[&id_tag] as u32,
                    TcgCard {
                        count: entry.varints[&count_tag] as u32,
                        proficiency: proficiency_tag
                            .and_then(|tag| entry.varints.get(&tag))
                            .map_or(0, |v| *v as u32),
                    },
                )
            })
            .collect(),
    )
}

// ------------------------------------------------------------------ aranara

/// `AranaraCollectionState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AranaraState {
    None,
    Collectable,
    Collected,
    Finished,
}

impl AranaraState {
    fn from(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Collectable),
            2 => Some(Self::Collected),
            3 => Some(Self::Finished),
            _ => None,
        }
    }
}

/// The most Aranara collections believed per type (7.1: 11).
const MAX_ARANARA: usize = 500;

/// Recover the Aranara collections, by collection type then id, from an
/// `AranaraCollectionDataNotify` payload, or `None` if it is not one: one
/// repeated field of suites, each a small type (1-64) beside one map of
/// collection ids (1-`MAX_ARANARA`) to a state (0-3), and nothing else; at
/// least one suite holds a collection.
pub fn matches_aranara_notify(data: &[u8]) -> Option<BTreeMap<u32, BTreeMap<u32, AranaraState>>> {
    let msg = parse(data)?;
    let fields = fields_by_tag(&msg);
    if fields.len() != 1 {
        return None;
    }
    let suites = entries(fields.values().next()?)?;
    let mut out = BTreeMap::new();
    for suite in &suites {
        let [(_, kind)] = suite.varints.iter().collect::<Vec<_>>()[..] else {
            return None;
        };
        if !(1..=64).contains(kind) || suite.fixed {
            return None;
        }
        if suite.other.len() != 1 {
            return None;
        }
        let map = suite.other.values().next()?;
        let mut states = BTreeMap::new();
        for bytes in map.iter() {
            let pair = entry(bytes)?;
            if !pair.lists.is_empty()
                || !pair.other.is_empty()
                || pair.fixed
                || pair.varints.keys().any(|t| *t > 2)
            {
                return None;
            }
            let id = pair.varints.get(&1).copied().unwrap_or(0);
            let state = AranaraState::from(pair.varints.get(&2).copied().unwrap_or(0))?;
            if !(1..=MAX_ARANARA as u64).contains(&id) || states.insert(id as u32, state).is_some()
            {
                return None;
            }
        }
        if out.insert(*kind as u32, states).is_some() {
            return None;
        }
    }
    out.values().any(|states| !states.is_empty()).then_some(out)
}

// ------------------------------------------------------------- weapon skins

/// Weapon skin ids (7.1: 310 001-390 001).
const WEAPON_SKIN_IDS: RangeInclusive<u64> = 300_000..=399_999;

/// Guids are minted as `(uid << 32) + counter`, so a guid is at least this.
const MIN_GUID: u64 = 1 << 32;

/// A plausible expiry, in unix seconds (2020 to 2106).
const EXPIRY: RangeInclusive<u64> = 1_577_836_800..=u32::MAX as u64;

/// One weapon skin the player holds (`_AvatarWeaponSkinDataNotify`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponSkin {
    pub id: u32,
    /// The avatar guids wearing it.
    pub worn_by: Vec<u64>,
    /// Held only until then (a trial), in unix seconds.
    pub expires: Option<u32>,
}

/// Recover the weapon skins from an `_AvatarWeaponSkinDataNotify` payload, or
/// `None` if it is not one: a repeated field of entries, each one weapon skin
/// id (unique, `WEAPON_SKIN_IDS`), maybe an expiry time and maybe a list of
/// avatar guids, beside at most a list of skin ids.
pub fn matches_weapon_skin_notify(data: &[u8]) -> Option<Vec<WeaponSkin>> {
    let msg = parse(data)?;
    let fields = fields_by_tag(&msg);
    let mut skins: Option<Vec<WeaponSkin>> = None;
    for values in fields.values() {
        let LengthDelimited(_) = values.first()? else {
            return None;
        };
        if let [LengthDelimited(bytes)] = values.as_slice()
            && let Some(ids) = packed(bytes)
            && ids.iter().all(|id| WEAPON_SKIN_IDS.contains(id))
        {
            continue;
        }
        let list = entries(values)?;
        let found = list
            .iter()
            .map(|entry| {
                let ids: Vec<u64> = entry
                    .varints
                    .values()
                    .copied()
                    .filter(|v| WEAPON_SKIN_IDS.contains(v))
                    .collect();
                let [id] = ids.as_slice() else {
                    return None;
                };
                let mut skin = WeaponSkin {
                    id: *id as u32,
                    ..Default::default()
                };
                for value in entry.varints.values().filter(|v| **v != *id) {
                    if !EXPIRY.contains(value) || skin.expires.replace(*value as u32).is_some() {
                        return None;
                    }
                }
                if entry.fixed {
                    return None;
                }
                for list in entry.lists.values() {
                    if !list.iter().all(|guid| *guid >= MIN_GUID) || !skin.worn_by.is_empty() {
                        return None;
                    }
                    skin.worn_by = list.clone();
                }
                Some(skin)
            })
            .collect::<Option<Vec<_>>>()?;
        if found.is_empty() || skins.replace(found).is_some() {
            return None;
        }
    }
    let skins = skins?;
    let ids: Vec<u64> = skins.iter().map(|s| u64::from(s.id)).collect();
    all_distinct(&ids).then_some(skins)
}

// ----------------------------------------------------------------- wardrobe

/// Wind glider ids (7.1: 140 001-140 018; they are `MATERIAL_FLYCLOAK` items).
pub const GLIDER_IDS: RangeInclusive<u64> = 140_000..=149_999;

/// Outfit ids (7.1: 200 302-212 301, the character's number in the middle).
pub const OUTFIT_IDS: RangeInclusive<u64> = 200_000..=214_999;

/// Sprint trail ids (7.1: 215 001-215 018, `MATERIAL_AVATAR_TRACE` items).
pub const TRAIL_IDS: RangeInclusive<u64> = 215_000..=215_999;

/// The most of one wardrobe kind believed.
const MAX_WARDROBE: usize = 2_000;

/// The most saved teams believed (7.1: 14 slots).
const MAX_TEAMS: usize = 100;

/// One saved team.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedTeam {
    /// The player's own text.
    pub name: String,
    pub avatar_guids: Vec<u64>,
}

/// What `AvatarDataNotify` holds beside the characters: the outfits, wind
/// gliders and sprint trails owned, and the saved teams.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AvatarWardrobe {
    pub gliders: Vec<u32>,
    pub outfits: Vec<u32>,
    pub trails: Vec<u32>,
    /// By team slot.
    pub teams: BTreeMap<u32, SavedTeam>,
    pub current_team: Option<u32>,
}

/// One `map<uint32, AvatarTeam>` entry: a slot (1-[`MAX_TEAMS`]) and a team
/// of a name (text, maybe empty) and up to four avatar guids.
fn saved_team(bytes: &[u8]) -> Option<(u32, SavedTeam)> {
    let pair = entry(bytes)?;
    let slot = *pair.varints.get(&1)?;
    if !(1..=MAX_TEAMS as u64).contains(&slot) || pair.varints.len() != 1 || pair.fixed {
        return None;
    }
    let team = match pair.other.get(&2).map(Vec::as_slice) {
        None => Entry::default(),
        Some([bytes]) => entry(bytes)?,
        Some(_) => return None,
    };
    if !team.varints.is_empty() || team.fixed {
        return None;
    }
    let mut out = SavedTeam::default();
    for (tag, values) in &team.other {
        let [bytes] = values.as_slice() else {
            return None;
        };
        match team.lists.get(tag) {
            Some(guids)
                if !guids.is_empty()
                    && guids.len() <= 4
                    && guids.iter().all(|g| *g >= MIN_GUID) =>
            {
                out.avatar_guids = guids.clone();
            }
            _ => out.name = String::from_utf8(bytes.clone()).ok()?,
        }
    }
    Some((slot as u32, out))
}

/// Read the wardrobe from an `AvatarDataNotify` payload (the message
/// [`crate::matches_avatar_packet`] already recognised): its top-level number
/// lists are told apart by their ids' blocks ([`GLIDER_IDS`], [`OUTFIT_IDS`],
/// [`TRAIL_IDS`]), the saved teams are the one repeated field of slot-to-team
/// map entries, and the team in use the lone number naming one of them. A
/// kind the message does not hold is left empty.
pub fn avatar_wardrobe(data: &[u8]) -> Option<AvatarWardrobe> {
    let msg = parse(data)?;
    let mut out = AvatarWardrobe::default();
    let mut lone = Vec::new();
    for values in fields_by_tag(&msg).values() {
        if let [Varint(value)] = values.as_slice() {
            lone.push(*value);
            continue;
        }
        if let Some(ids) =
            numbers(values).filter(|ids| !ids.is_empty() && ids.len() <= MAX_WARDROBE)
        {
            let ids32 = || {
                let mut ids: Vec<u32> = ids.iter().map(|id| *id as u32).collect();
                ids.sort_unstable();
                ids
            };
            if ids.iter().all(|id| GLIDER_IDS.contains(id)) {
                out.gliders = ids32();
            } else if ids.iter().all(|id| OUTFIT_IDS.contains(id)) {
                out.outfits = ids32();
            } else if ids.iter().all(|id| TRAIL_IDS.contains(id)) {
                out.trails = ids32();
            }
            continue;
        }
        if out.teams.is_empty() && values.len() <= MAX_TEAMS {
            let teams: Option<BTreeMap<u32, SavedTeam>> = values
                .iter()
                .map(|value| match value {
                    LengthDelimited(bytes) => saved_team(bytes),
                    _ => None,
                })
                .collect();
            if let Some(teams) = teams.filter(|teams| teams.len() == values.len()) {
                out.teams = teams;
            }
        }
    }
    out.current_team = lone
        .into_iter()
        .find(|v| out.teams.contains_key(&(*v as u32)))
        .map(|v| v as u32);
    Some(out)
}

// ----------------------------------------------------------------- hangouts

/// Hangout Event chapter ids (7.1: 101 401-108 301). Everything inside a
/// chapter -- its endings, pictures and story points -- is numbered
/// `chapter * 100 + n`, which is how its lists are recognised.
const HANGOUT_CHAPTER_IDS: RangeInclusive<u64> = 100_000..=999_999;

/// The most chapters believed (7.1: 19).
const MAX_HANGOUTS: usize = 1_000;

/// `CoopReward.State` (one reward per ending).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndingState {
    /// Seen, its reward not taken yet.
    Unclaimed,
    /// Not seen yet.
    Locked,
    /// Seen and its reward taken.
    Claimed,
}

/// One Hangout Event chapter (`CoopDataNotify`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HangoutChapter {
    /// `CoopChapter.State` (0 closed, 1 conditions not met, 2 open, 3
    /// accepted), when it could be told apart from the ending counts.
    pub state: Option<u32>,
    /// By ending (= its reward's id).
    pub endings: BTreeMap<u32, EndingState>,
    /// Pictures (CGs) by id: unlocked or not.
    pub pictures: BTreeMap<u32, bool>,
}

/// A chapter's list of `{id, one small number}` entries whose ids are the
/// chapter's own, by id; `None` if it is not one.
fn chapter_pairs(chapter: u64, values: &[Vec<u8>]) -> Option<BTreeMap<u32, u64>> {
    let mut out = BTreeMap::new();
    for bytes in values {
        let pair = entry(bytes)?;
        if !pair.lists.is_empty() || !pair.other.is_empty() || pair.fixed || pair.varints.len() > 2
        {
            return None;
        }
        let ids: Vec<(u32, u64)> = pair
            .varints
            .iter()
            .filter(|(_, v)| **v / 100 == chapter)
            .map(|(t, v)| (*t, *v))
            .collect();
        let [(id_tag, id)] = ids.as_slice() else {
            return None;
        };
        let value = pair
            .varints
            .iter()
            .find(|(t, _)| **t != *id_tag)
            .map_or(0, |(_, v)| *v);
        if out.insert(*id as u32, value).is_some() {
            return None;
        }
    }
    Some(out)
}

/// Recover the Hangout Events, by chapter, from a `CoopDataNotify` payload,
/// or `None` if it is not one: one repeated field of chapters, each with a
/// unique chapter id (`HANGOUT_CHAPTER_IDS`) and lists of entries numbered
/// after it.
///
/// Of a chapter's `{id, number}` lists, the endings are the one whose length
/// is one of the chapter's own counts (its ending total) and whose numbers are
/// 0-2; the other, with 0/1 flags, the pictures. An ending's state says
/// whether it was seen, so the counts beside it need no reading; the chapter's
/// state is the one small number left once those counts are set aside, and
/// `None` when it cannot be.
pub fn matches_coop_notify(data: &[u8]) -> Option<BTreeMap<u32, HangoutChapter>> {
    let msg = parse(data)?;
    for values in fields_by_tag(&msg).values() {
        if values.len() < 2 || values.len() > MAX_HANGOUTS {
            continue;
        }
        let Some(chapters) = entries(values) else {
            continue;
        };
        let mut out = BTreeMap::new();
        let ok = chapters.iter().all(|chapter| {
            let ids: Vec<u64> = chapter
                .varints
                .values()
                .copied()
                .filter(|v| HANGOUT_CHAPTER_IDS.contains(v))
                .collect();
            let [id] = ids.as_slice() else {
                return false;
            };
            let pair_lists: Vec<BTreeMap<u32, u64>> = chapter
                .other
                .values()
                .filter(|values| values.len() >= 2)
                .filter_map(|values| chapter_pairs(*id, values))
                .collect();
            let small: Vec<u64> = chapter
                .varints
                .values()
                .copied()
                .filter(|v| *v != *id && *v <= 1_000)
                .collect();
            let ending_lists: Vec<&BTreeMap<u32, u64>> = pair_lists
                .iter()
                .filter(|list| {
                    small.contains(&(list.len() as u64)) && list.values().all(|v| *v <= 2)
                })
                .collect();
            // Pictures hold 0/1 flags only; endings also 2 (claimed).
            let endings = match ending_lists.as_slice() {
                [one] => *one,
                several => match several
                    .iter()
                    .filter(|list| list.values().any(|v| *v == 2))
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    [one] => **one,
                    _ => return false,
                },
            };
            let picture_lists: Vec<&BTreeMap<u32, u64>> = pair_lists
                .iter()
                .filter(|list| !std::ptr::eq(*list, endings) && list.values().all(|v| *v <= 1))
                .collect();
            let seen = endings.values().filter(|v| **v != 1).count() as u64;
            let total = endings.len() as u64;
            let mut rest = small.clone();
            for count in [total, seen] {
                if let Some(at) = rest.iter().position(|v| *v == count) {
                    rest.remove(at);
                }
            }
            let state = match rest.as_slice() {
                [one] if *one <= 3 => Some(*one as u32),
                _ => None,
            };
            let found = HangoutChapter {
                state,
                endings: endings
                    .iter()
                    .map(|(id, v)| {
                        let state = match v {
                            0 => EndingState::Unclaimed,
                            1 => EndingState::Locked,
                            _ => EndingState::Claimed,
                        };
                        (*id, state)
                    })
                    .collect(),
                pictures: match picture_lists.as_slice() {
                    [one] => one.iter().map(|(id, v)| (*id, *v == 1)).collect(),
                    _ => BTreeMap::new(),
                },
            };
            out.insert(*id as u32, found).is_none()
        });
        if ok && !out.is_empty() {
            return Some(out);
        }
    }
    None
}

// ------------------------------------------------------------------- bosses

/// Monster ids (7.1: 20 010 101-29 xxx xxx for the bosses and elites).
const MONSTER_IDS: RangeInclusive<u64> = 20_000_000..=99_999_999;

/// The most boss entries believed (7.1: 103).
const MAX_BOSSES: usize = 2_000;

/// The weekly bosses' resin discounts (`WeeklyBossResinDiscountInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeeklyDiscounts {
    /// Discounts left this week.
    pub left: u32,
    pub limit: u32,
    /// What a weekly boss costs now, and without the discount.
    pub cost: u32,
    pub full_cost: u32,
}

/// The bosses and elites of the Adventurer Handbook (`GetInvestigationMonsterRsp`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BossList {
    /// By the Handbook's monster id: when each spot respawns next (unix
    /// seconds; a time already past means it is up).
    pub respawns: BTreeMap<u32, Vec<u32>>,
    pub weekly: Option<WeeklyDiscounts>,
}

/// A nested `{monster id, scene, group}`: the spot's monster config.
fn monster_config(bytes: &[u8]) -> bool {
    entry(bytes).is_some_and(|e| {
        e.lists.is_empty()
            && e.varints
                .values()
                .filter(|v| MONSTER_IDS.contains(v))
                .count()
                == 1
    })
}

/// A nested `{left, limit, cost, full cost}`: two pairs of small numbers,
/// left no more than the limit (at most 10) and the cost below the full cost
/// (at most 1,000 resin).
fn weekly_discounts(bytes: &[u8]) -> Option<WeeklyDiscounts> {
    let e = entry(bytes)?;
    if !e.lists.is_empty() || !e.other.is_empty() || e.fixed || e.varints.len() > 4 {
        return None;
    }
    let mut small: Vec<u64> = e.varints.values().copied().filter(|v| *v <= 10).collect();
    let mut resin: Vec<u64> = e
        .varints
        .values()
        .copied()
        .filter(|v| *v > 10 && *v <= 1_000)
        .collect();
    small.sort_unstable();
    resin.sort_unstable();
    // proto3 leaves 0 off: no discounts left shows as the limit alone.
    let (left, limit) = match small.as_slice() {
        [limit] => (0, *limit),
        [left, limit] => (*left, *limit),
        _ => return None,
    };
    let [cost, full_cost] = resin.as_slice() else {
        return None;
    };
    (limit > 0).then_some(WeeklyDiscounts {
        left: left as u32,
        limit: limit as u32,
        cost: *cost as u32,
        full_cost: *full_cost as u32,
    })
}

/// Recover the bosses from a `GetInvestigationMonsterRsp` payload, or `None`
/// if it is not one: one repeated field of monster entries (a unique id and a
/// list of spots), each spot holding one next-respawn time, a nested monster
/// config with a monster id (`MONSTER_IDS`) and, on the weekly bosses, the
/// week's resin discounts.
pub fn matches_investigation_monster_rsp(data: &[u8]) -> Option<BossList> {
    let msg = parse(data)?;
    for values in fields_by_tag(&msg).values() {
        if values.len() < 2 || values.len() > MAX_BOSSES {
            continue;
        }
        let Some(monsters) = entries(values) else {
            continue;
        };
        // The id: distinct across the entries (the city beside it repeats).
        let Some(id_tag) = only_tag(&monsters, &[], |values| {
            all_distinct(values) && values.iter().all(|v| (1..=99_999).contains(v))
        }) else {
            continue;
        };
        let mut out = BossList::default();
        let ok = monsters.iter().all(|monster| {
            let id = &monster.varints[&id_tag];
            let spot_lists: Vec<&Vec<Vec<u8>>> = monster.other.values().collect();
            let [spots] = spot_lists.as_slice() else {
                return false;
            };
            let mut times = Vec::new();
            for bytes in spots.iter() {
                let Some(spot) = entry(bytes) else {
                    return false;
                };
                let stamps: Vec<u64> = spot
                    .varints
                    .values()
                    .copied()
                    .filter(|v| EXPIRY.contains(v))
                    .collect();
                let [at] = stamps.as_slice() else {
                    return false;
                };
                if !spot.other.values().flatten().any(|b| monster_config(b)) {
                    return false;
                }
                if let Some(found) = spot
                    .other
                    .values()
                    .flatten()
                    .find_map(|b| weekly_discounts(b))
                {
                    out.weekly = Some(found);
                }
                times.push(*at as u32);
            }
            times.sort_unstable();
            out.respawns.insert(*id as u32, times).is_none()
        });
        if ok && !out.respawns.is_empty() {
            return Some(out);
        }
    }
    None
}

// ------------------------------------------------------------------- events

/// Event (activity) ids (7.1: 2051-6016); each run's schedule id is the
/// event's id times 1000 plus the run (2 051 001).
const EVENT_IDS: RangeInclusive<u64> = 1_000..=99_999;

/// The most events believed in one reply (7.1: ~15).
const MAX_EVENTS: usize = 500;

/// One progress counter of an event (`ActivityWatcherInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventWatcher {
    pub progress: u32,
    pub total: u32,
    pub claimed: bool,
}

/// One event running (`ActivityInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventInfo {
    pub schedule: u32,
    /// When it opened and closes (unix seconds).
    pub start: u32,
    pub end: u32,
    /// Its progress counters by watcher id.
    pub watchers: BTreeMap<u32, EventWatcher>,
}

/// An event's watchers: entries with a unique watcher id (above the event
/// ids), a total on every one, and beside it the progress (never above it)
/// and the claimed flag (only ever 1), both left off at 0. The progress is the
/// tag that ever goes past 1; when none does, the two are both 0 or 1 and read
/// the same either way.
fn event_watchers(values: &[Vec<u8>]) -> Option<BTreeMap<u32, EventWatcher>> {
    let list: Vec<Entry> = values.iter().map(|b| entry(b)).collect::<Option<_>>()?;
    if list
        .iter()
        .any(|e| !e.lists.is_empty() || !e.other.is_empty() || e.fixed || e.varints.len() > 4)
    {
        return None;
    }
    let id_tag = only_tag(&list, &[], |values| {
        all_distinct(values) && values.iter().all(|v| (100_000..=999_999_999).contains(v))
    })?;
    let tags: BTreeSet<u32> = list
        .iter()
        .flat_map(|e| e.varints.keys().copied())
        .filter(|t| *t != id_tag)
        .collect();
    if tags.len() > 3 {
        return None;
    }
    let at = |e: &Entry, t: u32| e.varints.get(&t).copied().unwrap_or(0);
    // The total: on every entry, never below any other number of the entry
    // but the claimed flag; the progress: the other tag that ever passes 1.
    let on_all = |t: u32| list.iter().all(|e| e.varints.contains_key(&t));
    let total_tag = tags.iter().copied().filter(|t| on_all(*t)).find(|t| {
        list.iter().all(|e| {
            tags.iter()
                .all(|o| o == t || at(e, *o) <= 1 || at(e, *o) <= at(e, *t))
        })
    })?;
    let over_one = |tag: u32| list.iter().any(|e| at(e, tag) > 1);
    let progress_tag = tags
        .iter()
        .copied()
        .find(|t| *t != total_tag && over_one(*t));
    let claimed_tag = tags
        .iter()
        .copied()
        .find(|t| *t != total_tag && Some(*t) != progress_tag);
    list.iter()
        .map(|e| {
            let total = e.varints[&total_tag];
            let progress = progress_tag
                .and_then(|t| e.varints.get(&t))
                .copied()
                .unwrap_or(0);
            if progress > total {
                return None;
            }
            Some((
                e.varints[&id_tag] as u32,
                EventWatcher {
                    progress: progress as u32,
                    total: total as u32,
                    claimed: claimed_tag
                        .and_then(|t| e.varints.get(&t))
                        .is_some_and(|v| *v == 1),
                },
            ))
        })
        .collect()
}

/// Recover the events running, by event id, from a `GetActivityInfoRsp`
/// payload, or `None` if it is not one: one repeated field of events, each
/// with its id (`EVENT_IDS`) and a schedule id that is that id times 1000
/// plus the run, two or three times (when it opened, its first day, when it
/// closes) and maybe a list of progress counters. Each event's own details
/// (a submessage per kind of event) are not read.
pub fn matches_activity_info_rsp(data: &[u8]) -> Option<BTreeMap<u32, EventInfo>> {
    let msg = parse(data)?;
    for values in fields_by_tag(&msg).values() {
        if values.is_empty() || values.len() > MAX_EVENTS {
            continue;
        }
        let Some(events) = entries(values) else {
            continue;
        };
        let mut out = BTreeMap::new();
        let ok = events.iter().all(|event| {
            let pair = event.varints.values().find_map(|id| {
                EVENT_IDS.contains(id).then_some(())?;
                let schedule = event
                    .varints
                    .values()
                    .find(|s| **s / 1_000 == *id && **s % 1_000 > 0)?;
                Some((*id, *schedule))
            });
            let Some((id, schedule)) = pair else {
                return false;
            };
            let times: Vec<u64> = event
                .varints
                .values()
                .copied()
                .filter(|v| EXPIRY.contains(v))
                .collect();
            if !(2..=4).contains(&times.len()) {
                return false;
            }
            let watchers = event
                .other
                .values()
                .filter(|values| !values.is_empty())
                .filter_map(|values| event_watchers(values))
                .max_by_key(BTreeMap::len)
                .unwrap_or_default();
            out.insert(
                id as u32,
                EventInfo {
                    schedule: schedule as u32,
                    start: *times.iter().min().unwrap_or(&0) as u32,
                    end: *times.iter().max().unwrap_or(&0) as u32,
                    watchers,
                },
            )
            .is_none()
        });
        // The schedule list (`ActivityScheduleInfoNotify`) pairs ids and times
        // too, but no event of it carries details of its own.
        let detailed = events.iter().any(|event| !event.other.is_empty());
        if ok && detailed && !out.is_empty() {
            return Some(out);
        }
    }
    None
}

// ------------------------------------------------------------------ comfort

/// The most comfort a realm or mansion is believed to hold (7.1: ~15,000).
const MAX_COMFORT: u64 = 10_000_000;

/// One realm's comfort (`HomeModuleComfortInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HomeComfort {
    /// The mansion's comfort.
    pub mansion: u32,
    /// Each outdoor block's comfort.
    pub blocks: Vec<u32>,
}

/// Recover the Serenitea Pot's comfort, by realm (module) id, from a
/// `HomeComfortInfoNotify` payload, or `None` if it is not one: the one field
/// is a list of realms, each a small realm id (1-64), the mansion's comfort
/// (a number past 100) and one list of 1-10 block comforts, and nothing
/// else. Each realm's comfort is at least one of its blocks' (the mansion sits
/// beside them), which other messages of a number and a short list don't
/// show.
pub fn matches_home_comfort_notify(data: &[u8]) -> Option<BTreeMap<u32, HomeComfort>> {
    let msg = parse(data)?;
    let fields = fields_by_tag(&msg);
    if fields.len() != 1 {
        return None;
    }
    let realms = entries(fields.values().next()?)?;
    if realms.is_empty() || realms.len() > 64 {
        return None;
    }
    let mut out = BTreeMap::new();
    for realm in &realms {
        if realm.fixed || realm.lists.len() != 1 || realm.varints.len() != 2 {
            return None;
        }
        let blocks = realm.lists.values().next()?;
        if blocks.is_empty()
            || blocks.len() > 10
            || blocks.iter().any(|v| *v > MAX_COMFORT || *v < 100)
        {
            return None;
        }
        let mut small = realm
            .varints
            .values()
            .copied()
            .filter(|v| (1..=64).contains(v));
        let id = small.next()?;
        if small.next().is_some() {
            return None;
        }
        let mansion = realm.varints.values().copied().find(|v| *v != id)?;
        if !(100..=MAX_COMFORT).contains(&mansion) {
            return None;
        }
        let found = HomeComfort {
            mansion: mansion as u32,
            blocks: blocks.iter().map(|v| *v as u32).collect(),
        };
        if out.insert(id as u32, found).is_some() {
            return None;
        }
    }
    Some(out)
}

// --------------------------------------------------------------- tcg extras

/// TCG level (challenge) ids: five or six digits (7.1: 11 001-270 501).
const TCG_LEVEL_IDS: RangeInclusive<u64> = 10_000..=999_999;

/// The TCG challenges cleared (`GCGLevelChallengeNotify`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TcgChallenges {
    /// By level id: the challenge ids finished in it (empty: played, none).
    pub levels: BTreeMap<u32, Vec<u32>>,
    /// Boss challenges unlocked, by id: the level ids that unlock it.
    pub bosses: BTreeMap<u32, Vec<u32>>,
}

/// Entries of one id and at most one list each, the id unique and in `ids`,
/// the list's values in `items`; by id. `None` if they are not that.
fn id_lists(
    values: &[Value<'_>],
    ids: RangeInclusive<u64>,
    items: RangeInclusive<u64>,
) -> Option<BTreeMap<u32, Vec<u32>>> {
    let list = entries(values)?;
    let mut out = BTreeMap::new();
    for e in &list {
        if e.fixed || e.varints.len() != 1 || e.lists.len() > 1 {
            return None;
        }
        // Every length-delimited field is the list (a string would not be).
        if e.other.len() != e.lists.len() {
            return None;
        }
        let id = *e.varints.values().next()?;
        let inner = e.lists.values().next().cloned().unwrap_or_default();
        if !ids.contains(&id) || !inner.iter().all(|v| items.contains(v)) {
            return None;
        }
        if out
            .insert(id as u32, inner.iter().map(|v| *v as u32).collect())
            .is_some()
        {
            return None;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Recover the TCG challenges cleared from a `GCGLevelChallengeNotify`
/// payload, or `None` if it is not one: one repeated field of levels (a level
/// id, `TCG_LEVEL_IDS`, and maybe a list of small challenge ids) beside one of
/// boss challenges (a small id and a list of level ids), and maybe a list of
/// world challenge ids.
pub fn matches_tcg_challenges(data: &[u8]) -> Option<TcgChallenges> {
    let msg = parse(data)?;
    let mut levels = None;
    let mut bosses = None;
    for values in fields_by_tag(&msg).values() {
        let LengthDelimited(_) = values.first()? else {
            return None;
        };
        if let Some(found) = id_lists(values, TCG_LEVEL_IDS, 1..=9_999) {
            if levels.replace(found).is_some() {
                return None;
            }
        } else if let Some(found) = id_lists(values, 1..=9_999, TCG_LEVEL_IDS) {
            if bosses.replace(found).is_some() {
                return None;
            }
        } else if values.len() == 1
            && let [LengthDelimited(bytes)] = values.as_slice()
            && packed(bytes).is_some()
        {
            // The world challenges unlocked: a plain list, not kept.
        } else {
            return None;
        }
    }
    let levels = levels?;
    // A handful of levels is any old list of ids; the Tavern's levels alone
    // have no boss list beside them.
    (levels.len() >= 3 && bosses.is_some()).then(|| TcgChallenges {
        levels,
        bosses: bosses.unwrap_or_default(),
    })
}

/// Recover the Tavern challenges unlocked, by character id, from a
/// `GCGTCTavernChallengeDataNotify` payload, or `None` if it is not one: its
/// one field a list of characters, each a small id and a list of the level
/// ids unlocked with them.
pub fn matches_tcg_tavern(data: &[u8]) -> Option<BTreeMap<u32, Vec<u32>>> {
    let msg = parse(data)?;
    let fields = fields_by_tag(&msg);
    if fields.len() != 1 {
        return None;
    }
    let values = fields.values().next()?;
    // At least 3 characters, their levels all six digits (7.1: 14, 110 101-
    // 160 204); `ExclusiveRuleNotify`'s two rules list shorter ids too.
    if values.len() < 3 {
        return None;
    }
    let found = id_lists(values, 1..=999, 100_000..=999_999)?;
    found
        .values()
        .all(|levels| !levels.is_empty())
        .then_some(found)
}

// --------------------------------------------------------------- music game

/// One song of the music game (`_MusicGameBookAllDataNotify`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MusicSong {
    /// Best score; 0 when never played.
    pub score: u32,
    pub claimed: bool,
}

/// Recover the music game's songs, by beatmap id, from a
/// `_MusicGameBookAllDataNotify` payload, or `None` if it is not one: a
/// repeated field of at least 20 varint-only songs (a unique beatmap id,
/// 1-99,999, on every one; a best score past 100 on some; a claimed flag of 1
/// on some) beside a settings submessage and number lists.
pub fn matches_music_game_notify(data: &[u8]) -> Option<BTreeMap<u32, MusicSong>> {
    let msg = parse(data)?;
    let mut songs = None;
    let mut settings = false;
    for values in fields_by_tag(&msg).values() {
        match values.first()? {
            Varint(_) => {}
            Fixed32(_) | Fixed64(_) => return None,
            LengthDelimited(_) if values.len() >= 20 => {
                let list = entries(values)?;
                if list.iter().any(|e| {
                    !e.lists.is_empty() || !e.other.is_empty() || e.fixed || e.varints.len() > 3
                }) {
                    return None;
                }
                let id_tag = only_tag(&list, &[], |v| {
                    all_distinct(v) && v.iter().all(|id| (1..=99_999).contains(id))
                })?;
                let tags: BTreeSet<u32> = list
                    .iter()
                    .flat_map(|e| e.varints.keys().copied())
                    .filter(|t| *t != id_tag)
                    .collect();
                if tags.len() > 2 {
                    return None;
                }
                let values_of =
                    |t: u32| list.iter().filter_map(move |e| e.varints.get(&t).copied());
                let score_tag = tags
                    .iter()
                    .copied()
                    .find(|t| values_of(*t).all(|v| (100..=10_000_000).contains(&v)));
                let flag_tag = tags
                    .iter()
                    .copied()
                    .find(|t| Some(*t) != score_tag && values_of(*t).all(|v| v == 1));
                if tags.len() != usize::from(score_tag.is_some()) + usize::from(flag_tag.is_some())
                {
                    return None;
                }
                let found: BTreeMap<u32, MusicSong> = list
                    .iter()
                    .map(|e| {
                        (
                            e.varints[&id_tag] as u32,
                            MusicSong {
                                score: score_tag
                                    .and_then(|t| e.varints.get(&t))
                                    .map_or(0, |v| *v as u32),
                                claimed: flag_tag.is_some_and(|t| e.varints.contains_key(&t)),
                            },
                        )
                    })
                    .collect();
                if songs.replace(found).is_some() {
                    return None;
                }
            }
            LengthDelimited(_) => {
                if let [LengthDelimited(bytes)] = values.as_slice()
                    && entry(bytes).is_some_and(|e| e.varints.len() >= 3)
                {
                    settings = true;
                }
            }
        }
    }
    songs.filter(|_| settings)
}

// ---------------------------------------------------------------- miliastra

/// Miliastra Wonderland cosmetic ids: six digits (7.1: 260 087-550 912).
const BEYOND_COSMETIC_IDS: RangeInclusive<u64> = 200_000..=999_999;

/// Recover the Miliastra Wonderland cosmetics owned, by id with when each was
/// obtained (0 when the game sent no time), from a `_BeyondCosmeticDataNotify`
/// payload, or `None` if it is not one: at least two repeated fields of
/// entries, each one cosmetic id (unique, `BEYOND_COSMETIC_IDS`) and maybe a
/// time and a small flag, beside number lists (emotes, poses).
pub fn matches_beyond_cosmetic_notify(data: &[u8]) -> Option<BTreeMap<u32, u32>> {
    let msg = parse(data)?;
    let mut out = BTreeMap::new();
    let mut kinds = 0;
    for values in fields_by_tag(&msg).values() {
        if values.len() < 2 {
            continue;
        }
        let LengthDelimited(_) = values.first()? else {
            continue;
        };
        let Some(list) = entries(values) else {
            continue;
        };
        let mut found = BTreeMap::new();
        let ok = list.iter().all(|e| {
            if e.fixed || !e.lists.is_empty() || !e.other.is_empty() || e.varints.len() > 3 {
                return false;
            }
            let ids: Vec<u64> = e
                .varints
                .values()
                .copied()
                .filter(|v| BEYOND_COSMETIC_IDS.contains(v))
                .collect();
            let [id] = ids.as_slice() else {
                return false;
            };
            let time = e
                .varints
                .values()
                .copied()
                .find(|v| EXPIRY.contains(v))
                .unwrap_or(0);
            if e.varints.values().any(|v| v != id && *v != time && *v > 10) {
                return false;
            }
            found.insert(*id as u32, time as u32).is_none()
        });
        if !ok {
            return None;
        }
        kinds += 1;
        out.extend(found);
    }
    (kinds >= 2).then_some(out)
}

// ------------------------------------------------------------------ sign-in

/// One login event's calendar (`SignInInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignIn {
    /// Days signed in.
    pub days: u32,
    /// Days whose reward was taken.
    pub claimed: Vec<u32>,
    pub last_at: u32,
    pub start: u32,
    pub end: u32,
}

/// Recover the login events, by config id, from a `SignInInfoRsp` payload, or
/// `None` if it is not one: a repeated field of calendars, each with a config
/// id (4-6 digits), three times (the last sign-in, when it opened, when it
/// closes), the days signed in (1-31) and the list of days claimed (each no
/// later than the days signed in), beside the calendar's days (submessages).
pub fn matches_sign_in_rsp(data: &[u8]) -> Option<BTreeMap<u32, SignIn>> {
    let msg = parse(data)?;
    for values in fields_by_tag(&msg).values() {
        let Some(list) = entries(values) else {
            continue;
        };
        let mut out = BTreeMap::new();
        let ok = !list.is_empty()
            && list.iter().all(|e| {
                let mut times: Vec<u64> = e
                    .varints
                    .values()
                    .copied()
                    .filter(|v| EXPIRY.contains(v))
                    .collect();
                if times.len() != 3 || e.other.is_empty() {
                    return false;
                }
                times.sort_unstable();
                let Some(id) = e
                    .varints
                    .values()
                    .copied()
                    .find(|v| (1_000..=999_999).contains(v))
                else {
                    return false;
                };
                let claimed: Vec<&Vec<u64>> = e
                    .lists
                    .values()
                    .filter(|l| l.iter().all(|d| (1..=31).contains(d)))
                    .collect();
                let [claimed] = claimed.as_slice() else {
                    return false;
                };
                let max = claimed.iter().copied().max().unwrap_or(0);
                let Some(days) = e
                    .varints
                    .values()
                    .copied()
                    .find(|v| (1..=31).contains(v) && *v >= max)
                else {
                    return false;
                };
                // The last sign-in falls inside the event; it opened before it closes.
                let (last, start, end) = if times[0] < times[1] && times[1] <= times[2] {
                    // Opened, last, closes.
                    (times[1], times[0], times[2])
                } else {
                    return false;
                };
                out.insert(
                    id as u32,
                    SignIn {
                        days: days as u32,
                        claimed: claimed.iter().map(|d| *d as u32).collect(),
                        last_at: last as u32,
                        start: start as u32,
                        end: end as u32,
                    },
                )
                .is_none()
            });
        if ok {
            return Some(out);
        }
    }
    None
}

// ------------------------------------------------------------------ profile

/// Text a player typed: valid UTF-8 without control characters (a newline
/// allowed), so a small message of numbers (the birthday's four bytes are
/// valid UTF-8) is never taken for one.
fn player_text(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    (!text.chars().any(|c| c.is_control() && c != '\n')).then(|| text.to_string())
}

/// The in-game nickname from `PlayerDataNotify` (7.1: command 3272, tag 13),
/// the login message the player properties come from: its one text field
/// sent once (the property map's entries repeat one tag), 1-64 bytes.
pub fn player_nickname(data: &[u8]) -> Option<String> {
    let msg = parse(data)?;
    let texts: Vec<String> = fields_by_tag(&msg)
        .values()
        .filter_map(|values| match values.as_slice() {
            [LengthDelimited(bytes)] if (1..=64).contains(&bytes.len()) => player_text(bytes),
            _ => None,
        })
        .collect();
    match texts.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// A profile picture as the game sends it; every part optional.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProfilePicture {
    /// `ProfilePictureExcelConfigData` id (7.1: 1001...).
    pub id: Option<u32>,
    /// A character's icon (10000xxx), older clients' way.
    pub avatar_id: Option<u32>,
    pub costume_id: Option<u32>,
    /// The frame around it (7.1: 100016...).
    pub frame_id: Option<u32>,
}

/// One player's profile card (`SocialDetail` in `GetPlayerSocialDetailRsp`,
/// 7.1: command 25989; the game asks for the player's own at login, and for
/// others' when their profile is opened).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SocialProfile {
    pub uid: u32,
    /// The card's texts by tag: the nickname and, when set, the signature.
    pub texts: Vec<String>,
    pub name_card: Option<u32>,
    pub picture: Option<ProfilePicture>,
    /// (month, day).
    pub birthday: Option<(u32, u32)>,
}

/// Name card item ids (7.1: 210001-210279).
const NAME_CARD_IDS: RangeInclusive<u64> = 210_001..=210_999;

/// A UID: nine or ten digits.
const UID_RANGE: RangeInclusive<u64> = 100_000_000..=4_294_967_295;

fn profile_picture(bytes: &[u8]) -> Option<ProfilePicture> {
    let msg = parse(bytes)?;
    let mut out = ProfilePicture::default();
    for values in fields_by_tag(&msg).values() {
        let value = single_varint(values)?;
        let slot = match value {
            1_000..=99_999 => &mut out.id,
            100_000..=199_999 => &mut out.frame_id,
            200_000..=999_999 => &mut out.costume_id,
            10_000_000..=10_999_999 => &mut out.avatar_id,
            _ => return None,
        };
        if slot.replace(value as u32).is_some() {
            return None;
        }
    }
    (out.id.is_some() || out.avatar_id.is_some()).then_some(out)
}

fn birthday(bytes: &[u8]) -> Option<(u32, u32)> {
    let msg = parse(bytes)?;
    let fields = fields_by_tag(&msg);
    let [month, day] = fields.values().collect::<Vec<_>>()[..] else {
        return None;
    };
    let (month, day) = (single_varint(month)?, single_varint(day)?);
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((month as u32, day as u32))
}

/// Recover a profile card from a `GetPlayerSocialDetailRsp` payload, or `None`
/// if it is not one: one nested detail (beside at most a retcode) holding
/// exactly one UID-sized number, at least one text, and a name card id. The
/// other parts are told apart by value: the birthday is a small message of a
/// month and a day, the picture one of picture, frame, costume and character
/// ids.
pub fn matches_social_detail(data: &[u8]) -> Option<SocialProfile> {
    let msg = parse(data)?;
    let mut detail = None;
    for values in fields_by_tag(&msg).values() {
        match values.as_slice() {
            [Varint(_)] => {}
            [LengthDelimited(bytes)] if detail.is_none() => detail = Some(*bytes),
            _ => return None,
        }
    }
    let msg = parse(detail?)?;
    let mut out = SocialProfile::default();
    let mut uid = None;
    for values in fields_by_tag(&msg).values() {
        match values.as_slice() {
            [Varint(v)] if UID_RANGE.contains(v) => {
                if uid.replace(*v as u32).is_some() {
                    return None;
                }
            }
            [Varint(v)] if NAME_CARD_IDS.contains(v) => {
                if out.name_card.replace(*v as u32).is_some() {
                    return None;
                }
            }
            [LengthDelimited(bytes)] => {
                if let Some(text) = player_text(bytes) {
                    out.texts.push(text);
                } else if let Some(day) = birthday(bytes) {
                    out.birthday.get_or_insert(day);
                } else if let Some(picture) = profile_picture(bytes) {
                    out.picture.get_or_insert(picture);
                }
            }
            _ => {}
        }
    }
    out.uid = uid?;
    if out.texts.is_empty() || out.texts.len() > 4 || out.name_card.is_none() {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn v(tag: u32, value: u64) -> Vec<u8> {
        let mut out = varint(u64::from(tag) << 3);
        out.extend(varint(value));
        out
    }

    fn b(tag: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = varint((u64::from(tag) << 3) | 2);
        out.extend(varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    fn list(tag: u32, values: &[u64]) -> Vec<u8> {
        b(
            tag,
            &values.iter().flat_map(|v| varint(*v)).collect::<Vec<u8>>(),
        )
    }

    #[test]
    fn the_archive_is_read_by_tab() {
        // CodexDataFullNotify-like: per tab {type 4, ids 2, viewed 11}, read state beside.
        let tab = |kind: u64, ids: &[u64]| {
            b(
                15,
                &[v(4, kind), list(2, ids), list(11, &vec![1; ids.len()])].concat(),
            )
        };
        let data = [
            list(6, &[60_100_024, 60_100_025]),
            v(10, 60_100_025),
            tab(1, &[10_010_002, 11_080_004]),
            tab(2, &[30_211_101, 30_215_516]),
            tab(3, &[20_010_101]),
            tab(4, &[40_100_001]),
            tab(5, &[50_001_001]),
            tab(7, &[70_100_001]),
        ]
        .concat();
        let archive = matches_codex_notify(&data).unwrap();
        assert_eq!(archive[&ArchiveTab::Weapons], vec![30_211_101, 30_215_516]);
        assert_eq!(archive[&ArchiveTab::Viewpoints], vec![70_100_001]);
        assert_eq!(archive.len(), 6);
        // Two tabs alike, or too few, is something else.
        let twice = [
            tab(1, &[10_010_002]),
            tab(1, &[10_010_003]),
            tab(3, &[20_010_101]),
            tab(4, &[40_100_001]),
            tab(5, &[50_001_001]),
        ]
        .concat();
        assert_eq!(matches_codex_notify(&twice), None);
        assert_eq!(matches_codex_notify(&tab(1, &[10_010_002])), None);
    }

    #[test]
    fn defeats_are_told_from_captures() {
        let data = [
            list(14, &[20_010_101, 20_010_201, 28_010_101]),
            list(10, &[57, 3, 0]),
            list(15, &[0, 0, 12]),
            v(2, 0),
        ]
        .concat();
        let found = matches_codex_kill_count(&data).unwrap();
        assert_eq!(
            found[&20_010_101],
            DefeatCount {
                defeated: 57,
                captured: 0
            }
        );
        assert_eq!(
            found[&28_010_101],
            DefeatCount {
                defeated: 0,
                captured: 12
            }
        );
        // Unequal lengths, or no id list, is something else.
        let short = [
            list(14, &[20_010_101, 20_010_201]),
            list(10, &[1]),
            list(15, &[0, 0]),
        ]
        .concat();
        assert_eq!(matches_codex_kill_count(&short), None);
        let no_ids = [list(14, &[1, 2]), list(10, &[3, 4]), list(15, &[0, 1])].concat();
        assert_eq!(matches_codex_kill_count(&no_ids), None);
    }

    #[test]
    fn the_handbook_reads_chapters_and_targets() {
        let chapter = |id: u64, progress: u64, total: u64, state: u64| {
            let mut e = v(7, id);
            if progress > 0 {
                e.extend(v(1, progress));
            }
            e.extend(v(13, total));
            e.extend(v(15, state));
            b(2, &e)
        };
        let target = |quest: u64, chapter: u64, progress: u64, total: u64, state: u64| {
            let mut e = v(2, total);
            e.extend(v(5, chapter));
            e.extend(v(6, state));
            e.extend(v(8, quest));
            if progress > 0 {
                e.extend(v(13, progress));
            }
            b(10, &e)
        };
        let data = [
            chapter(10_001, 10, 10, 3),
            chapter(20_001, 4, 11, 1),
            target(60_001, 10_001, 1, 1, 3),
            target(60_002, 20_001, 0, 5, 1),
            target(60_003, 20_001, 50_000, 50_000, 2),
        ]
        .concat();
        let handbook = matches_investigation_notify(&data).unwrap();
        assert_eq!(
            handbook.chapters[&20_001],
            HandbookProgress {
                progress: 4,
                total: 11,
                state: HandbookState::InProgress
            }
        );
        assert_eq!(handbook.targets[&60_002].chapter, 20_001);
        assert_eq!(handbook.targets[&60_002].progress.progress, 0);
        assert_eq!(
            handbook.targets[&60_003].progress.state,
            HandbookState::Complete
        );
    }

    #[test]
    fn tcg_cards_and_decks_are_read() {
        let card = |id: u64, num: u64, proficiency: u64| {
            let mut e = v(9, id);
            e.extend(v(14, num));
            if proficiency > 0 {
                e.extend(v(13, proficiency));
            }
            b(6, &e)
        };
        let cards: Vec<u64> = (0..30).map(|i| 211_011 + i).collect();
        let deck = |id: u64, name: &str| {
            b(
                10,
                &[
                    list(6, &[1101, 1202, 1303]),
                    list(12, &cards),
                    b(13, name.as_bytes()),
                    v(3, id),
                    v(5, 1),
                ]
                .concat(),
            )
        };
        let data = [
            v(1, 3),
            list(3, &[0, 1, 2]),
            card(1101, 1, 27),
            card(211_011, 2, 0),
            card(333_008, 2, 6),
            deck(1, "Freeze"),
            deck(3, "Mono Pyro"),
        ]
        .concat();
        let found = matches_tcg_notify(&data).unwrap();
        // A card held 0 times comes without its count: still the collection,
        // without that card.
        let owned: Vec<u8> = (0..9).flat_map(|i| card(332_001 + i, 1, 0)).collect();
        let unowned = b(6, &v(9, 211_012));
        let with_unowned = [&data[..], &owned[..], &unowned[..]].concat();
        let more = matches_tcg_notify(&with_unowned).unwrap();
        assert_eq!(more.cards.len(), found.cards.len() + 9);
        assert!(!more.cards.contains_key(&211_012));
        assert_eq!(
            found.cards[&1101],
            TcgCard {
                count: 1,
                proficiency: 27
            }
        );
        assert_eq!(
            found.cards[&211_011],
            TcgCard {
                count: 2,
                proficiency: 0
            }
        );
        assert_eq!(found.decks[&3].name, "Mono Pyro");
        assert_eq!(found.decks[&3].characters, vec![1101, 1202, 1303]);
        assert!(found.decks[&3].valid);
        assert!(found.decks[&1].valid);
        assert_eq!(found.current_deck, Some(3));
    }

    #[test]
    fn the_nickname_is_the_one_text_beside_the_properties() {
        let prop = |id: u64| b(8, &[v(1, id), b(2, &[v(1, id), v(4, 5)].concat())].concat());
        let data = [
            prop(10_009),
            prop(10_010),
            b(13, b"player01"),
            v(15, 1_792_000_000),
        ]
        .concat();
        assert_eq!(player_nickname(&data).as_deref(), Some("player01"));
        // Two texts, or none: not the nickname.
        let two = [&data[..], &b(14, b"player02")[..]].concat();
        assert_eq!(player_nickname(&two), None);
        assert_eq!(player_nickname(&prop(10_009)), None);
    }

    #[test]
    fn a_profile_card_is_read_by_value() {
        let detail = [
            v(1, 812_345_678),
            b(2, b"player01"),
            v(3, 60),
            b(5, "Hello\nTeyvat".as_bytes()),
            b(6, &[v(1, 12), v(2, 18)].concat()),
            v(7, 8),
            v(14, 210_279),
            b(22, &[v(1, 10_000_140), v(2, 90)].concat()),
            b(22, &[v(1, 10_000_143), v(2, 90)].concat()),
            list(23, &[210_001, 210_279]),
            b(25, &[v(3, 1001), v(4, 100_016)].concat()),
        ]
        .concat();
        let data = b(6, &detail);
        let found = matches_social_detail(&data).unwrap();
        assert_eq!(found.uid, 812_345_678);
        assert_eq!(found.texts, ["player01", "Hello\nTeyvat"]);
        assert_eq!(found.birthday, Some((12, 18)));
        assert_eq!(found.name_card, Some(210_279));
        assert_eq!(
            found.picture,
            Some(ProfilePicture {
                id: Some(1001),
                frame_id: Some(100_016),
                ..Default::default()
            })
        );
        // An older picture: a character in an outfit.
        let picture = profile_picture(&[v(1, 10_000_046), v(2, 204_601)].concat()).unwrap();
        assert_eq!(picture.avatar_id, Some(10_000_046));
        assert_eq!(picture.costume_id, Some(204_601));
        // A list of details (a friend list) or no name card: another message.
        assert_eq!(
            matches_social_detail(&[b(6, &detail), b(6, &detail)].concat()),
            None
        );
        assert_eq!(
            matches_social_detail(&b(6, &[v(1, 812_345_678), b(2, b"player01")].concat())),
            None
        );
    }

    #[test]
    fn aranara_states_are_read() {
        let pair = |id: u64, state: u64| b(4, &[v(1, id), v(2, state)].concat());
        let data = b(3, &[pair(1, 3), pair(2, 0), pair(11, 1), v(7, 1)].concat());
        let found = matches_aranara_notify(&data).unwrap();
        assert_eq!(found[&1][&1], AranaraState::Finished);
        assert_eq!(found[&1][&11], AranaraState::Collectable);
        // A state past 3 is another message.
        assert_eq!(
            matches_aranara_notify(&b(3, &[pair(1, 9), v(7, 1)].concat())),
            None
        );
    }

    #[test]
    fn weapon_skins_are_read() {
        let guid = (812_345_678u64 << 32) + 5;
        let skin = |id: u64, expires: Option<u64>, worn: &[u64]| {
            let mut e = v(13, id);
            if let Some(time) = expires {
                e.extend(v(1, time));
            }
            if !worn.is_empty() {
                e.extend(list(10, worn));
            }
            b(1, &e)
        };
        let data = [
            skin(310_001, None, &[guid]),
            skin(390_001, Some(1_794_362_399), &[]),
            list(2, &[310_001, 390_001]),
        ]
        .concat();
        let found = matches_weapon_skin_notify(&data).unwrap();
        assert_eq!(
            found[0],
            WeaponSkin {
                id: 310_001,
                worn_by: vec![guid],
                expires: None
            }
        );
        assert_eq!(found[1].expires, Some(1_794_362_399));
    }

    #[test]
    fn the_wardrobe_is_read_beside_the_characters() {
        let guid = (812_345_678u64 << 32) + 7;
        let team = |slot: u64, name: &str, guids: &[u64]| {
            let mut inner = Vec::new();
            if !name.is_empty() {
                inner.extend(b(6, name.as_bytes()));
            }
            inner.extend(list(15, guids));
            b(8, &[v(1, slot), b(2, &inner)].concat())
        };
        let data = [
            list(1, &[5, 6, 7]),
            v(3, 14),
            list(5, &[140_001, 140_018]),
            list(6, &[212_301, 200_302]),
            team(5, "Freeze", &[guid]),
            team(14, "", &[guid, guid + 1]),
            v(9, guid),
            list(13, &[215_001]),
        ]
        .concat();
        let found = avatar_wardrobe(&data).unwrap();
        assert_eq!(found.gliders, vec![140_001, 140_018]);
        assert_eq!(found.outfits, vec![200_302, 212_301]);
        assert_eq!(found.trails, vec![215_001]);
        assert_eq!(found.teams[&5].name, "Freeze");
        assert_eq!(found.teams[&14].avatar_guids, vec![guid, guid + 1]);
        assert_eq!(found.current_team, Some(14));
    }

    #[test]
    fn hangouts_are_read_by_chapter() {
        let pair = |tag: u32, id: u64, id_tag: u32, value_tag: u32, value: u64| {
            let mut e = v(id_tag, id);
            if value > 0 {
                e.extend(v(value_tag, value));
            }
            b(tag, &e)
        };
        // {id 1, state 2, finished 3, total 4, pictures 5, endings 6}
        let chapter = |id: u64, state: u64, seen: u64, total: u64| {
            let mut e = v(1, id);
            if state > 0 {
                e.extend(v(2, state));
            }
            if seen > 0 {
                e.extend(v(3, seen));
            }
            e.extend(v(4, total));
            for n in 1..=3 {
                e.extend(pair(5, id * 100 + n, 13, 5, u64::from(n <= 2)));
            }
            for n in 1..=total {
                e.extend(pair(6, id * 100 + n, 10, 13, if n <= seen { 2 } else { 1 }));
            }
            b(9, &e)
        };
        let data = [
            list(1, &[101_401]),
            chapter(101_401, 3, 5, 5),
            chapter(102_301, 3, 2, 6),
        ]
        .concat();
        let found = matches_coop_notify(&data).unwrap();
        let done = &found[&101_401];
        assert_eq!(done.state, Some(3));
        assert_eq!(done.endings.len(), 5);
        assert!(done.endings.values().all(|e| *e == EndingState::Claimed));
        assert_eq!(done.pictures.values().filter(|u| **u).count(), 2);
        let partial = &found[&102_301];
        let locked = partial
            .endings
            .values()
            .filter(|e| **e == EndingState::Locked);
        assert_eq!(locked.count(), 4);
        assert_eq!(partial.state, Some(3));
        // A chapter holding another chapter's entries: something else.
        let mut stray = v(1, 102_301);
        stray.extend(v(4, 2));
        stray.extend(pair(6, 10_140_101, 10, 13, 1));
        stray.extend(pair(6, 10_140_102, 10, 13, 1));
        let wrong = [chapter(101_401, 3, 1, 2), b(9, &stray)].concat();
        assert_eq!(matches_coop_notify(&wrong), None);
    }

    #[test]
    fn bosses_and_the_weekly_discounts_are_read() {
        let float = |tag: u32| {
            let mut out = varint((u64::from(tag) << 3) | 5);
            out.extend([0, 0, 0x80, 0x3f]);
            out
        };
        let spot = |monster: u64, at: u64, discount: bool| {
            let mut e = v(1, 90);
            e.extend(v(3, at));
            e.extend(v(9, 86_400));
            e.extend(b(4, &[float(1), float(2)].concat()));
            e.extend(b(7, &[v(1, 3), v(2, monster), v(5, 133_220_051)].concat()));
            if discount {
                e.extend(b(12, &[v(1, 3), v(2, 3), v(3, 30), v(4, 60)].concat()));
            }
            b(6, &e)
        };
        let data = [
            b(
                5,
                &[
                    v(1, 1),
                    v(2, 1),
                    spot(20_010_801, 1_791_000_000, false),
                    spot(20_010_801, 1_791_000_100, false),
                ]
                .concat(),
            ),
            b(
                5,
                &[v(1, 1), v(2, 40), spot(29_020_102, 1_791_200_000, true)].concat(),
            ),
            v(3, 0),
        ]
        .concat();
        let found = matches_investigation_monster_rsp(&data).unwrap();
        assert_eq!(found.respawns[&1], vec![1_791_000_000, 1_791_000_100]);
        assert_eq!(
            found.weekly,
            Some(WeeklyDiscounts {
                left: 3,
                limit: 3,
                cost: 30,
                full_cost: 60
            })
        );
        // Spots without a monster config: something else.
        let bare = [
            b(5, &[v(2, 1), b(2, &v(3, 1_791_000_000))].concat()),
            b(5, &[v(2, 2), b(2, &v(3, 1_791_000_000))].concat()),
        ]
        .concat();
        assert_eq!(matches_investigation_monster_rsp(&bare), None);
    }

    #[test]
    fn events_are_read_with_their_progress() {
        let watcher = |id: u64, progress: u64, total: u64, claimed: bool| {
            let mut e = v(1, id);
            if progress > 0 {
                e.extend(v(2, progress));
            }
            e.extend(v(3, total));
            if claimed {
                e.extend(v(4, 1));
            }
            b(7, &e)
        };
        let event = |id: u64, watchers: Vec<u8>| {
            let mut e = v(1, id);
            e.extend(v(2, id * 1_000 + 1));
            e.extend(v(3, 1_790_000_000));
            e.extend(v(4, 1_791_000_000));
            e.extend(v(5, 1_790_020_000));
            e.extend(v(6, 4));
            e.extend(watchers);
            e.extend(b(1044, &[v(1, 5), v(2, 6)].concat()));
            b(9, &e)
        };
        let data = [
            event(
                2051,
                [
                    watcher(1_523_001, 3, 10, false),
                    watcher(1_523_002, 1, 1, true),
                ]
                .concat(),
            ),
            event(6016, Vec::new()),
            v(4, 0),
        ]
        .concat();
        let found = matches_activity_info_rsp(&data).unwrap();
        let first = &found[&2051];
        assert_eq!(first.schedule, 2_051_001);
        assert_eq!((first.start, first.end), (1_790_000_000, 1_791_000_000));
        assert_eq!(
            first.watchers[&1_523_001],
            EventWatcher {
                progress: 3,
                total: 10,
                claimed: false
            }
        );
        assert!(first.watchers[&1_523_002].claimed);
        assert!(found[&6016].watchers.is_empty());
        // An entry whose numbers don't pair an id with its schedule: something else.
        let other = b(
            9,
            &[
                v(1, 2051),
                v(2, 9_999_001),
                v(3, 1_790_000_000),
                v(4, 1_791_000_000),
            ]
            .concat(),
        );
        assert_eq!(matches_activity_info_rsp(&other), None);
    }

    #[test]
    fn the_pot_comfort_is_read_by_realm() {
        let realm = |id: u64, mansion: u64, blocks: &[u64]| {
            b(2, &[v(5, mansion), list(3, blocks), v(9, id)].concat())
        };
        let data = realm(2, 14_600, &[13_470, 4_330, 4_980, 6_120]);
        let found = matches_home_comfort_notify(&data).unwrap();
        assert_eq!(
            found[&2],
            HomeComfort {
                mansion: 14_600,
                blocks: vec![13_470, 4_330, 4_980, 6_120]
            }
        );
        // Small numbers in the list, or two lone numbers that are both small:
        // other messages.
        assert_eq!(
            matches_home_comfort_notify(&realm(2, 14_600, &[1, 2, 3])),
            None
        );
        assert_eq!(matches_home_comfort_notify(&realm(2, 3, &[13_470])), None);
    }

    #[test]
    fn tcg_challenges_and_the_tavern_are_read() {
        let level = |id: u64, done: &[u64]| {
            let mut e = v(3, id);
            if !done.is_empty() {
                e.extend(list(5, done));
            }
            b(2, &e)
        };
        let boss = |id: u64, unlock: &[u64]| b(7, &[v(1, id), list(4, unlock)].concat());
        let data = [
            level(11_001, &[1, 2]),
            level(11_002, &[]),
            level(260_101, &[212]),
            boss(1, &[210_101]),
            boss(2, &[210_201, 270_501]),
            list(9, &[3, 4, 142]),
        ]
        .concat();
        let found = matches_tcg_challenges(&data).unwrap();
        assert_eq!(found.levels[&11_001], vec![1, 2]);
        assert!(found.levels[&11_002].is_empty());
        assert_eq!(found.bosses[&2], vec![210_201, 270_501]);
        let tavern = [
            b(1, &[v(2, 6), list(3, &[110_101, 110_102])].concat()),
            b(1, &[v(2, 7), list(3, &[120_101])].concat()),
            b(1, &[v(2, 39), list(3, &[160_204])].concat()),
        ]
        .concat();
        assert_eq!(matches_tcg_tavern(&tavern).unwrap()[&39], vec![160_204]);
        // The boss list alone, or levels alone, is something else.
        assert_eq!(
            matches_tcg_challenges(&[boss(1, &[210_101]), boss(2, &[210_201])].concat()),
            None
        );
        assert_eq!(
            matches_tcg_challenges(
                &[level(11_001, &[1]), level(11_002, &[]), level(11_003, &[])].concat()
            ),
            None
        );
    }

    #[test]
    fn songs_cosmetics_and_sign_ins_are_read() {
        let song = |id: u64, score: u64, claimed: bool| {
            let mut e = v(4, id);
            if score > 0 {
                e.extend(v(9, score));
            }
            if claimed {
                e.extend(v(2, 1));
            }
            b(3, &e)
        };
        let mut music = [
            v(1, 23),
            list(2, &[1, 2, 3]),
            b(5, &[v(1, 1), v(2, 4), v(3, 100)].concat()),
        ]
        .concat();
        for id in 33..60 {
            music.extend(song(id, if id % 2 == 0 { 25_175 } else { 0 }, id % 3 == 0));
        }
        let songs = matches_music_game_notify(&music).unwrap();
        assert_eq!(
            songs[&34],
            MusicSong {
                score: 25_175,
                claimed: false
            }
        );
        assert_eq!(
            songs[&33],
            MusicSong {
                score: 0,
                claimed: true
            }
        );

        let cosmetic = |tag: u32, id: u64, time: u64| b(tag, &[v(1, id), v(3, time)].concat());
        let beyond = [
            cosmetic(5, 260_087, 1_790_000_000),
            cosmetic(5, 550_912, 0),
            cosmetic(6, 270_002, 1_790_000_100),
            cosmetic(6, 275_250, 1_790_000_200),
            list(8, &[1001, 3022]),
        ]
        .concat();
        let owned = matches_beyond_cosmetic_notify(&beyond).unwrap();
        assert_eq!(owned.len(), 4);
        assert_eq!(owned[&270_002], 1_790_000_100);

        let calendar = b(
            1,
            &[
                v(3, 10_019),
                v(4, 1_791_087_107),
                v(5, 4),
                list(6, &[1, 2, 3, 4]),
                v(7, 316),
                v(8, 1_790_020_800),
                v(9, 1_792_353_599),
                b(
                    10,
                    &[v(1, 1), b(2, &[v(1, 202), v(2, 80_000)].concat())].concat(),
                ),
            ]
            .concat(),
        );
        let sign_ins = matches_sign_in_rsp(&calendar).unwrap();
        assert_eq!(sign_ins[&10_019].days, 4);
        assert_eq!(sign_ins[&10_019].claimed, vec![1, 2, 3, 4]);
        assert_eq!(sign_ins[&10_019].end, 1_792_353_599);
    }
}
