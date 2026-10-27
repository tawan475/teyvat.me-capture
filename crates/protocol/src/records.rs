// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Messages made of small records, read without knowing what they are.
//!
//! Many account messages are lists of small numbers: one repeated list of
//! records (`{city, level, oculi}`, `{offering, level, rewards taken}`, ...)
//! and/or a few packed lists of ids, beside a scalar or two -- the Serenitea
//! Pot's unlocked music (the Radiant Spincrystals) is one packed list alone.
//! That shape is common, so nothing here decides what a message is: a
//! [`RecordsMessage`] is a candidate, which the caller believes only when its
//! ids are the game's own (the game data's tables) and its other values are in
//! range.
//!
//! Like every message this library reads, command ids and field numbers are
//! reshuffled by each game version, so fields are kept by number here only for
//! the caller to tell them apart by their values.

use std::collections::BTreeMap;

use protobuf::Message;
use protobuf::UnknownValueRef::{Fixed32, Fixed64, LengthDelimited, Varint};

use crate::r#gen::protos::Unk;

/// The most records believed in one list (the map point list is a packed list,
/// not records; the longest record lists in 7.1 hold a few hundred).
const MAX_RECORDS: usize = 5_000;

/// The most numbers believed in one packed list (Teyvat's map points: ~900 of
/// ~1,700 in 7.1).
const MAX_LIST: usize = 50_000;

/// The most field numbers one record may have (a map's entries share one, so
/// they count once: 7.1's offering 6 is 11 fields on the wire).
const MAX_RECORD_FIELDS: usize = 16;

/// Below this, a message is not worth the caller's look: fewer records and no
/// list this long.
const MIN_RECORDS: usize = 2;
const MIN_LIST: usize = 2;

/// One record: its lone numbers and its packed lists, by field number.
/// Submessages inside it are left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub varints: BTreeMap<u32, u64>,
    pub lists: BTreeMap<u32, Vec<u64>>,
}

/// A message of records: at most one repeated list of them, with the lone
/// numbers and packed lists beside it, by field number.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordsMessage {
    /// The repeated field's records (empty when the message has none).
    pub records: Vec<Record>,
    pub varints: BTreeMap<u32, u64>,
    pub lists: BTreeMap<u32, Vec<u64>>,
}

