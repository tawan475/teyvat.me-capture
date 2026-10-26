// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The wish key's per-login state, fed by the decoder's wish messages.
//!
//! The key belongs to the account logged in, and one device may log in with
//! several accounts (alt swapping), so a key only ever counts for the login
//! it came after: every new login (a new connection's first account data, or
//! another UID mid-run) clears the key, the sync's unfinished banners and
//! pending triggers, and what was seen ([`WishHub::new_login`]); a key is
//! bound to that login's UID. Nothing else is a key source: the game's web
//! cache is never read.
//!
//! The key is a password while it lasts (about a day): memory only, never
//! logged or persisted, and only ever sent to the game's API.

use std::sync::Arc;
use std::time::Instant;

use teyvatme_protocol::{CommandMatch, GachaAuthKey, GachaRecordLink, GachaWish};
use tokio::sync::{mpsc, watch};

use super::link::{LinkParts, WishKey};

/// A banner as the wish history API and the tracker number it (UIGF): 301
/// is both character event banners (its records say 301 or 400).
pub type Banner = u32;

/// The banners synced, in order.
pub const BANNERS: [Banner; 5] = [301, 302, 500, 200, 100];

/// The banner a wish made on the game's `gacha_type` shows up under.
pub fn banner_for_gacha_type(gacha_type: u32) -> Option<Banner> {
    match gacha_type {
        400 => Some(301),
        other if BANNERS.contains(&other) => Some(other),
        _ => None,
    }
}

/// What the monitor saw that concerns wish history.
#[derive(Debug)]
pub enum WishEvent {
    Key(GachaAuthKey),
    Wish(GachaWish),
    /// Every banner's history link, sent when the Wish screen opens.
    Banners(Vec<GachaRecordLink>),
    /// The client's version, sent at login.
    ClientVersion(String),
}

/// Take the wish history messages out of a classified batch.
///
/// They are not account data: they must not count as a batch that replaces
/// the snapshot, or reach the GOOD export.
pub fn take_wish_events(classified: &mut Vec<(u16, CommandMatch)>) -> Vec<WishEvent> {
    if classified.iter().all(|(_, found)| found.is_account_data()) {
        return Vec::new();
    }
    let mut events = Vec::new();
    let mut rest = Vec::with_capacity(classified.len());
    for (command_id, found) in std::mem::take(classified) {
        match found {
            CommandMatch::AuthKey(key) => events.push(WishEvent::Key(key)),
            CommandMatch::Wish(wish) => events.push(WishEvent::Wish(wish)),
            CommandMatch::GachaInfo(links) => {
                // The banner list also carries the account's Epitomized Path,
                // which `apply_commands` keeps: it stays in the batch for that.
                if links.iter().any(|link| link.epitomized.is_some()) {
                    rest.push((command_id, CommandMatch::GachaInfo(links.clone())));
                }
                events.push(WishEvent::Banners(links));
            }
            CommandMatch::ClientVersion(version) => events.push(WishEvent::ClientVersion(version)),
            other => rest.push((command_id, other)),
        }
    }
    *classified = rest;
    events
}

/// What the sync task is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// A new key arrived: sync every banner.
    NewKey,
    /// A wish was made on this banner (`None`: on one that could not be told).
    Wish(Option<Banner>),
    /// A new login: forget everything about the previous one's sync.
    Cleared,
}

/// What the game's wish history link is made of besides the key, as seen
/// so far this session.
#[derive(Debug, Default)]
struct Captured {
    banners: Vec<GachaRecordLink>,
    game_version: Option<String>,
}

