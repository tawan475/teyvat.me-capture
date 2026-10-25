// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Wish history from the game's own key.
//!
//! When the game's wish history page opens, the server sends a web view key
//! (`GetAuthkeyRsp`, recognised by teyvatme-protocol). The key belongs to
//! the account logged in, and one PC may log in with several accounts (alt
//! swapping), so a key only ever counts for the login it came after:
//!
//! * every new login (a new connection's first account data, or another UID
//!   mid-run) clears the key, the sync's unfinished banners and pending
//!   triggers, and what was seen, and stops a sync in flight before it sends
//!   ([`WishHub::new_login`]);
//! * a key is bound to that login's UID; the sync sends nothing until the UID
//!   is known, and refuses records of any other UID.
//!
//! Nothing else is a key source: the game's web cache is the PC's, not the
//! login's, and is never read.
//!
//! From the key Teyvat.me
//!
//! * gives the wish URL that paimon.moe ("Manual" import) and stardb take,
//!   for anyone to copy -- see [`WishView`]; and
//! * when a tracker credential is set and uploads are on, syncs the history
//!   to the tracker: only what the tracker does not have yet, paging no
//!   further than needed (see [`run`]). A new key syncs every banner; a wish
//!   made in game (`DoGachaRsp`) syncs that banner shortly after. Nothing
//!   polls.
//!
//! The key is a password while it lasts (about a day): it lives in memory
//! only, is never logged or persisted, and only ever goes to the game's API.
//!
//! The monitor's only hooks are [`take_wish_events`] and [`WishHub::forward`];
//! the UI reads a [`WishView`].

mod gacha_log;
mod run;
#[cfg(test)]
mod run_tests;
mod save;
mod sync;
#[cfg(test)]
pub(crate) mod test_server;
mod tracker;

use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
pub(crate) use teyvatme_core::wish::hub::WishEvent;
pub(crate) use teyvatme_core::wish::hub::{
    BANNERS, Banner, Notify, Seen, Trigger, WishHub, take_wish_events,
};
use tokio::sync::watch;

pub(crate) use self::gacha_log::WishKey;
pub(crate) use self::save::{HistorySave, SaveState};
pub(crate) use self::sync::{SyncStatus, WishSync};
use self::tracker::TrackerCredential;
use crate::app::SavedAppState;
use crate::game_watch::GameStatus;
use crate::signin::{TrackerAuth, UploadCredential};

/// The API base and credential a sync uses, or `None` when it must not run.
///
/// The wish card's switch ([`crate::app::want_wish_sync`]), then the same
/// choice as snapshot uploads: the sign-in while signed in, else a saved and
/// verified import key ([`crate::app::upload_credential`]).
fn credential_for(
    state: &SavedAppState,
    auth: &TrackerAuth,
) -> Option<(String, TrackerCredential)> {
    if !crate::app::want_wish_sync(state) {
        return None;
    }
    Some(match crate::app::upload_credential(state)? {
        UploadCredential::SignedIn { base } => (
            base.clone(),
            TrackerCredential::SignedIn {
                auth: auth.clone(),
                base,
            },
        ),
        UploadCredential::ImportKey { key, .. } => (
            state.tracker_api_url.clone(),
            TrackerCredential::ImportKey(key),
        ),
    })
}

/// Whether a captured key would be synced to the tracker.
pub(crate) fn sync_enabled(state: &SavedAppState) -> bool {
    crate::app::want_wish_sync(state)
}

/// The UI's end.
pub(crate) struct WishView {
    key_rx: watch::Receiver<Option<WishKey>>,
    status_rx: watch::Receiver<SyncStatus>,
    seen_rx: watch::Receiver<Seen>,
    /// "Save history": the whole history as a UIGF file.
    pub(crate) save: HistorySave,
}

impl WishView {
    /// The captured key, while it is believed to work.
    pub(crate) fn key(&self, now: Instant) -> Option<WishKey> {
        self.key_rx
            .borrow()
            .as_ref()
            .filter(|key| key.is_fresh(now))
            .cloned()
    }

