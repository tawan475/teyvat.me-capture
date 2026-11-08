// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The session hand-over: a running game connection carried from one run of
//! the app to the next.
//!
//! A clean exit (the update's relaunch, the window closed, Quit) writes one
//! while the session key is live; the next start, within
//! [`SessionHandover::MAX_AGE_MS`], reads and deletes it and picks the
//! connection up where the previous process left it: the key and conversation
//! ([`teyvatme_protocol::SnifferHandover`]), the account as held
//! ([`crate::player_data::PlayerHandover`]) and live sync's journal
//! ([`crate::live::LiveHandover`]). Without it a restart mid-session meant a
//! new login: the key is per connection, and the held data is what the
//! connection's later changes apply to.
//!
//! The file is JSON (`v` first), the account's own data: it stays in the
//! app's data directory and is deleted once read. A crash writes none.

use serde::{Deserialize, Serialize};
use teyvatme_protocol::SnifferHandover;

/// A map with keys JSON cannot carry (tuples), written as a list of pairs:
/// `#[serde(with = "crate::handover::pairs")]`.
pub mod pairs {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<K: Serialize, V: Serialize, S: Serializer>(
        map: &BTreeMap<K, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(map.iter())
    }

    pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(Vec::<(K, V)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}

/// [`pairs`] for an optional map.
pub mod option_pairs {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<K: Serialize, V: Serialize, S: Serializer>(
        map: &Option<BTreeMap<K, V>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match map {
            Some(map) => serializer.collect_seq(map.iter()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, K, V, D>(deserializer: D) -> Result<Option<BTreeMap<K, V>>, D::Error>
    where
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Ok(Option::<Vec<(K, V)>>::deserialize(deserializer)?
            .map(|pairs| pairs.into_iter().collect()))
    }
}

use crate::good::GiResin;
use crate::live::LiveHandover;
use crate::player_data::PlayerHandover;
use crate::wish::hub::WishHandover;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionHandover {
    /// Layout version ([`Self::VERSION`]); another version is not read.
    pub v: u32,
    /// When it was written (unix ms), for [`Self::is_fresh`].
    pub written_ms: u64,
    /// The game client's version the data was captured under, when known:
    /// a hand-over from another game build is not read.
    pub game_data: Option<String>,
    pub sniffer: SnifferHandover,
    pub player: PlayerHandover,
    pub live: Option<LiveHandover>,
    /// The wish link's parts captured this login (since 1.17.26).
    #[serde(default)]
    pub wish: Option<WishHandover>,
    /// The resin alert already sent on the current resin clock, so it is not
    /// sent again (since 1.17.26).
    #[serde(default)]
    pub resin_alerted: Option<(GiResin, u32)>,
}

impl SessionHandover {
    pub const VERSION: u32 = 1;

    /// How old a hand-over may be and still be picked up: the restart takes
    /// seconds; past this the connection has moved on too far for its KCP
    /// window, or ended.
    pub const MAX_AGE_MS: u64 = 120_000;

    /// The file's name in the app's data directory.
    pub const FILE_NAME: &'static str = "session-handover.json";

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Read one back; `None` for another layout version or unreadable bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let handover: Self = serde_json::from_slice(bytes).ok()?;
        (handover.v == Self::VERSION).then_some(handover)
    }

    /// Written within [`Self::MAX_AGE_MS`] of `now_ms` (and not in the future
    /// by more than a clock step).
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.written_ms) <= Self::MAX_AGE_MS
            && self.written_ms.saturating_sub(now_ms) <= 60_000
    }
}
