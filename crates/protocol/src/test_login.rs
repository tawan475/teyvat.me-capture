// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! A game login on the wire, for tests here and in Teyvat.me's (with the
//! `test-support` feature): the handshake, the token exchange under a made-up
//! dispatch key with the seed RSA-encrypted to the embedded client key, and
//! server messages under the session key. A [`GameSniffer`] given
//! [`initial_keys`] recovers that session key the way it recovers a real one.
//!
//! Every value is made up: no real key, seed, UID or capture is in here.

use std::collections::HashMap;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;

use crate::GameSniffer;
use crate::crypto::{decrypt_command, new_key_from_seed};
use crate::cs_rand::Random;

/// The token request's command id in these fixtures.
pub const TOKEN_REQ: u16 = 100;
/// The token response's command id in these fixtures.
pub const TOKEN_RSP: u16 = 101;
/// A server message under the session key.
pub const SESSION_MESSAGE: u16 = 102;

/// A realistic login time, in the epoch milliseconds `PacketHead` carries.
pub const LOGIN_MS: u64 = 1_759_553_801_000;

fn dispatch_key() -> Vec<u8> {
    new_key_from_seed(0xD15_0A7C)
}

/// The dispatch keys a sniffer needs for these logins
/// ([`GameSniffer::set_initial_keys`]): the version a key answers to is its
/// own first two bytes, since the plaintext starts with the magic.
pub fn initial_keys() -> HashMap<u16, Vec<u8>> {
    let key = dispatch_key();
    HashMap::from([(u16::from_be_bytes([key[0], key[1]]), key)])
}

/// The `draw`-th client seed the game's `System.Random` produces from
/// `time_seed` (0 is the first login of a game process, 1 its next login).
pub fn client_draw(time_seed: u64, draw: usize) -> u64 {
    let mut generator = Random::seeded(time_seed as i32);
    let mut value = 0;
    for _ in 0..=draw {
        value = generator.next_safe_uint64();
    }
    value
}

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

fn field_varint(tag: u32, value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    varint(u64::from(tag) << 3, &mut out);
    varint(value, &mut out);
    out
}

fn field_bytes(tag: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    varint((u64::from(tag) << 3) | 2, &mut out);
    varint(payload.len() as u64, &mut out);
    out.extend_from_slice(payload);
    out
}

/// A plaintext `GameCommand` on the wire.
fn command_bytes(command_id: u16, header: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x45, 0x67];
    out.extend(command_id.to_be_bytes());
    out.extend((header.len() as u16).to_be_bytes());
    out.extend((payload.len() as u32).to_be_bytes());
    out.extend_from_slice(header);
    out.extend_from_slice(payload);
    out.extend([0x89, 0xAB]);
    out
}

fn udp_frame(src_port: u16, dest_port: u16, payload: &[u8]) -> Vec<u8> {
    let builder = etherparse::PacketBuilder::ethernet2([1, 2, 3, 4, 5, 6], [7, 8, 9, 10, 11, 12])
        .ipv4([10, 0, 0, 2], [10, 0, 0, 1], 64)
        .udp(src_port, dest_port);
    let mut out = Vec::with_capacity(builder.size(payload.len()));
    builder
        .write(&mut out, payload)
        .expect("a fixture frame writes");
    out
}

/// A handshake request datagram from the client.
pub fn handshake_frame() -> Vec<u8> {
    let mut payload = vec![0u8; 20];
    payload[..4].copy_from_slice(&0xFFu32.to_be_bytes());
    udp_frame(50000, 22102, &payload)
}

/// PKCS#1 v1.5 padding wants random bytes, not secret ones; a fixed stream
/// keeps the fixture deterministic.
struct FixedRng(u64);

impl rsa::rand_core::RngCore for FixedRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for byte in dest {
            *byte = self.next_u64() as u8;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl rsa::rand_core::CryptoRng for FixedRng {}

/// One game connection on the wire, numbering its own segments.
pub struct TestLogin {
    conv: u32,
    sent_sn: u32,
    recv_sn: u32,
    session_key: Option<Vec<u8>>,
}

impl TestLogin {
    /// A connection on KCP conversation `conv` (each login its own).
    pub fn new(conv: u32) -> Self {
        Self {
            conv,
            sent_sn: 0,
            recv_sn: 0,
            session_key: None,
        }
    }

