// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

//! Syncing the wish history to the tracker: only what the tracker does not
//! have yet, paging no further than needed ([`Run`]), when a new key or a
//! wish calls for it ([`Schedule`]). Each app brings its HTTP clients and
//! its credential ([`WishTracker`]).

mod run;
mod schedule;
mod tracker;

pub use run::{PAGES_PER_RUN, Partial, Run, RunError, RunReport};
pub use schedule::{CONTINUE_DELAY, MIN_RESYNC, Schedule, WISH_DELAY};
pub use tracker::{
    Cursor, CursorError, ImportBody, ImportRecord, Imported, Known, MAX_RECORDS_PER_IMPORT,
    RECORD_LANG, WishTracker, error_code, import_bodies, is_record_time, parse_cursor,
    parse_import, region_timezone,
};
