// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The session hand-over file: the running game connection carried across a
//! restart of the app (`teyvatme_core::handover`).
//!
//! The monitor writes it as it shuts down cleanly while a session key is live
//! (the update's relaunch, the window closed, Quit) and reads it back at the
//! next start, within two minutes, deleting it either way. A crash writes
//! none. The file holds the key and the account's data: it never leaves the
//! data directory.

use std::path::PathBuf;

use anyhow::{Context, Result};
use teyvatme_core::handover::SessionHandover;

/// Where the file lives: the app's data directory.
fn path() -> Result<PathBuf> {
    let mut path = crate::data_dir()?;
    path.push(SessionHandover::FILE_NAME);
    Ok(path)
}

/// Write `handover` for the next run (through a temporary name, so a crash
/// mid-write leaves no half file).
pub fn write(handover: &SessionHandover) -> Result<()> {
    let path = path()?;
    let bytes = handover
        .to_bytes()
        .context("the hand-over does not serialize")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("cannot move the hand-over into {}", path.display()))?;
    Ok(())
}

/// Take the hand-over the previous run left, if it is fresh at `now_ms`
/// (unix). The file is deleted whether it is used or not: a stale one is
/// worthless, and a used one must not be picked up twice.
pub fn take(now_ms: u64) -> Option<SessionHandover> {
    let path = path().ok()?;
    let bytes = std::fs::read(&path).ok()?;
    if let Err(e) = std::fs::remove_file(&path) {
        tracing::warn!(
            "could not delete the session hand-over {}: {e}",
            path.display()
        );
    }
    let Some(handover) = SessionHandover::from_bytes(&bytes) else {
        tracing::warn!(
            bytes = bytes.len(),
            "the previous run\'s session hand-over does not read (another version?); ignored"
        );
        return None;
    };
    if !handover.is_fresh(now_ms) {
        tracing::info!(
            age_s = now_ms.saturating_sub(handover.written_ms) / 1_000,
            "the previous run\'s session hand-over is too old to pick up; ignored"
        );
        return None;
    }
    Some(handover)
}