    /// The next push from the server (`received`) or the client, carrying
    /// `plain` encrypted under `key`.
    fn push(&mut self, received: bool, key: &[u8], plain: &[u8]) -> Vec<u8> {
        let mut data = plain.to_vec();
        decrypt_command(key, &mut data);
        let (sn, una) = if received {
            self.recv_sn += 1;
            (self.recv_sn - 1, self.sent_sn)
        } else {
            self.sent_sn += 1;
            (self.sent_sn - 1, self.recv_sn)
        };
        let mut segment = Vec::new();
        segment.extend_from_slice(&self.conv.to_le_bytes());
        segment.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        segment.push(81); // cmd: push
        segment.push(0); // frg
        segment.extend_from_slice(&128u16.to_le_bytes()); // wnd
        segment.extend_from_slice(&0u32.to_le_bytes()); // ts
        segment.extend_from_slice(&sn.to_le_bytes());
        segment.extend_from_slice(&una.to_le_bytes());
        segment.extend_from_slice(&(data.len() as u32).to_le_bytes());
        segment.extend_from_slice(&0xFEED_FACEu32.to_le_bytes());
        segment.extend_from_slice(&data);
        if received {
            udp_frame(22102, 50000, &segment)
        } else {
            udp_frame(50000, 22102, &segment)
        }
    }

    /// The whole login, as frames: the handshake, the token request and the
    /// token response (stamped `sent_ms`, carrying `client_seed ^ combined`,
    /// stating `uid` when given) under the dispatch key, then one server
    /// message under the session key `combined` derives. A sniffer that
    /// knows [`initial_keys`] and can find `client_seed` from `sent_ms` (the
    /// first draw of a login's time, or the next draw of an earlier login's)
    /// ends it holding that session key.
    pub fn log_in(
        &mut self,
        sent_ms: u64,
        client_seed: u64,
        combined: u64,
        uid: Option<u32>,
    ) -> Vec<Vec<u8>> {
        let dispatch = dispatch_key();
        let seed = client_seed ^ combined;
        let public = GameSniffer::new().rsa_keys[0].to_public_key();
        let encrypted = public
            .encrypt(
                &mut FixedRng(seed),
                rsa::Pkcs1v15Encrypt,
                &seed.to_be_bytes(),
            )
            .expect("an 8-byte seed fits the key");
        let mut payload = field_bytes(11, BASE64_STANDARD.encode(encrypted).as_bytes());
        if let Some(uid) = uid {
            payload.extend(field_varint(14, u64::from(uid)));
        }
        let request = command_bytes(TOKEN_REQ, &[], &field_varint(1, 1));
        let response = command_bytes(TOKEN_RSP, &field_varint(6, sent_ms), &payload);
        let mut frames = vec![
            handshake_frame(),
            self.push(false, &dispatch, &request),
            self.push(true, &dispatch, &response),
        ];
        self.session_key = Some(new_key_from_seed(combined));
        frames.push(self.server_message(SESSION_MESSAGE));
        frames
    }

    /// A server message with its own command id under this login's session
    /// key (after [`Self::log_in`]).
    pub fn server_message(&mut self, command_id: u16) -> Vec<u8> {
        let key = self
            .session_key
            .clone()
            .expect("a server message needs the login's session key");
        let plain = command_bytes(command_id, &[], &field_varint(1, u64::from(command_id)));
        self.push(true, &key, &plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GamePacket, KeyState};

    #[test]
    fn a_sniffer_follows_the_fixture_login_and_the_next_one() {
        let mut sniffer = GameSniffer::new().set_initial_keys(initial_keys());
        let mut first = TestLogin::new(122_486);
        for frame in first.log_in(
            LOGIN_MS,
            client_draw(LOGIN_MS, 0),
            0x1111,
            Some(812_345_678),
        ) {
            sniffer.receive_packet(frame);
        }
        assert_eq!(sniffer.key_state(), KeyState::Session);
        assert_eq!(sniffer.login_uid(), Some(812_345_678));
        assert!(matches!(
            sniffer.receive_packet(first.server_message(SESSION_MESSAGE + 1)),
            Some(GamePacket::Commands(commands)) if commands.len() == 1
        ));

        let mut second = TestLogin::new(122_628);
        for frame in second.log_in(
            LOGIN_MS + 3_600_000,
            client_draw(LOGIN_MS, 1),
            0x2222,
            Some(812_345_679),
        ) {
            sniffer.receive_packet(frame);
        }
        assert_eq!(sniffer.session_generation(), 2);
        assert_eq!(sniffer.key_state(), KeyState::Session);
        assert_eq!(sniffer.login_uid(), Some(812_345_679));
        assert!(!sniffer.reset_armed());
    }
}
