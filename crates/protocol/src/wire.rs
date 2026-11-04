// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! A protobuf wire-format reader for the shape matchers: a message's fields
//! grouped by tag, borrowed from the bytes, no copies.
//!
//! The matchers used to parse every payload into `protobuf`'s unknown-field
//! map, which copies every length-delimited value into its own `Vec` and
//! hashes every tag; two dozen matchers each doing that to every command of
//! a login made the parse itself most of the decoding time. This reads the
//! same wire format into the same [`Value`]s (`protobuf::UnknownValueRef`),
//! in the same order that map iterates -- within a tag, fixed32s first, then
//! fixed64s, varints, length-delimited values; that order decides which
//! matchers see what, so a test pins it against `protobuf` on random
//! messages -- and accepts and refuses exactly the inputs it accepted and
//! refused: a group is skipped unrecorded, a stray end group or a short
//! length-delimited value refuses the message, a varint of more than ten
//! bytes too, a field number of 0 too.

use std::collections::BTreeMap;

use protobuf::UnknownValueRef::{Fixed32, Fixed64, LengthDelimited, Varint};

/// One field value, borrowed from the message bytes.
pub(crate) type Value<'a> = protobuf::UnknownValueRef<'a>;

/// A parsed message's fields by tag.
pub(crate) type Fields<'a> = BTreeMap<u32, Vec<Value<'a>>>;

const WIRE_VARINT: u32 = 0;
const WIRE_FIXED64: u32 = 1;
const WIRE_LENGTH_DELIMITED: u32 = 2;
const WIRE_START_GROUP: u32 = 3;
const WIRE_END_GROUP: u32 = 4;
const WIRE_FIXED32: u32 = 5;

/// The most bytes a varint may take (`protobuf` reads at most ten).
const MAX_VARINT_BYTES: usize = 10;

/// Where a value's wire type comes in the order the unknown-field map
/// yields one tag's values: fixed32s, then fixed64s, varints, and
/// length-delimited values.
fn kind_rank(value: &Value<'_>) -> u8 {
    match value {
        Fixed32(_) => 0,
        Fixed64(_) => 1,
        Varint(_) => 2,
        LengthDelimited(_) => 3,
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for i in 0..MAX_VARINT_BYTES {
            let byte = *self.bytes.get(self.at)?;
            self.at += 1;
            // The tenth byte has one bit of room: more is refused, as there.
            if i == MAX_VARINT_BYTES - 1 && byte & 0x7f > 1 {
                return None;
            }
            value |= u64::from(byte & 0x7f) << (i * 7);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn fixed32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")))
    }

    fn fixed64(&mut self) -> Option<u64> {
        self.take(8)
            .map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")))
    }

    /// A length-delimited value: a `u32` length (a longer varint is truncated
    /// to one, as `protobuf` does) and that many bytes.
    fn length_delimited(&mut self) -> Option<&'a [u8]> {
        let len = self.varint()? as u32;
        self.take(len as usize)
    }

    /// A tag: the field number and wire type. `None` at a field number of 0
    /// or a wire type that does not exist.
    fn tag(&mut self) -> Option<(u32, u32)> {
        let tag = self.varint()?;
        let tag = u32::try_from(tag).ok()?;
        let field = tag >> 3;
        let wire = tag & 7;
        (field != 0 && wire <= WIRE_FIXED32).then_some((field, wire))
    }

    /// Skip a group's contents up to and including the next end group tag
    /// (any field's: `protobuf` does not check that it is this group's).
    fn skip_group(&mut self) -> Option<()> {
        loop {
            let (_, wire) = self.tag()?;
            match wire {
                WIRE_VARINT => {
                    self.varint()?;
                }
                WIRE_FIXED64 => {
                    self.fixed64()?;
                }
                WIRE_LENGTH_DELIMITED => {
                    self.length_delimited()?;
                }
                WIRE_START_GROUP => self.skip_group()?,
                WIRE_END_GROUP => return Some(()),
                WIRE_FIXED32 => {
                    self.fixed32()?;
                }
                _ => return None,
            }
        }
    }
}