impl Captured {
    /// The link parts for a key arriving now, for the account `uid`.
    ///
    /// The game builds the link from the history link of the banner on screen
    /// when History is pressed, which the traffic does not say. The banner
    /// the Wish screen opens on is the character event banner (`gacha_type`
    /// 301, the highest sort order); a recorded 7.1 session's link was that
    /// banner's, so that is the one taken, else the first. (The API answers
    /// the same for any banner's link, or for none.)
    fn link_parts(&self, uid: Option<u32>) -> LinkParts {
        let banner = self
            .banners
            .iter()
            .find(|banner| banner.gacha_type == Some(301))
            .or_else(|| self.banners.first());
        LinkParts {
            record_query: banner.map(|banner| banner.query.clone()),
            game_version: self.game_version.clone(),
            region: uid.and_then(region_for_uid).map(str::to_owned),
        }
    }
}

/// The server region of a UID, as the game's dispatch names it: the region
/// is in the UID's first digit (after the leading 1 of a ten-digit one).
///
/// The game learns its region from the dispatch server over HTTP, not from
/// the traffic Teyvat.me reads, so this is how the link gets its `region`.
pub fn region_for_uid(uid: u32) -> Option<&'static str> {
    let digits = uid.to_string();
    let first = match digits.len() {
        9 => digits.as_bytes()[0],
        10 if digits.starts_with('1') => digits.as_bytes()[1],
        _ => return None,
    };
    match (digits.len(), first) {
        (_, b'6') => Some("os_usa"),
        (_, b'7') => Some("os_euro"),
        (_, b'8') => Some("os_asia"),
        (_, b'9') => Some("os_cht"),
        (9, b'1' | b'2' | b'3') => Some("cn_gf01"),
        (9, b'5') => Some("cn_qd01"),
        _ => None,
    }
}

/// What this session's traffic has shown the wish history so far: the UI's
/// "seen" list, which says why a key or a part of the link is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// The banner list (`GetGachaInfoRsp`): the Wish screen was opened.
    pub banner_list: bool,
    /// A wish history key (`webview_gacha`) in the traffic: the game's wish
    /// history page opened.
    pub wish_key: bool,
    /// Web view keys for something else (the login's `ugc_login`), ignored.
    pub other_keys: u32,
    /// The client's version, from its login.
    pub client_version: bool,
}

/// Asks the UI to repaint: what the monitor hands on changes what it shows.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// The monitor's end: hands captured keys and wishes on, and says when a new
/// login begins.
#[derive(Clone)]
pub struct WishHub {
    key_tx: Arc<watch::Sender<Option<WishKey>>>,
    trigger_tx: mpsc::UnboundedSender<Trigger>,
    captured: Arc<std::sync::Mutex<Captured>>,
    seen_tx: Arc<watch::Sender<Seen>>,
    /// The UID of the login the session is in, once known.
    login_uid: Arc<std::sync::Mutex<Option<u32>>>,
    /// Counts logins; the sync stops a run in flight when it moves.
    login_tx: Arc<watch::Sender<u64>>,
    notify: Notify,
}

impl WishHub {
    /// A new login began (a new connection's first account data, or the
    /// user cleared the data): forget the previous login's key, the sync's
    /// unfinished banners and pending triggers, and what was seen; a sync in
    /// flight stops before it sends.
    pub fn new_login(&self) {
        *self.login_uid.lock().unwrap() = None;
        self.clear();
    }

    fn clear(&self) {
        self.key_tx.send_replace(None);
        self.captured.lock().unwrap().banners.clear();
        // The client's version came with this login's token request, before
        // its account data: it stays.
        self.seen_tx.send_modify(|seen| {
            *seen = Seen {
                client_version: seen.client_version,
                ..Seen::default()
            };
        });
        self.login_tx.send_modify(|logins| *logins += 1);
        let _ = self.trigger_tx.send(Trigger::Cleared);
        tracing::info!("wish: new login, cleared the wish key");
        (self.notify)();
    }

