// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Live sync: between two snapshots, send the tracker what changed.
//!
//! Plan: the tracker's `docs/plans/live-sync.md` (§4.3 sending rules, §6
//! irminsul, §13 the contract as built). The game sends the whole account at
//! login and at every world change; the login's is a snapshot tagged with
//! `gi_capture`, and so is a world change's while no journal is open (with
//! one open, the world change's differences go as a batch). In between, this
//! sends batches of changes
//! (`delta.rs`) to `POST /irminsul/changes`, adaptively in time and amount,
//! and nothing at all while nothing changes.
//!
//! [`LiveSync`] is the policy and the bookkeeping only: no I/O, no clock of
//! its own (every call takes `now` in milliseconds), so the monitor and
//! `--replay-journal` (simulated time) run exactly the same decisions.
//!
//! Live mode needs the tracker's consent: the answer to a snapshot upload
//! names the journal's `live.base`. Without it (an older tracker, live sync
//! off for the account, the switch off here) Teyvat.me stays in snapshot mode,
//! today's behaviour: every change uploads the whole account after the 5 s
//! debounce, tagged `reason: "change"`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::delta::{self, GoodDelta, Keyed, Path, Sum, Unbatched};
use crate::good::{GiCapture, GiCaptureJournal, Good};

/// No batch sooner than this after the previous one (except a big one).
pub const BATCH_SPACING_MS: u64 = 15_000;
/// The spacing while a teyvat.me page of the account is open (the tracker's
/// `watching`): the user's pick, 2026-11-20. 12 batches a minute at most,
/// inside the tracker's 20 a minute.
pub const WATCHED_SPACING_MS: u64 = 5_000;
/// A batch waits until the changes have rested this long, so one reward's
/// packets (resin, Mora, EXP, the drops: several messages over a few
/// hundred ms) go together instead of one part now and the rest a spacing
/// later.
pub const SETTLE_MS: u64 = 1_000;
/// Changes that never rest (Mora in a long fight) hold a batch back at most
/// this long.
pub const MAX_SETTLE_MS: u64 = 3_000;
/// A lone change waits at most this long.
pub const LONE_CHANGE_MS: u64 = 120_000;
/// Changes at once that send a batch straight away.
pub const BIG_BATCH: usize = 2_000;
/// Changes that make a batch worth sending after [`BATCH_SPACING_MS`].
pub const NORMAL_BATCH: usize = 5;
/// The first retry after a failed request; doubled up to [`MAX_BACKOFF_MS`].
pub const FIRST_BACKOFF_MS: u64 = 15_000;
pub const MAX_BACKOFF_MS: u64 = 120_000;
/// Items in one item packet for it to be a full send of the account (the
/// game's own login and world-change sends hold thousands; the matcher
/// itself needs ten).
pub const FULL_SEND_MIN_ITEMS: usize = 100;

/// Why a snapshot is taken (`gi_capture.reason`; contract, append only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Login,
    World,
    Manual,
    Collapse,
    Logout,
    Resync,
    Change,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::World => "world",
            Self::Manual => "manual",
            Self::Collapse => "collapse",
            Self::Logout => "logout",
            Self::Resync => "resync",
            Self::Change => "change",
        }
    }

    /// Whether this snapshot waits for the debounced automation export (the
    /// game's full sends arrive over several seconds) rather than going now.
    fn debounced(self) -> bool {
        matches!(self, Self::Login | Self::World)
    }

    /// Whether live sync retries this snapshot itself when it fails (the
    /// automation retries its own, by releasing its signature).
    fn retried_here(self) -> bool {
        matches!(self, Self::Collapse | Self::Logout | Self::Resync)
    }
}

/// Whether an item packet is the game sending the whole inventory (login, or
/// a world change), as opposed to a change to a few items.
pub fn is_full_send(items_in_packet: usize, items_held: usize) -> bool {
    items_in_packet >= FULL_SEND_MIN_ITEMS && items_in_packet * 2 >= items_held
}

/// A new id for one game login: 16 characters of `[A-Za-z0-9_-]`.
pub fn new_session_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // Unique enough for one PC's logins: the time, spread over the bytes.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = (nanos >> ((i % 16) * 8)) as u8 ^ (i as u8).wrapping_mul(37);
        }
    }
    bytes
        .iter()
        .map(|byte| ALPHABET[usize::from(byte & 63)] as char)
        .collect()
}

/// The game login the data belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Login {
    session: String,
    /// Full sends seen in this connection: 1 = the login's own.
    world: u32,
}

/// The tracker's journal this follows: what it has acknowledged.
#[derive(Clone, Debug)]
struct Journal {
    base: String,
    /// The last batch the tracker stored on it (0: none yet).
    seq: u64,
    /// The state it holds: the snapshot plus every stored batch.
    keyed: Keyed,
    /// The [`delta::LAYOUT`] `keyed` is in: older after a hand-over from an
    /// older Capture, until the first export seeds the sections added since
    /// ([`LiveSync::seed`]).
    layout: u32,
    /// The base snapshot's fingerprints of the parts no batch carries
    /// ([`Unbatched`]), from its own export; batches never change them.
    /// `None`: unknown (a hand-over from an older Capture), taken as changed.
    unbatched: Option<Unbatched>,
}

/// How the ended login's journal is being closed, with the tracker's word
/// for final batches (`upload.finalBatch`): see [`LiveSync::tick`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Ending {
    /// Not decided (or no login ending).
    #[default]
    Open,
    /// A final batch closes it.
    FinalBatch,
    /// The logout snapshot does, as without the word.
    Snapshot,
}

/// A snapshot upload in flight.
#[derive(Clone, Debug)]
struct SnapshotFlight {
    ticket: u64,
    reason: Reason,
    keyed: Keyed,
}

/// A batch in flight.
#[derive(Clone, Debug)]
struct BatchFlight {
    delta: GoodDelta,
    keyed: Keyed,
    /// Changes noted when its export was built: what it covers.
    pending_at_build: usize,
}

/// Live sync's place in the running login, as the session hand-over carries
/// it ([`LiveSync::handover`] / [`LiveSync::resume`]): the login and the
/// tracker's journal with the state it holds, so the next process's batches
/// continue the journal instead of starting with a resync snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveHandover {
    session: String,
    world: u32,
    journal: Option<JournalHandover>,
    pending: usize,
    last_sent_ms: u64,
    /// The tracker's last word on a tab watching (else the first lone
    /// change after the resume waits the lone-change time).
    #[serde(default)]
    watching: bool,
    /// Endgame records waiting for their attach (else lost until the next
    /// login).
    #[serde(default)]
    attach_wanted: bool,
    /// Endgame state the tracker has not acknowledged
    /// ([`LiveSync::note_endgame_change`]); absent from an older Capture:
    /// unknown, taken as not acknowledged (the login then ends on a logout
    /// snapshot, like a journal without fingerprints).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endgame_unacked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct JournalHandover {
    base: String,
    seq: u64,
    entries: Vec<(Path, Value)>,
    hints: Vec<(Path, Value)>,
    /// [`delta::LAYOUT`] the entries were keyed by; absent before 1.17.26
    /// (layout 1 or 2, taken as another one).
    #[serde(default)]
    layout: Option<u32>,
    /// The base's fingerprints of the parts no batch carries; absent from
    /// an older Capture (unknown: the login then ends on a logout snapshot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unbatched: Option<Unbatched>,
}

/// What the monitor should do now.
#[derive(Debug, PartialEq)]
pub enum Action {
    Nothing,
    /// Upload the current state as a snapshot, tagged with this capture.
    Snapshot(Reason),
    /// POST this batch.
    Batch(Box<GoodDelta>),
    /// POST the endgame parts held to `/irminsul/attach`, for the newest
    /// snapshot of this login (`session`): see [`LiveSync::note_endgame_change`].
    Attach {
        session: String,
    },
}

/// The attach request (`POST /irminsul/attach`, Capture 1.17.21): the
/// endgame parts the game sends only when a screen is opened, after the
/// login's snapshot, written by the tracker into that login's newest
/// snapshot in place of what it held of them. Contract: the tracker's
/// `docs/plans/live-sync.md` §13.5.
#[derive(Clone, Debug, Serialize)]
pub struct AttachRequest {
    pub v: u32,
    pub uid: u32,
    pub session: String,
    pub gi_endgame: crate::good::GiEndgame,
}

/// The tracker's answer to an attach, as far as live sync cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachAnswer {
    /// Written, or already there.
    Stored,
    /// Rate limited, a server error, no connection: send it again after a wait.
    Retry,
    /// 404 (no snapshot yet), 409 (the newest snapshot is another login's),
    /// 400, 401, 403: dropped; the next login's snapshot carries the data.
    Dropped,
}

impl AttachAnswer {
    /// `status` 0 = the request never got an answer.
    pub fn of(status: u16) -> Self {
        match status {
            200..=299 => Self::Stored,
            0 | 429 | 500..=599 => Self::Retry,
            _ => Self::Dropped,
        }
    }
}

/// The tracker's answer to a snapshot upload, as far as live sync cares.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportAnswer {
    /// `live.base` when the tracker now follows this snapshot with a journal.
    pub base: Option<String>,
    pub watching: bool,
}

impl ImportAnswer {
    /// Read the import's answer: the hosted tracker's `ImportResponse`, or
    /// the old backend's `{status, message, data}` wrapper. Anything else
    /// means no live sync.
    pub fn parse(body: &str) -> Self {
        #[derive(Deserialize)]
        struct Live {
            base: String,
            #[serde(default)]
            watching: bool,
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return Self::default();
        };
        let live = value
            .get("live")
            .or_else(|| value.get("data").and_then(|data| data.get("live")))
            .cloned()
            .and_then(|live| serde_json::from_value::<Live>(live).ok())
            .filter(|live| !live.base.is_empty() && live.base.len() <= 32);
        match live {
            Some(live) => Self {
                base: Some(live.base),
                watching: live.watching,
            },
            None => Self::default(),
        }
    }
}

/// Whether a tracker answer (verify-key, whoami, an import's) says snapshot
/// uploads may be gzipped: `upload.gzip`, at the top level or inside the old
/// backend's `data` wrapper. A tracker that does not say so gets plain JSON,
/// as every tracker did before.
pub fn accepts_gzip(answer: &serde_json::Value) -> bool {
    upload_option(answer, "gzip")
}

/// Whether a tracker answer says a game login may end its journal with a
/// final batch (`"end": true`) instead of a logout snapshot:
/// `upload.finalBatch`, read like [`accepts_gzip`]. A tracker that does not
/// say so gets the logout snapshot, as every tracker did before.
pub fn accepts_final_batch(answer: &serde_json::Value) -> bool {
    upload_option(answer, "finalBatch")
}

/// `upload.<option>` is `true` in `answer` or its `data` wrapper.
fn upload_option(answer: &serde_json::Value, option: &str) -> bool {
    [Some(answer), answer.get("data")]
        .into_iter()
        .flatten()
        .filter_map(|at| at.get("upload")?.get(option)?.as_bool())
        .next()
        .unwrap_or(false)
}

/// What the tracker said to a batch (§13's answer table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchAnswer {
    Stored {
        collapse: bool,
        watching: bool,
    },
    /// A retry of a batch it already stored.
    Duplicate,
    /// Live sync is off for the account.
    LiveOff,
    /// 409 `resync`, or the batch was too big: send a snapshot.
    Resync,
    /// 429 `daily_live_limit`: snapshots until the next UTC day.
    DailyLimit,
    /// Rate limited, a server error, no connection: keep the changes, retry.
    Retry,
    /// 401 / 403 and the like: snapshot mode; the next snapshot upload shows
    /// the user what is wrong, as uploads always have.
    Refused,
}

impl BatchAnswer {
    /// `status` 0 = the request never got an answer.
    pub fn of(status: u16, body: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let code = value
            .get("error")
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        match status {
            200..=299 => {
                if value.get("ok") == Some(&serde_json::Value::Bool(false)) {
                    Self::LiveOff
                } else if value.get("duplicate") == Some(&serde_json::Value::Bool(true)) {
                    Self::Duplicate
                } else if value.get("ok") == Some(&serde_json::Value::Bool(true)) {
                    Self::Stored {
                        collapse: value.get("collapse") == Some(&serde_json::Value::Bool(true)),
                        watching: value.get("watching") == Some(&serde_json::Value::Bool(true)),
                    }
                } else {
                    // A 2xx that is not this API (an old tracker's app page).
                    Self::LiveOff
                }
            }
            409 if code == "resync" => Self::Resync,
            409 => Self::Refused,
            413 | 422 => Self::Resync,
            429 if code == "daily_live_limit" => Self::DailyLimit,
            404 | 405 => Self::LiveOff,
            0 | 429 | 500..=599 => Self::Retry,
            _ => Self::Refused,
        }
    }
}

