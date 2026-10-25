// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Coming over from irminsul, on the first launch.
//!
//! irminsul's updater installs Teyvat.me over its own exe (the bridge
//! release), so the first start of Teyvat.me can find everything irminsul
//! kept under irminsul's names. What moves where:
//!
//! - **Settings**: irminsul's `app.ron` is copied into Teyvat.me's storage
//!   directory ([`settings_from_irminsul`]), only when Teyvat.me has none yet.
//!   irminsul's own copy stays, so going back to an irminsul build still works.
//! - **The tracker sign-in**: moved, not copied, by `signin.rs` the first time
//!   it is read. A refresh token rotates on every use; a second copy left
//!   behind would end the session the moment irminsul used it.
//! - **Start on startup**: irminsul's task is moved to Teyvat.me's by
//!   `autostart.rs`.
//!
//! irminsul's logs, crash reports and packet logs stay where they are: they
//! describe irminsul's runs.

use std::path::Path;

/// eframe's settings file in a storage directory.
const SETTINGS_FILE: &str = "app.ron";

/// Copy irminsul's settings into `new_dir` unless it already has its own.
/// Either directory may be unknown (no storage directory on this platform).
/// Returns whether a copy was made; failures are logged and leave defaults.
pub fn settings_from_irminsul(old_dir: Option<&Path>, new_dir: Option<&Path>) -> bool {
    let (Some(old_dir), Some(new_dir)) = (old_dir, new_dir) else {
        return false;
    };
    let old = old_dir.join(SETTINGS_FILE);
    let new = new_dir.join(SETTINGS_FILE);
    if new.exists() || !old.is_file() {
        return false;
    }
    let copied = std::fs::create_dir_all(new_dir).and_then(|()| std::fs::copy(&old, &new));
    match copied {
        Ok(_) => {
            tracing::info!("copied irminsul's settings from {old:?} to {new:?}");
            true
        }
        Err(e) => {
            tracing::warn!("could not copy irminsul's settings from {old:?}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irminsuls_settings_are_copied_when_teyvatme_has_none() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("Irminsul").join("data");
        let new = root.path().join("Teyvat.me").join("data");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(SETTINGS_FILE), "(tracker: \"kept\")").unwrap();

        assert!(settings_from_irminsul(Some(&old), Some(&new)));
        assert_eq!(
            std::fs::read_to_string(new.join(SETTINGS_FILE)).unwrap(),
            "(tracker: \"kept\")"
        );
        // irminsul keeps its own.
        assert!(old.join(SETTINGS_FILE).is_file());
    }

    #[test]
    fn teyvatmes_own_settings_are_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join(SETTINGS_FILE), "irminsul").unwrap();
        std::fs::write(new.join(SETTINGS_FILE), "teyvatme").unwrap();

        assert!(!settings_from_irminsul(Some(&old), Some(&new)));
        assert_eq!(
            std::fs::read_to_string(new.join(SETTINGS_FILE)).unwrap(),
            "teyvatme"
        );
    }

    #[test]
    fn nothing_to_copy_is_not_an_error() {
        let root = tempfile::tempdir().unwrap();
        let new = root.path().join("new");
        assert!(!settings_from_irminsul(
            Some(&root.path().join("none")),
            Some(&new)
        ));
        assert!(!settings_from_irminsul(None, Some(&new)));
        assert!(!new.exists());
    }
}