    /// A key of this login is held but no longer believed to work.
    pub(crate) fn key_expired(&self, now: Instant) -> bool {
        self.key_rx
            .borrow()
            .as_ref()
            .is_some_and(|key| !key.is_fresh(now))
    }

    /// The URL to copy: the game's link for this login's key.
    pub(crate) fn wish_url(&self, now: Instant) -> Option<String> {
        self.key(now).map(|key| key.wish_url())
    }

    /// The section's status line and its tooltip, by what is really the
    /// case: see [`status_line`].
    pub(crate) fn status(&self, now: Instant, game: GameStatus, uid_known: bool) -> WishLine {
        let mut line = status_line(self.key(now).as_ref(), now, game);
        line.tooltip.push_str("\n\n");
        line.tooltip
            .push_str(&seen_summary(&self.seen_rx.borrow(), uid_known));
        line
    }

    /// One line about the tracker sync, or `None` when there is nothing to
    /// say.
    pub(crate) fn sync_line(&self, now: Instant) -> Option<String> {
        if self.key(now).is_some_and(|key| key.uid().is_none()) {
            return Some("Sync waits for the account's UID".to_owned());
        }
        self.status_rx.borrow().line()
    }
}

/// The status line and what its tooltip says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WishLine {
    pub(crate) text: String,
    pub(crate) tooltip: String,
}

/// Where a key comes from, for the tooltips.
const KEY_SOURCES: &str = "A wish key comes when the game opens its wish history page, and \
     lasts about a day. Each login starts fresh: a new login (or another account) clears \
     the key.";

/// What the wish section says, by what is really the case:
///
/// * a key of this login is held: how long it is believed to last;
/// * no key, and this session is not decrypting: waiting for the login
///   (with the capture's own guidance);
/// * no key, decrypting: no key yet.
///
/// Never an instruction: where keys come from is in the tooltip.
pub(crate) fn status_line(key: Option<&WishKey>, now: Instant, game: GameStatus) -> WishLine {
    if let Some(key) = key {
        let hours = key.remaining(now).as_secs().div_ceil(3600);
        let text = if hours <= 1 {
            "Key valid < 1 h".to_owned()
        } else {
            format!("Key valid ~{hours} h")
        };
        return WishLine {
            text,
            tooltip: "The wish history key the game sent when its history page opened \
                      in this login. It lasts about a day; until then Copy wish URL gives \
                      the game's own link, and new wishes sync by themselves when the \
                      Tracker is set up. A new login clears it."
                .to_owned(),
        };
    }
    if game != GameStatus::Decoding {
        return WishLine {
            text: "Waiting for the game's login".to_owned(),
            tooltip: format!("{}\n{}\n\n{KEY_SOURCES}", game.label(), game.tooltip()),
        };
    }
    WishLine {
        text: "No wish key yet".to_owned(),
        tooltip: KEY_SOURCES.to_owned(),
    }
}

/// The marks in the "seen this session" list.
mod marks {
    pub(crate) const ICON_CHECK: &str = "✓";
    pub(crate) const ICON_CLOSE: &str = "✗";
}

/// The "seen this session" list for the tooltip.
pub(crate) fn seen_summary(seen: &Seen, uid_known: bool) -> String {
    use crate::wish_history::marks::{ICON_CHECK, ICON_CLOSE};
    let mark = |yes: bool| if yes { ICON_CHECK } else { ICON_CLOSE };
    let mut lines = vec![
        "Seen this session:".to_owned(),
        format!("{} banner list (Wish screen)", mark(seen.banner_list)),
        format!("{} wish key from the game", mark(seen.wish_key)),
        format!("{} client version", mark(seen.client_version)),
        format!("{} UID", mark(uid_known)),
    ];
    if seen.other_keys > 0 {
        lines.push(format!(
            "{} other web key{} ignored",
            seen.other_keys,
            if seen.other_keys == 1 { "" } else { "s" }
        ));
    }
    lines.join("\n")
}