/// Live sync's state for one running Teyvat.me.
#[derive(Debug, Default)]
pub struct LiveSync {
    login: Option<Login>,
    journal: Option<Journal>,
    snapshot_flight: Option<SnapshotFlight>,
    batch_flight: Option<BatchFlight>,
    tickets: u64,
    /// A batch that failed to get an answer: resent unchanged, so a retry
    /// the tracker already stored is a duplicate of exactly it.
    unacked: Option<BatchFlight>,
    /// A snapshot that must be uploaded.
    wanted: Option<Reason>,
    /// The capture a wanted logout snapshot carries (the login is over).
    logout_login: Option<(Login, u64)>,
    /// Account changes noted since the last batch was built (an estimate:
    /// the diff decides what is sent).
    pending: usize,
    last_sent_ms: u64,
    watching: bool,
    retry_at_ms: u64,
    backoff_ms: u64,
    /// A look (`tick`) that found the pending changes not worth a batch yet
    /// waits until this before looking again. Building the export is the
    /// expensive part of a look, and looking every time the monitor woke
    /// rebuilt it three times a second for a minute at a time while a lone
    /// change waited. New data brings the next look forward.
    recheck_at_ms: u64,
    /// Snapshot mode until this UTC day is over (the daily batch limit).
    off_day: Option<u64>,
    /// The endgame parts changed since the last attach was sent: one goes
    /// at the next tick (unless a snapshot is due, which carries them).
    attach_wanted: bool,
    /// An attach request in flight.
    attach_flight: bool,
    /// Not before this (a failed attach backs off like a batch).
    attach_retry_at_ms: u64,
    attach_backoff_ms: u64,
    /// The endgame parts changed and no attach of them was stored, nor a
    /// snapshot started since: `gi_endgame` is in no fingerprint and no
    /// batch, so a login that ends with this set keeps its logout snapshot
    /// (an attach answered Dropped leaves the parts with no one else).
    endgame_unacked: bool,
    /// The parts shared with the tracker last changed then, and the journal
    /// (kept against the old set) has not been replaced yet: see
    /// [`Self::note_sharing_changed`].
    sharing_changed_ms: Option<u64>,
    /// When the changes waiting for a batch first and last came
    /// ([`Self::note_change_at`]); `None` with nothing timed waiting.
    changing_ms: Option<(u64, u64)>,
    /// How the ended login's journal is being closed.
    ending: Ending,
    /// The tracker's word for final batches at the last [`Self::tick`].
    final_word: bool,
    /// The parts that kept the last logout snapshot from being a final
    /// batch ([`Self::logout_snapshot_kept`]).
    kept_parts: Vec<String>,
}

const DAY_MS: u64 = 86_400_000;

/// How long the sharing switches must be left alone before the journal is
/// started over: several flips in a row make one new journal, not one each.
pub const SHARING_QUIET_MS: u64 = 10_000;

impl LiveSync {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether batches go to the tracker now (else snapshot mode).
    pub fn is_live(&self, now_ms: u64) -> bool {
        self.journal.is_some() && self.off_day.is_none_or(|day| now_ms / DAY_MS > day)
    }

    /// The journal's last stored batch.
    #[cfg(test)]
    pub fn seq(&self) -> Option<u64> {
        self.journal.as_ref().map(|journal| journal.seq)
    }

    /// The game sent the whole inventory: a login (the first of a
    /// connection) or a world change. A world change while the tracker
    /// follows a journal uploads nothing of its own: whatever it changed goes
    /// in the next batch, like any change (a ~1 MB snapshot per teleport into
    /// a domain was most of the tracker's CPU time).
    pub fn note_full_send(&mut self) {
        match &mut self.login {
            Some(login) => {
                login.world += 1;
                if self.journal.is_none() {
                    self.wanted = Some(Reason::World);
                }
            }
            None => {
                self.login = Some(Login {
                    session: new_session_id(),
                    world: 1,
                });
                self.wanted = Some(Reason::Login);
            }
        }
    }

    /// The endgame parts differ from what was held (the Theater's state, the
    /// Stygian records, the Abyss record book: they arrive when a screen is
    /// opened, after the login's snapshot, and no batch path carries them).
    /// An attach goes up at the next tick, in live mode and snapshot mode
    /// alike, unless a snapshot is due anyway (a login or world change
    /// carries them) or no login has been seen yet (nothing uploads before
    /// the first full send). Before 1.17.21, records opened after the
    /// login's snapshot reached the tracker only with the next login.
    pub fn note_endgame_change(&mut self) {
        self.endgame_unacked = true;
        if self.login.is_some() {
            self.attach_wanted = true;
        }
    }

    /// The attach request finished.
    pub fn attach_answered(&mut self, answer: AttachAnswer, now_ms: u64) {
        self.attach_flight = false;
        match answer {
            AttachAnswer::Stored => {
                self.attach_backoff_ms = 0;
                self.attach_retry_at_ms = 0;
                // Unless a newer change waits for its own attach.
                if !self.attach_wanted {
                    self.endgame_unacked = false;
                }
            }
            AttachAnswer::Retry => {
                self.attach_wanted = true;
                self.attach_backoff_ms = if self.attach_backoff_ms == 0 {
                    FIRST_BACKOFF_MS
                } else {
                    (self.attach_backoff_ms * 2).min(MAX_BACKOFF_MS)
                };
                self.attach_retry_at_ms = now_ms + self.attach_backoff_ms;
            }
            AttachAnswer::Dropped => {
                self.attach_backoff_ms = 0;
                self.attach_retry_at_ms = 0;
            }
        }
    }

    /// Whether an attach goes now: wanted, none in flight, no snapshot due
    /// (it would carry the parts), the backoff over.
    fn attach_due(&self, now_ms: u64) -> bool {
        self.attach_wanted
            && !self.attach_flight
            && self.wanted.is_none()
            && self.snapshot_flight.is_none()
            && now_ms >= self.attach_retry_at_ms
    }

    /// The login and journal, for the session hand-over; `None` before a
    /// login's snapshot named the session.
    pub fn handover(&self) -> Option<LiveHandover> {
        let login = self.login.as_ref()?;
        Some(LiveHandover {
            session: login.session.clone(),
            world: login.world,
            journal: self.journal.as_ref().map(|journal| JournalHandover {
                base: journal.base.clone(),
                seq: journal.seq,
                entries: journal.keyed.entries.clone().into_iter().collect(),
                hints: journal.keyed.hints.clone().into_iter().collect(),
                layout: Some(delta::LAYOUT),
                unbatched: journal.unbatched.clone(),
            }),
            pending: self.pending,
            last_sent_ms: self.last_sent_ms,
            watching: self.watching,
            attach_wanted: self.attach_wanted,
            endgame_unacked: Some(self.endgame_unacked),
        })
    }

    /// Continue the login another process handed over: its session for the
    /// snapshots and attaches, its journal for the batches. Fresh senders
    /// only (`false` otherwise).
    pub fn resume(&mut self, handover: LiveHandover) -> bool {
        if self.login.is_some() || self.journal.is_some() {
            return false;
        }
        self.login = Some(Login {
            session: handover.session,
            world: handover.world,
        });
        self.journal = handover.journal.map(|journal| Journal {
            base: journal.base,
            seq: journal.seq,
            keyed: Keyed {
                entries: journal.entries.into_iter().collect(),
                hints: journal.hints.into_iter().collect(),
                unbatched: Unbatched::default(),
            },
            // Keyed by an older Capture's layout (absent before 1.17.26:
            // layout 1): the sections added since are seeded from the first
            // export, else 1.17.24's journal against 1.17.25's export read
            // as 2,452 added quests, a whole snapshot. A newer layout than
            // ours is left to the diff, as any journal is.
            layout: journal.layout.unwrap_or(1).min(delta::LAYOUT),
            unbatched: journal.unbatched,
        });
        self.pending = handover.pending;
        self.last_sent_ms = handover.last_sent_ms;
        self.watching = handover.watching;
        self.attach_wanted = handover.attach_wanted;
        self.endgame_unacked = handover.endgame_unacked.unwrap_or(true);
        true
    }