fn varints_of(bytes: &[u8]) -> Option<Vec<u64>> {
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

/// A submessage as a record, or `None` when it is not one (it does not parse,
/// repeats a lone number, has more than [`MAX_RECORD_FIELDS`] field numbers, or holds
/// fixed-width numbers).
fn read_record(bytes: &[u8]) -> Option<Record> {
    let msg = Unk::parse_from_bytes(bytes).ok()?;
    let mut record = Record::default();
    let mut tags = std::collections::BTreeSet::new();
    for (tag, value) in msg.unknown_fields().iter() {
        tags.insert(tag);
        if tags.len() > MAX_RECORD_FIELDS {
            return None;
        }
        match value {
            Varint(v) => {
                if record.varints.insert(tag, v).is_some() {
                    return None;
                }
            }
            LengthDelimited(inner) => {
                // A packed list of numbers, or a submessage (an oneof, a
                // scrambled map): kept only as the former.
                if let Some(list) = varints_of(inner).filter(|_| read_record(inner).is_none()) {
                    record.lists.insert(tag, list);
                }
            }
            Fixed32(_) | Fixed64(_) => return None,
        }
    }
    Some(record)
}

/// Read a message as records, or `None` if it is not one: at most one field
/// repeats (the records; each must read as a [`Record`]), every other field is
/// a lone number or a packed list of numbers, nothing is fixed-width, and
/// there are at least `MIN_RECORDS` (2) records or one list of `MIN_LIST` (2)
/// numbers.
///
/// A packed list and a one-record list look alike on the wire; a lone
/// length-delimited field is a list when it reads as numbers but not as a
/// record.
pub fn matches_records_message(data: &[u8]) -> Option<RecordsMessage> {
    let msg = Unk::parse_from_bytes(data).ok()?;
    let mut by_tag: BTreeMap<u32, Vec<&[u8]>> = BTreeMap::new();
    let mut out = RecordsMessage::default();
    for (tag, value) in msg.unknown_fields().iter() {
        match value {
            Varint(v) => {
                if out.varints.insert(tag, v).is_some() {
                    return None;
                }
            }
            LengthDelimited(bytes) => by_tag.entry(tag).or_default().push(bytes),
            Fixed32(_) | Fixed64(_) => return None,
        }
    }
    let mut records_tag = None;
    for (tag, values) in by_tag {
        if values.len() > 1 {
            if records_tag.replace(tag).is_some() || values.len() > MAX_RECORDS {
                return None;
            }
            out.records = values
                .iter()
                .map(|bytes| read_record(bytes))
                .collect::<Option<_>>()?;
        } else {
            let bytes = values[0];
            // A single record (or a list that also reads as one) is left
            // out: no use to the callers, which want many.
            if let (Some(list), None) = (varints_of(bytes), read_record(bytes)) {
                if list.len() > MAX_LIST {
                    return None;
                }
                out.lists.insert(tag, list);
            }
        }
    }
    let worth =
        out.records.len() >= MIN_RECORDS || out.lists.values().any(|list| list.len() >= MIN_LIST);
    worth.then_some(out)
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

    fn packed(tag: u32, values: &[u64]) -> Vec<u8> {
        b(
            tag,
            &values.iter().flat_map(|v| varint(*v)).collect::<Vec<u8>>(),
        )
    }

    #[test]
    fn a_list_of_records_beside_a_scalar_is_read() {
        // GetSceneAreaRsp-like: {crystal_num 3, level 5, city_id 7} records and a scene id.
        let mut data = v(11, 3);
        for (city, level, oculi) in [(1, 10, 0), (2, 10, 0), (99, 4, 2)] {
            let mut entry = v(5, level);
            entry.extend(v(7, city));
            if oculi > 0 {
                entry.extend(v(3, oculi));
            }
            data.extend(b(8, &entry));
        }
        let found = matches_records_message(&data).unwrap();
        assert_eq!(found.varints, BTreeMap::from([(11, 3)]));
        assert_eq!(found.records.len(), 3);
        assert_eq!(
            found.records[2].varints,
            BTreeMap::from([(3, 2), (5, 4), (7, 99)])
        );
    }

    #[test]
    fn packed_lists_are_read_as_lists() {
        // GetAreaExplorePointRsp-like: two parallel packed lists.
        let data = [
            packed(4, &[1, 2, 6, 300]),
            packed(10, &[1000, 840, 1500, 20]),
        ]
        .concat();
        let found = matches_records_message(&data).unwrap();
        assert!(found.records.is_empty());
        assert_eq!(found.lists[&4], vec![1, 2, 6, 300]);
        assert_eq!(found.lists[&10], vec![1000, 840, 1500, 20]);
    }

    #[test]
    fn a_record_keeps_its_packed_list_and_drops_its_submessages() {
        // PlayerOfferingData-like: id, level, rewards taken, and a nested oneof.
        let entry = |id: u64, level: u64| {
            [
                v(1, id),
                v(10, level),
                packed(7, &[1, 2, 3]),
                b(1452, &v(1, 9)),
            ]
            .concat()
        };
        let data = [b(8, &entry(1, 10)), b(8, &entry(2, 8))].concat();
        let found = matches_records_message(&data).unwrap();
        assert_eq!(found.records[0].varints, BTreeMap::from([(1, 1), (10, 10)]));
        assert_eq!(found.records[0].lists, BTreeMap::from([(7, vec![1, 2, 3])]));
    }

    #[test]
    fn other_shapes_are_not_records() {
        // Two repeated fields, a fixed-width field, a lone number, a big list.
        let two = [
            b(8, &v(1, 1)),
            b(8, &v(1, 2)),
            b(9, &v(1, 1)),
            b(9, &v(1, 2)),
        ]
        .concat();
        assert_eq!(matches_records_message(&two), None);
        let fixed = [b(8, &v(1, 1)), b(8, &v(1, 2)), vec![0x0d, 0, 0, 0, 0]].concat();
        assert_eq!(matches_records_message(&fixed), None);
        assert_eq!(matches_records_message(&v(1, 5)), None);
        let huge: Vec<u64> = (0..=MAX_LIST as u64).collect();
        assert_eq!(matches_records_message(&packed(4, &huge)), None);
    }
}
