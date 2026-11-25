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
    /// The held account data was waiting to be replaced by a new login's
    /// ([`crate::decode::DataReplacement::is_pending`]): its sniffer part is
    /// another login's, so the player part is not resumed. Writers refuse
    /// to write in that state ([`write_refusal`]); this is for a file that
    /// says so anyway. Absent (false) in older files, and left out when
    /// false, so a file from this build reads in older builds as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replacement_pending: bool,
}

/// Why no hand-over may be written now, or `None` when one may.
///
/// * `replacement_pending`: a new login's connection is live while the
///   previous login's data is held ([`crate::decode::DataReplacement`]). A
///   hand-over would pair that connection with the other account's data,
///   and the next run would apply the new login's data on top of it.
/// * `unhandled_reset`: the decoding thread concluded a new connection that
///   the monitor never handled (decoded after its loop ended), so the same
///   pairing, with the replacement never even started.
///
/// Writing nothing is always safe: the next start is a start with the game
/// already running.
pub fn write_refusal(replacement_pending: bool, unhandled_reset: bool) -> Option<&'static str> {
    if replacement_pending {
        Some("the held data is waiting to be replaced by a new login's")
    } else if unhandled_reset {
        Some("a new game connection began after the last event handled")
    } else {
        None
    }
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

    /// Whether the player part (the account data, and what rides on it: the
    /// live journal, the wish link, the resin alert) may be resumed with the
    /// sniffer part; `Err` names why not. The sniffer part alone is always
    /// safe: the data then starts empty and the game's next full send fills
    /// it.
    ///
    /// Not when the file says a replacement was pending, nor when both
    /// halves carry a login UID and they differ: the connection is then
    /// another login's than the data (a file written by a build without the
    /// write guards, or a case they miss).
    pub fn player_resumable(&self) -> Result<(), &'static str> {
        if self.replacement_pending {
            return Err("the held data was waiting to be replaced by a new login's");
        }
        match (self.sniffer.login_uid(), self.player.login_uid()) {
            (Some(connection), Some(data)) if connection != data => {
                Err("the connection's login UID is not the account data's")
            }
            _ => Ok(()),
        }
    }

    /// Written within [`Self::MAX_AGE_MS`] of `now_ms` (and not in the future
    /// by more than a clock step).
    pub fn is_fresh(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.written_ms) <= Self::MAX_AGE_MS
            && self.written_ms.saturating_sub(now_ms) <= 60_000
    }
}

#[cfg(test)]
mod tests {
    use teyvatme_game_data::AnimeGameData;

    use super::*;
    use crate::decode::{DataReplacement, DataVerdict};
    use crate::player_data::PlayerData;

    /// Placeholder UIDs: the held account's and the next login's.
    const UID_A: u32 = 812_345_678;
    const UID_B: u32 = 812_345_679;

    /// A sniffer part carrying `login_uid` (made-up key bytes).
    fn sniffer(login_uid: Option<u32>) -> SnifferHandover {
        serde_json::from_value(serde_json::json!({
            "v": 1, "key": [1, 2, 3, 4], "last_time_seed": null, "time_anchors": [],
            "lanes": [null, null], "login_uid": login_uid,
        }))
        .unwrap()
    }

    fn handover(connection: Option<u32>, data: Option<u32>) -> SessionHandover {
        let mut player = PlayerData::new(AnimeGameData::new());
        player.set_login_uid(data);
        SessionHandover {
            v: SessionHandover::VERSION,
            written_ms: 0,
            game_data: None,
            sniffer: sniffer(connection),
            player: player.handover(),
            live: None,
            wish: None,
            resin_alerted: None,
            replacement_pending: false,
        }
    }

    #[test]
    fn nothing_is_written_while_a_new_logins_data_is_awaited() {
        // A's data held; B's connection begins, its reply and key go by, and
        // what comes before its first data is held back (Hold).
        let mut replacement = DataReplacement::default();
        assert_eq!(write_refusal(replacement.is_pending(), false), None);
        assert!(replacement.connection_reset());
        assert_eq!(
            replacement.verdict(teyvatme_protocol::KeyState::Session, false),
            DataVerdict::Hold
        );
        assert!(write_refusal(replacement.is_pending(), false).is_some());
        // B's data replaces A's: a hand-over may be written again.
        assert_eq!(
            replacement.verdict(teyvatme_protocol::KeyState::Session, true),
            DataVerdict::ReplaceThenApply
        );
        assert_eq!(write_refusal(replacement.is_pending(), false), None);
        // Clear data during the wait: nothing left to replace.
        replacement.connection_reset();
        replacement.clear();
        assert_eq!(write_refusal(replacement.is_pending(), false), None);
    }

    #[test]
    fn nothing_is_written_after_a_reset_the_monitor_never_handled() {
        assert!(write_refusal(false, true).is_some());
        assert!(write_refusal(true, true).is_some());
    }

    #[test]
    fn a_hand_over_pairing_two_logins_does_not_resume_its_data() {
        assert_eq!(
            handover(Some(UID_A), Some(UID_A)).player_resumable(),
            Ok(())
        );
        assert!(
            handover(Some(UID_B), Some(UID_A))
                .player_resumable()
                .is_err()
        );
        // One side without a UID: nothing to compare (a 1.18.4 file, a
        // reply that stated none, Clear data before the next batch).
        assert_eq!(handover(Some(UID_B), None).player_resumable(), Ok(()));
        assert_eq!(handover(None, Some(UID_A)).player_resumable(), Ok(()));
        assert_eq!(handover(None, None).player_resumable(), Ok(()));
        // A file that says a replacement was pending, whatever the UIDs.
        let mut pending = handover(Some(UID_A), Some(UID_A));
        pending.replacement_pending = true;
        assert!(pending.player_resumable().is_err());
        let back = SessionHandover::from_bytes(&pending.to_bytes().unwrap()).unwrap();
        assert!(back.replacement_pending);
        assert!(back.player_resumable().is_err());
    }

    #[test]
    fn a_hand_over_from_1_18_4_still_loads() {
        let mut json = serde_json::to_value(handover(None, None)).unwrap();
        // Its layout: no login UID in either half, no pending flag.
        json["sniffer"].as_object_mut().unwrap().remove("login_uid");
        json["player"].as_object_mut().unwrap().remove("login_uid");
        assert!(json.get("replacement_pending").is_none());
        let old = SessionHandover::from_bytes(&serde_json::to_vec(&json).unwrap()).unwrap();
        assert_eq!(old.player_resumable(), Ok(()));
    }

    #[test]
    fn a_hand_over_from_this_build_loads_in_a_reader_without_the_new_fields() {
        /// The layout as an older build reads it (no `replacement_pending`).
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Older {
            v: u32,
            written_ms: u64,
            game_data: Option<String>,
            sniffer: SnifferHandover,
            player: crate::player_data::PlayerHandover,
            live: Option<LiveHandover>,
            #[serde(default)]
            wish: Option<WishHandover>,
            #[serde(default)]
            resin_alerted: Option<(GiResin, u32)>,
        }
        let written = handover(Some(UID_A), Some(UID_A));
        let bytes = written.to_bytes().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            json.get("replacement_pending").is_none(),
            "false is left out"
        );
        assert!(serde_json::from_slice::<Older>(&bytes).is_ok());
        // Even a file that carries the flag loads there (unknown fields are
        // not refused).
        let mut pending = written;
        pending.replacement_pending = true;
        assert!(serde_json::from_slice::<Older>(&pending.to_bytes().unwrap()).is_ok());
    }
}