    /// The session's player UID, as the captured data says it. Another UID
    /// than the login's is a new login; the first one binds a key that
    /// arrived before it, which then syncs.
    pub fn note_uid(&self, uid: Option<u32>) {
        let Some(uid) = uid else {
            return;
        };
        let previous = self.login_uid.lock().unwrap().replace(uid);
        match previous {
            Some(previous) if previous == uid => return,
            Some(_) => {
                self.clear();
                return;
            }
            None => {}
        }
        let bound = self.key_tx.send_if_modified(|key| match key {
            Some(waiting) if waiting.uid().is_none() => {
                *waiting = waiting.clone().with_uid(Some(uid));
                true
            }
            _ => false,
        });
        if bound {
            tracing::info!("wish: the key's account is known now; syncing");
            let _ = self.trigger_tx.send(Trigger::NewKey);
            (self.notify)();
        }
    }

    /// Hand `events` on; `uid` is the account's, when captured (for the
    /// link's `region`, and the key's account).
    pub fn forward(&self, events: Vec<WishEvent>, uid: Option<u32>) {
        if events.is_empty() {
            return;
        }
        self.note_uid(uid);
        for event in events {
            match event {
                WishEvent::Key(key) => self.key(key, uid, Instant::now()),
                WishEvent::Banners(banners) => {
                    // Info the first time: what a user's log shows when the
                    // wish section stays empty.
                    if self.see(|seen| !std::mem::replace(&mut seen.banner_list, true)) {
                        tracing::info!(
                            banners = banners.len(),
                            "wish history: saw the banner list (the Wish screen opened)"
                        );
                    } else {
                        tracing::debug!(banners = banners.len(), "banner list");
                    }
                    self.captured.lock().unwrap().banners = banners;
                }
                WishEvent::ClientVersion(version) => {
                    if self.see(|seen| !std::mem::replace(&mut seen.client_version, true)) {
                        tracing::info!("wish history: saw the client version");
                    }
                    self.captured.lock().unwrap().game_version = Some(version);
                }
                WishEvent::Wish(wish) => {
                    let banner = wish.gacha_type.and_then(banner_for_gacha_type);
                    tracing::info!(
                        gacha_type = ?wish.gacha_type,
                        pulls = wish.pulls,
                        "a wish was made"
                    );
                    let _ = self.trigger_tx.send(Trigger::Wish(banner));
                }
            }
        }
        (self.notify)();
    }

    /// Update what was seen; `true` when `change` says it was news.
    fn see(&self, change: impl FnOnce(&mut Seen) -> bool) -> bool {
        let mut news = false;
        self.seen_tx.send_modify(|seen| news = change(seen));
        news
    }

    fn key(&self, key: GachaAuthKey, uid: Option<u32>, now: Instant) {
        if !key.is_wish_history_key() {
            // The login's keys (`ugc_login`) are for something else.
            let first = self.see(|seen| {
                seen.other_keys += 1;
                seen.other_keys == 1
            });
            if first {
                tracing::info!(app = %key.auth_appid, "wish history: ignoring a web view key for another page");
            } else {
                tracing::debug!(app = %key.auth_appid, "ignoring a web view key that is not for wishes");
            }
            return;
        }
        self.see(|seen| {
            seen.wish_key = true;
            false
        });
        let link = self.captured.lock().unwrap().link_parts(uid);
        tracing::info!(
            banner_link = link.record_query.is_some(),
            game_version = link.game_version.is_some(),
            region = link.region.as_deref(),
            "the wish history link's parts"
        );
        let login_uid = *self.login_uid.lock().unwrap();
        let key = WishKey::new(key.authkey(), &key.game_biz, now)
            .with_link(link)
            .with_uid(login_uid);
        let new = self.key_tx.send_if_modified(|current| {
            if current
                .as_ref()
                .is_some_and(|current| current.same_key(&key))
            {
                return false;
            }
            *current = Some(key);
            true
        });
        if !new {
            return;
        }
        if login_uid.is_some() {
            tracing::info!("captured a wish history key");
            let _ = self.trigger_tx.send(Trigger::NewKey);
        } else {
            tracing::info!("captured a wish history key; its account's UID is not known yet");
        }
    }
}