/// Everything the wish history needs, made together.
pub(crate) struct WishChannels {
    pub(crate) hub: WishHub,
    pub(crate) view: WishView,
    pub(crate) sync: WishSync,
}

/// Build the hub, the view and the sync task (not yet running).
pub(crate) fn channels(
    saved_state_rx: watch::Receiver<SavedAppState>,
    toast_tx: crate::activity::Sender,
    ctx: egui::Context,
    auth: TrackerAuth,
) -> WishChannels {
    let (status_tx, status_rx) = watch::channel(SyncStatus::default());
    let repaint = ctx.clone();
    let notify: Notify = Arc::new(move || repaint.request_repaint());
    let (hub, receivers) = WishHub::new(notify);
    let save = HistorySave::new(receivers.login_rx.clone());
    let sync = WishSync::new(
        receivers.trigger_rx,
        receivers.login_rx,
        hub.key_sender(),
        status_tx,
        saved_state_rx,
        toast_tx,
        ctx,
        auth,
    );
    let (key_rx, seen_rx) = (receivers.key_rx, receivers.seen_rx);
    WishChannels {
        hub,
        view: WishView {
            key_rx,
            status_rx,
            seen_rx,
            save,
        },
        sync,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UID: Option<u32> = Some(811_111_111);

    #[test]
    fn the_status_line_says_what_is_really_the_case() {
        let now = Instant::now();
        // Nothing decrypts: never "open the history".
        for game in [
            GameStatus::NotRunning,
            GameStatus::AwaitingLogin,
            GameStatus::KeyLost,
            GameStatus::LaunchMissed(crate::game_watch::MissedLaunch::AlreadyRunning),
        ] {
            let line = status_line(None, now, game);
            assert_eq!(line.text, "Waiting for the game's login", "{game:?}");
            assert!(
                line.tooltip.contains(game.tooltip()),
                "{game:?}: {}",
                line.tooltip
            );
            assert!(line.tooltip.contains("Each login starts fresh"), "{game:?}");
        }
        // Decrypting, no key yet: neutral, where keys come from in the tooltip.
        let line = status_line(None, now, GameStatus::Decoding);
        assert_eq!(line.text, "No wish key yet");
        assert!(line.tooltip.contains("wish history page"));
        assert!(line.tooltip.contains("Each login starts fresh"));
        assert!(line.tooltip.contains("about a day"));
        // A key: how long it lasts, whatever the game does now.
        let key = WishKey::new("LLLL", "hk4e_global", now);
        for game in [GameStatus::Decoding, GameStatus::NotRunning] {
            assert_eq!(status_line(Some(&key), now, game).text, "Key valid ~24 h");
        }
        let late = now + gacha_log::KEY_LIFETIME - std::time::Duration::from_secs(600);
        assert_eq!(
            status_line(Some(&key), late, GameStatus::Decoding).text,
            "Key valid < 1 h"
        );
        // Nowhere an instruction to open the history.
        for line in [
            status_line(None, now, GameStatus::Decoding),
            status_line(None, now, GameStatus::NotRunning),
            status_line(Some(&key), now, GameStatus::Decoding),
        ] {
            let all = format!("{} {}", line.text, line.tooltip);
            assert!(
                !all.contains("Open Wish") && !all.contains("→ History"),
                "{all}"
            );
        }
    }

    #[test]
    fn the_seen_list_marks_each_part() {
        use crate::wish_history::marks::{ICON_CHECK, ICON_CLOSE};

        let seen = Seen {
            banner_list: true,
            wish_key: false,
            other_keys: 1,
            client_version: true,
        };
        assert_eq!(
            seen_summary(&seen, false),
            format!(
                "Seen this session:\n{ICON_CHECK} banner list (Wish screen)\n\
                 {ICON_CLOSE} wish key from the game\n{ICON_CHECK} client version\n\
                 {ICON_CLOSE} UID\n1 other web key ignored"
            )
        );
        assert!(!seen_summary(&Seen::default(), true).contains("ignored"));
        assert!(seen_summary(&Seen::default(), true).contains(&format!("{ICON_CHECK} UID")));
        assert!(seen_summary(&Seen::default(), true).ends_with(&format!("{ICON_CHECK} UID")));
    }

    #[test]
    fn the_view_gives_the_held_keys_link_and_says_how_long_it_lasts() {
        let (key_tx, key_rx) = watch::channel(None);
        let (_status_tx, status_rx) = watch::channel(SyncStatus::default());
        let (_seen_tx, seen_rx) = watch::channel(Seen::default());
        let (_login_tx, login_rx) = watch::channel(0_u64);
        let view = WishView {
            key_rx,
            status_rx,
            seen_rx,
            save: HistorySave::new(login_rx),
        };
        let status = |at| view.status(at, GameStatus::Decoding, true).text;
        let now = Instant::now();
        assert_eq!(view.wish_url(now), None);
        assert_eq!(status(now), "No wish key yet");

        key_tx.send_replace(Some(WishKey::new("EEEE", "hk4e_global", now)));
        let url = view.wish_url(now).unwrap();
        assert!(url.contains("authkey=EEEE"), "{url}");
        assert_eq!(status(now), "Key valid ~24 h");
        assert_eq!(
            view.sync_line(now).as_deref(),
            Some("Sync waits for the account's UID")
        );
        key_tx.send_replace(Some(WishKey::new("EEEE", "hk4e_global", now).with_uid(UID)));
        assert_eq!(view.sync_line(now), None);
        assert!(
            view.status(now, GameStatus::Decoding, true)
                .tooltip
                .contains("Seen this session:")
        );

        // A day later the key is gone.
        let later = now + gacha_log::KEY_LIFETIME;
        assert_eq!(view.wish_url(later), None);
        assert_eq!(status(later), "No wish key yet");
    }

    #[test]
    fn syncing_is_gated_and_authenticated_like_automated_uploads() {
        use crate::app::TrackerSignIn;

        let auth = TrackerAuth::new(
            "https://tracker.invalid/api",
            Arc::new(crate::signin::MemoryStore::default()),
            "TEST-PC".to_owned(),
            Arc::new(|_| {}),
            Arc::new(|| {}),
        );
        let mut state = SavedAppState {
            auto_export_to_tracker: true,
            tracker_api_url: "https://tracker.invalid/api".to_owned(),
            tracker_import_key: "gdt_uk_test".to_owned(),
            tracker_verified: true,
            tracker_sign_in: TrackerSignIn::SignedOut,
            ..Default::default()
        };
        let credential = |state: &SavedAppState| {
            credential_for(state, &auth).map(|(base, credential)| (base, format!("{credential:?}")))
        };

        // Not signed in: the verified import key.
        assert!(sync_enabled(&state));
        assert_eq!(
            credential(&state),
            Some((
                "https://tracker.invalid/api".to_owned(),
                "ImportKey(<redacted>)".to_owned()
            ))
        );
        state.tracker_verified = false;
        assert_eq!(credential(&state), None);

        // Signed in: the sign-in, whatever the key says.
        state.tracker_sign_in = TrackerSignIn::SignedIn;
        assert_eq!(
            credential(&state),
            Some((
                "https://tracker.invalid/api".to_owned(),
                "SignedIn(https://tracker.invalid/api)".to_owned()
            ))
        );

        // The wish switch off, or the sign-in not read yet: nothing syncs.
        // Snapshot uploads are not the gate any more: the wish card has its
        // own switch.
        state.send_wishes = false;
        assert!(!sync_enabled(&state));
        assert_eq!(credential(&state), None);
        state.send_wishes = true;
        state.auto_export_to_tracker = false;
        assert!(sync_enabled(&state));
        state.tracker_sign_in = TrackerSignIn::Unknown;
        assert_eq!(credential(&state), None);
    }
}
