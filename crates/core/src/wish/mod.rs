// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Wish history from the game's own key, shared by the desktop app and the
//! Android app: the key's per-login state ([`hub`]), the game's link
//! ([`link`]), the paced API client ([`api`]), reading every banner
//! ([`history`]), writing it as UIGF ([`uigf`]) and syncing it to the
//! tracker ([`sync`]). Each app brings its own
//! HTTP client and clock.

pub mod api;
pub mod history;
pub mod hub;
pub mod link;
pub mod sync;
pub mod uigf;

pub use hub::Banner;
