// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Every parent quest the player has touched, and which are finished,
//! recognised by shape.
//!
//! The server sends the whole list at every login (`FinishedParentQuestNotify`,
//! command 3062 in 7.1, about 2,400 entries and 200 KB): one repeated message
//! field, one entry per parent quest, each with its id, a finished flag and
//! the list of its sub-quests. Like everything else this library reads, its
//! command id and field numbers are reshuffled by every game version, so they
//! are told apart by their values:
//!
//! * the parent id is the one varint field in every entry whose values are
//!   distinct ids, and whose entries' nested lists hold sub-quest ids that are
//!   that id × 100 + n -- true of every sub-quest in the game's tables, and of
//!   nothing else in the message;
//! * the finished flag is a field that only ever holds 1 (a protobuf `true`)
//!   and is set on a good part of the list. 7.1 has two such fields (the flag
//!   and the parent quest state, 1 = finished) on nearly the same entries;
//!   a quest counts as finished only when it carries all of them.
//!
//! An entry also carries text fields (two content decryption keys among
//! them). They are never read: only varints and the sub-quest lists are.
//!
//! The live update (`FinishedParentQuestUpdateNotify`) is not matched: in the
//! recorded 7.1 sessions it carried an id and a time and nothing else (no
//! sub-quests, never a finished quest), which is a dozen messages' shape. The
//! next login's list replaces what was captured.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use protobuf::Message;
use protobuf::UnknownValueRef::{LengthDelimited, Varint};

use crate::r#gen::protos::Unk;

/// Parent quest ids: 7.1 runs from the 300s to about 80,000; the contract
/// with the tracker allows up to 999,999.
pub const PARENT_QUEST_IDS: RangeInclusive<u64> = 1..=999_999;

/// The fewest entries believed to be the whole list (a new account has
/// touched a few dozen quests by the end of the prologue).
pub const MIN_PARENT_QUESTS: usize = 20;

/// The most entries believed (the tracker keeps up to 20,000 per list).
pub const MAX_PARENT_QUESTS: usize = 20_000;

/// The parent quests as the login listed them, ids ascending.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParentQuests {
    pub finished: Vec<u32>,
    /// Accepted, not finished.
    pub active: Vec<u32>,
}

/// One entry's lone varint fields, and its repeated or nested fields' raw
/// values (which may be text: never kept, only tried as sub-quest lists).
struct Entry<'a> {
    varints: BTreeMap<u32, u64>,
    nested: BTreeMap<u32, Vec<&'a [u8]>>,
}

fn read_entry(msg: &Unk) -> Entry<'_> {
    let mut varints = BTreeMap::new();
    let mut repeated = BTreeSet::new();
    let mut nested: BTreeMap<u32, Vec<&[u8]>> = BTreeMap::new();
    for (tag, value) in msg.unknown_fields().iter() {
        match value {
            Varint(v) => {
                if varints.insert(tag, v).is_some() {
                    repeated.insert(tag);
                }
            }
            LengthDelimited(bytes) => nested.entry(tag).or_default().push(bytes),
            _ => {}
        }
    }
    // A varint field given twice is a list, not a lone value.
    for tag in repeated {
        varints.remove(&tag);
    }
    Entry { varints, nested }
}

/// Whether `values` is a list of sub-quests of `parent`: every value a
/// message whose varints include one that is `parent` × 100 + n.
fn sub_quests_of(values: &[&[u8]], parent: u64) -> bool {
    !values.is_empty()
        && values.iter().all(|bytes| {
            Unk::parse_from_bytes(bytes).is_ok_and(|msg| {
                msg.unknown_fields()
                    .iter()
                    .any(|(_, value)| matches!(value, Varint(v) if v >= 100 && v / 100 == parent))
            })
        })
}

