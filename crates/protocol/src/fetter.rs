// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Friendship as it changes: `AvatarFetterDataNotify` (7.1 id 20382).
//!
//! The login roster carries each character's friendship (`AvatarInfo`'s
//! `AvatarFetterInfo`); after that the game sends only this notify, when a
//! domain, a commission or the Serenitea Pot gives friendship EXP. It is a
//! `map<uint64, AvatarFetterInfo>`: the avatar guid, then the same record the
//! roster holds. A map of guid to some other record has the same outer shape
//! (`AvatarExpeditionDataNotify` is one), so every value must parse as a whole
//! `AvatarFetterInfo` -- no field it doesn't have, a level the game can show,
//! the character's friendship entries -- and the caller keeps it only when
//! every guid is a character it holds.

use std::ops::RangeInclusive;

use protobuf::Message;
use protobuf::UnknownValueRef::{LengthDelimited, Varint};

use crate::r#gen::protos::{AvatarFetterInfo, Unk};

/// Guids are minted as `(uid << 32) + counter`, so a guid is at least this.
const MIN_GUID: u64 = 1 << 32;

/// Friendship levels the game shows.
const FRIENDSHIP_LEVELS: RangeInclusive<u32> = 1..=10;

/// Recover the friendship records from an `AvatarFetterDataNotify` payload, as
/// `(avatar guid, record)`, or `None` if it is not one: one repeated field and
/// nothing else, each entry a guid on 1 and on 2 a record with no field
/// `AvatarFetterInfo` lacks, a level in 1..=10 and at least one friendship
/// entry (the voice lines and stories every character has).
pub fn matches_fetter_notify(data: &[u8]) -> Option<Vec<(u64, AvatarFetterInfo)>> {
    let msg = Unk::parse_from_bytes(data).ok()?;
    let mut field = None;
    let mut found = Vec::new();
    for (number, value) in msg.unknown_fields().iter() {
        let LengthDelimited(bytes) = value else {
            return None;
        };
        if *field.get_or_insert(number) != number {
            return None;
        }
        found.push(entry(bytes)?);
    }
    (!found.is_empty()).then_some(found)
}

/// One map entry: the guid, then the record.
fn entry(bytes: &[u8]) -> Option<(u64, AvatarFetterInfo)> {
    let entry = Unk::parse_from_bytes(bytes).ok()?;
    let mut guid = None;
    let mut record = None;
    for (number, value) in entry.unknown_fields().iter() {
        match (number, value) {
            (1, Varint(value)) if guid.is_none() && value >= MIN_GUID => guid = Some(value),
            (2, LengthDelimited(bytes)) if record.is_none() => record = Some(fetter_info(bytes)?),
            _ => return None,
        }
    }
    Some((guid?, record?))
}

fn fetter_info(bytes: &[u8]) -> Option<AvatarFetterInfo> {
    let info = AvatarFetterInfo::parse_from_bytes(bytes).ok()?;
    let whole = info.special_fields.unknown_fields().iter().next().is_none()
        && info.fetter_list.iter().all(|fetter| {
            fetter
                .special_fields
                .unknown_fields()
                .iter()
                .next()
                .is_none()
                && fetter.fetter_id != 0
        });
    (whole && FRIENDSHIP_LEVELS.contains(&info.exp_level) && !info.fetter_list.is_empty())
        .then_some(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r#gen::protos::FetterData;

    fn record(level: u32, exp: u32) -> AvatarFetterInfo {
        let mut info = AvatarFetterInfo::new();
        info.exp_level = level;
        info.exp_number = exp;
        for (id, state) in [(10_011, 3), (10_012, 3), (10_013, 1)] {
            let mut fetter = FetterData::new();
            fetter.fetter_id = id;
            fetter.fetter_state = state;
            info.fetter_list.push(fetter);
        }
        info
    }

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

    fn bytes_field(tag: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = varint((u64::from(tag) << 3) | 2);
        out.extend(varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    fn map_entry(guid: u64, value: &[u8]) -> Vec<u8> {
        let mut out = varint(1 << 3);
        out.extend(varint(guid));
        out.extend(bytes_field(2, value));
        out
    }

    /// The 7.1 layout: the map on field 8.
    fn notify(entries: &[(u64, AvatarFetterInfo)]) -> Vec<u8> {
        entries
            .iter()
            .flat_map(|(guid, info)| {
                bytes_field(8, &map_entry(*guid, &info.write_to_bytes().unwrap()))
            })
            .collect()
    }

    const GUID: u64 = (812_345_678 << 32) + 5;

    #[test]
    fn a_friendship_notify_gives_each_guid_its_record() {
        let found = matches_fetter_notify(&notify(&[
            (GUID, record(9, 2_500)),
            (GUID + 1, record(10, 0)),
        ]))
        .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, GUID);
        assert_eq!((found[0].1.exp_level, found[0].1.exp_number), (9, 2_500));
        assert_eq!(found[1].1.exp_level, 10);
    }

    #[test]
    fn other_maps_of_guids_are_not_friendship() {
        // A level the game can't show.
        assert!(matches_fetter_notify(&notify(&[(GUID, record(11, 0))])).is_none());
        // No friendship entries: an expedition record reads that way.
        let mut bare = record(3, 100);
        bare.fetter_list.clear();
        assert!(matches_fetter_notify(&notify(&[(GUID, bare)])).is_none());
        // A field the record doesn't have.
        let mut extra = record(9, 2_500).write_to_bytes().unwrap();
        extra.extend(varint(9 << 3));
        extra.extend(varint(1));
        assert!(matches_fetter_notify(&bytes_field(8, &map_entry(GUID, &extra))).is_none());
        // A key too small for a guid.
        assert!(matches_fetter_notify(&notify(&[(42, record(9, 0))])).is_none());
        // A varint beside the map.
        let mut beside = notify(&[(GUID, record(9, 0))]);
        beside.extend(varint(1 << 3));
        beside.extend(varint(1));
        assert!(matches_fetter_notify(&beside).is_none());
        assert!(matches_fetter_notify(&[]).is_none());
    }
}
