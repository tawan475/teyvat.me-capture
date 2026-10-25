// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Teyvat.me's platform-free core, shared by the desktop app and the Android
//! app: the decode layer from captured frames to account data ([`decode`]),
//! the decoded account model ([`player_data`]), its GOOD export
//! ([`good`]), the live-sync deltas and policy ([`delta`], [`live`]), and the
//! capture recordings ([`pcapng`] writes them, [`recording`] reads them).
//!
//! Nothing here captures packets, draws UI or talks to the tracker; those
//! stay with each app.

pub mod decode;
pub mod delta;
pub mod good;
pub mod live;
pub mod pcapng;
pub mod player_data;
pub mod recording;
