// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What the decoder finds in a "Log raw packets" file (Settings › Diagnostics):
//! the command ids each kind of data was read from, and the count of messages
//! nothing claimed. For user reports; only ids and counts are printed.
//!
//! ```text
//! cargo run --release -p teyvatme-core --example packet_log -- <packet_log/*.bin>
//! ```
//!
//! A record, little endian: `cmd u16, header len u32, data len u32, header,
//! data` (monitor.rs `encode_packet_log_record`). The file has no direction:
//! the client's requests are in it too, read here as if the server sent them.

use std::collections::BTreeMap;

use teyvatme_core::decode::classify_commands;
use teyvatme_protocol::GameCommand;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: packet_log <file.bin>");
    let buf = std::fs::read(&path).expect("read the packet log");
    let mut commands = Vec::new();
    let mut i = 0;
    while i + 10 <= buf.len() {
        let cmd = u16::from_le_bytes([buf[i], buf[i + 1]]);
        let header_len = u32::from_le_bytes(buf[i + 2..i + 6].try_into().unwrap()) as usize;
        let data_len = u32::from_le_bytes(buf[i + 6..i + 10].try_into().unwrap()) as usize;
        let head = i + 10;
        let Some(data) = buf.get(head + header_len..head + header_len + data_len) else {
            break;
        };
        commands.push(GameCommand {
            command_id: cmd,
            header_len: header_len as u16,
            data_len: data_len as u32,
            proto_header: buf[head..head + header_len].to_vec(),
            proto_data: data.to_vec(),
            sent: false,
        });
        i = head + header_len + data_len;
    }
    println!(
        "{} messages, {} bytes read of {}",
        commands.len(),
        i,
        buf.len()
    );
    let mut found: BTreeMap<String, BTreeMap<u16, usize>> = BTreeMap::new();
    let mut unclaimed = 0;
    for command in &commands {
        let claims = classify_commands(std::slice::from_ref(command));
        if claims.is_empty() {
            unclaimed += 1;
        }
        for (id, claim) in claims {
            let name = format!("{claim:?}");
            let name = name.split(['(', ' ', '{']).next().unwrap_or("").to_string();
            *found.entry(name).or_default().entry(id).or_default() += 1;
        }
    }
    for (name, ids) in &found {
        println!("{name}: {ids:?}");
    }
    println!("unclaimed: {unclaimed}");
    // DUMP=<cmd>: the wire shape of that command (tags, types, counts).
    if let Some(cmd) = std::env::var("DUMP")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
    {
        for command in commands.iter().filter(|c| c.command_id == cmd) {
            println!("== {cmd}: {} bytes", command.proto_data.len());
            shape(&command.proto_data, 1);
        }
    }
}

fn varint(buf: &[u8], i: &mut usize) -> Option<u64> {
    let mut out = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*i)?;
        *i += 1;
        out |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(out);
        }
    }
    None
}

/// One field: tag, wire type, payload (length-delimited) and value (varint).
type Field<'a> = (u64, u8, &'a [u8], u64);

/// Every field, or None when `buf` isn't a message.
fn fields(buf: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        let key = varint(buf, &mut i)?;
        let (tag, wire) = (key >> 3, (key & 7) as u8);
        if tag == 0 {
            return None;
        }
        match wire {
            0 => out.push((tag, 0, &buf[0..0], varint(buf, &mut i)?)),
            1 => {
                i += 8;
                out.push((tag, 1, &buf[0..0], 0));
            }
            2 => {
                let n = varint(buf, &mut i)? as usize;
                let body = buf.get(i..i + n)?;
                i += n;
                out.push((tag, 2, body, 0));
            }
            5 => {
                i += 4;
                out.push((tag, 5, &buf[0..0], 0));
            }
            _ => return None,
        }
    }
    (i == buf.len()).then_some(out)
}

fn shape(buf: &[u8], depth: usize) {
    let Some(fields) = fields(buf) else {
        return;
    };
    let mut by_tag: BTreeMap<(u64, u8), Vec<&[u8]>> = BTreeMap::new();
    for (tag, wire, body, _) in &fields {
        by_tag.entry((*tag, *wire)).or_default().push(body);
    }
    let pad = "  ".repeat(depth);
    for ((tag, wire), bodies) in &by_tag {
        let kind = match wire {
            0 => "varint".to_string(),
            2 => {
                let nested = bodies
                    .iter()
                    .filter(|b| !b.is_empty() && self::fields(b).is_some())
                    .count();
                format!(
                    "bytes (avg {} B, {} parse as messages)",
                    bodies.iter().map(|b| b.len()).sum::<usize>() / bodies.len().max(1),
                    nested
                )
            }
            1 => "fixed64".to_string(),
            _ => "fixed32".to_string(),
        };
        println!("{pad}tag {tag}: {kind} x{}", bodies.len());
        if *wire == 2 && bodies.len() > 1 && depth == 1 {
            // Per inner varint tag: min..max over the repeated entries.
            let mut ranges: BTreeMap<u64, (u64, u64, usize)> = BTreeMap::new();
            for body in bodies {
                for (t, w, _, v) in self::fields(body).unwrap_or_default() {
                    if w == 0 {
                        let r = ranges.entry(t).or_insert((u64::MAX, 0, 0));
                        r.0 = r.0.min(v);
                        r.1 = r.1.max(v);
                        r.2 += 1;
                    }
                }
            }
            for (t, (lo, hi, n)) in ranges {
                println!("{pad}  · inner tag {t}: {lo}..{hi} on {n}");
            }
        }
        if *wire == 2
            && depth < 3
            && let Some(first) = bodies
                .iter()
                .find(|b| !b.is_empty() && self::fields(b).is_some())
        {
            shape(first, depth + 1);
        }
    }
}