/// Recover the parent quest list from a `FinishedParentQuestNotify` payload,
/// or `None` if it is not one: exactly one field, repeated
/// [`MIN_PARENT_QUESTS`]-[`MAX_PARENT_QUESTS`] times, each value a message
/// with exactly one id field (see the module docs) under which at least half
/// of the entries hold their sub-quests, and at least one finished flag.
pub fn matches_finished_parent_quest_notify(data: &[u8]) -> Option<ParentQuests> {
    let msg = Unk::parse_from_bytes(data).ok()?;
    let mut list_tag = None;
    let mut raw = Vec::new();
    for (tag, value) in msg.unknown_fields().iter() {
        let LengthDelimited(bytes) = value else {
            return None;
        };
        if *list_tag.get_or_insert(tag) != tag {
            return None;
        }
        raw.push(bytes);
    }
    if !(MIN_PARENT_QUESTS..=MAX_PARENT_QUESTS).contains(&raw.len()) {
        return None;
    }
    let parsed = raw
        .iter()
        .map(|bytes| Unk::parse_from_bytes(bytes).ok())
        .collect::<Option<Vec<_>>>()?;
    let entries: Vec<Entry> = parsed.iter().map(read_entry).collect();

    // Id candidates: in every entry, in range, all distinct.
    let first = entries.first()?;
    let candidates: Vec<u32> = first
        .varints
        .keys()
        .copied()
        .filter(|tag| {
            let mut seen = BTreeSet::new();
            entries.iter().all(|e| {
                e.varints
                    .get(tag)
                    .is_some_and(|v| PARENT_QUEST_IDS.contains(v) && seen.insert(*v))
            })
        })
        .collect();
    let related = |tag: u32| {
        entries
            .iter()
            .filter(|e| {
                let parent = e.varints[&tag];
                e.nested
                    .values()
                    .any(|values| sub_quests_of(values, parent))
            })
            .count()
    };
    let id_tags: Vec<u32> = candidates
        .into_iter()
        .filter(|tag| related(*tag) * 2 >= entries.len())
        .collect();
    let [id_tag] = id_tags.as_slice() else {
        return None;
    };

    // Finished flags: only ever 1, on at least a quarter of the entries.
    let mut ones: BTreeMap<u32, usize> = BTreeMap::new();
    let mut other: BTreeSet<u32> = BTreeSet::new();
    for e in &entries {
        for (tag, value) in &e.varints {
            if tag == id_tag {
                continue;
            }
            if *value == 1 {
                *ones.entry(*tag).or_default() += 1;
            } else {
                other.insert(*tag);
            }
        }
    }
    let flags: Vec<u32> = ones
        .into_iter()
        .filter(|(tag, count)| !other.contains(tag) && count * 4 >= entries.len())
        .map(|(tag, _)| tag)
        .collect();
    if flags.is_empty() {
        return None;
    }

    let mut out = ParentQuests::default();
    for e in &entries {
        let id = e.varints[id_tag] as u32;
        if flags.iter().all(|flag| e.varints.contains_key(flag)) {
            out.finished.push(id);
        } else {
            out.active.push(id);
        }
    }
    out.finished.sort_unstable();
    out.active.sort_unstable();
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

    /// One entry in 7.1's layout: id 14, finished 4, state 5 (1 = finished),
    /// accept time 15, sub-quests 13 (`{quest_id 5, state 3}`), a key 22.
    fn quest(id: u64, finished: bool, children: u64) -> Vec<u8> {
        let mut out = v(14, id);
        if finished {
            out.extend(v(4, 1));
            out.extend(v(5, 1));
        }
        out.extend(v(15, 1_790_000_000 + id));
        for n in 1..=children {
            out.extend(b(13, &[v(5, id * 100 + n), v(3, 3)].concat()));
        }
        out.extend(b(22, b"not-a-real-key"));
        out
    }

    fn notify(entries: &[Vec<u8>]) -> Vec<u8> {
        entries.iter().flat_map(|entry| b(4, entry)).collect()
    }

    fn sample() -> Vec<Vec<u8>> {
        (0..30)
            .map(|n| quest(1_000 + n * 7, n % 5 != 0, if n % 6 == 0 { 0 } else { 3 }))
            .collect()
    }

    #[test]
    fn the_list_says_which_parent_quests_are_finished() {
        let found = matches_finished_parent_quest_notify(&notify(&sample())).unwrap();
        assert_eq!(found.finished.len() + found.active.len(), 30);
        assert_eq!(found.active, vec![1_000, 1_035, 1_070, 1_105, 1_140, 1_175]);
        assert!(found.finished.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_state_alone_is_not_finished() {
        // 7.1 had two entries with the state but not the flag.
        let mut entries = sample();
        entries.push([v(14, 9_001), v(5, 1), v(15, 1_790_000_000)].concat());
        let found = matches_finished_parent_quest_notify(&notify(&entries)).unwrap();
        assert!(found.active.contains(&9_001));
    }

    #[test]
    fn other_lists_are_not_quests() {
        // Too short.
        assert_eq!(
            matches_finished_parent_quest_notify(&notify(&sample()[..10])),
            None
        );
        // Sub-quests that are not the parent's.
        let strangers: Vec<Vec<u8>> = (0..30)
            .map(|n| [v(14, 1_000 + n), v(4, 1), b(13, &v(5, 555_501))].concat())
            .collect();
        assert_eq!(
            matches_finished_parent_quest_notify(&notify(&strangers)),
            None
        );
        // A second top-level field.
        let mut two = notify(&sample());
        two.extend(v(9, 1));
        assert_eq!(matches_finished_parent_quest_notify(&two), None);
        // No finished flag anywhere.
        let none: Vec<Vec<u8>> = (0..30).map(|n| quest(1_000 + n, false, 2)).collect();
        assert_eq!(matches_finished_parent_quest_notify(&notify(&none)), None);
    }
}