/// What a [`WishHub`] is read through: the held key, the sync's triggers, what
/// was seen, and the login count (a run in flight stops when it moves).
pub struct HubReceivers {
    pub key_rx: watch::Receiver<Option<WishKey>>,
    pub trigger_rx: mpsc::UnboundedReceiver<Trigger>,
    pub seen_rx: watch::Receiver<Seen>,
    pub login_rx: watch::Receiver<u64>,
}

impl WishHub {
    /// A hub and its receivers. `notify` runs whenever what it holds changes
    /// (the desktop repaints; the phone polls, so its does nothing).
    pub fn new(notify: Notify) -> (Self, HubReceivers) {
        let (key_tx, key_rx) = watch::channel(None);
        let (trigger_tx, trigger_rx) = mpsc::unbounded_channel();
        let (seen_tx, seen_rx) = watch::channel(Seen::default());
        let (login_tx, login_rx) = watch::channel(0);
        let hub = Self {
            key_tx: Arc::new(key_tx),
            trigger_tx,
            captured: Arc::default(),
            seen_tx: Arc::new(seen_tx),
            login_uid: Arc::default(),
            login_tx: Arc::new(login_tx),
            notify,
        };
        (
            hub,
            HubReceivers {
                key_rx,
                trigger_rx,
                seen_rx,
                login_rx,
            },
        )
    }