    /// The handed-over journal was keyed by an older layout and waits for
    /// the state as it is now ([`Self::seed`]).
    pub fn needs_seed(&self) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|journal| journal.layout < delta::LAYOUT)
    }

    /// Seed the handed-over journal's sections that its older layout never
    /// keyed from `current` (the state as it is now): they then read as
    /// unchanged, and the next batch carries only what the older layout
    /// keyed and moved (the restart gap's changes included).
    pub fn seed(&mut self, current: &Keyed) {
        if let Some(journal) = self.journal.as_mut()
            && journal.layout < delta::LAYOUT
        {
            let seeded = journal
                .keyed
                .seed_sections_added_after(journal.layout, current);
            tracing::info!(
                from = journal.layout,
                to = delta::LAYOUT,
                seeded,
                "live sync: the handed-over journal was keyed by an older layout; the sections added since now hold the state as it is"
            );
            journal.layout = delta::LAYOUT;
        }
    }

    /// Some account data changed (`count` entries, an estimate).
    ///
    /// The export is not rebuilt for it: the next tick looks when the batch
    /// spacing allows (`recheck_at_ms`), so a stream of small changes (Mora
    /// after every pickup, five a second in a fight) costs one export per
    /// spacing, not one per change. Before 1.17.27 every change zeroed the
    /// recheck time: ~3,000 full exports an hour on a real account.
    pub fn note_change(&mut self, count: usize) {
        self.pending = self.pending.saturating_add(count.max(1));
    }

    /// [`Self::note_change`] with when it came: the batch that carries it
    /// waits for the changes to rest ([`SETTLE_MS`], [`MAX_SETTLE_MS`]).
    pub fn note_change_at(&mut self, count: usize, now_ms: u64) {
        self.note_change(count);
        let first = self.changing_ms.map_or(now_ms, |(first, _)| first);
        self.changing_ms = Some((first, now_ms));
    }

    /// No batch sooner than this after the previous one.
    fn spacing_ms(&self) -> u64 {
        if self.watching {
            WATCHED_SPACING_MS
        } else {
            BATCH_SPACING_MS
        }
    }

    /// When the waiting changes have rested (0 with none timed).
    fn settled_ms(&self) -> u64 {
        self.changing_ms.map_or(0, |(first, last)| {
            (last + SETTLE_MS).min(first + MAX_SETTLE_MS)
        })
    }

    /// A change worth a batch at once came (a character's level, talent,
    /// constellation or gear, as the monitor saw it; see
    /// [`delta::is_important`]): look as soon as the spacing allows instead
    /// of at the next scheduled look.
    pub fn note_important_change(&mut self) {
        self.recheck_at_ms = 0;
    }

    /// The game connection ended (a logout, the game closing, a reconnect).
    /// When the journal holds anything, a logout snapshot of the data as it
    /// is goes up first, with the login that is ending.
    pub fn note_connection_ended(&mut self) {
        let Some(login) = self.login.take() else {
            return;
        };
        let journal_open = self.journal.as_ref().is_some_and(|journal| journal.seq > 0)
            || self.unacked.is_some()
            || self.pending > 0
            || self.batch_flight.is_some();
        if journal_open && self.journal.is_some() {
            let seq = self.journal.as_ref().map_or(0, |journal| journal.seq);
            self.logout_login = Some((login, seq));
            self.wanted = Some(Reason::Logout);
            self.ending = Ending::Open;
            self.kept_parts.clear();
        } else {
            // Nothing to send for it, and nothing can journal without a
            // login: the next one opens its own. Kept, it made every later
            // change (Mora after a pickup, five a second) build and diff the
            // whole export and find no login to batch it under, over and
            // over (a 2026-11-15 log: 330 exports a second for 1 h 42 min).
            self.journal = None;
            self.pending = 0;
            self.changing_ms = None;
        }
    }

    /// The captured data was replaced (a new connection's first data, or the
    /// user cleared it): nothing journals against the old state any more.
    pub fn note_data_replaced(&mut self) {
        self.journal = None;
        self.unacked = None;
        self.pending = 0;
        self.changing_ms = None;
        self.logout_login = None;
        self.attach_wanted = false;
        if self.wanted == Some(Reason::Logout) {
            self.wanted = None;
        }
        self.ending = Ending::Open;
    }

    /// The parts no batch carries that changed since the journal's base, and
    /// so kept the last ended login's logout snapshot although the tracker
    /// takes final batches (`unknown` for a base without fingerprints,
    /// `too big` / `no UID` for a final batch that could not be one, or the
    /// tracker's refusal of it). Empty when none was kept.
    pub fn logout_snapshot_kept(&self) -> &[String] {
        &self.kept_parts
    }

    /// The user changed which parts are shared. The tracker's base for this
    /// journal was made with the old set, so the journal cannot go on:
    /// nothing is batched until the switches were left alone for
    /// [`SHARING_QUIET_MS`], then one full snapshot (reason `resync`, with
    /// the new `gi_off`) starts a new journal.
    pub fn note_sharing_changed(&mut self, now_ms: u64) {
        self.sharing_changed_ms = Some(now_ms);
    }

    /// When a change of the shared parts is settled, if one is waiting.
    fn sharing_settles_ms(&self) -> Option<u64> {
        self.sharing_changed_ms.map(|at| at + SHARING_QUIET_MS)
    }

    /// The reason the debounced automation export uploads a snapshot with,
    /// or `None` when it should not upload one (live mode: batches carry the
    /// changes). `live_enabled`: the switch here and the tracker upload on.
    pub fn automation_reason(&self, live_enabled: bool, now_ms: u64) -> Option<Reason> {
        if live_enabled && self.is_live(now_ms) {
            return self.wanted.filter(|reason| reason.debounced());
        }
        Some(match self.wanted {
            Some(reason) => reason,
            None => Reason::Change,
        })
    }

    /// The `gi_capture` for a snapshot uploaded now for `reason`.
    pub fn capture(&self, reason: Reason) -> Option<GiCapture> {
        let (login, seq) = match (reason, &self.logout_login) {
            (Reason::Logout, Some((login, seq))) => (login, *seq),
            _ => (
                self.login.as_ref()?,
                self.journal.as_ref().map_or(0, |journal| journal.seq),
            ),
        };
        Some(GiCapture {
            v: 1,
            reason: reason.as_str().to_string(),
            session: login.session.clone(),
            world: login.world,
            journal: GiCaptureJournal {
                seq: if reason == Reason::Login { 0 } else { seq },
            },
        })
    }

    /// A snapshot of `keyed` went up for `reason`; the ticket names it in
    /// [`Self::snapshot_answered`] (a newer snapshot supersedes it).
    pub fn snapshot_started(&mut self, reason: Reason, keyed: Keyed) -> u64 {
        if self.wanted == Some(reason) || reason == Reason::Manual {
            self.wanted = None;
        }
        // The export just built holds the endgame parts as they are.
        self.attach_wanted = false;
        self.endgame_unacked = false;
        self.tickets += 1;
        self.snapshot_flight = Some(SnapshotFlight {
            ticket: self.tickets,
            reason,
            keyed,
        });
        self.tickets
    }

    /// Whether a request is in flight (the sender sends one at a time).
    pub fn busy(&self) -> bool {
        self.snapshot_flight.is_some() || self.batch_flight.is_some()
    }

    /// The snapshot upload `ticket` finished: `Some(answer)` when it was
    /// stored.
    pub fn snapshot_answered(&mut self, ticket: u64, answer: Option<ImportAnswer>, now_ms: u64) {
        if self.snapshot_flight.as_ref().map(|flight| flight.ticket) != Some(ticket) {
            return;
        }
        let Some(SnapshotFlight { reason, keyed, .. }) = self.snapshot_flight.take() else {
            return;
        };
        match answer {
            Some(answer) => {
                self.backoff_ms = 0;
                self.retry_at_ms = 0;
                self.unacked = None;
                self.watching = answer.watching;
                self.last_sent_ms = now_ms;
                if reason == Reason::Logout {
                    self.logout_login = None;
                    self.ending = Ending::Open;
                }
                // What came in while it was in flight is in the next diff.
                let unbatched = Some(keyed.unbatched.clone());
                self.journal = answer.base.map(|base| Journal {
                    base,
                    seq: 0,
                    keyed,
                    layout: delta::LAYOUT,
                    unbatched,
                });
            }
            None => {
                // The debounced ones are retried by the automation (its
                // signature is released); the others from here, after a wait.
                if reason.retried_here() {
                    self.wanted.get_or_insert(reason);
                    self.back_off(now_ms);
                }
            }
        }
    }

    fn back_off(&mut self, now_ms: u64) {
        self.backoff_ms = if self.backoff_ms == 0 {
            FIRST_BACKOFF_MS
        } else {
            (self.backoff_ms * 2).min(MAX_BACKOFF_MS)
        };
        self.retry_at_ms = now_ms + self.backoff_ms;
    }

    /// When [`Self::tick`] next has something to decide, or `None` when only
    /// new data (or an answer) can change that.
    pub fn next_tick_ms(&self, live_enabled: bool, now_ms: u64) -> Option<u64> {
        if self.busy() {
            return None;
        }
        if let Some(settles) = self.sharing_settles_ms() {
            return Some(settles.max(now_ms));
        }
        // A login ending on a final batch waits for the attach in flight:
        // its answer (which brings the next tick) decides what follows.
        if self.attach_flight
            && self.ending == Ending::FinalBatch
            && self.wanted == Some(Reason::Logout)
        {
            return None;
        }
        if let Some(reason) = self.wanted
            && !reason.debounced()
        {
            return Some(self.retry_at_ms.max(now_ms));
        }
        // Without a login there is nothing to batch or attach under: a tick
        // could only build an export to throw away.
        self.login.as_ref()?;
        if self.attach_wanted && !self.attach_flight && self.wanted.is_none() {
            return Some(self.attach_retry_at_ms.max(now_ms));
        }
        if !live_enabled || !self.is_live(now_ms) || self.wanted.is_some() {
            return None;
        }
        if self.unacked.is_some() {
            return Some(self.retry_at_ms.max(now_ms));
        }
        if self.pending == 0 {
            return None;
        }
        if self.pending >= BIG_BATCH {
            return Some(self.retry_at_ms.max(now_ms));
        }
        Some(
            (self.last_sent_ms + self.spacing_ms())
                .max(self.retry_at_ms)
                .max(self.recheck_at_ms)
                .max(self.settled_ms())
                .max(now_ms),
        )
    }

    /// Decide what to send now. `export` builds the current state when a
    /// batch may be due: the export (for its `sum` and UID) and its keyed
    /// form.
    ///
    /// `final_batch`: the tracker said it takes a final batch
    /// ([`accepts_final_batch`]). Then a login that ended with its journal
    /// open closes it with one last batch marked `end` (the ended login's
    /// session, everything not sent yet, possibly nothing) instead of the
    /// logout snapshot, unless a part no batch carries changed since the
    /// journal's base ([`Unbatched`]): that keeps the logout snapshot, as
    /// does any refusal of the final batch. Without the word, everything
    /// goes as it always did.
    pub fn tick(
        &mut self,
        live_enabled: bool,
        final_batch: bool,
        now_ms: u64,
        export: impl FnOnce() -> Option<(Good, Keyed)>,
    ) -> Action {
        self.final_word = final_batch;
        if self.busy() || now_ms < self.retry_at_ms {
            return Action::Nothing;
        }
        if let Some(settles) = self.sharing_settles_ms() {
            // Nothing goes against the old set's journal meanwhile.
            if now_ms < settles {
                return Action::Nothing;
            }
            self.sharing_changed_ms = None;
            // Without a journal the next snapshot is whole anyway.
            if self.journal.is_some() {
                self.journal = None;
                self.unacked = None;
                self.pending = 0;
                self.changing_ms = None;
                if self.wanted.is_none_or(Reason::debounced) {
                    self.wanted = Some(Reason::Resync);
                }
            }
        }
        let mut export = Some(export);
        // Only while batches go at all (the switch on, no daily limit hit).
        if final_batch
            && live_enabled
            && self.is_live(now_ms)
            && self.wanted == Some(Reason::Logout)
            && self.ending != Ending::Snapshot
            && let Some(action) = self.end_with_final_batch(now_ms, &mut export)
        {
            return action;
        }
        if let Some(reason) = self.wanted
            && !reason.debounced()
        {
            return Action::Snapshot(reason);
        }
        if self.attach_due(now_ms)
            && let Some(login) = &self.login
        {
            self.attach_wanted = false;
            self.attach_flight = true;
            return Action::Attach {
                session: login.session.clone(),
            };
        }
        if !live_enabled || !self.is_live(now_ms) || self.wanted.is_some() {
            return Action::Nothing;
        }
        if self.login.is_none() {
            self.pending = 0;
            self.changing_ms = None;
            return Action::Nothing;
        }
        if let Some(flight) = self.unacked.take() {
            let delta = flight.delta.clone();
            self.batch_flight = Some(flight);
            return Action::Batch(Box::new(delta));
        }
        // A hand-over's journal from an older layout: seed what it never
        // keyed from this export before anything is diffed.
        let mut exported = None;
        if self.needs_seed() {
            let Some(current) = export.take().and_then(|export| export()) else {
                return Action::Nothing;
            };
            self.seed(&current.1);
            exported = Some(current);
        }
        if self.pending == 0 {
            return Action::Nothing;
        }
        let since = now_ms.saturating_sub(self.last_sent_ms);
        let spacing = self.spacing_ms();
        if self.pending < BIG_BATCH
            && (since < spacing || now_ms < self.recheck_at_ms || now_ms < self.settled_ms())
        {
            return Action::Nothing;
        }
        self.changing_ms = None;
        let Some((good, keyed)) = exported.or_else(|| export.take().and_then(|export| export()))
        else {
            return Action::Nothing;
        };
        let journal = self.journal.as_ref().expect("is_live checked it");
        let changes = delta::diff(&journal.keyed, &keyed);
        if changes.is_empty() {
            self.pending = 0;
            self.changing_ms = None;
            return Action::Nothing;
        }
        let due = changes.len() >= BIG_BATCH
            || changes.len() >= NORMAL_BATCH
            || self.watching
            || since >= LONE_CHANGE_MS
            || delta::is_important(&changes, &keyed);
        if !due {
            // Look again in a spacing, or when the lone-change wait is up.
            self.recheck_at_ms = (now_ms + spacing).min(self.last_sent_ms + LONE_CHANGE_MS);
            return Action::Nothing;
        }
        let Some(uid) = good.gi_player.as_ref().and_then(|player| player.uid) else {
            // The tracker routes a batch by its UID; without one, snapshots.
            self.journal = None;
            return Action::Nothing;
        };
        let Some(login) = &self.login else {
            return Action::Nothing;
        };
        let mut batch = GoodDelta::new(uid, &login.session, journal.seq + 1, &journal.base, now_ms);
        batch.changes = changes;
        batch.sum = Some(Sum::of(&good));
        if batch.changes.len() > delta::MAX_CHANGES
            || serde_json::to_vec(&batch).map_or(usize::MAX, |json| json.len()) > delta::MAX_BYTES
        {
            // Too much for a batch: the whole state is cheaper.
            self.wanted = Some(Reason::Collapse);
            return Action::Snapshot(Reason::Collapse);
        }
        self.batch_flight = Some(BatchFlight {
            delta: batch.clone(),
            keyed,
            pending_at_build: self.pending,
        });
        Action::Batch(Box::new(batch))
    }

    /// The ended login's journal closes on a final batch: decided once
    /// (`None` then means the logout snapshot, `self.ending` remembers it),
    /// then the attach the login still owes, an unacked batch resent
    /// unchanged, and the final batch itself, one per tick.
    fn end_with_final_batch<F>(&mut self, now_ms: u64, export: &mut Option<F>) -> Option<Action>
    where
        F: FnOnce() -> Option<(Good, Keyed)>,
    {
        let journal = self.journal.as_ref()?;
        let (login, _) = self.logout_login.as_ref()?;
        let session = login.session.clone();
        let mut exported = None;
        if self.ending == Ending::Open {
            let Some((good, keyed)) = export.take().and_then(|export| export()) else {
                return Some(Action::Nothing);
            };
            let kept = match &journal.unbatched {
                Some(base) => keyed.unbatched.changed_since(base),
                None => vec!["unknown".to_string()],
            };
            if !kept.is_empty() {
                return self.keep_logout_snapshot(kept);
            }
            self.ending = Ending::FinalBatch;
            exported = Some((good, keyed));
        }
        // The endgame parts the logout snapshot would have carried: never
        // overtaken by the final batch. One in flight is waited for.
        if self.attach_flight {
            return Some(Action::Nothing);
        }
        if self.attach_wanted {
            // Never tried: it goes now, and its answer comes first.
            if self.attach_backoff_ms == 0 && self.attach_retry_at_ms == 0 {
                self.attach_wanted = false;
                self.attach_flight = true;
                return Some(Action::Attach { session });
            }
            // It failed before (or was answered Retry while ending): the
            // logout snapshot carries the parts instead.
            return self.keep_logout_snapshot(vec!["attach".to_string()]);
        }
        // Dropped by the tracker (or never sent): nothing else carries them.
        if self.endgame_unacked {
            return self.keep_logout_snapshot(vec!["gi_endgame".to_string()]);
        }
        if let Some(flight) = self.unacked.take() {
            let delta = flight.delta.clone();
            self.batch_flight = Some(flight);
            return Some(Action::Batch(Box::new(delta)));
        }
        let Some((good, keyed)) = exported.or_else(|| export.take().and_then(|export| export()))
        else {
            return Some(Action::Nothing);
        };
        let Some(uid) = good.gi_player.as_ref().and_then(|player| player.uid) else {
            return self.keep_logout_snapshot(vec!["no UID".to_string()]);
        };
        let journal = self.journal.as_ref()?;
        let mut batch = GoodDelta::new(uid, &session, journal.seq + 1, &journal.base, now_ms);
        batch.changes = delta::diff(&journal.keyed, &keyed);
        batch.sum = Some(Sum::of(&good));
        batch.end = Some(true);
        if batch.changes.len() > delta::MAX_CHANGES
            || serde_json::to_vec(&batch).map_or(usize::MAX, |json| json.len()) > delta::MAX_BYTES
        {
            return self.keep_logout_snapshot(vec!["too big".to_string()]);
        }
        tracing::info!(
            seq = batch.seq,
            changes = batch.changes.len(),
            "live sync: the login ended; its journal closes with a final batch"
        );
        self.batch_flight = Some(BatchFlight {
            delta: batch.clone(),
            keyed,
            pending_at_build: self.pending,
        });
        Some(Action::Batch(Box::new(batch)))
    }

    /// The ended login closes on its logout snapshot after all, because of
    /// `parts`: `None`, so the tick goes on to it.
    fn keep_logout_snapshot(&mut self, parts: Vec<String>) -> Option<Action> {
        tracing::info!("live sync: logout snapshot kept: {}", parts.join(", "));
        self.ending = Ending::Snapshot;
        self.kept_parts = parts;
        None
    }

    /// The final batch was stored: the ended login's journal is closed.
    fn closed(&mut self, now_ms: u64) {
        self.journal = None;
        self.logout_login = None;
        if self.wanted == Some(Reason::Logout) {
            self.wanted = None;
        }
        self.ending = Ending::Open;
        self.unacked = None;
        self.pending = 0;
        self.changing_ms = None;
        self.recheck_at_ms = 0;
        self.last_sent_ms = now_ms;
        self.backoff_ms = 0;
        self.retry_at_ms = 0;
    }

    /// The batch request finished with `answer`.
    pub fn batch_answered(&mut self, answer: BatchAnswer, now_ms: u64) {
        let Some(BatchFlight {
            delta,
            keyed,
            pending_at_build,
        }) = self.batch_flight.take()
        else {
            return;
        };
        // A snapshot since started a new journal: this batch's is gone.
        if self.journal.as_ref().map(|journal| journal.base.as_str()) != Some(delta.base.as_str()) {
            return;
        }
        if delta.end == Some(true) {
            match answer {
                // Closed: nothing more for that login (a collapse or a tab
                // watching no longer matters).
                BatchAnswer::Stored { .. } | BatchAnswer::Duplicate => self.closed(now_ms),
                BatchAnswer::Retry => {
                    self.unacked = Some(BatchFlight {
                        delta,
                        keyed,
                        pending_at_build,
                    });
                    self.back_off(now_ms);
                }
                refused => {
                    // The logout snapshot at the next tick, as without the word.
                    if refused == BatchAnswer::DailyLimit {
                        self.off_day = Some(now_ms / DAY_MS);
                    }
                    self.journal = None;
                    self.backoff_ms = 0;
                    self.retry_at_ms = 0;
                    self.ending = Ending::Snapshot;
                    self.kept_parts = vec![format!("refused ({refused:?})")];
                    tracing::info!(answer = ?refused, "live sync: the final batch was refused");
                }
            }
            return;
        }
        match answer {
            BatchAnswer::Stored { collapse, watching } => {
                self.watching = watching;
                // A login ending on a final batch needs no collapse.
                if collapse && !(self.final_word && self.logout_login.is_some()) {
                    self.wanted = Some(Reason::Collapse);
                }
                self.stored(delta.seq, keyed, pending_at_build, now_ms);
            }
            BatchAnswer::Duplicate => self.stored(delta.seq, keyed, pending_at_build, now_ms),
            BatchAnswer::LiveOff => {
                self.journal = None;
                self.wanted = Some(Reason::Change);
                self.backoff_ms = 0;
                self.retry_at_ms = 0;
            }
            BatchAnswer::Resync => {
                self.journal = None;
                self.wanted = Some(Reason::Resync);
                self.backoff_ms = 0;
                self.retry_at_ms = 0;
            }
            BatchAnswer::DailyLimit => {
                self.off_day = Some(now_ms / DAY_MS);
                self.journal = None;
                self.wanted = Some(Reason::Change);
                self.backoff_ms = 0;
                self.retry_at_ms = 0;
            }
            BatchAnswer::Retry => {
                self.unacked = Some(BatchFlight {
                    delta,
                    keyed,
                    pending_at_build,
                });
                self.back_off(now_ms);
            }
            BatchAnswer::Refused => {
                // Snapshot mode: the next change uploads whole, and that
                // upload's failure is what the user sees.
                self.journal = None;
            }
        }
    }

    fn stored(&mut self, seq: u64, keyed: Keyed, pending_at_build: usize, now_ms: u64) {
        if let Some(journal) = &mut self.journal {
            journal.seq = seq;
            journal.keyed = keyed;
        }
        self.pending = self.pending.saturating_sub(pending_at_build);
        self.last_sent_ms = now_ms;
        self.backoff_ms = 0;
        self.retry_at_ms = 0;
    }

    /// Everything not sent yet as one batch, taken as stored at once:
    /// `--replay-journal` uses it before each snapshot and at the end, so
    /// the snapshot plus the batches can be checked against the whole state.
    /// `None` when not live or nothing changed.
    pub fn flush(&mut self, now_ms: u64, good: &Good, keyed: Keyed) -> Option<GoodDelta> {
        if self.busy() || !self.is_live(now_ms) {
            return None;
        }
        let journal = self.journal.as_ref()?;
        let changes = delta::diff(&journal.keyed, &keyed);
        if changes.is_empty() {
            return None;
        }
        let uid = good.gi_player.as_ref().and_then(|player| player.uid)?;
        let session = self
            .login
            .as_ref()
            .or(self.logout_login.as_ref().map(|(login, _)| login))?
            .session
            .clone();
        let mut batch = GoodDelta::new(uid, &session, journal.seq + 1, &journal.base, now_ms);
        batch.changes = changes;
        batch.sum = Some(Sum::of(good));
        self.unacked = None;
        let pending = self.pending;
        self.stored(batch.seq, keyed, pending, now_ms);
        Some(batch)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::delta::Change;
    use crate::good::GiPlayer;

    fn good(mora: u32) -> Good {
        let mut good: Good = serde_json::from_value(serde_json::json!({
            "format": "GOOD", "version": 3, "source": "Teyvat.me",
            "characters": [], "artifacts": [], "weapons": [], "materials": {}
        }))
        .unwrap();
        good.materials = HashMap::from([("Mora".to_string(), mora)]);
        good.gi_player = Some(GiPlayer {
            uid: Some(812_345_678),
            ..Default::default()
        });
        good
    }

    fn with_materials(count: usize) -> Good {
        let mut good = good(1);
        for i in 0..count {
            good.materials.insert(format!("Thing{i}"), 1);
        }
        good
    }

    #[test]
    fn gzip_is_sent_only_to_a_tracker_that_says_so() {
        let yes = |json: &str| accepts_gzip(&serde_json::from_str(json).unwrap());
        assert!(yes(r#"{"upload":{"gzip":true,"snapshotParts":1}}"#));
        assert!(yes(r#"{"status":200,"data":{"upload":{"gzip":true}}}"#));
        assert!(!yes(r#"{"upload":{"gzip":false}}"#));
        assert!(!yes(r#"{"upload":{"gzip":"true"}}"#));
        assert!(!yes(r#"{"upload":true}"#));
        assert!(!yes(r#"{"live":{"base":"abc"}}"#));
        assert!(!yes("[]"));
    }

    fn keyed(good: &Good) -> Keyed {
        Keyed::of(good, |_| None)
    }

    fn export(good: Good) -> impl FnOnce() -> Option<(Good, Keyed)> {
        move || {
            let keyed = keyed(&good);
            Some((good, keyed))
        }
    }

    /// Logged in, snapshot stored with a journal; the clock at `t`.
    fn live_at(t: u64) -> LiveSync {
        let mut live = LiveSync::new();
        live.note_full_send();
        assert_eq!(live.automation_reason(true, t), Some(Reason::Login));
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(100)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k1".into()),
                watching: false,
            }),
            t,
        );
        assert!(live.is_live(t));
        live
    }

    fn sent(action: Action) -> GoodDelta {
        match action {
            Action::Batch(delta) => *delta,
            other => panic!("expected a batch, got {other:?}"),
        }
    }

    #[test]
    fn a_handed_over_journal_continues_in_a_fresh_sender() {
        let mut live = live_at(0);
        live.note_change(5);
        let batch = sent(live.tick(true, false, 15_000, export(with_materials(5))));
        assert_eq!(batch.seq, 1);
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: false,
            },
            15_100,
        );
        assert_eq!(LiveSync::new().handover(), None, "nothing before a login");

        live.note_endgame_change();
        let handover = live.handover().expect("a logged-in sender hands over");
        assert_eq!(
            handover.journal.as_ref().unwrap().layout,
            Some(delta::LAYOUT)
        );
        assert!(handover.attach_wanted && !handover.watching);
        let json = serde_json::to_vec(&handover).unwrap();
        let back: LiveHandover = serde_json::from_slice(&json).unwrap();
        assert_eq!(back, handover);

        // The next process: same session, same journal, the next batch is
        // seq 2 against what the tracker holds (no resync snapshot).
        let mut resumed = LiveSync::new();
        assert!(resumed.resume(back.clone()));
        assert!(!resumed.resume(back), "a sender with a login keeps it");
        assert!(resumed.is_live(15_200));
        assert!(!resumed.needs_seed(), "the same layout: nothing to seed");
        assert!(resumed.attach_wanted && !resumed.watching);
        assert_eq!(resumed.automation_reason(true, 15_200), None);
        // The pending attach goes first.
        assert!(matches!(
            resumed.tick(true, false, 15_200, || panic!("no export for an attach")),
            Action::Attach { .. }
        ));
        resumed.attach_answered(AttachAnswer::Stored, 15_300);
        // One small change: due after the lone-change wait, as within a run.
        resumed.note_change(1);
        assert_eq!(
            resumed.tick(true, false, 30_200, export(with_materials(6))),
            Action::Nothing
        );
        let batch = sent(resumed.tick(true, false, 140_000, export(with_materials(6))));
        assert_eq!(batch.seq, 2);
        assert_eq!(batch.base, "k1");
        assert_eq!(batch.changes.len(), 1, "only the one material that moved");
        assert_eq!(
            resumed.capture(Reason::World).unwrap().session,
            live.capture(Reason::World).unwrap().session
        );
    }

    #[test]
    fn a_journal_keyed_by_an_older_layout_is_seeded_not_collapsed() {
        let mut live = live_at(0);
        live.note_change(5);
        let batch = sent(live.tick(true, false, 15_000, export(with_materials(5))));
        assert_eq!(batch.seq, 1);
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: true,
            },
            15_100,
        );
        let mut handover = live.handover().unwrap();
        // Written by 1.17.24: keyed before the quests had entries.
        handover.journal.as_mut().unwrap().layout = Some(1);
        let json = serde_json::to_vec(&handover).unwrap();
        let back: LiveHandover = serde_json::from_slice(&json).unwrap();

        let mut resumed = LiveSync::new();
        assert!(resumed.resume(back));
        assert!(resumed.needs_seed());
        assert!(resumed.watching);
        // The export holds 2,452 quests the old journal never keyed, and one
        // material that moved in the restart gap: the quests are seeded, the
        // material is the batch (not 2,453 changes and a collapse).
        let mut current = with_materials(5);
        current.materials.insert("Thing0".to_owned(), 2);
        current.gi_collections = Some(crate::good::GiCollections {
            quests: Some(crate::good::GiQuests {
                finished: (1..=2_160).collect(),
                active: (10_001..=10_292).collect(),
            }),
            ..Default::default()
        });
        resumed.note_change(1);
        let batch = sent(resumed.tick(true, false, 140_000, export(current.clone())));
        assert_eq!(batch.seq, 2);
        assert_eq!(batch.base, "k1");
        assert_eq!(batch.changes.len(), 1, "the material alone");
        assert!(!resumed.needs_seed());
        // From here a finished quest is one change, as within a run.
        resumed.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: true,
            },
            140_100,
        );
        let mut later = current.clone();
        let quests = later
            .gi_collections
            .as_mut()
            .unwrap()
            .quests
            .as_mut()
            .unwrap();
        quests.finished.push(10_001);
        quests.active.retain(|id| *id != 10_001);
        resumed.note_change(1);
        let batch = sent(resumed.tick(true, false, 160_000, export(later)));
        assert_eq!(batch.seq, 3);
        assert_eq!(batch.changes.len(), 1, "the one quest that finished");

        // No layout recorded (written by 1.17.25 or before): layout 1.
        let mut json: serde_json::Value = serde_json::to_value(&handover).unwrap();
        json["journal"].as_object_mut().unwrap().remove("layout");
        let old: LiveHandover = serde_json::from_value(json).unwrap();
        let mut fresh = LiveSync::new();
        assert!(fresh.resume(old));
        assert!(fresh.needs_seed());
        // A newer layout than ours (a downgrade) is left to the diff.
        handover.journal.as_mut().unwrap().layout = Some(delta::LAYOUT + 1);
        let mut downgraded = LiveSync::new();
        assert!(downgraded.resume(handover));
        assert!(!downgraded.needs_seed());
    }

    #[test]
    fn a_stream_of_small_changes_builds_one_export_per_spacing() {
        use std::cell::Cell;
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: false,
            },
            15_100,
        );
        // Mora moves after every pickup: a change every 200 ms for a minute,
        // each a tick. The export is built at most once per batch spacing,
        // not once per change (the whole account, keyed and diffed each time).
        let builds = Cell::new(0);
        let mut good = with_materials(5);
        for i in 0..300u64 {
            let now = 15_200 + i * 200;
            good.materials.insert("Mora".to_owned(), 1_000 + i as u32);
            live.note_change(1);
            let action = live.tick(true, false, now, || {
                builds.set(builds.get() + 1);
                Some((good.clone(), keyed(&good)))
            });
            assert_eq!(
                action,
                Action::Nothing,
                "a lone material change is not due at {now}"
            );
        }
        // 60 s of changes: the export is looked at every 15 s at most (the
        // pending estimate passes NORMAL_BATCH, so each look diffs).
        assert!(
            builds.get() <= 4,
            "built {} exports in a minute",
            builds.get()
        );
        // The lone-change wait is up: one batch with the material as it is.
        let batch = sent(live.tick(true, false, 15_100 + LONE_CHANGE_MS, export(good.clone())));
        assert_eq!(batch.changes.len(), 1);
    }

    #[test]
    fn a_connection_that_ended_with_nothing_open_builds_no_more_exports() {
        use std::cell::Cell;
        let mut live = live_at(0);
        // The game closes (or the connection drops) before any batch: the
        // journal had nothing open, so no logout snapshot is wanted.
        live.note_connection_ended();
        assert_eq!(live.automation_reason(true, 1_000), Some(Reason::Change));
        // Property updates keep arriving (the next account's login, say).
        let builds = Cell::new(0);
        for i in 0..100u64 {
            live.note_change(1);
            let now = 20_000 + i * 3;
            assert_eq!(live.next_tick_ms(true, now), None, "nothing to wake for");
            let action = live.tick(true, false, now, || {
                builds.set(builds.get() + 1);
                Some((
                    with_materials(i as usize + 1),
                    keyed(&with_materials(i as usize + 1)),
                ))
            });
            assert_eq!(action, Action::Nothing);
        }
        assert_eq!(builds.get(), 0, "no export without a login");
        // The next login's full send opens a new journal as before.
        live.note_full_send();
        assert_eq!(live.automation_reason(true, 30_000), Some(Reason::Login));
    }

    #[test]
    fn an_open_journal_still_sends_its_logout_snapshot() {
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: false,
            },
            15_100,
        );
        live.note_connection_ended();
        assert_eq!(
            live.tick(true, false, 15_200, || panic!(
                "the snapshot builds its own"
            )),
            Action::Snapshot(Reason::Logout)
        );
    }

    #[test]
    fn afk_sends_nothing() {
        let mut live = live_at(0);
        assert_eq!(live.next_tick_ms(true, 1_000), None);
        assert_eq!(
            live.tick(true, false, 10_000_000, || panic!("no export")),
            Action::Nothing
        );
    }

    #[test]
    fn a_normal_batch_waits_15_s_for_5_changes() {
        let mut live = live_at(0);
        live.note_change(5);
        assert_eq!(live.next_tick_ms(true, 1_000), Some(BATCH_SPACING_MS));
        assert_eq!(
            live.tick(true, false, 14_999, export(with_materials(5))),
            Action::Nothing
        );
        let batch = sent(live.tick(true, false, 15_000, export(with_materials(5))));
        assert_eq!(batch.seq, 1);
        assert_eq!(batch.base, "k1");
        assert_eq!(batch.uid, 812_345_678);
        assert_eq!(batch.sum.unwrap().materials, 6);
        assert!(live.busy());
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: false,
            },
            15_100,
        );
        assert_eq!(live.seq(), Some(1));
        assert_eq!(live.next_tick_ms(true, 15_200), None);
    }

    #[test]
    fn a_few_changes_wait_for_two_minutes() {
        let mut live = live_at(0);
        live.note_change(1);
        assert_eq!(
            live.tick(true, false, 20_000, export(good(101))),
            Action::Nothing
        );
        assert_eq!(
            live.tick(true, false, 119_999, export(good(101))),
            Action::Nothing
        );
        let batch = sent(live.tick(true, false, 120_000, export(good(101))));
        assert_eq!(batch.changes.len(), 1);
    }

    #[test]
    fn an_undue_change_is_looked_at_once_per_spacing() {
        let mut live = live_at(0);
        live.note_change(1);
        assert_eq!(
            live.tick(true, false, 20_000, export(good(101))),
            Action::Nothing
        );
        // Not due yet: the next look waits a spacing, instead of the monitor
        // waking at once and rebuilding the export until the change is due.
        assert_eq!(live.next_tick_ms(true, 20_000), Some(35_000));
        assert_eq!(
            live.tick(true, false, 25_000, || panic!(
                "no export before the next look"
            )),
            Action::Nothing
        );
        // New data waits for that look too: looking means building and
        // diffing the whole account, and a fight moves Mora five times a
        // second (before 1.17.27 every change was looked at at once: ~3,000
        // exports an hour on a real account).
        live.note_change(1);
        assert_eq!(live.next_tick_ms(true, 25_000), Some(35_000));
        assert_eq!(
            live.tick(true, false, 25_000, || panic!(
                "no export before the next look"
            )),
            Action::Nothing
        );
        assert_eq!(
            live.tick(true, false, 35_000, export(good(101))),
            Action::Nothing
        );
        assert_eq!(live.next_tick_ms(true, 35_000), Some(50_000));
        // A character change is looked at as soon as the spacing allows.
        live.note_important_change();
        assert_eq!(live.next_tick_ms(true, 36_000), Some(36_000));
        assert_eq!(
            live.tick(true, false, 36_000, export(good(101))),
            Action::Nothing
        );
        let batch = sent(live.tick(true, false, 120_000, export(good(101))));
        assert_eq!(batch.changes.len(), 1);
    }

    #[test]
    fn someone_watching_gets_every_change_after_5_s() {
        let mut live = LiveSync::new();
        live.note_full_send();
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(100)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k1".into()),
                watching: true,
            }),
            0,
        );
        live.note_change(1);
        assert_eq!(
            live.tick(true, false, 4_999, export(good(101))),
            Action::Nothing
        );
        assert_eq!(
            sent(live.tick(true, false, 5_000, export(good(101))))
                .changes
                .len(),
            1
        );
    }

    #[test]
    fn someone_watching_gets_changes_every_5_s_and_a_reward_whole() {
        let mut live = LiveSync::new();
        live.note_full_send();
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(100)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k1".into()),
                watching: true,
            }),
            0,
        );
        // A reward's packets come over 300 ms, well after the last batch:
        // nothing goes while they do, and one batch goes 1 s after the last.
        live.note_change_at(1, 20_000);
        assert_eq!(
            live.tick(true, false, 20_000, export(good(101))),
            Action::Nothing
        );
        assert_eq!(live.next_tick_ms(true, 20_000), Some(21_000));
        live.note_change_at(1, 20_300);
        assert_eq!(
            live.tick(true, false, 21_000, export(good(102))),
            Action::Nothing
        );
        assert_eq!(live.next_tick_ms(true, 21_000), Some(21_300));
        let batch = sent(live.tick(true, false, 21_300, export(good(102))));
        assert_eq!(batch.changes.len(), 1);
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: true,
            },
            21_400,
        );
        // The next one is 5 s after that, not 15.
        live.note_change_at(1, 22_000);
        let next = live.next_tick_ms(true, 22_000).unwrap();
        assert!((26_300..=26_400).contains(&next), "{next}");
        assert_eq!(
            live.tick(true, false, next - 1, export(good(103))),
            Action::Nothing
        );
        sent(live.tick(true, false, next, export(good(103))));
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: false,
            },
            next + 100,
        );
        // Nobody watching any more: 15 s again.
        live.note_change_at(5, next + 600);
        assert!(live.next_tick_ms(true, next + 600).unwrap() >= next + 15_000);
    }

    #[test]
    fn changes_that_never_rest_hold_a_batch_3_s_at_most() {
        let mut live = live_at(0);
        // Mora every 200 ms in a long fight, long after the last batch.
        let mut now = 200_000;
        while now < 202_900 {
            live.note_change_at(1, now);
            assert_eq!(
                live.tick(true, false, now, export(good(101))),
                Action::Nothing
            );
            now += 200;
        }
        live.note_change_at(5, 202_900);
        assert_eq!(live.next_tick_ms(true, 202_900), Some(203_000));
        sent(live.tick(true, false, 203_000, export(good(101))));
    }

    #[test]
    fn an_important_change_goes_after_15_s() {
        let mut live = live_at(0);
        let mut levelled = good(100);
        levelled.characters.push(crate::good::Character {
            key: "Furina".into(),
            level: 90,
            constellation: 0,
            ascension: 6,
            talent: crate::good::TalentLevel {
                auto: 1,
                skill: 1,
                burst: 1,
            },
        });
        live.note_change(1);
        let batch = sent(live.tick(true, false, 15_000, export(levelled)));
        assert!(matches!(batch.changes[0], Change::Add(..)));
    }

    #[test]
    fn a_big_burst_goes_at_once() {
        let mut live = live_at(0);
        live.note_change(BIG_BATCH);
        assert_eq!(live.next_tick_ms(true, 1_000), Some(1_000));
        // 2,000 distinct changes is a whole snapshot's worth: a collapse.
        assert_eq!(
            live.tick(true, false, 1_000, export(with_materials(2_001))),
            Action::Snapshot(Reason::Collapse)
        );
    }

    #[test]
    fn nothing_changed_after_all_clears_the_estimate() {
        let mut live = live_at(0);
        live.note_change(3);
        assert_eq!(
            live.tick(true, false, 200_000, export(good(100))),
            Action::Nothing
        );
        assert_eq!(live.next_tick_ms(true, 200_000), None);
    }

    #[test]
    fn a_failed_batch_is_resent_unchanged_with_backoff() {
        let mut live = live_at(0);
        live.note_change(5);
        let first = sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(BatchAnswer::Retry, 15_000);
        assert_eq!(live.next_tick_ms(true, 15_001), Some(30_000));
        // More changes meanwhile: the retry is still exactly the first batch.
        live.note_change(5);
        assert_eq!(
            live.tick(true, false, 29_999, export(with_materials(9))),
            Action::Nothing
        );
        let again = sent(live.tick(true, false, 30_000, || panic!("the retry needs no export")));
        assert_eq!(again, first);
        live.batch_answered(BatchAnswer::Retry, 30_000);
        assert_eq!(live.next_tick_ms(true, 30_000), Some(60_000));
        sent(live.tick(true, false, 60_000, || None));
        live.batch_answered(BatchAnswer::Retry, 60_000);
        assert_eq!(live.next_tick_ms(true, 60_000), Some(120_000));
        sent(live.tick(true, false, 120_000, || None));
        live.batch_answered(BatchAnswer::Retry, 120_000);
        // Capped at two minutes.
        assert_eq!(live.next_tick_ms(true, 120_000), Some(240_000));
        sent(live.tick(true, false, 240_000, || None));
        live.batch_answered(BatchAnswer::Duplicate, 240_000);
        assert_eq!(live.seq(), Some(1));
        // The changes made meanwhile follow as seq 2 (four: a lone-change wait).
        assert_eq!(
            live.tick(true, false, 255_000, export(with_materials(9))),
            Action::Nothing
        );
        let next = sent(live.tick(true, false, 360_000, export(with_materials(9))));
        assert_eq!(next.seq, 2);
        assert_eq!(next.changes.len(), 4);
    }

    #[test]
    fn changed_sharing_starts_one_new_journal_once_the_switches_rest() {
        let mut live = live_at(0);
        live.note_change(5);
        // Three flips in a row: the quiet time counts from the last.
        live.note_sharing_changed(20_000);
        live.note_sharing_changed(22_000);
        live.note_sharing_changed(25_000);
        assert_eq!(
            live.next_tick_ms(true, 25_000),
            Some(25_000 + SHARING_QUIET_MS)
        );
        // Meanwhile nothing goes against the old set's journal, however
        // due the batch is, and no export is built for it.
        assert_eq!(
            live.tick(true, false, 30_000, || panic!(
                "no export while the switches settle"
            )),
            Action::Nothing
        );
        assert!(live.is_live(30_000));
        // Then one full snapshot (it carries the new gi_off), not a batch.
        assert_eq!(
            live.tick(true, false, 25_000 + SHARING_QUIET_MS, || None),
            Action::Snapshot(Reason::Resync)
        );
        assert!(!live.is_live(25_000 + SHARING_QUIET_MS));
        let ticket = live.snapshot_started(Reason::Resync, keyed(&with_materials(5)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k2".into()),
                watching: false,
            }),
            36_000,
        );
        // One new journal, and the change is done with.
        assert!(live.is_live(36_000));
        assert_eq!(live.tick(true, false, 36_001, || None), Action::Nothing);

        // In snapshot mode there is no journal to replace: the next
        // snapshot is whole anyway.
        let mut snapshots = LiveSync::new();
        snapshots.note_sharing_changed(0);
        assert_eq!(
            snapshots.tick(true, false, SHARING_QUIET_MS, || None),
            Action::Nothing
        );
        assert_eq!(snapshots.next_tick_ms(true, SHARING_QUIET_MS + 1), None);
    }

    #[test]
    fn the_trackers_answers_steer_the_sender() {
        // collapse: true asks for a snapshot now.
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: true,
                watching: false,
            },
            15_000,
        );
        assert_eq!(
            live.tick(true, false, 15_001, || None),
            Action::Snapshot(Reason::Collapse)
        );

        // resync: a snapshot, and no batches until its answer.
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(BatchAnswer::Resync, 15_000);
        assert!(!live.is_live(15_000));
        assert_eq!(
            live.tick(true, false, 15_001, || None),
            Action::Snapshot(Reason::Resync)
        );
        let ticket = live.snapshot_started(Reason::Resync, keyed(&with_materials(5)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k2".into()),
                watching: false,
            }),
            16_000,
        );
        assert!(live.is_live(16_000));

        // live: false means snapshot mode, starting with a snapshot.
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(BatchAnswer::LiveOff, 15_000);
        assert!(!live.is_live(15_000));
        assert_eq!(
            live.tick(true, false, 15_001, || None),
            Action::Snapshot(Reason::Change)
        );
        live.snapshot_started(Reason::Change, keyed(&with_materials(5)));
        assert_eq!(live.automation_reason(true, 15_000), Some(Reason::Change));

        // The daily limit: snapshot mode for the rest of the UTC day.
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, false, 15_000, export(with_materials(5))));
        live.batch_answered(BatchAnswer::DailyLimit, 15_000);
        let ticket = live.snapshot_started(Reason::Change, keyed(&with_materials(5)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k3".into()),
                watching: false,
            }),
            16_000,
        );
        assert!(!live.is_live(16_000));
        assert!(live.is_live(DAY_MS + 1));
    }

    #[test]
    fn snapshot_mode_without_the_trackers_consent() {
        let mut live = LiveSync::new();
        live.note_full_send();
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(100)));
        // An older tracker: no `live` in the answer.
        live.snapshot_answered(ticket, Some(ImportAnswer::default()), 0);
        assert!(!live.is_live(0));
        live.note_change(5);
        assert_eq!(
            live.tick(true, false, 100_000, || panic!("no batches")),
            Action::Nothing
        );
        assert_eq!(live.automation_reason(true, 100_000), Some(Reason::Change));
        // The switch off: always snapshot mode.
        let live = live_at(0);
        assert_eq!(live.automation_reason(false, 1), Some(Reason::Change));
    }

    #[test]
    fn the_capture_reasons_of_a_session() {
        let mut live = LiveSync::new();
        // Login.
        live.note_full_send();
        let login = live.capture(Reason::Login).unwrap();
        assert_eq!(
            (login.reason.as_str(), login.world, login.journal.seq),
            ("login", 1, 0)
        );
        assert!(login.session.len() >= 16);
        assert!(
            login
                .session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(1)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k1".into()),
                watching: false,
            }),
            0,
        );
        // Two world changes in the same connection: no snapshot while the
        // journal is open (their changes go as batches), but counted.
        for world in [2, 3] {
            live.note_full_send();
            assert_eq!(live.automation_reason(true, 1), None);
            let capture = live.capture(Reason::World).unwrap();
            assert_eq!((capture.reason.as_str(), capture.world), ("world", world));
            assert_eq!(capture.session, login.session);
        }
        // A batch, then the tracker asks for a collapse.
        live.note_change(5);
        sent(live.tick(true, false, 20_000, export(with_materials(5))));
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: true,
                watching: false,
            },
            20_000,
        );
        assert_eq!(
            live.tick(true, false, 20_001, || None),
            Action::Snapshot(Reason::Collapse)
        );
        let collapse = live.capture(Reason::Collapse).unwrap();
        assert_eq!(
            (collapse.reason.as_str(), collapse.journal.seq),
            ("collapse", 1)
        );
        let ticket = live.snapshot_started(Reason::Collapse, keyed(&with_materials(5)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k4".into()),
                watching: false,
            }),
            21_000,
        );
        // A change, then the game closes: a logout snapshot with this login.
        live.note_change(1);
        live.note_connection_ended();
        assert_eq!(
            live.tick(true, false, 21_001, || None),
            Action::Snapshot(Reason::Logout)
        );
        let logout = live.capture(Reason::Logout).unwrap();
        assert_eq!((logout.reason.as_str(), logout.world), ("logout", 3));
        assert_eq!(logout.session, login.session);
        let ticket = live.snapshot_started(Reason::Logout, keyed(&with_materials(6)));
        live.snapshot_answered(ticket, Some(ImportAnswer::default()), 22_000);
        // The next full send is a new login.
        live.note_full_send();
        let next = live.capture(Reason::Login).unwrap();
        assert_ne!(next.session, login.session);
        assert_eq!(next.world, 1);
    }

    #[test]
    fn an_endgame_change_goes_up_as_an_attach() {
        // Live mode: the records opened after the login's snapshot go up at
        // the next tick as one attach for this login, once.
        let mut live = live_at(0);
        let session = live.capture(Reason::World).unwrap().session;
        live.note_endgame_change();
        assert_eq!(live.next_tick_ms(true, 1), Some(1));
        assert_eq!(
            live.tick(true, false, 1, || panic!("no export")),
            Action::Attach {
                session: session.clone()
            }
        );
        // In flight: nothing more, a batch still goes; stored: done.
        assert_eq!(live.next_tick_ms(true, 2), None);
        live.attach_answered(AttachAnswer::Stored, 2);
        assert_eq!(
            live.tick(true, false, 3, || panic!("no export")),
            Action::Nothing
        );
        // A failure is retried after a wait, a refusal dropped.
        live.note_endgame_change();
        assert!(matches!(
            live.tick(true, false, 4, || None),
            Action::Attach { .. }
        ));
        live.attach_answered(AttachAnswer::Retry, 4);
        assert_eq!(live.next_tick_ms(true, 5), Some(4 + FIRST_BACKOFF_MS));
        assert_eq!(live.tick(true, false, 5, || None), Action::Nothing);
        assert!(matches!(
            live.tick(true, false, 4 + FIRST_BACKOFF_MS, || None),
            Action::Attach { .. }
        ));
        live.attach_answered(AttachAnswer::Dropped, 100_000);
        assert_eq!(live.next_tick_ms(true, 100_001), None);
        // A change while one is in flight sends again after the answer.
        live.note_endgame_change();
        assert!(matches!(
            live.tick(true, false, 100_002, || None),
            Action::Attach { .. }
        ));
        live.note_endgame_change();
        assert_eq!(live.tick(true, false, 100_003, || None), Action::Nothing);
        live.attach_answered(AttachAnswer::Stored, 100_004);
        assert!(matches!(
            live.tick(true, false, 100_005, || None),
            Action::Attach { .. }
        ));
        live.attach_answered(AttachAnswer::Stored, 100_006);
        // Snapshot mode (no journal) too.
        let mut live = LiveSync::new();
        live.note_full_send();
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(1)));
        live.snapshot_answered(ticket, Some(ImportAnswer::default()), 0);
        live.note_endgame_change();
        assert!(matches!(
            live.tick(false, false, 1, || None),
            Action::Attach { .. }
        ));
        // A login due carries it: no attach, not after the snapshot either
        // (the export built holds the parts). Before any login: nothing.
        let mut live = LiveSync::new();
        live.note_endgame_change();
        assert_eq!(live.tick(true, false, 1, || None), Action::Nothing);
        live.note_full_send();
        live.note_endgame_change();
        assert_eq!(live.automation_reason(true, 1), Some(Reason::Login));
        assert_eq!(live.tick(true, false, 1, || None), Action::Nothing);
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(1)));
        assert_eq!(live.tick(true, false, 2, || None), Action::Nothing);
        live.snapshot_answered(ticket, Some(ImportAnswer::default()), 3);
        assert_eq!(live.tick(true, false, 4, || None), Action::Nothing);
        live.note_endgame_change();
        assert!(matches!(
            live.tick(true, false, 5, || None),
            Action::Attach { .. }
        ));
        assert_eq!(AttachAnswer::of(200), AttachAnswer::Stored);
        assert_eq!(AttachAnswer::of(0), AttachAnswer::Retry);
        assert_eq!(AttachAnswer::of(503), AttachAnswer::Retry);
        assert_eq!(AttachAnswer::of(409), AttachAnswer::Dropped);
    }
    #[test]
    fn a_world_change_without_a_journal_is_a_snapshot() {
        let mut live = LiveSync::new();
        live.note_full_send();
        let ticket = live.snapshot_started(Reason::Login, keyed(&good(1)));
        // Live sync off for the account: no journal.
        live.snapshot_answered(ticket, Some(ImportAnswer::default()), 0);
        live.note_full_send();
        assert_eq!(live.automation_reason(true, 1), Some(Reason::World));
    }

    #[test]
    fn an_idle_logout_sends_nothing() {
        let mut live = live_at(0);
        live.note_connection_ended();
        assert_eq!(live.tick(true, false, 1, || None), Action::Nothing);
    }

    #[test]
    fn a_failed_logout_snapshot_is_retried_after_a_wait() {
        let mut live = live_at(0);
        live.note_change(1);
        live.note_connection_ended();
        assert_eq!(
            live.tick(true, false, 1, || None),
            Action::Snapshot(Reason::Logout)
        );
        let ticket = live.snapshot_started(Reason::Logout, keyed(&good(101)));
        live.snapshot_answered(ticket, None, 1);
        assert_eq!(live.tick(true, false, 2, || None), Action::Nothing);
        assert_eq!(live.next_tick_ms(true, 2), Some(1 + FIRST_BACKOFF_MS));
        assert_eq!(
            live.tick(true, false, 1 + FIRST_BACKOFF_MS, || None),
            Action::Snapshot(Reason::Logout)
        );
    }

    #[test]
    fn replaced_data_ends_the_journal() {
        let mut live = live_at(0);
        live.note_change(5);
        live.note_data_replaced();
        assert!(!live.is_live(0));
        assert_eq!(live.tick(true, false, 100_000, || None), Action::Nothing);
    }

    #[test]
    fn full_sends_are_told_from_item_changes() {
        assert!(is_full_send(5_543, 5_543));
        assert!(is_full_send(5_543, 0));
        assert!(!is_full_send(12, 5_543));
        assert!(!is_full_send(99, 0));
        assert!(!is_full_send(1_000, 5_543));
    }

    #[test]
    fn the_answers_are_read_leniently() {
        assert_eq!(
            ImportAnswer::parse(
                r#"{"status":"created","snapshotId":3,"contentKey":"x","live":{"base":"k9","watching":true}}"#
            ),
            ImportAnswer {
                base: Some("k9".into()),
                watching: true
            }
        );
        assert_eq!(
            ImportAnswer::parse(r#"{"status":"created","snapshotId":3}"#),
            ImportAnswer::default()
        );
        assert_eq!(ImportAnswer::parse("<html>"), ImportAnswer::default());
        assert_eq!(
            BatchAnswer::of(
                200,
                r#"{"ok":true,"seq":3,"collapse":false,"watching":true}"#
            ),
            BatchAnswer::Stored {
                collapse: false,
                watching: true
            }
        );
        assert_eq!(
            BatchAnswer::of(200, r#"{"ok":true,"duplicate":true,"seq":3}"#),
            BatchAnswer::Duplicate
        );
        assert_eq!(
            BatchAnswer::of(200, r#"{"ok":false,"live":false}"#),
            BatchAnswer::LiveOff
        );
        assert_eq!(
            BatchAnswer::of(
                409,
                r#"{"error":{"code":"resync","message":"Send a snapshot first"}}"#
            ),
            BatchAnswer::Resync
        );
        assert_eq!(
            BatchAnswer::of(
                429,
                r#"{"error":{"code":"daily_live_limit","message":"x"}}"#
            ),
            BatchAnswer::DailyLimit
        );
        assert_eq!(
            BatchAnswer::of(429, r#"{"error":{"code":"rate_limited","message":"x"}}"#),
            BatchAnswer::Retry
        );
        assert_eq!(BatchAnswer::of(0, ""), BatchAnswer::Retry);
        assert_eq!(BatchAnswer::of(503, ""), BatchAnswer::Retry);
        assert_eq!(BatchAnswer::of(413, ""), BatchAnswer::Resync);
        assert_eq!(BatchAnswer::of(404, "<html>"), BatchAnswer::LiveOff);
        assert_eq!(
            BatchAnswer::of(401, r#"{"error":{"code":"session_revoked","message":"x"}}"#),
            BatchAnswer::Refused
        );
    }

    // -- the final batch (`upload.finalBatch`) ------------------------------

    #[test]
    fn a_final_batch_is_taken_only_from_a_tracker_that_says_so() {
        let yes = |json: &str| accepts_final_batch(&serde_json::from_str(json).unwrap());
        assert!(yes(r#"{"upload":{"gzip":true,"finalBatch":true}}"#));
        assert!(yes(
            r#"{"status":200,"data":{"upload":{"finalBatch":true}}}"#
        ));
        assert!(!yes(r#"{"upload":{"gzip":true}}"#), "absent");
        assert!(!yes(r#"{"upload":{"finalBatch":false}}"#));
        assert!(!yes(r#"{"upload":{"finalBatch":"true"}}"#));
        assert!(!yes("[]"));
        assert!(serde_json::from_str::<serde_json::Value>("<html>").is_err());
    }

    /// Logged in on `base` (a snapshot stored with a journal), one batch of
    /// `then` stored, and the login over.
    fn ended_after_a_batch(base: Good, then: Good) -> LiveSync {
        let mut live = LiveSync::new();
        live.note_full_send();
        let ticket = live.snapshot_started(Reason::Login, keyed(&base));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k1".into()),
                watching: false,
            }),
            0,
        );
        let mut batch = then;
        for i in 0..5 {
            batch.materials.insert(format!("Thing{i}"), 1);
        }
        live.note_change(5);
        assert_eq!(sent(live.tick(true, true, 15_000, export(batch))).seq, 1);
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: false,
                watching: false,
            },
            15_100,
        );
        live.note_connection_ended();
        live
    }

    fn stored() -> BatchAnswer {
        BatchAnswer::Stored {
            collapse: false,
            watching: false,
        }
    }

    #[test]
    fn without_the_word_the_login_ends_on_its_logout_snapshot() {
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        assert_eq!(
            live.tick(true, false, 15_200, || panic!(
                "the snapshot builds its own"
            )),
            Action::Snapshot(Reason::Logout)
        );
        assert!(live.logout_snapshot_kept().is_empty());
        // With the word but live sync switched off here: the same.
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        assert_eq!(
            live.tick(false, true, 15_200, || panic!(
                "the snapshot builds its own"
            )),
            Action::Snapshot(Reason::Logout)
        );
    }

    #[test]
    fn with_the_word_the_login_ends_on_a_final_batch_and_nothing_more() {
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        let session = live.capture(Reason::Logout).unwrap().session;
        // Pending changes at the end go in it.
        let batch = sent(live.tick(true, true, 15_200, export(with_materials(7))));
        assert_eq!(batch.end, Some(true));
        assert_eq!(batch.session, session, "the ended login's");
        assert_eq!(batch.seq, 2);
        assert_eq!(batch.base, "k1");
        assert_eq!(
            batch.changes.len(),
            2,
            "the two materials since the last batch"
        );
        assert!(batch.sum.is_some());
        assert_eq!(
            live.next_tick_ms(true, 15_300),
            None,
            "waits for its answer"
        );
        live.batch_answered(stored(), 15_400);
        // Closed: no journal, nothing wanted, nothing more sent.
        assert!(!live.is_live(15_500));
        assert!(live.handover().is_none());
        assert_eq!(live.automation_reason(true, 15_500), Some(Reason::Change));
        assert_eq!(live.next_tick_ms(true, 15_500), None);
        assert_eq!(
            live.tick(true, true, 1_000_000, || panic!("no export")),
            Action::Nothing
        );
        // The next login opens its own journal as always.
        live.note_full_send();
        assert_eq!(live.automation_reason(true, 1_000_000), Some(Reason::Login));
    }

    #[test]
    fn a_session_whose_batches_sent_everything_still_ends_on_an_empty_final_batch() {
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        let batch = sent(live.tick(true, true, 15_200, export(with_materials(5))));
        assert_eq!(batch.end, Some(true));
        assert!(batch.changes.is_empty());
        assert_eq!(batch.seq, 2);
        live.batch_answered(BatchAnswer::Duplicate, 15_300);
        assert!(!live.is_live(15_300));
    }

    #[test]
    fn a_session_with_nothing_stored_or_pending_sends_nothing_at_the_end() {
        let mut live = live_at(0);
        live.note_connection_ended();
        assert_eq!(live.next_tick_ms(true, 1), None);
        assert_eq!(live.tick(true, true, 1, || None), Action::Nothing);
    }

    #[test]
    fn an_unacked_batch_goes_again_unchanged_before_the_final_batch() {
        let mut live = live_at(0);
        live.note_change(5);
        let first = sent(live.tick(true, true, 15_000, export(with_materials(5))));
        live.batch_answered(BatchAnswer::Retry, 15_100);
        live.note_change(1);
        live.note_connection_ended();
        // After the batch's backoff: first the unacked batch, as it was.
        assert_eq!(
            live.next_tick_ms(true, 15_200),
            Some(15_100 + FIRST_BACKOFF_MS)
        );
        assert_eq!(
            live.tick(true, true, 15_200, || panic!("not yet")),
            Action::Nothing
        );
        let at = 15_100 + FIRST_BACKOFF_MS;
        let again = sent(live.tick(true, true, at, export(with_materials(6))));
        assert_eq!(again, first);
        live.batch_answered(stored(), at + 1);
        let last = sent(live.tick(true, true, at + 2, export(with_materials(6))));
        assert_eq!(last.seq, 2);
        assert_eq!(last.end, Some(true));
        assert_eq!(last.changes.len(), 1, "what the resent batch did not carry");
    }

    #[test]
    fn a_batch_in_flight_at_the_end_is_answered_before_the_final_batch() {
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, true, 15_000, export(with_materials(5))));
        live.note_connection_ended();
        assert_eq!(live.next_tick_ms(true, 15_050), None);
        assert_eq!(
            live.tick(true, true, 15_050, || panic!("in flight")),
            Action::Nothing
        );
        // Its answer asks for a collapse: no collapse snapshot for a login
        // that ends on a final batch.
        live.batch_answered(
            BatchAnswer::Stored {
                collapse: true,
                watching: false,
            },
            15_100,
        );
        let last = sent(live.tick(true, true, 15_200, export(with_materials(5))));
        assert_eq!((last.seq, last.end), (2, Some(true)));
    }

    /// `good(100)` with `edit` applied.
    fn edited(edit: impl FnOnce(&mut Good)) -> Good {
        let mut good = good(100);
        edit(&mut good);
        good
    }

    #[test]
    fn a_part_no_batch_carries_that_changed_keeps_the_logout_snapshot() {
        use std::collections::BTreeMap;

        use crate::good::{
            GiAchievementProgress, GiCollections, GiComfort, GiExploration, GiTeapot, GiWardrobe,
            GiWardrobeKind,
        };

        let progress = |done| {
            edited(|good| {
                good.gi_achievement_progress = Some(BTreeMap::from([(
                    84_000,
                    GiAchievementProgress {
                        progress: done,
                        total: 10,
                    },
                )]))
            })
        };
        let exploration = |level| {
            edited(|good| {
                good.gi_exploration = Some(GiExploration {
                    offerings: Some(BTreeMap::from([(1, level)])),
                    ..Default::default()
                })
            })
        };
        let teapot = |mansion| {
            edited(|good| {
                good.gi_teapot = Some(GiTeapot {
                    comfort: Some(BTreeMap::from([(
                        1,
                        GiComfort {
                            mansion,
                            blocks: vec![],
                        },
                    )])),
                })
            })
        };
        let recipes = |count| {
            edited(|good| {
                good.gi_collections = Some(GiCollections {
                    recipes: Some(BTreeMap::from([(1, count)])),
                    ..Default::default()
                })
            })
        };
        let outfits = |id| {
            edited(|good| {
                good.gi_wardrobe = Some(GiWardrobe {
                    outfits: Some(GiWardrobeKind {
                        owned: vec![id],
                        ..Default::default()
                    }),
                    ..Default::default()
                })
            })
        };
        let guidless = |level: u32| {
            let mut good = good(100);
            good.artifacts = serde_json::from_value(serde_json::json!([{
                "setKey": "GoldenTroupe", "slotKey": "plume", "level": level, "rarity": 5,
                "mainStatKey": "atk", "location": "", "lock": false,
                "substats": [], "totalRolls": 4, "astralMark": false, "elixirCrafted": false,
                "unactivatedSubstats": []
            }]))
            .unwrap();
            good
        };
        let cases: [(&str, Good, Good); 6] = [
            ("gi_achievement_progress", progress(1), progress(2)),
            ("gi_exploration", exploration(1), exploration(2)),
            ("gi_teapot", teapot(100), teapot(200)),
            ("gi_collections", recipes(1), recipes(2)),
            ("gi_wardrobe", outfits(1), outfits(2)),
            ("artifacts", guidless(0), guidless(4)),
        ];
        for (part, base, now) in cases {
            let mut live = ended_after_a_batch(base, now.clone());
            assert_eq!(
                live.tick(true, true, 15_200, export(now)),
                Action::Snapshot(Reason::Logout),
                "{part}"
            );
            assert_eq!(live.logout_snapshot_kept(), [part.to_string()], "{part}");
            // Decided once: the snapshot is retried as today, no export.
            let ticket = live.snapshot_started(Reason::Logout, keyed(&good(1)));
            live.snapshot_answered(ticket, None, 15_300);
            assert_eq!(
                live.tick(true, true, 15_300 + FIRST_BACKOFF_MS, || panic!("decided")),
                Action::Snapshot(Reason::Logout),
                "{part}"
            );
        }
    }

    #[test]
    fn dropped_parts_and_the_endgame_alone_still_end_on_a_final_batch() {
        use std::collections::BTreeMap;

        use crate::good::{GiCollections, GiEndgame, GiEvent, GiQuests, GiWardrobe};

        let base = edited(|good| {
            good.gi_collections = Some(GiCollections {
                monsters_seen: Some(vec![1, 2]),
                ..Default::default()
            });
            good.gi_wardrobe = Some(GiWardrobe {
                current_team: Some(1),
                ..Default::default()
            });
        });
        let now = edited(|good| {
            good.gi_events = Some(BTreeMap::from([(
                1,
                GiEvent {
                    schedule: 1_001,
                    start: 0,
                    end: 9,
                    progress: None,
                },
            )]));
            good.gi_endgame = Some(GiEndgame::default());
            // Seen monsters as a count; quests and the team are batched.
            good.gi_collections = Some(GiCollections {
                monsters_seen: Some(vec![3, 4]),
                quests: Some(GiQuests {
                    finished: vec![7],
                    active: vec![],
                }),
                ..Default::default()
            });
            good.gi_wardrobe = Some(GiWardrobe {
                current_team: Some(2),
                ..Default::default()
            });
        });
        let mut live = ended_after_a_batch(base, now.clone());
        let last = sent(live.tick(true, true, 15_200, export(now)));
        assert_eq!(last.end, Some(true));
        assert!(live.logout_snapshot_kept().is_empty());
    }

    #[test]
    fn a_refused_final_batch_falls_back_to_the_logout_snapshot() {
        for answer in [
            BatchAnswer::Resync,
            BatchAnswer::LiveOff,
            BatchAnswer::DailyLimit,
            BatchAnswer::Refused,
            BatchAnswer::of(413, ""),
        ] {
            let mut live = ended_after_a_batch(good(100), with_materials(5));
            let session = live.capture(Reason::Logout).unwrap().session;
            sent(live.tick(true, true, 15_200, export(with_materials(6))));
            live.batch_answered(answer.clone(), 15_300);
            assert_eq!(live.next_tick_ms(true, 15_300), Some(15_300), "{answer:?}");
            assert_eq!(
                live.tick(true, true, 15_300, || panic!("the snapshot builds its own")),
                Action::Snapshot(Reason::Logout),
                "{answer:?}"
            );
            let capture = live.capture(Reason::Logout).unwrap();
            assert_eq!(capture.session, session, "{answer:?}");
            assert_eq!(capture.journal.seq, 1, "{answer:?}");
        }
    }

    #[test]
    fn a_final_batch_that_got_no_answer_goes_again_unchanged() {
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        let first = sent(live.tick(true, true, 15_200, export(with_materials(6))));
        live.batch_answered(BatchAnswer::Retry, 15_300);
        let at = 15_300 + FIRST_BACKOFF_MS;
        assert_eq!(live.next_tick_ms(true, 15_400), Some(at));
        assert_eq!(live.tick(true, true, 15_400, || None), Action::Nothing);
        let again = sent(live.tick(true, true, at, export(with_materials(9))));
        assert_eq!(
            serde_json::to_vec(&again).unwrap(),
            serde_json::to_vec(&first).unwrap(),
            "the same bytes"
        );
        live.batch_answered(stored(), at + 1);
        assert!(!live.is_live(at + 1));
    }

    #[test]
    fn an_attach_owed_goes_with_the_ended_session_before_the_final_batch() {
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, true, 15_000, export(with_materials(5))));
        live.batch_answered(stored(), 15_100);
        // A record opened just before the login ended: its attach is owed.
        live.note_endgame_change();
        live.note_connection_ended();
        let session = live.capture(Reason::Logout).unwrap().session;
        assert_eq!(
            live.tick(true, true, 15_200, export(with_materials(5))),
            Action::Attach {
                session: session.clone()
            }
        );
        // Its answer comes before anything else: no tick while it flies.
        assert_eq!(live.next_tick_ms(true, 15_201), None);
        assert_eq!(
            live.tick(true, true, 15_201, || panic!("decided")),
            Action::Nothing
        );
        live.attach_answered(AttachAnswer::Stored, 15_202);
        assert_eq!(live.next_tick_ms(true, 15_203), Some(15_203));
        let last = sent(live.tick(true, true, 15_203, export(with_materials(5))));
        assert_eq!(last.end, Some(true));
        assert_eq!(last.session, session);
        live.batch_answered(stored(), 15_204);
        assert!(!live.is_live(15_204));
    }

    /// Logged in with one batch stored and an endgame change noted.
    fn with_endgame_change() -> LiveSync {
        let mut live = live_at(0);
        live.note_change(5);
        sent(live.tick(true, true, 15_000, export(with_materials(5))));
        live.batch_answered(stored(), 15_100);
        live.note_endgame_change();
        live
    }

    #[test]
    fn an_attach_in_its_backoff_at_the_end_keeps_the_logout_snapshot() {
        // In its backoff, and after it: never overtaken by a final batch.
        for at in [15_400, 15_300 + FIRST_BACKOFF_MS] {
            let mut live = with_endgame_change();
            assert!(matches!(
                live.tick(true, true, 15_200, || None),
                Action::Attach { .. }
            ));
            live.attach_answered(AttachAnswer::Retry, 15_300);
            live.note_connection_ended();
            assert_eq!(
                live.tick(true, true, at, export(with_materials(5))),
                Action::Snapshot(Reason::Logout),
                "{at}"
            );
            assert_eq!(live.logout_snapshot_kept(), ["attach".to_string()]);
        }
    }

    #[test]
    fn an_attach_in_flight_at_the_end_is_answered_first() {
        for (answer, kept) in [
            (AttachAnswer::Stored, None),
            (AttachAnswer::Retry, Some("attach")),
            (AttachAnswer::Dropped, Some("gi_endgame")),
        ] {
            let mut live = with_endgame_change();
            let session = live.capture(Reason::World).unwrap().session;
            assert_eq!(
                live.tick(true, true, 15_200, || None),
                Action::Attach {
                    session: session.clone()
                }
            );
            live.note_connection_ended();
            // Decided, then waits for the attach's answer.
            assert_eq!(live.next_tick_ms(true, 15_250), Some(15_250));
            assert_eq!(
                live.tick(true, true, 15_250, export(with_materials(5))),
                Action::Nothing,
                "{answer:?}"
            );
            assert_eq!(live.next_tick_ms(true, 15_260), None, "{answer:?}");
            live.attach_answered(answer, 15_300);
            assert_eq!(live.next_tick_ms(true, 15_300), Some(15_300), "{answer:?}");
            let action = live.tick(true, true, 15_300, export(with_materials(5)));
            match kept {
                None => {
                    let last = sent(action);
                    assert_eq!((last.end, last.session), (Some(true), session));
                }
                Some(part) => {
                    assert_eq!(action, Action::Snapshot(Reason::Logout), "{answer:?}");
                    assert_eq!(live.logout_snapshot_kept(), [part.to_string()]);
                }
            }
        }
    }

    #[test]
    fn an_attach_sent_at_the_end_and_not_stored_keeps_the_logout_snapshot() {
        for (answer, part) in [
            (AttachAnswer::Retry, "attach"),
            (AttachAnswer::Dropped, "gi_endgame"),
        ] {
            let mut live = with_endgame_change();
            live.note_connection_ended();
            assert!(matches!(
                live.tick(true, true, 15_200, export(with_materials(5))),
                Action::Attach { .. }
            ));
            live.attach_answered(answer, 15_300);
            assert_eq!(
                live.tick(true, true, 15_300, export(with_materials(5))),
                Action::Snapshot(Reason::Logout),
                "{answer:?}"
            );
            assert_eq!(live.logout_snapshot_kept(), [part.to_string()]);
            // The logout snapshot holds the parts: nothing owed after it.
            let ticket = live.snapshot_started(Reason::Logout, keyed(&with_materials(5)));
            live.snapshot_answered(ticket, Some(ImportAnswer::default()), 15_400);
            assert_eq!(live.next_tick_ms(true, 15_500), None);
        }
    }

    #[test]
    fn endgame_state_the_tracker_dropped_keeps_the_logout_snapshot() {
        // Dropped during the login (404, 409, 400, 413, or no UID here).
        let dropped = || {
            let mut live = with_endgame_change();
            assert!(matches!(
                live.tick(true, true, 15_200, || None),
                Action::Attach { .. }
            ));
            live.attach_answered(AttachAnswer::Dropped, 15_300);
            live.note_connection_ended();
            live
        };
        let mut live = dropped();
        assert_eq!(
            live.tick(true, true, 15_400, export(with_materials(5))),
            Action::Snapshot(Reason::Logout)
        );
        assert_eq!(live.logout_snapshot_kept(), ["gi_endgame".to_string()]);
        // Without the word: the logout snapshot as always, nothing named.
        let mut live = dropped();
        assert_eq!(
            live.tick(true, false, 15_400, || panic!(
                "the snapshot builds its own"
            )),
            Action::Snapshot(Reason::Logout)
        );
        assert!(live.logout_snapshot_kept().is_empty());
        // Stored during the login: the final batch.
        let mut live = with_endgame_change();
        assert!(matches!(
            live.tick(true, true, 15_200, || None),
            Action::Attach { .. }
        ));
        live.attach_answered(AttachAnswer::Stored, 15_300);
        live.note_connection_ended();
        assert_eq!(
            sent(live.tick(true, true, 15_400, export(with_materials(5)))).end,
            Some(true)
        );
        // A snapshot since holds the parts: the final batch.
        let mut live = with_endgame_change();
        let ticket = live.snapshot_started(Reason::Resync, keyed(&with_materials(5)));
        live.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k2".into()),
                watching: false,
            }),
            15_200,
        );
        live.note_change(1);
        live.note_connection_ended();
        assert_eq!(
            sent(live.tick(true, true, 15_400, export(with_materials(6)))).end,
            Some(true)
        );
        // A newer change while one was in flight: that one's Stored does
        // not cover the newer change, whose attach is then dropped.
        let mut live = with_endgame_change();
        assert!(matches!(
            live.tick(true, true, 15_200, || None),
            Action::Attach { .. }
        ));
        live.note_endgame_change();
        live.attach_answered(AttachAnswer::Stored, 15_300);
        assert!(matches!(
            live.tick(true, true, 15_400, || None),
            Action::Attach { .. }
        ));
        live.attach_answered(AttachAnswer::Dropped, 15_500);
        live.note_connection_ended();
        assert_eq!(
            live.tick(true, true, 15_600, export(with_materials(5))),
            Action::Snapshot(Reason::Logout)
        );
        assert_eq!(live.logout_snapshot_kept(), ["gi_endgame".to_string()]);
    }

    #[test]
    fn a_game_data_change_alone_keeps_the_logout_snapshot() {
        let on = |sha: &str| {
            let mut good = good(100);
            good.gi_player.as_mut().unwrap().game_data = Some(sha.to_string());
            good
        };
        let mut live = ended_after_a_batch(on("a"), on("a"));
        assert_eq!(
            sent(live.tick(true, true, 15_200, export(on("a")))).end,
            Some(true)
        );
        let mut live = ended_after_a_batch(on("a"), on("a"));
        assert_eq!(
            live.tick(true, true, 15_200, export(on("b"))),
            Action::Snapshot(Reason::Logout)
        );
        assert_eq!(
            live.logout_snapshot_kept(),
            ["gi_player.gameData".to_string()]
        );
    }

    #[test]
    fn replaced_data_drops_a_final_batch_or_its_answer() {
        // Before it went: nothing goes.
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        live.note_data_replaced();
        assert_eq!(live.next_tick_ms(true, 15_200), None);
        assert_eq!(live.tick(true, true, 15_200, || None), Action::Nothing);
        // After it went: its answer changes nothing.
        let mut live = ended_after_a_batch(good(100), with_materials(5));
        sent(live.tick(true, true, 15_200, export(with_materials(6))));
        live.note_data_replaced();
        live.batch_answered(BatchAnswer::Resync, 15_300);
        assert_eq!(live.next_tick_ms(true, 15_300), None);
        assert_eq!(live.tick(true, true, 15_300, || None), Action::Nothing);
    }

    #[test]
    fn a_hand_over_carries_the_bases_fingerprints() {
        let base = edited(|good| {
            good.gi_teapot = Some(crate::good::GiTeapot::default());
            good.gi_achievement_progress = Some(std::collections::BTreeMap::from([(
                84_000,
                crate::good::GiAchievementProgress {
                    progress: 1,
                    total: 10,
                },
            )]));
        });
        // The run that hands over, its login still going.
        let mut running = LiveSync::new();
        running.note_full_send();
        let ticket = running.snapshot_started(Reason::Login, keyed(&base));
        running.snapshot_answered(
            ticket,
            Some(ImportAnswer {
                base: Some("k1".into()),
                watching: false,
            }),
            0,
        );
        let handover = running.handover().unwrap();
        let json = serde_json::to_value(&handover).unwrap();
        assert!(json["journal"]["unbatched"].is_object(), "written");
        let back: LiveHandover = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back, handover, "round trip");
        let mut resumed = LiveSync::new();
        assert!(resumed.resume(back));
        resumed.note_change(1);
        resumed.note_connection_ended();
        assert_eq!(
            sent(resumed.tick(true, true, 1_000, export(base.clone()))).end,
            Some(true)
        );

        // An older Capture's hand-over has none: the logout snapshot.
        let mut old = json;
        old["journal"].as_object_mut().unwrap().remove("unbatched");
        let mut resumed = LiveSync::new();
        assert!(resumed.resume(serde_json::from_value(old).unwrap()));
        resumed.note_change(1);
        resumed.note_connection_ended();
        assert_eq!(
            resumed.tick(true, true, 1_000, export(base)),
            Action::Snapshot(Reason::Logout)
        );
        assert_eq!(resumed.logout_snapshot_kept(), ["unknown".to_string()]);
    }

    #[test]
    fn a_hand_over_without_the_endgame_flag_ends_on_the_logout_snapshot() {
        let mut running = with_endgame_change();
        assert!(matches!(
            running.tick(true, true, 15_200, || None),
            Action::Attach { .. }
        ));
        running.attach_answered(AttachAnswer::Stored, 15_300);
        let json = serde_json::to_value(running.handover().unwrap()).unwrap();
        assert_eq!(json["endgame_unacked"], serde_json::json!(false), "written");
        let end = |json: serde_json::Value| {
            let mut resumed = LiveSync::new();
            assert!(resumed.resume(serde_json::from_value(json).unwrap()));
            resumed.note_change(1);
            resumed.note_connection_ended();
            resumed.tick(true, true, 16_000, export(with_materials(6)))
        };
        assert_eq!(sent(end(json.clone())).end, Some(true));
        // An older Capture's hand-over has no flag: unknown, kept.
        let mut old = json;
        old.as_object_mut().unwrap().remove("endgame_unacked");
        assert_eq!(end(old), Action::Snapshot(Reason::Logout));
    }
}
