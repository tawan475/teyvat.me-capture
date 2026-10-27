// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Which command ids each matcher claims, over every server message of one or
//! more decoded traces (unions unwrapped): the check every by-shape matcher
//! must pass, that it claims its own message and nothing else.
//!
//! ```text
//! cargo run --release -p teyvatme-protocol --example audit_matchers -- <trace.bin>...
//! ```
//!
//! A trace is the research build's `ONET_TRACE_DECODED` file. Record, little
//! endian: `dir u8, conv u32, cmd u16, field8 u32, header len u32 + bytes,
//! data len u32 + bytes` (dir 1 = from the server). Traces are personal data:
//! read them where they are, never commit them. Only command ids and counts
//! are printed.

use std::collections::BTreeMap;

use teyvatme_protocol::{CommandMatch, GameCommand, classify_command, classify_union};

fn read_u32(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

/// Every server command of one trace file.
fn server_commands(buf: &[u8]) -> Vec<GameCommand> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 15 <= buf.len() {
        let dir = buf[i];
        let cmd = u16::from_le_bytes([buf[i + 5], buf[i + 6]]);
        let Some(header_len) = read_u32(buf, i + 11).map(|n| n as usize) else {
            break;
        };
        let head_at = i + 15;
        let Some(data_len) = read_u32(buf, head_at + header_len).map(|n| n as usize) else {
            break;
        };
        let data_at = head_at + header_len + 4;
        let Some(data) = buf.get(data_at..data_at + data_len) else {
            break;
        };
        if dir == 1 {
            out.push(GameCommand {
                command_id: cmd,
                header_len: 0,
                data_len: data_len as u32,
                proto_header: Vec::new(),
                proto_data: data.to_vec(),
                sent: false,
            });
        }
        i = data_at + data_len;
    }
    out
}

fn main() {
    // kind -> command id -> (top-level claims, claims inside a union)
    let mut claims: BTreeMap<&'static str, BTreeMap<u16, (usize, usize)>> = BTreeMap::new();
    let mut messages = 0usize;
    for path in std::env::args().skip(1) {
        let buf = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let commands = server_commands(&buf);
        eprintln!("{path}: {} server commands", commands.len());
        for command in &commands {
            messages += 1;
            if let Some(found) = classify_command(command)
                && !matches!(found, CommandMatch::Flat(_))
            {
                claims
                    .entry(found.kind())
                    .or_default()
                    .entry(command.command_id)
                    .or_default()
                    .0 += 1;
            }
            for (inner_id, found) in classify_union(command) {
                messages += 1;
                if !matches!(found, CommandMatch::Flat(_)) {
                    claims
                        .entry(found.kind())
                        .or_default()
                        .entry(inner_id)
                        .or_default()
                        .1 += 1;
                }
            }
        }
    }
    println!("{messages} messages");
    for (kind, ids) in claims {
        let list: Vec<String> = ids
            .iter()
            .map(|(id, (top, wrapped))| format!("{id} x{top}+{wrapped}u"))
            .collect();
        println!("{kind:>16}: {}", list.join(", "));
    }
}