    /// The key's sender, for a sync that learns a key expired.
    pub fn key_sender(&self) -> Arc<watch::Sender<Option<WishKey>>> {
        Arc::clone(&self.key_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn hub() -> (
        WishHub,
        watch::Receiver<Option<WishKey>>,
        mpsc::UnboundedReceiver<Trigger>,
    ) {
        hub_with(Arc::new(|| {}))
    }

    fn hub_with(
        notify: Notify,
    ) -> (
        WishHub,
        watch::Receiver<Option<WishKey>>,
        mpsc::UnboundedReceiver<Trigger>,
    ) {
        let (hub, receivers) = WishHub::new(notify);
        (hub, receivers.key_rx, receivers.trigger_rx)
    }

    fn authkey(app: &str, key: &str) -> GachaAuthKey {
        GachaAuthKey::new(key, app, "hk4e_global")
    }

    const UID: Option<u32> = Some(811_111_111);

    #[test]
    fn what_the_session_showed_is_counted_and_the_ui_is_repainted() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let repaints = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&repaints);
        let (hub, _key_rx, _triggers) = hub_with(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(*hub.seen_tx.borrow(), Seen::default());

        hub.forward(
            vec![
                WishEvent::ClientVersion("OSRELWin7.1.0_R1_S2_D3".to_owned()),
                WishEvent::Key(authkey("ugc_login", "IIII")),
                WishEvent::Key(authkey("ugc_login", "JJJJ")),
            ],
            None,
        );
        assert_eq!(
            *hub.seen_tx.borrow(),
            Seen {
                banner_list: false,
                wish_key: false,
                other_keys: 2,
                client_version: true,
            }
        );
        assert_eq!(repaints.load(Ordering::SeqCst), 1, "one repaint per batch");

        hub.forward(
            vec![
                WishEvent::Banners(vec![banner(301, "cccc")]),
                WishEvent::Key(authkey("webview_gacha", "KKKK")),
            ],
            Some(811_111_111),
        );
        let seen = hub.seen_tx.borrow().clone();
        assert!(seen.banner_list && seen.wish_key);
        assert_eq!(repaints.load(Ordering::SeqCst), 2);

        hub.forward(Vec::new(), None);
        assert_eq!(
            repaints.load(Ordering::SeqCst),
            2,
            "nothing new, no repaint"
        );
    }

    #[test]
    fn only_wish_keys_are_kept_and_a_repeat_triggers_nothing() {
        let (hub, key_rx, mut triggers) = hub();
        hub.forward(vec![WishEvent::Key(authkey("ugc_login", "AAAA"))], UID);
        assert!(key_rx.borrow().is_none());
        assert!(triggers.try_recv().is_err());

        hub.forward(vec![WishEvent::Key(authkey("webview_gacha", "BBBB"))], UID);
        assert_eq!(key_rx.borrow().as_ref().unwrap().uid(), UID);
        assert_eq!(triggers.try_recv(), Ok(Trigger::NewKey));

        hub.forward(vec![WishEvent::Key(authkey("webview_gacha", "BBBB"))], UID);
        assert!(
            triggers.try_recv().is_err(),
            "the same key again is not new"
        );

        hub.forward(vec![WishEvent::Key(authkey("webview_gacha", "CCCC"))], UID);
        assert_eq!(triggers.try_recv(), Ok(Trigger::NewKey));
    }

    #[test]
    fn a_new_login_clears_the_key_and_what_was_seen_and_tells_the_sync() {
        let (hub, key_rx, mut triggers) = hub();
        let logins = hub.login_tx.subscribe();
        hub.forward(
            vec![
                WishEvent::ClientVersion("OSRELWin7.1.0_R1_S2_D3".to_owned()),
                WishEvent::Banners(vec![banner(301, "cccc")]),
                WishEvent::Key(authkey("webview_gacha", "DDDD")),
            ],
            UID,
        );
        assert_eq!(triggers.try_recv(), Ok(Trigger::NewKey));

        hub.new_login();
        assert!(key_rx.borrow().is_none());
        assert_eq!(triggers.try_recv(), Ok(Trigger::Cleared));
        assert!(logins.has_changed().unwrap(), "a run in flight is told");
        let seen = hub.seen_tx.borrow().clone();
        assert_eq!(
            seen,
            Seen {
                client_version: true,
                ..Seen::default()
            }
        );
        assert!(hub.captured.lock().unwrap().banners.is_empty());

        // A key now, before the new login's UID is known: held, not synced.
        hub.forward(vec![WishEvent::Key(authkey("webview_gacha", "EEEE"))], None);
        assert_eq!(key_rx.borrow().as_ref().unwrap().uid(), None);
        assert!(triggers.try_recv().is_err());
        // The UID arrives: the key is that login's, and syncs.
        hub.note_uid(Some(822_222_222));
        assert_eq!(key_rx.borrow().as_ref().unwrap().uid(), Some(822_222_222));
        assert_eq!(triggers.try_recv(), Ok(Trigger::NewKey));
        hub.note_uid(Some(822_222_222));
        assert!(
            triggers.try_recv().is_err(),
            "the same UID again is nothing"
        );
    }

    #[test]
    fn another_uid_mid_run_is_a_new_login() {
        let (hub, key_rx, mut triggers) = hub();
        hub.forward(vec![WishEvent::Key(authkey("webview_gacha", "FFFF"))], UID);
        assert_eq!(triggers.try_recv(), Ok(Trigger::NewKey));

        hub.note_uid(Some(833_333_333));
        assert!(key_rx.borrow().is_none());
        assert_eq!(triggers.try_recv(), Ok(Trigger::Cleared));

        // A key after it is the new account's.
        hub.forward(
            vec![WishEvent::Key(authkey("webview_gacha", "GGGG"))],
            Some(833_333_333),
        );
        assert_eq!(key_rx.borrow().as_ref().unwrap().uid(), Some(833_333_333));
        assert_eq!(triggers.try_recv(), Ok(Trigger::NewKey));
    }

    #[test]
    fn a_wish_on_the_second_character_banner_syncs_the_first() {
        let (hub, _key_rx, mut triggers) = hub();
        let wish = |gacha_type| {
            WishEvent::Wish(GachaWish {
                gacha_type,
                pulls: 10,
            })
        };
        hub.forward(vec![wish(Some(400)), wish(Some(302)), wish(None)], None);
        assert_eq!(triggers.try_recv(), Ok(Trigger::Wish(Some(301))));
        assert_eq!(triggers.try_recv(), Ok(Trigger::Wish(Some(302))));
        assert_eq!(triggers.try_recv(), Ok(Trigger::Wish(None)));
    }

    #[test]
    fn regions_come_from_the_uid_like_the_dispatch_names_them() {
        assert_eq!(region_for_uid(811_111_111), Some("os_asia"));
        assert_eq!(region_for_uid(611_111_111), Some("os_usa"));
        assert_eq!(region_for_uid(711_111_111), Some("os_euro"));
        assert_eq!(region_for_uid(911_111_111), Some("os_cht"));
        assert_eq!(region_for_uid(1_811_111_111), Some("os_asia"));
        assert_eq!(region_for_uid(111_111_111), Some("cn_gf01"));
        assert_eq!(region_for_uid(511_111_111), Some("cn_qd01"));
        assert_eq!(region_for_uid(411_111_111), None);
        assert_eq!(region_for_uid(81_111_111), None);
    }

    fn banner(gacha_type: u32, gacha_id: &str) -> GachaRecordLink {
        GachaRecordLink {
            gacha_type: Some(gacha_type),
            query: format!(
                "win_mode=fullscreen&no_joypad_close=1&authkey_ver=1&sign_type=2\
                 &auth_appid=webview_gacha&init_type=301&gacha_id={gacha_id}&timestamp=1790000000"
            ),
            epitomized: None,
        }
    }

    #[test]
    fn the_key_carries_the_character_banners_link_the_version_and_the_region() {
        let (hub, key_rx, _triggers) = hub();
        hub.forward(
            vec![
                WishEvent::ClientVersion("OSRELWin7.1.0_R1_S2_D3".to_owned()),
                WishEvent::Banners(vec![
                    banner(200, "aaaa"),
                    banner(400, "bbbb"),
                    banner(301, "cccc"),
                ]),
                WishEvent::Key(authkey("webview_gacha", "HHHH")),
            ],
            Some(811_111_111),
        );
        let url = key_rx.borrow().as_ref().unwrap().wish_url();
        assert_eq!(
            url,
            "https://public-operation-hk4e-sg.hoyoverse.com/gacha_info/api/getGachaLog\
             ?win_mode=fullscreen&no_joypad_close=1&authkey_ver=1&sign_type=2\
             &auth_appid=webview_gacha&init_type=301&gacha_id=cccc&timestamp=1790000000\
             &lang=en&device_type=pc&game_version=OSRELWin7.1.0_R1_S2_D3&region=os_asia\
             &authkey=HHHH&game_biz="
        );
    }

    #[test]
    fn without_a_character_banner_the_first_banners_link_is_taken() {
        let captured = Captured {
            banners: vec![banner(200, "aaaa"), banner(302, "dddd")],
            game_version: None,
        };
        let parts = captured.link_parts(None);
        assert!(parts.record_query.unwrap().contains("gacha_id=aaaa"));
        assert_eq!(parts.region, None);
        assert_eq!(Captured::default().link_parts(None), LinkParts::default());
    }

    #[test]
    fn banner_types_map_to_the_synced_banners() {
        assert_eq!(banner_for_gacha_type(400), Some(301));
        for banner in BANNERS {
            assert_eq!(banner_for_gacha_type(banner), Some(banner));
        }
        assert_eq!(banner_for_gacha_type(303), None);
    }

    #[test]
    fn wish_messages_are_taken_out_of_the_batch() {
        let mut classified = vec![
            (1, CommandMatch::DeletedItems(vec![1])),
            (2, CommandMatch::AuthKey(authkey("webview_gacha", "DDDD"))),
            (
                3,
                CommandMatch::Wish(GachaWish {
                    gacha_type: Some(301),
                    pulls: 1,
                }),
            ),
            (4, CommandMatch::GachaInfo(vec![banner(301, "cccc")])),
            (
                5,
                CommandMatch::ClientVersion("OSRELWin7.1.0_R1_S2_D3".to_owned()),
            ),
        ];
        let events = take_wish_events(&mut classified);
        assert_eq!(events.len(), 4);
        assert_eq!(classified.len(), 1);
        assert!(matches!(classified[0], (1, CommandMatch::DeletedItems(_))));
    }
}
