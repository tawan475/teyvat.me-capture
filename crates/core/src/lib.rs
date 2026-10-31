// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Teyvat.me's platform-free core, shared by the desktop app and the Android
//! app: the decode layer from captured frames to account data ([`decode`]),
//! the decoded account model ([`player_data`]), its GOOD export
//! ([`good`]), the live-sync deltas and policy ([`delta`], [`live`]), and the
//! capture recordings ([`pcapng`] writes them, [`recording`] reads them),
//! wish history from the game's own key ([`wish`]), and the tracker link's
//! presence heartbeat and farm plan ([`link`]).
//!
//! Nothing here captures packets, draws UI or talks to the tracker; those
//! stay with each app (the wish history API is called through the app's own
//! HTTP client).

pub mod decode;
pub mod delta;
pub mod good;
pub mod link;
pub mod live;
pub mod pcapng;
pub mod player_data;
pub mod recording;
pub mod wish;

/// The export format's display name, the one place both apps take it from
/// (the desktop's texts fill it in; Android's build reads this line into
/// `BuildConfig.FORMAT_NAME`, so keep it a plain one-line string literal).
///
/// Only ever shown to people: the name never goes into the file, which stays
/// GOOD (`"format": "GOOD"`) with Teyvat.me's optional `gi_*` keys beside it.
/// The file says which Leyline it is with [`LEYLINE_VERSION`].
pub const FORMAT_NAME: &str = "Leyline";

/// The Leyline version: every export and snapshot upload carries it as its
/// top-level `"leyline"` key (`good::Good::leyline`). GOOD readers skip it.
pub const LEYLINE_VERSION: u32 = 1;

/// A saved export's file name: `leyline-<date>.json`, `date` as the caller
/// formats it.
pub fn export_file_name(date: impl std::fmt::Display) -> String {
    format!("{}-{date}.json", FORMAT_NAME.to_ascii_lowercase())
}