/// Read `bytes` as one message, or `None` if it is not well-formed.
pub(crate) fn fields(bytes: &[u8]) -> Option<Fields<'_>> {
    let mut reader = Reader { bytes, at: 0 };
    // One list in wire order, sorted once: most messages are a handful of
    // fields, and a map of per-kind lists per tag cost more in allocation
    // than the parse itself.
    let mut found: Vec<(u32, Value<'_>)> = Vec::new();
    while !reader.done() {
        let (field, wire) = reader.tag()?;
        let value = match wire {
            WIRE_VARINT => Varint(reader.varint()?),
            WIRE_FIXED64 => Fixed64(reader.fixed64()?),
            WIRE_LENGTH_DELIMITED => LengthDelimited(reader.length_delimited()?),
            // A group is skipped unrecorded; a tag whose only fields were
            // groups is a tag the map never saw.
            WIRE_START_GROUP => {
                reader.skip_group()?;
                continue;
            }
            WIRE_END_GROUP => return None,
            WIRE_FIXED32 => Fixed32(reader.fixed32()?),
            _ => return None,
        };
        found.push((field, value));
    }
    // Stable, so values of one tag and kind keep their wire order.
    found.sort_by_key(|(tag, value)| (*tag, kind_rank(value)));
    let mut out: Fields<'_> = BTreeMap::new();
    let mut values = found.into_iter().peekable();
    while let Some((tag, value)) = values.next() {
        let mut of_tag = vec![value];
        while let Some((_, value)) = values.next_if(|(next, _)| *next == tag) {
            of_tag.push(value);
        }
        out.insert(tag, of_tag);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use protobuf::Message;

    use super::*;
    use crate::r#gen::protos::Unk;

    /// What `protobuf` reads: the same grouping and order, built the way the
    /// matchers used to.
    fn reference(bytes: &[u8]) -> Option<Vec<(u32, String)>> {
        let msg = Unk::parse_from_bytes(bytes).ok()?;
        let mut by_tag: BTreeMap<u32, Vec<String>> = BTreeMap::new();
        for (tag, value) in msg.unknown_fields().iter() {
            by_tag.entry(tag).or_default().push(describe(&value));
        }
        Some(
            by_tag
                .into_iter()
                .flat_map(|(tag, values)| values.into_iter().map(move |v| (tag, v)))
                .collect(),
        )
    }

    fn describe(value: &Value<'_>) -> String {
        match value {
            Fixed32(v) => format!("f32:{v}"),
            Fixed64(v) => format!("f64:{v}"),
            Varint(v) => format!("v:{v}"),
            LengthDelimited(b) => format!("ld:{b:02x?}"),
        }
    }

    fn ours(bytes: &[u8]) -> Option<Vec<(u32, String)>> {
        Some(
            fields(bytes)?
                .into_iter()
                .flat_map(|(tag, values)| {
                    values
                        .into_iter()
                        .map(move |v| (tag, describe(&v)))
                        .collect::<Vec<_>>()
                })
                .collect(),
        )
    }

    /// A small deterministic generator (xorshift), so the cases are the same
    /// on every run.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn varint_bytes(mut value: u64, out: &mut Vec<u8>) {
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

    /// A well-formed message with random fields, nested messages and groups,
    /// repeated tags across wire types included.
    fn random_message(rng: &mut Rng, depth: u32, out: &mut Vec<u8>) {
        let fields = rng.below(8);
        for _ in 0..fields {
            let field = 1 + rng.below(6) as u32;
            let wire = match rng.below(10) {
                0..=3 => WIRE_VARINT,
                4 => WIRE_FIXED64,
                5 => WIRE_FIXED32,
                6..=8 => WIRE_LENGTH_DELIMITED,
                _ => WIRE_START_GROUP,
            };
            if wire == WIRE_START_GROUP && depth == 0 {
                continue;
            }
            varint_bytes(u64::from(field << 3 | wire), out);
            match wire {
                WIRE_VARINT => varint_bytes(rng.next() >> rng.below(64), out),
                WIRE_FIXED64 => out.extend_from_slice(&rng.next().to_le_bytes()),
                WIRE_FIXED32 => out.extend_from_slice(&(rng.next() as u32).to_le_bytes()),
                WIRE_LENGTH_DELIMITED => {
                    let mut inner = Vec::new();
                    if depth > 0 && rng.below(2) == 0 {
                        random_message(rng, depth - 1, &mut inner);
                    } else {
                        for _ in 0..rng.below(6) {
                            inner.push(rng.next() as u8);
                        }
                    }
                    varint_bytes(inner.len() as u64, out);
                    out.extend_from_slice(&inner);
                }
                _ => {
                    random_message(rng, depth - 1, out);
                    varint_bytes(u64::from(field << 3 | WIRE_END_GROUP), out);
                }
            }
        }
    }

    #[test]
    fn reads_what_protobuf_reads_on_random_messages() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut well_formed = 0;
        for _ in 0..20_000 {
            let mut bytes = Vec::new();
            random_message(&mut rng, 2, &mut bytes);
            let expected = reference(&bytes);
            assert_eq!(ours(&bytes), expected, "message {bytes:02x?}");
            well_formed += usize::from(expected.is_some());
        }
        assert!(
            well_formed > 19_000,
            "the generator makes well-formed messages"
        );
    }

    #[test]
    fn refuses_what_protobuf_refuses_on_random_bytes() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        let mut accepted = 0;
        for _ in 0..20_000 {
            let len = rng.below(24) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            let expected = reference(&bytes);
            assert_eq!(ours(&bytes), expected, "bytes {bytes:02x?}");
            accepted += usize::from(expected.is_some());
        }
        assert!(accepted > 100, "some random bytes parse");
    }

    #[test]
    fn the_edge_cases_agree_with_protobuf() {
        let cases: &[&[u8]] = &[
            &[],
            &[0x08, 0x01],
            // Field number 0.
            &[0x00, 0x01],
            // Wire types 6 and 7.
            &[0x0e, 0x00],
            &[0x0f, 0x00],
            // A stray end group.
            &[0x0c],
            // A group with a varint inside, then a field.
            &[0x0b, 0x10, 0x05, 0x0c, 0x18, 0x07],
            // A group closed by another field's end tag.
            &[0x0b, 0x10, 0x05, 0x14],
            // A length over the end.
            &[0x12, 0x05, 0x01],
            // A ten-byte varint and an eleven-byte one.
            &[
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
            ],
            &[
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
            ],
            // Ten-byte varints whose tenth byte holds more than one bit.
            &[
                0x08, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x7f,
            ],
            &[
                0x08, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x02,
            ],
            // A tag varint that does not fit a u32.
            &[0x80, 0x80, 0x80, 0x80, 0x10, 0x01],
            // One tag with every wire type, out of order.
            &[
                0x12, 0x01, 0xaa, 0x08, 0x01, 0x0d, 0x01, 0, 0, 0, 0x09, 0x02, 0, 0, 0, 0, 0, 0, 0,
                0x08, 0x02,
            ],
            // Truncated fixed values.
            &[0x0d, 0x01, 0x02],
            &[0x09, 0x01],
        ];
        for bytes in cases {
            assert_eq!(ours(bytes), reference(bytes), "bytes {bytes:02x?}");
        }
    }
}
