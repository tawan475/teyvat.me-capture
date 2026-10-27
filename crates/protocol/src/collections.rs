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

fn tcg_cards(list: &[Entry]) -> Option<BTreeMap<u32, TcgCard>> {
    if list.len() < 2 || list.len() > MAX_TCG_CARDS {
        return None;
    }
    let id_tag = only_tag(list, &[], |values| {
        all_distinct(values) && values.iter().all(|v| TCG_CARD_IDS.contains(v))
    })?;
    let count_tag = only_tag(list, &[id_tag], |values| {
        values.iter().all(|v| (1..=MAX_TCG_COPIES).contains(v))
    })?;
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
}
