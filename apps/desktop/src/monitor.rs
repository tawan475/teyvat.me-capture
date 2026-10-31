// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc as blocking_mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use base64::prelude::*;
use chrono::prelude::*;
use flate2::read::GzDecoder;
use teyvatme_game_data::AnimeGameData;
use teyvatme_protocol::{
    CommandMatch, ConnectionPacket, GameCommand, GamePacket, GameSniffer, KeyState, PacketDirection,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::activity::{Event, Topic};
use crate::capture::{self, BackendType, CaptureSource, create_capture};
// The decode layer moved to teyvatme-core; the replay and this module's tests
// still reach it through here.
pub(crate) use crate::decode::{
    AppliedData, DataReplacement, DataVerdict, KeyReport, STATS_INTERVAL, SnifferEvent,
    SnifferStats, apply_commands, changes_in, classify_commands, decode_one_packet, full_send_in,
};
use crate::game_watch::{self, GameStatus, GameWatch, SystemProcessDetector};
use crate::i18n::{tr, trf};
use crate::link::{Alerts, AlertsAnswer, Command, Heartbeat, PresenceOutcome, PresenceState};
use crate::live::{Action, BatchAnswer, ImportAnswer, LiveSync, Reason};
use crate::player_data::{ExportReport, ExportSettings, PlayerData};
use crate::signin::{AuthStatus, TrackerAuth, UploadCredential};
use crate::{AppState, DataUpdated, Message, State};

/// How long the automation trigger waits after the last new data before it
/// exports. A login burst arrives over several seconds, so exporting on the
/// first packet would snapshot a half-parsed account.
const AUTOMATION_DEBOUNCE: Duration = Duration::from_secs(5);

/// How long a countdown waits, from its start, for the world's exploration
/// points: 7.1 sends them 4-8 s after the bag, and without this wait most
/// login snapshots went without them until the next world change.
const AUTOMATION_EXPLORE_WAIT: Duration = Duration::from_secs(12);

/// New data restarts a countdown only this long after it began, so a stream
/// of changes cannot hold the export back.
const AUTOMATION_MAX_WAIT: Duration = Duration::from_secs(20);

/// Whole-request timeout for a tracker call that only asks a question.
///
/// `reqwest::Client` has no timeout of its own, so a wedged self-hosted backend
/// used to hang the verify task -- and the "Verifying key…" modal with it -- for
/// the rest of the process's life.
const TRACKER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Whole-request timeout for a snapshot upload, replacing
/// [`TRACKER_REQUEST_TIMEOUT`] on that one request.
///
/// An upload is not a question: the tracker parses the GOOD JSON, hashes every
/// artifact and runs the Prisma upserts *inside* the request, so a first
/// full-inventory import against a slow self-hosted Postgres can legitimately
/// run for minutes. Holding it to the verify budget would fail uploads that
/// used to succeed, which is the whole reason this is a separate number.
const TRACKER_UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Connect timeout for every tracker call.
///
/// Covers only the TCP/TLS phase -- it does not bound the request body -- so an
/// unreachable host fails fast while a reachable but slow one still gets the
/// whole-request budget above.
const TRACKER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long to wait for a cancelled capture task to actually finish before
/// building its replacement.
///
/// `CancellationToken::cancel` only *signals*; the backend is released when the
/// task's `Drop` runs, and on Windows pktmon opens a fixed global ETW session
/// (`"PktMon Rust"`), so a replacement built before that teardown completes can
/// lose the race for the session and then capture nothing, silently, forever.
///
/// `app::MONITOR_SHUTDOWN_TIMEOUT` -- the deadline `TeyvatmeApp::drop` gives
/// the whole monitor thread -- is derived from this and must stay strictly
/// larger than it, or a teardown spending its full budget here is detached
/// at the moment it would have succeeded. `app.rs` mirrors the value and a
/// test there checks the two against this line.
const CAPTURE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Undecoded packets queued for the sniffer thread before it is worth saying so.
///
/// The queue is deliberately unbounded: KCP delivery is strictly ordered and
/// this app is a passive listener that can never ask for a retransmit, so
/// *dropping* a packet stalls that direction of the stream permanently. Falling
/// behind costs memory; dropping costs the rest of the session.
const PACKET_BACKLOG_WARN: usize = 8192;

/// How often the game's process list entry is looked for.
///
/// Two seconds is fast enough that the status line follows a game launch while
/// the splash screen is still up, and slow enough to be free: one tick is a
/// process-name scan with every expensive refresh turned off (see
/// [`game_watch::SystemProcessDetector`]). It deliberately runs in the
/// monitor's `select!` loop rather than on the UI thread, where it would run
/// per frame.
const GAME_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Raw packet logs kept, newest first, including the one being written.
///
/// `rotate_logs` only sweeps `log_dir()`; the packet log lives under
/// `storage_dir(APP_ID)/packet_log`, a tree nothing else touches, so it prunes
/// itself. These files are decrypted account data, so keeping them forever is a
/// privacy problem as much as a disk one.
const PACKET_LOG_RETENTION: usize = 6;

/// The tracker's host as the activity line names it (`teyvat.me`).
fn destination_host(state: &crate::app::SavedAppState) -> String {
    crate::signin::host_of(&state.tracker_api_url).unwrap_or_else(|| tr("the tracker").to_string())
}

/// A path's file name, for the activity line.
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

struct AppStateManager {
    app_state: AppState,
    state_tx: watch::Sender<AppState>,
}

impl AppStateManager {
    fn new(app_state: AppState, state_tx: watch::Sender<AppState>) -> Self {
        Self {
            app_state,
            state_tx,
        }
    }

    pub fn update_app_state(&mut self, state: State) {
        self.app_state.state = state;
        let _ = self.state_tx.send(self.app_state.clone());
    }

    pub fn update_capturing_state(&mut self, capturing: bool) {
        self.app_state.capturing = capturing;
        let _ = self.state_tx.send(self.app_state.clone());
    }

    /// Publish what the captured data now holds: when each class last arrived,
    /// and what it says about the account.
    pub fn update_captured(
        &mut self,
        updated: DataUpdated,
        player: Option<crate::good::GiPlayer>,
        counts: Option<crate::player_data::ExportCounts>,
        tasks: Option<crate::good::GiTasks>,
        anecdotes: Option<usize>,
    ) {
        self.app_state.updated = updated;
        self.app_state.player = player;
        self.app_state.captured_at = counts.map(|_| chrono::Local::now());
        self.app_state.counts = counts;
        // The resin clock moves on its own; anything else is a new login's
        // (or world change's) routine.
        let routine = |tasks: &Option<crate::good::GiTasks>| {
            tasks.clone().map(|tasks| crate::good::GiTasks {
                resin: None,
                ..tasks
            })
        };
        if tasks.is_none() {
            self.app_state.tasks_at = None;
        } else if routine(&tasks) != routine(&self.app_state.tasks) {
            self.app_state.tasks_at = Some(chrono::Local::now());
        }
        self.app_state.tasks = tasks;
        self.app_state.anecdotes = anecdotes;
        let _ = self.state_tx.send(self.app_state.clone());
    }

    /// Publish a new game status, and only a *new* one.
    ///
    /// Unlike the other setters this one is driven by a timer rather than by an
    /// event, and every send wakes the repaint task on the other end of the
    /// watch channel. Sending an unchanged status would repaint the window
    /// twice a second for as long as the app is open.
    pub fn update_capture_done(&mut self, done: bool) {
        if self.app_state.capture_done == done {
            return;
        }
        tracing::info!(done, "capture done");
        self.app_state.capture_done = done;
        let _ = self.state_tx.send(self.app_state.clone());
    }

    /// Publish the tracker's alert settings (the UI keeps them in
    /// `app.ron`), and only a change.
    pub fn update_tracker_alerts(&mut self, alerts: Option<Alerts>) {
        if self.app_state.tracker_alerts == alerts {
            return;
        }
        self.app_state.tracker_alerts = alerts;
        let _ = self.state_tx.send(self.app_state.clone());
    }

    pub fn update_game_status(&mut self, game_status: GameStatus) {
        if self.app_state.game_status == game_status {
            return;
        }
        // Logged because this line makes a claim about why a capture is or is
        // not working, and when it gets that wrong the only way to tell what it
        // saw is from the log a user can send back.
        tracing::info!(
            from = ?self.app_state.game_status,
            to = ?game_status,
            "game status changed"
        );
        self.app_state.game_status = game_status;
        let _ = self.state_tx.send(self.app_state.clone());
    }
}

/// Where the outcome of a tracker upload should be reported.
///
/// Both upload paths run the same request; only the reporting differs, and
/// keeping them in one function is what stops them drifting apart again (the
/// repo's own `CLAUDE.md` calls the duplication out).
/// Whether a capture from `source` may export automatically or upload.
///
/// Not when replaying a recording (`-r`): its data is old, and uploading it
/// would file a stale snapshot under today's capture. `--replay-export` is the
/// supported way to inspect a recording; it never reaches this code at all.
fn uploads_allowed(source: &CaptureSource) -> bool {
    !matches!(source, CaptureSource::File(_))
}

/// An automation export's `purpose`, as its toast names it.
fn purpose_label(purpose: &str) -> &str {
    match purpose {
        "automation file" => tr("automation file"),
        other => other,
    }
}

/// What `spawn_tracker_upload` answers instead of uploading during a replay.
const REPLAY_UPLOAD_REFUSED: &str =
    "Uploads are off while replaying a recording (-r); use --replay-export to inspect one";

enum UploadReport {
    /// Answer the UI's oneshot. `app.rs` turns it into a toast and drops the
    /// verified state when it recognises a 401/403.
    Reply(oneshot::Sender<std::result::Result<(), String>>),
    /// An automated upload nobody is waiting on: toast it directly, and hand
    /// the snapshot's signature back on failure so it can be retried.
    Toast {
        retry_tx: mpsc::UnboundedSender<AutomationSignature>,
        signature: AutomationSignature,
    },
    /// A snapshot live sync asked for (a collapse, a logout, a resync):
    /// toasted only when it fails; live sync retries it itself.
    Quiet,
    /// A snapshot the website asked for ("Send a capture"): toasted, never
    /// retried (the website can ask again).
    Asked,
}

/// What a tracker request reports back to the monitor's live sync.
#[derive(Debug)]
enum LiveEvent {
    /// The snapshot upload with this ticket finished (`None`: it failed).
    Snapshot {
        ticket: u64,
        answer: Option<ImportAnswer>,
    },
    /// The batch in flight was answered.
    Batch(BatchAnswer),
}

/// Whole-request timeout for one batch of live changes.
const LIVE_BATCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Whole-request timeout for a presence heartbeat.
const LINK_TIMEOUT: Duration = Duration::from_secs(30);

/// The presence heartbeat's `platform`.
const PLATFORM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

/// What a link request reports back to the monitor.
#[derive(Debug)]
enum LinkEvent {
    Presence(PresenceOutcome),
}

/// What the tracker hears about the game: closed, running (nothing decodes
/// yet, or the launch was missed) or decoding.
fn presence_state(status: GameStatus) -> PresenceState {
    match status {
        GameStatus::NotRunning => PresenceState::Stopped,
        GameStatus::Decoding => PresenceState::Decoding,
        GameStatus::LaunchCaptured
        | GameStatus::CaptureOff
        | GameStatus::AwaitingLogin
        | GameStatus::LaunchMissed(_)
        | GameStatus::KeyLost => PresenceState::Running,
    }
}

/// The time live sync runs on: Unix milliseconds.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

/// Whether live sync may send batches: the switch, and automated tracker
/// uploads on with a way to upload.
fn live_enabled(state: &crate::app::SavedAppState) -> bool {
    state.live_sync && crate::app::want_tracker_upload(state)
}

/// The tracker's batch route for an API base.
pub(crate) fn changes_url(base_url: &str) -> String {
    format!("{}/irminsul/changes", base_url.trim_end_matches('/'))
}

/// The three timestamps that identify one snapshot of captured data.
///
/// An export is suppressed while this still matches the last one that
/// succeeded, so it doubles as the retry token: clearing it lets the same data
/// be exported again.
type AutomationSignature = (Option<Instant>, Option<Instant>, Option<Instant>);

/// What an automated export writes, and with which settings.
///
/// The two destinations want different exports. The automation file is a
/// Genshin Optimizer export and follows the user's settings; the tracker keeps
/// the account's history and always gets all of it, whatever those settings
/// filter out ([`ExportSettings::for_tracker`]). They used to share one export
/// built with the user's settings, so the tracker never saw a 1★ or 2★
/// artifact or weapon. The trigger and the export both read this, so what the
/// trigger waits for is what the export writes.
#[derive(Debug, PartialEq)]
struct AutomationPlan {
    /// The automation file's settings, when one is saved.
    file: Option<ExportSettings>,
    /// The tracker upload's settings, when one is made.
    tracker: Option<ExportSettings>,
}

impl AutomationPlan {
    fn for_state(state: &crate::app::SavedAppState) -> Self {
        Self {
            file: state
                .save_result_to_file
                .then(|| state.export_settings.clone()),
            // The same predicate as the manual upload button.
            tracker: crate::app::want_tracker_upload(state).then(ExportSettings::for_tracker),
        }
    }

    fn is_empty(&self) -> bool {
        self.file.is_none() && self.tracker.is_none()
    }

    /// Every export the plan builds, by its settings.
    fn settings(&self) -> impl Iterator<Item = &ExportSettings> {
        self.file.iter().chain(self.tracker.iter())
    }
}

/// What a capture task reports to the monitor.
///
/// [`CaptureEvent::Started`] exists so `capturing` can be set when the backend
/// actually exists rather than when its task is spawned -- the failure path is
/// entirely inside the task, so spawning proves nothing.
enum CaptureEvent {
    Started,
    Packet(Vec<u8>),
    Failed(anyhow::Error),
}

/// Handle to the thread that owns the [`GameSniffer`].
struct SnifferThread {
    packet_tx: blocking_mpsc::Sender<Vec<u8>>,
    /// Packets handed over but not yet decoded.
    queued: Arc<AtomicUsize>,
    /// When the newest game process started (unix ms; 0 = none running), for
    /// [`GameSniffer::set_game_started_ms`].
    game_started_ms: Arc<AtomicU64>,
}

/// Move packet decoding off the monitor's `select!` loop.
///
/// Recovering a session key can take seconds -- it is a search over candidate
/// send times -- and running that inline in a `select!` arm made every other arm
/// wait for it: cancellation, Stop Capture, and every export or upload request
/// the UI is holding a channel open for. The sniffer therefore lives on its own
/// thread, handed raw frames and sending decoded packets back, so a slow decode
/// delays nothing but decoding.
fn spawn_sniffer_thread(
    mut sniffer: GameSniffer,
    decoded_tx: mpsc::UnboundedSender<SnifferEvent>,
) -> Result<SnifferThread> {
    let (packet_tx, packet_rx) = blocking_mpsc::channel::<Vec<u8>>();
    let queued = Arc::new(AtomicUsize::new(0));
    let thread_queued = Arc::clone(&queued);
    let game_started_ms = Arc::new(AtomicU64::new(0));
    let thread_game_started = Arc::clone(&game_started_ms);

    std::thread::Builder::new()
        .name("teyvatme-sniffer".to_string())
        .spawn(move || {
            let mut last_generation = sniffer.session_generation();
            let mut stats = SnifferStats::default();
            let mut next_report = Instant::now() + STATS_INTERVAL;
            loop {
                // Woken at least once per interval, so the stats line keeps its
                // schedule when no packet arrives.
                let wait = next_report.saturating_duration_since(Instant::now());
                match packet_rx.recv_timeout(wait) {
                    Ok(packet) => {
                        stats.note_frame(thread_queued.load(Ordering::Relaxed));
                        let started = thread_game_started.load(Ordering::Relaxed);
                        sniffer.set_game_started_ms((started != 0).then_some(started));
                        let delivered = decode_one_packet(
                            &mut sniffer,
                            &mut last_generation,
                            packet,
                            &decoded_tx,
                            &mut stats,
                        );
                        thread_queued.fetch_sub(1, Ordering::Relaxed);
                        if !delivered {
                            break;
                        }
                    }
                    Err(blocking_mpsc::RecvTimeoutError::Timeout) => {}
                    // The monitor dropped its sender, i.e. the app is exiting.
                    Err(blocking_mpsc::RecvTimeoutError::Disconnected) => break,
                }

                let now = Instant::now();
                if now >= next_report {
                    next_report = now + STATS_INTERVAL;
                    let duplicates = capture::DUPLICATES_DROPPED.load(Ordering::Relaxed);
                    if let Some(line) = stats.report(duplicates, now) {
                        tracing::info!(
                            frames = line.frames,
                            duplicates_dropped = line.duplicates,
                            max_queue = line.max_queue,
                            commands_since_reset = line.commands_since_reset,
                            key = ?sniffer.key_state(),
                            key_given_up = sniffer.key_recovery_failed(),
                            sent_conv = ?sniffer.bound_conversation(PacketDirection::Sent),
                            recv_conv = ?sniffer.bound_conversation(PacketDirection::Received),
                            last_decode_secs = ?line.last_decode_age.map(|age| age.as_secs()),
                            "capture stats (last minute)"
                        );
                    }
                }
            }
            tracing::info!("sniffer thread exiting");
        })
        .context("Unable to start the packet decoding thread")?;

    Ok(SnifferThread {
        packet_tx,
        queued,
        game_started_ms,
    })
}

/// One binary record for the raw packet log.
///
/// `GameCommand::proto_data` is the payload *only* -- the `PacketHead` lives in
/// `proto_header` -- so the two are written with explicit lengths rather than
/// concatenated. Writing just the payload (what the matchers consume) would
/// quietly drop the envelope, and writing the two back to back would leave no
/// way to tell where one ends.
///
/// Layout, little endian: `command_id: u16`, `header_len: u32`, `data_len: u32`,
/// then the header and payload bytes.
fn encode_packet_log_record(command: &GameCommand) -> Vec<u8> {
    let header = &command.proto_header;
    let data = &command.proto_data;

    let mut record = Vec::with_capacity(10 + header.len() + data.len());
    record.extend_from_slice(&command.command_id.to_le_bytes());
    record.extend_from_slice(&(header.len() as u32).to_le_bytes());
    record.extend_from_slice(&(data.len() as u32).to_le_bytes());
    record.extend_from_slice(header);
    record.extend_from_slice(data);
    record
}

/// The raw packet log written while the "Log raw packets" power tool is on.
///
/// One appended-to file per session. This used to be one *file per command*,
/// each preceded by a known-folder lookup and a `create_dir_all`, on the
/// monitor's hot loop -- 4 KiB of NTFS allocation per decrypted command.
///
/// The directory is pruned to [`PACKET_LOG_RETENTION`] files when a session
/// first opens one; `main.rs::rotate_logs` cannot do it, because it only sweeps
/// `log_dir()` and this tree lives under `storage_dir(APP_ID)`.
struct PacketLog {
    /// `None` when the storage directory could not be resolved at all.
    dir: Option<PathBuf>,
    writer: Option<BufWriter<std::fs::File>>,
}

impl PacketLog {
    fn new() -> Self {
        Self::with_dir(crate::data_dir().ok().map(|mut path| {
            path.push("packet_log");
            path
        }))
    }

    fn with_dir(dir: Option<PathBuf>) -> Self {
        Self { dir, writer: None }
    }

    fn append(&mut self, command: &GameCommand) -> Result<()> {
        let record = encode_packet_log_record(command);

        let writer = match self.writer {
            Some(ref mut writer) => writer,
            None => {
                let file = self.open_file()?;
                self.writer.insert(BufWriter::new(file))
            }
        };

        writer.write_all(&record)?;
        // Buffered writing still beats the file-per-command this replaced by a
        // long way, but the log is only ever read after something went wrong,
        // so it must not lose its tail to a crash.
        writer.flush()?;
        Ok(())
    }

    fn open_file(&self) -> Result<std::fs::File> {
        let dir = self.dir.as_ref().context("Storage dir not found")?;
        std::fs::create_dir_all(dir).with_context(|| format!("can't create directory {dir:?}"))?;
        // Room is made *before* the new file exists, so the one about to be
        // written can never be the file that gets deleted.
        prune_packet_logs(dir, PACKET_LOG_RETENTION.saturating_sub(1));
        let path = dir.join(format!(
            "{}.bin",
            Local::now().format("%Y-%m-%d_%H-%M-%S%.3f")
        ));
        std::fs::File::create(&path).with_context(|| format!("can't create file {path:?}"))
    }
}

/// Delete all but the newest `keep` `*.bin` files in the packet-log directory.
///
/// Ordered by file *name*, not by mtime: the names are fixed-width
/// `%Y-%m-%d_%H-%M-%S%.3f` stamps, so lexicographic order is chronological
/// order, and unlike mtime it does not depend on the filesystem's timestamp
/// resolution or survive a copy that rewrites it.
///
/// Every failure here is ignored on purpose. Failing to tidy up is not a reason
/// to refuse to log.
fn prune_packet_logs(dir: &std::path::Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut logs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("bin"))
        .collect();

    if logs.len() <= keep {
        return;
    }

    logs.sort_unstable_by(|a, b| b.file_name().cmp(&a.file_name()));
    for path in logs.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// First delay before restarting a capture that failed, doubling each further
/// consecutive failure up to [`CAPTURE_RETRY_MAX`].
///
/// Capture has no on/off control any more: it is meant to be running whenever
/// Teyvat.me is, so a backend that dies has to come back on its own. Common
/// causes are transient by nature -- an ETW session still held by a previous
/// process, an adapter going down with a VPN -- and retrying is what turns them
/// from "restart the app" into a pause nobody notices.
const CAPTURE_RETRY_BASE: std::time::Duration = std::time::Duration::from_secs(3);
/// Ceiling for the retry backoff, so a permanent failure (no elevation, no
/// capture device) settles into a slow poll instead of hammering the OS.
const CAPTURE_RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(30);

/// How long a capture that has been delivering frames may then deliver none,
/// while the game runs, before it is restarted.
///
/// A connected game talks continuously -- heartbeats every few seconds even
/// when idle -- so a minute of nothing is a capture that has stopped seeing
/// traffic without reporting an error.
const CAPTURE_SILENCE_LIMIT: Duration = Duration::from_secs(60);

/// How long the capture has gone without a frame, counted only while the game
/// runs.
///
/// Only a capture that *was* delivering during this game session counts: a
/// game on its title screen sends nothing, and restarting a capture that never
/// heard anything would only open more blind windows -- including right after
/// this watchdog's own restart, which therefore happens at most once per
/// silence.
#[derive(Debug, Default)]
struct CaptureSilence {
    /// When the current capture last delivered a frame while the game ran;
    /// `None` until it has.
    last_frame_at: Option<Instant>,
}

impl CaptureSilence {
    fn note_frame(&mut self, now: Instant) {
        self.last_frame_at = Some(now);
    }

    /// A capture (re)started: it has heard nothing yet.
    fn capture_started(&mut self) {
        self.last_frame_at = None;
    }

    /// Checked on every game poll. `Some(silence)` when a capture that is up
    /// has gone quiet on a running game for [`CAPTURE_SILENCE_LIMIT`] and
    /// should be restarted.
    ///
    /// While the game is not running the count is dropped, not paused: the
    /// quiet then is the game's absence, not the capture's fault. Carrying it
    /// over is what restarted capture the moment Genshin started on
    /// 2026-10-06 (`silent_secs=37116`, the whole night it had been closed),
    /// opening a blind window at exactly the moment the login it needs is
    /// about to go by.
    fn stalled(&mut self, game_running: bool, capturing: bool, now: Instant) -> Option<Duration> {
        if !game_running {
            self.last_frame_at = None;
            return None;
        }
        let silence = now.saturating_duration_since(self.last_frame_at?);
        (capturing && silence >= CAPTURE_SILENCE_LIMIT).then_some(silence)
    }
}

/// How long to let a killed game process actually exit before re-deriving the
/// game status. See `Message::KillGame`.
const KILL_GAME_SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

pub struct Monitor {
    app_state: AppStateManager,
    ui_message_rx: mpsc::UnboundedReceiver<Message>,
    log_packet_rx: watch::Receiver<bool>,
    player_data: PlayerData,
    data_replacement: DataReplacement,
    /// `None` once the decoding thread has died. Nothing can be decoded after
    /// that, and the user has been told.
    sniffer: Option<SnifferThread>,
    decoded_rx: mpsc::UnboundedReceiver<SnifferEvent>,
    backlog_warned: bool,
    cancel_token: CancellationToken,
    capture_cancel_token: Option<CancellationToken>,
    capture_handle: Option<JoinHandle<Result<()>>>,
    packet_tx: mpsc::UnboundedSender<CaptureEvent>,
    packet_rx: mpsc::UnboundedReceiver<CaptureEvent>,
    capture_backend: BackendType,
    capture_source: CaptureSource,

    /// Did Teyvat.me watch the current game session start? Without that, the
    /// handshake -- and so the session key -- was missed and nothing will ever
    /// decrypt, however healthy the capture backend looks.
    /// When the supervisor may next try to bring capture back up.
    capture_retry_at: Option<Instant>,
    /// Consecutive capture failures, for the backoff.
    capture_failures: u32,
    /// The watchdog's view of the current capture; see [`CaptureSilence`].
    capture_silence: CaptureSilence,
    game_watch: GameWatch,
    game_detector: SystemProcessDetector,
    game_poll: tokio::time::Interval,

    saved_state_rx: watch::Receiver<crate::app::SavedAppState>,
    toast_tx: crate::activity::Sender,
    http: reqwest::Client,
    /// The tracker sign-in (`signin.rs`); its HTTP goes through `http` too.
    tracker_auth: TrackerAuth,
    packet_log: PacketLog,

    /// When the newest captured data arrived, in epoch milliseconds. Uploaded as
    /// the snapshot's timestamp so the dashboard dates it by capture rather than
    /// by delivery.
    capture_timestamp_ms: Option<i64>,

    /// The "Capture done" countdown: like the snapshot's, but whatever the
    /// export settings (see `note_capture_settling`).
    settle_pending_since: Option<Instant>,
    settle_armed_at: Option<Instant>,
    /// The resin clock and the highest amount the resin alert was last sent
    /// for on it.
    resin_alerted: Option<(crate::good::GiResin, u32)>,
    automation_pending_since: Option<Instant>,
    /// When the pending countdown began (`automation_pending_since` moves on
    /// with new data).
    automation_armed_at: Option<Instant>,
    automation_last_signature: Option<AutomationSignature>,
    automation_cycle_started_at: Option<Instant>,
    /// A failed automated upload hands its signature back here so the same
    /// snapshot can be exported again. Without it one transient failure means
    /// no snapshot at all for that login.
    automation_retry_tx: mpsc::UnboundedSender<AutomationSignature>,
    automation_retry_rx: mpsc::UnboundedReceiver<AutomationSignature>,

    /// Debug builds keep a rolling pcapng of everything captured. One writer per
    /// *process*: it used to be one per capture task, which truncated the file
    /// on every capture restart.
    #[cfg(debug_assertions)]
    pcapng: Option<crate::pcapng::PcapngWriter>,

    ctx: egui::Context,

    /// Where captured wish history keys and wishes go; see
    /// [`crate::wish_history`].
    wish_hub: Option<crate::wish_history::WishHub>,

    /// Live sync's sender (`live.rs`), and where its requests answer.
    live: LiveSync,
    live_tx: mpsc::UnboundedSender<LiveEvent>,
    live_rx: mpsc::UnboundedReceiver<LiveEvent>,

    /// The tracker link beyond uploads (`link.rs`): presence heartbeats,
    /// while a credential is set ([`Self::linked`]).
    presence: Heartbeat,
    /// A credential is set (the sign-in, else a verified import key), and
    /// this is not a replay.
    linked: bool,
    link_tx: mpsc::UnboundedSender<LinkEvent>,
    link_rx: mpsc::UnboundedReceiver<LinkEvent>,
    /// This device's alert settings on the tracker; while held, the resin
    /// alert follows them instead of the local setting.
    tracker_alerts: Option<Alerts>,
}

impl Monitor {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        cancel_token: CancellationToken,
        state_tx: watch::Sender<AppState>,
        mut ui_message_rx: mpsc::UnboundedReceiver<Message>,
        log_packet_rx: watch::Receiver<bool>,
        capture_backend: BackendType,
        capture_source: CaptureSource,
        saved_state_rx: watch::Receiver<crate::app::SavedAppState>,
        toast_tx: crate::activity::Sender,
        tracker_auth: TrackerAuth,
        ctx: egui::Context,
    ) -> Result<Self> {
        let mut app_state = AppStateManager::new(state_tx.borrow().clone(), state_tx.clone());
        let game_data = get_database(&mut app_state, &mut ui_message_rx).await?;
        let player_data = PlayerData::new(game_data).with_game_data_sha(embedded_game_data_sha());
        let keys = load_keys()?;
        let sniffer = GameSniffer::new().set_initial_keys(keys);
        let (packet_tx, packet_rx) = mpsc::unbounded_channel();
        let (decoded_tx, decoded_rx) = mpsc::unbounded_channel();
        let (automation_retry_tx, automation_retry_rx) = mpsc::unbounded_channel();
        let (live_tx, live_rx) = mpsc::unbounded_channel();
        let (link_tx, link_rx) = mpsc::unbounded_channel();
        let sniffer = spawn_sniffer_thread(sniffer, decoded_tx)?;

        // One client for the whole process: clones share its connection pool,
        // and building one per request was also how every tracker call ended up
        // with no timeout at all.
        let http = crate::http_client_builder()
            .timeout(TRACKER_REQUEST_TIMEOUT)
            .connect_timeout(TRACKER_CONNECT_TIMEOUT)
            .build()
            .context("Unable to build the tracker HTTP client")?;

        #[cfg(debug_assertions)]
        let pcapng = crate::data_dir().ok().and_then(|mut path| {
            path.push("log");
            std::fs::create_dir_all(&path).ok()?;
            path.push("latest.pcapng");
            match crate::pcapng::PcapngWriter::new(path) {
                Ok(writer) => Some(writer),
                Err(e) => {
                    tracing::warn!("could not open the debug capture file: {e}");
                    None
                }
            }
        });

        // `interval` fires its first tick immediately, which is what makes
        // the "you started Teyvat.me second" case visible at startup rather than
        // two seconds into it. `Delay` keeps a monitor loop that was busy for
        // several seconds from answering with a burst of back-to-back scans.
        let mut game_poll = tokio::time::interval(GAME_POLL_INTERVAL);
        game_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // The last run's alert settings, until the tracker answers.
        let tracker_alerts = saved_state_rx.borrow().tracker_alerts;
        app_state.update_tracker_alerts(tracker_alerts);

        Ok(Self {
            app_state,
            player_data,
            data_replacement: DataReplacement::default(),
            ui_message_rx,
            log_packet_rx,
            sniffer: Some(sniffer),
            decoded_rx,
            backlog_warned: false,
            cancel_token,
            capture_cancel_token: None,
            capture_handle: None,
            packet_tx,
            packet_rx,
            capture_backend,
            capture_source,
            capture_retry_at: None,
            capture_failures: 0,
            capture_silence: CaptureSilence::default(),
            game_watch: GameWatch::default(),
            game_detector: SystemProcessDetector::new(),
            game_poll,
            saved_state_rx,
            toast_tx,
            http,
            tracker_auth,
            packet_log: PacketLog::new(),
            capture_timestamp_ms: None,
            settle_pending_since: None,
            settle_armed_at: None,
            resin_alerted: None,
            automation_pending_since: None,
            automation_armed_at: None,
            automation_last_signature: None,
            automation_cycle_started_at: None,
            automation_retry_tx,
            automation_retry_rx,
            #[cfg(debug_assertions)]
            pcapng,
            ctx,
            wish_hub: None,
            live: LiveSync::new(),
            live_tx,
            live_rx,
            presence: Heartbeat::new(),
            linked: false,
            link_tx,
            link_rx,
            tracker_alerts,
        })
    }

    /// Hand wish history keys and wishes to `hub`.
    pub(crate) fn with_wish_hub(mut self, hub: crate::wish_history::WishHub) -> Self {
        self.wish_hub = Some(hub);
        self
    }

    /// Take the wish history messages out of `classified` and hand them on.
    ///
    /// Not while replaying a recording: its key is old, and nothing may sync.
    fn forward_wish_events(&self, classified: &mut Vec<(u16, CommandMatch)>) {
        let events = crate::wish_history::take_wish_events(classified);
        if events.is_empty() {
            return;
        }
        if !uploads_allowed(&self.capture_source) {
            tracing::debug!("not handing wish history messages on during a replay");
            return;
        }
        if let Some(hub) = &self.wish_hub {
            let uid = self.player_data.gi_player().and_then(|player| player.uid);
            hub.forward(events, uid);
        }
    }

    pub async fn run(mut self) {
        self.app_state.update_app_state(State::Main);
        // A sign-in that loads (or signs in) after data arrived: check the
        // uploads again then, not only at the next new data.
        let mut auth_rx = self.tracker_auth.status();
        let mut settings_rx = self.saved_state_rx.clone();
        self.link_check();

        loop {
            // Checked before the `select!` rather than only inside it. On the
            // self-update restart path the token is already cancelled when
            // `run` is entered, and the startup `Message::StartCapture` is
            // already queued, so `select!` -- which picks pseudo-randomly among
            // ready branches -- would start a capture backend during shutdown
            // about half the time and then race the replacement process for the
            // device.
            if self.cancel_token.is_cancelled() {
                break;
            }

            let explore_held = self.player_data.has_exploration_points();
            let sleep_fut = async {
                if let Some(pending_since) = self.automation_pending_since {
                    let armed_at = self.automation_armed_at.unwrap_or(pending_since);
                    let rem =
                        automation_wait(pending_since, armed_at, explore_held, Instant::now());
                    if !rem.is_zero() {
                        tokio::time::sleep(rem).await;
                    }
                } else {
                    std::future::pending::<()>().await;
                }
            };

            let settle_fut = async {
                match (self.settle_pending_since, self.settle_armed_at) {
                    (Some(pending), Some(armed)) => {
                        let rem = automation_wait(pending, armed, explore_held, Instant::now());
                        if !rem.is_zero() {
                            tokio::time::sleep(rem).await;
                        }
                    }
                    _ => std::future::pending::<()>().await,
                }
            };

            let resin_at = self.resin_alert_time();
            let resin_fut = async move {
                match resin_at {
                    Some(at) => {
                        let now = chrono::Utc::now().timestamp().max(0) as u64;
                        tokio::time::sleep(Duration::from_secs(at.saturating_sub(now))).await;
                    }
                    None => std::future::pending::<()>().await,
                }
            };

            let live_at = self.live_next_tick();
            let live_fut = async move {
                match live_at {
                    Some(at) => {
                        tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await
                    }
                    None => std::future::pending::<()>().await,
                }
            };

            let link_at = self.link_next_ms();
            let link_fut = async move {
                match link_at {
                    Some(at) => {
                        tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await
                    }
                    None => std::future::pending::<()>().await,
                }
            };

            #[rustfmt::skip]
            tokio::select! {
                _ = self.cancel_token.cancelled() => {
                    break;
                }
                _ = sleep_fut => {
                    self.execute_automation_export().await;
                }
                _ = settle_fut => {
                    self.settle_pending_since = None;
                    self.app_state.update_capture_done(true);
                    self.ctx.request_repaint();
                }
                _ = live_fut => {
                    self.live_tick();
                }
                _ = link_fut => {
                    self.link_tick();
                }
                Some(event) = self.link_rx.recv() => {
                    self.handle_link_event(event);
                }
                Ok(()) = auth_rx.changed() => {
                    self.link_check();
                    if self.app_state.app_state.updated.items_updated.is_some()
                        || self.app_state.app_state.updated.characters_updated.is_some()
                    {
                        self.check_automation_trigger();
                    }
                }
                Some(event) = self.live_rx.recv() => {
                    self.handle_live_event(event);
                }
                _ = resin_fut => {
                    self.fire_resin_alert();
                }
                // A settings change can turn the resin alert on or move it,
                // or link a tracker (a verified key).
                Ok(()) = settings_rx.changed() => {
                    self.link_check();
                }
                Some(event) = self.packet_rx.recv() => {
                    self.handle_capture_event(event);
                }
                Some(event) = self.decoded_rx.recv() => {
                    self.handle_sniffer_event(event);
                }
                Some(signature) = self.automation_retry_rx.recv() => {
                    self.release_automation_signature(signature);
                }
                _ = self.game_poll.tick() => {
                    self.poll_game_status();
                    self.supervise_capture().await;
                    self.watch_capture_silence().await;
                }
                capture_result = join_capture(&mut self.capture_handle) => {
                    self.handle_capture_exit(capture_result);
                }
                Some(msg) = self.ui_message_rx.recv() => self.handle_ui_msg(msg).await,
            }
        }

        // The backend is only released when the capture task's `Drop` runs, and
        // `main` does not wait for the tokio runtime to unwind before spawning
        // the replacement executable on the self-update path. Releasing the
        // device here is what keeps the two processes from fighting over it.
        self.stop_capture().await;
    }

    async fn handle_ui_msg(&mut self, msg: Message) {
        match msg {
            Message::StartCapture => self.start_capture().await,
            Message::StopCapture => {
                // Reported before the (bounded, but not instant) teardown, so
                // the UI reacts to the click rather than to the backend.
                self.set_capturing(false);
                self.stop_capture().await;
            }
            Message::ClearData => {
                self.data_replacement.clear();
                self.replace_captured_data("clear data requested");
                // Nothing has decoded for whatever comes next, so the status
                // line drops back to what the process transitions can prove.
                if self.game_watch.note_data_cleared() {
                    let status = self.game_watch.status();
                    self.app_state.update_game_status(status);
                }
            }
            Message::KillGame(reply_tx) => {
                let killed = self.game_detector.kill_game_processes();
                tracing::info!("Kill game requested: {killed} game process(es) terminated");

                // The kill is asynchronous at the OS level, so an immediate
                // rescan can still see the dying process and leave the status
                // line stuck on the very warning the user just acted on. One
                // short wait is enough, and it costs a user-initiated click's
                // worth of latency on this loop rather than a poll interval's
                // worth of a stale, alarming line.
                tokio::time::sleep(KILL_GAME_SETTLE).await;
                self.poll_game_status();

                let _ = reply_tx.send(killed);
                self.ctx.request_repaint();
            }
            // The UI picks the settings: the user's for a Genshin Optimizer
            // file or clipboard export, the tracker's for an upload (see
            // `OptimizerExportTarget::export_settings` in `app.rs`).
            Message::ExportGenshinOptimizer(settings, reply_tx) => {
                let result = match self
                    .player_data
                    .export_genshin_optimizer_with_report(&settings)
                {
                    Ok((json, report)) => {
                        tracing::info!("export requested from the UI: {}", report.counts());
                        self.report_export_gaps(&report);
                        Ok((json, report.counts()))
                    }
                    Err(e) => Err(e),
                };
                let _ = reply_tx.send(result);
                self.ctx.request_repaint();
            }
            Message::ExportAchievements(reply_tx) => {
                let _ = reply_tx.send(self.player_data.export_achievements());
                self.ctx.request_repaint();
            }
            Message::VerifyTrackerKey(url, key, reply_tx) => {
                let ctx = self.ctx.clone();
                let client = self.http.clone();
                tokio::spawn(async move {
                    let result = verify_tracker_key(&client, &url, &key).await;
                    // For uploads while no frame is drawn (hidden in the tray).
                    crate::app::note_key_check(&key, result.is_ok());
                    let _ = reply_tx.send(result);
                    ctx.request_repaint();
                });
            }
            Message::UploadToTracker(json, credential, reply_tx) => {
                let (json, ticket) = self.manual_snapshot(json);
                self.spawn_tracker_upload(credential, json, UploadReport::Reply(reply_tx), ticket);
            }
            Message::Tracker(request) => {
                self.tracker_auth.handle(self.http.clone(), request);
            }
            _ => (),
        }
    }

    /// Start a capture backend, replacing any that is still running.
    async fn start_capture(&mut self) {
        if self.capture_cancel_token.is_some() || self.capture_handle.is_some() {
            tracing::warn!("Capture start request with a capture already running. Replacing it.");
        }
        self.stop_capture().await;

        let cancel_token = CancellationToken::new();
        let capture_handle = tokio::spawn(capture_task(
            cancel_token.clone(),
            self.packet_tx.clone(),
            self.capture_backend,
            self.capture_source.clone(),
        ));
        self.capture_cancel_token = Some(cancel_token);
        self.capture_handle = Some(capture_handle);
        self.capture_silence.capture_started();
        self.automation_cycle_started_at = Some(Instant::now());
        // Not `true`: everything that can fail -- no elevation, no such device,
        // an ETW session already owned by a previous process -- fails inside the
        // task, so a spawned task is not a running capture. The task says so
        // itself with `CaptureEvent::Started`.
        self.set_capturing(false);
    }

    /// Bring capture back up if it is not running.
    ///
    /// Capture is always-on: there is no user control for it, so nothing else
    /// would ever restart a backend that died. Driven from the game-status poll
    /// rather than a timer of its own, because that already ticks at a sensible
    /// rate and this only has to be approximately prompt.
    async fn supervise_capture(&mut self) {
        if self.capture_handle.is_some() || self.cancel_token.is_cancelled() {
            return;
        }
        if self.capture_retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }

        let attempt = self.capture_failures.saturating_add(1);
        tracing::info!(attempt, "restarting packet capture");
        self.start_capture().await;

        // Armed before the outcome is known: if this attempt also fails, the
        // next one has to wait. `CaptureEvent::Started` clears it.
        let backoff = CAPTURE_RETRY_BASE
            .saturating_mul(1u32 << self.capture_failures.min(4))
            .min(CAPTURE_RETRY_MAX);
        self.capture_failures = attempt;
        self.capture_retry_at = Some(Instant::now() + backoff);
    }

    /// Restart a capture that stopped delivering frames while the game runs.
    ///
    /// The supervisor above only sees a backend that failed or ended. One that
    /// stays up and silently stops delivering -- its packet stream still open,
    /// nothing in it -- looks healthy to everything else in this app.
    async fn watch_capture_silence(&mut self) {
        let Some(silence) = self.capture_silence.stalled(
            self.game_watch.game_running(),
            self.app_state.app_state.capturing,
            Instant::now(),
        ) else {
            return;
        };
        tracing::warn!(
            silent_secs = silence.as_secs(),
            "no packets captured for a minute while the game is running; restarting capture"
        );
        self.start_capture().await;
    }

    /// Cancel the capture task *and wait for it to finish*.
    ///
    /// The wait is the point: the backend is only released when the task's
    /// `Drop` runs, so a replacement started before that races the corpse of its
    /// predecessor for a device (pcap) or for a fixed-name ETW session (pktmon),
    /// and loses without saying anything.
    async fn stop_capture(&mut self) {
        if let Some(cancel_token) = self.capture_cancel_token.take() {
            cancel_token.cancel();
        }
        let Some(handle) = self.capture_handle.take() else {
            return;
        };

        match tokio::time::timeout(CAPTURE_SHUTDOWN_TIMEOUT, handle).await {
            Ok(result) => self.report_capture_exit(result),
            Err(_) => tracing::warn!(
                "capture task did not stop within {CAPTURE_SHUTDOWN_TIMEOUT:?}; \
                 continuing without it"
            ),
        }
    }

    fn handle_capture_event(&mut self, event: CaptureEvent) {
        match event {
            // Every capture task shares one channel, so a `Started` from a task
            // that has since been stopped can still be queued behind its own
            // packets. Reporting that as "capturing" would put the lie the
            // status line used to tell straight back into it.
            CaptureEvent::Started if self.capture_handle.is_some() => {
                tracing::info!("capture backend is running");
                // It came up, so the next failure starts its backoff from
                // scratch rather than inheriting a long delay from an outage
                // that is over.
                self.capture_failures = 0;
                self.capture_retry_at = None;
                self.set_capturing(true);
                self.ctx.request_repaint();
            }
            CaptureEvent::Started => {
                tracing::debug!("ignoring a start report from a capture that already stopped");
            }
            CaptureEvent::Packet(packet) => {
                self.capture_silence.note_frame(Instant::now());
                self.handle_packet(packet);
            }
            CaptureEvent::Failed(e) => {
                tracing::error!("Capture task encountered an error: {e}");
                let _ = self.toast_tx.send(Event::error(
                    Topic::Capture,
                    trf("Capture stopped: {0}", &[&e]),
                ));
                self.set_capturing(false);
                self.capture_cancel_token = None;
                self.ctx.request_repaint();
            }
        }
    }

    /// The capture task ended.
    ///
    /// Anything but a clean end is worth a toast: this used to be a `JoinHandle`
    /// nobody ever read, so a backend that failed to start -- or a task that
    /// panicked -- left the UI claiming to be capturing, with no log line, no
    /// toast, and nothing to notice.
    fn handle_capture_exit(&mut self, result: std::result::Result<Result<()>, JoinError>) {
        // A completed `JoinHandle` panics when polled again.
        self.capture_handle = None;
        self.capture_cancel_token = None;
        self.report_capture_exit(result);
        self.set_capturing(false);
        self.ctx.request_repaint();
    }

    fn report_capture_exit(&self, result: std::result::Result<Result<()>, JoinError>) {
        match result {
            Ok(Ok(())) => tracing::info!("capture task finished"),
            Ok(Err(e)) => {
                tracing::error!("capture task ended with an error: {e}");
                let _ = self.toast_tx.send(Event::error(
                    Topic::Capture,
                    trf("Capture stopped: {0}", &[&e]),
                ));
            }
            Err(e) => {
                tracing::error!("capture task did not finish cleanly: {e}");
                let _ = self.toast_tx.send(Event::error(
                    Topic::Capture,
                    tr("Capture stopped unexpectedly · see the log"),
                ));
            }
        }
    }

    /// Report the capture backend's state, and re-judge the game against it.
    ///
    /// The two travel together deliberately. Capture stopping while the game
    /// keeps running is precisely what turns a capturable session into an
    /// uncapturable one -- KCP is strictly ordered and this app can never ask
    /// for a retransmit -- so the verdict has to move with the backend rather
    /// than wait out a poll interval and let the line lag the button.
    fn set_capturing(&mut self, capturing: bool) {
        self.app_state.update_capturing_state(capturing);
        let game_status = self.game_watch.recheck(capturing);
        self.app_state.update_game_status(game_status);
    }

    /// Look for the game process and publish what its presence means.
    ///
    /// Nothing here talks to the UI directly: the status goes out on the
    /// `AppState` watch channel, and the repaint task `app.rs` already runs on
    /// the other end does the rest.
    fn poll_game_status(&mut self) {
        let first_look = !self.game_watch.has_polled();
        let capturing = self.app_state.app_state.capturing;
        let status = self.game_watch.poll(&mut self.game_detector, capturing);
        if let Some(sniffer) = &self.sniffer {
            let started = self.game_detector.game_started_ms().unwrap_or(0);
            sniffer.game_started_ms.store(started, Ordering::Relaxed);
        }

        // The two facts that explain any wrong verdict: whether the scan can
        // name the game's process on this machine at all, and how stale the
        // "data decrypting" claim is.
        tracing::debug!(
            ?status,
            capturing,
            scan_ever_found_game = self.game_watch.scan_ever_found_game(),
            decoded_age_secs = self.game_watch.decoded_age().map(|d| d.as_secs()),
            "game poll"
        );

        // The log record behind `app.rs`'s modal, so a support log shows why a
        // session captured nothing. An already-open game is not a miss by
        // itself (it may still be on the title screen); only game traffic
        // without a login makes it one.
        if first_look && status == GameStatus::AwaitingLogin {
            tracing::info!(
                "Genshin was already open when Teyvat.me started; waiting to see the login \
                 (entering the world)"
            );
        }
        let missed = GameStatus::LaunchMissed(game_watch::MissedLaunch::AlreadyRunning);
        if status == missed && self.app_state.app_state.game_status != missed {
            tracing::warn!(
                "Genshin was already in the world when Teyvat.me started: game traffic arrived \
                 with no login in it, so the session key was missed and nothing from this \
                 game session can be decrypted"
            );
        }

        let was = self.app_state.app_state.game_status;
        self.app_state.update_game_status(status);
        // The game closed: the login is over (live sync's logout snapshot).
        if status == GameStatus::NotRunning && was != GameStatus::NotRunning {
            self.live.note_connection_ended();
            self.live_tick();
        }
    }

    /// Hand a captured frame to the sniffer thread.
    fn handle_packet(&mut self, packet: Vec<u8>) {
        #[cfg(debug_assertions)]
        if let Some(writer) = self.pcapng.as_mut() {
            // Nanoseconds: `pcapng.rs` declares `if_tsresol = 9` in the
            // interface description block.
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since_epoch| since_epoch.as_nanos() as u64)
                .unwrap_or_default();
            let _ = writer.write_packet(ts, &packet);
        }

        // Game traffic: for a game that was already open, starts the clock on
        // telling "still on the title screen" from "already in the world".
        self.game_watch.note_game_traffic();

        let Some(sniffer) = self.sniffer.as_ref() else {
            return;
        };

        let queued = sniffer.queued.fetch_add(1, Ordering::Relaxed) + 1;
        if queued >= PACKET_BACKLOG_WARN && !self.backlog_warned {
            self.backlog_warned = true;
            tracing::warn!(queued, "packet decoding is falling behind capture");
        } else if self.backlog_warned && queued < PACKET_BACKLOG_WARN / 2 {
            self.backlog_warned = false;
        }

        if sniffer.packet_tx.send(packet).is_err() {
            self.sniffer = None;
            tracing::error!("the packet decoding thread is gone; no further data can be captured");
            let _ = self.toast_tx.send(Event::error(
                Topic::Capture,
                tr("Decoding stopped · restart Teyvat.me to capture again"),
            ));
            self.set_capturing(false);
            self.ctx.request_repaint();
        }
    }

    fn handle_sniffer_event(&mut self, event: SnifferEvent) {
        match event {
            // Sent by the sniffer thread before any packet from the new
            // connection, so nothing from the new account can land in the old
            // account's maps: the new connection's first data replaces them
            // (see `DataReplacement`). Captured state is insert-only, so without
            // this a second account logged in without restarting Teyvat.me
            // uploads the *union* of both inventories to whichever tracker
            // account holds the import key.
            SnifferEvent::SessionReset => self.handle_connection_reset(),
            SnifferEvent::Packet(packet, report) => self.handle_game_packet(packet, report),
        }
    }

    /// The game connection restarted. The captured data stays until the new
    /// connection replaces it (see [`DataReplacement`]); the old connection's
    /// claim to be decrypting ends now.
    fn handle_connection_reset(&mut self) {
        if self.data_replacement.connection_reset() {
            tracing::info!(
                "new game connection; keeping the captured data until it delivers its own"
            );
            // The login is over: a journal with anything in it ends on a
            // snapshot of the data as it is, before a new login replaces it.
            self.live.note_connection_ended();
            self.live_tick();
        }
        if self.game_watch.note_session_reset() {
            let status = self.game_watch.status();
            self.app_state.update_game_status(status);
        }
        self.ctx.request_repaint();
    }

    fn handle_game_packet(&mut self, game_packet: GamePacket, report: KeyReport) {
        let key = report.state;
        if report.recovery_failed && self.game_watch.note_key_recovery_failed() {
            let status = self.game_watch.status();
            self.app_state.update_game_status(status);
        }

        let commands = match game_packet {
            GamePacket::Commands(commands) => commands,
            GamePacket::Connection(conn) => {
                self.handle_connection_packet(&conn);
                return;
            }
        };

        // Non-empty is the whole point: a bare KCP ACK decodes to
        // `Commands(vec![])` with no key involved at all, while an actual
        // command batch means the XOR stream decrypted and the protobuf parsed.
        // Under the session key that is the evidence the status line may claim
        // "decrypting" on. Under the dispatch key it is only a login being
        // watched -- every reconnect of 2026-10-04 got that far and no further
        // while the line read "data decrypting" -- so it starts the clock on
        // the key arriving instead.
        if !commands.is_empty() {
            let changed = if key == KeyState::Session {
                self.game_watch.note_decoded_data()
            } else {
                self.game_watch.note_login_traffic()
            };
            if changed {
                let status = self.game_watch.status();
                self.app_state.update_game_status(status);
            }
        }

        let log_packets = *self.log_packet_rx.borrow_and_update();
        if log_packets {
            for command in &commands {
                let _span = tracing::info_span!("command", id = command.command_id).entered();
                // The wish history key is a password: never on disk.
                if teyvatme_protocol::matches_get_authkey_rsp(&command.proto_data).is_some() {
                    continue;
                }
                if let Err(e) = self.packet_log.append(command) {
                    tracing::info!("error logging command {e}");
                }
            }
        }

        // Classify the whole batch first: whether it replaces the previous
        // connection's data has to be settled before its timestamps are taken,
        // because replacing restarts the automation cycle they are compared
        // against.
        let mut classified = classify_commands(&commands);
        self.forward_wish_events(&mut classified);
        if classified.is_empty() {
            return;
        }

        match self
            .data_replacement
            .verdict_for_batch(key, &mut classified)
        {
            DataVerdict::Apply => {}
            DataVerdict::ReplaceThenApply => {
                self.replace_captured_data("the new game connection delivered data");
            }
            DataVerdict::Hold => {
                tracing::debug!(
                    commands = classified.len(),
                    ?key,
                    "keeping the previous connection's data; not applying these"
                );
                return;
            }
        }

        let full_send = full_send_in(&classified, self.player_data.item_count());
        let noted = changes_in(&classified);
        self.player_data
            .set_clock(chrono::Utc::now().timestamp().max(0) as u64);
        let applied = apply_commands(&mut self.player_data, classified);
        if !applied.any() {
            return;
        }
        if full_send {
            self.live.note_full_send();
        }
        self.live.note_change(noted + applied.live_updates);

        let mut updated = self.app_state.app_state.updated.clone();
        let now = instant_after(self.automation_cycle_started_at);
        if applied.items {
            updated.items_updated = Some(now);
        }
        if applied.characters {
            updated.characters_updated = Some(now);
        }
        if applied.achievements {
            updated.achievements_updated = Some(now);
            updated.achievements_updated_time = Some(chrono::Local::now());
        }
        // Properties deliberately stamp no timestamp: a property packet used
        // to set `items_updated`, so the UI's "Items" tick and the
        // export/upload readiness gate could both be green with not one item
        // ever parsed. They are still new data, though.

        self.capture_timestamp_ms = Some(Local::now().timestamp_millis());
        let player = self.player_data.gi_player();
        // The login's UID binds its wish key; another UID is a new login.
        if let Some(hub) = &self.wish_hub {
            hub.note_uid(player.as_ref().and_then(|player| player.uid));
        }
        let counts = self.player_data.held_counts();
        let tasks = self
            .player_data
            .gi_tasks(chrono::Utc::now().timestamp().max(0) as u64);
        let anecdotes = self.player_data.recorded_anecdotes();
        self.app_state
            .update_captured(updated, player, Some(counts), tasks, anecdotes);
        // A countdown waits for the last of a burst (the world's parts come
        // seconds after the bag).
        if let (Some(pending), Some(armed)) =
            (&mut self.automation_pending_since, self.automation_armed_at)
        {
            *pending = rearmed(*pending, armed, Instant::now());
        }
        self.note_capture_settling();
        self.check_automation_trigger();
    }

    /// Log a connection event. Deliberately does not touch captured data.
    ///
    /// None of these packets is authenticated: any process able to put a 20-byte
    /// datagram on a game port produces a `HandshakeRequested` or a
    /// `Disconnected`, so resetting on one is a one-packet "erase this user's
    /// capture" button -- and the "wait for the next command" latch that used to
    /// stand in for authentication was no protection either, since an ACK of the
    /// *still-live* session decodes to `Commands(vec![])` and consumed it.
    ///
    /// The reset now rides on [`SnifferEvent::SessionReset`], which
    /// teyvatme-protocol only raises once it has corroborated the reconnect.
    /// A real disconnect is always followed by the handshake it corroborates,
    /// so nothing is lost by ignoring these here.
    fn handle_connection_packet(&mut self, conn: &ConnectionPacket) {
        match conn {
            ConnectionPacket::HandshakeRequested | ConnectionPacket::HandshakeEstablished => {
                if matches!(conn, ConnectionPacket::HandshakeRequested) {
                    tracing::info!("Connection: Handshake Requested");
                } else {
                    tracing::info!("Connection: Handshake Established");
                }
                // A login being watched: whatever the process timing said, this
                // connection's key exchange is inside the capture window.
                if self.game_watch.note_login_seen() {
                    let status = self.game_watch.status();
                    self.app_state.update_game_status(status);
                }
            }
            ConnectionPacket::Disconnected => {
                tracing::info!("Connection: Disconnected");
            }
            // A KCP segment that did not complete a command.
            _ => {}
        }
    }

    /// Forget everything captured about the account -- its wish key too: a
    /// new connection's data may be another account's (alt swapping), and a
    /// key only counts for the login it came after.
    fn replace_captured_data(&mut self, reason: &str) {
        tracing::info!(reason, "clearing captured data");
        if let Some(hub) = &self.wish_hub {
            hub.new_login();
        }
        self.player_data.reset();
        self.live.note_data_replaced();
        self.app_state
            .update_captured(DataUpdated::new(), None, None, None, None);
        self.capture_timestamp_ms = None;
        // Without these the signature check can suppress the first export after
        // an account switch, because it still matches the previous account's.
        self.settle_pending_since = None;
        self.settle_armed_at = None;
        self.app_state.update_capture_done(false);
        self.automation_pending_since = None;
        self.automation_last_signature = None;
        self.automation_cycle_started_at = Some(Instant::now());
        self.ctx.request_repaint();
    }

    /// Tell the user when an export silently left entities out.
    ///
    /// The game data is baked in at build time, so after a Genshin version bump
    /// a brand new character or artifact set simply vanishes from the snapshot
    /// -- indistinguishable, on the dashboard, from never having owned it.
    fn report_export_gaps(&self, report: &crate::player_data::ExportReport) {
        if report.is_empty() {
            return;
        }
        tracing::warn!("export dropped entities: {}", report.summary());
        let _ = self.toast_tx.send(Event::warn(
            Topic::Export,
            trf("Export incomplete: {0}", &[&report.summary()]),
        ));
    }

    /// The settings with the live sign-in and key state; see
    /// [`crate::app::background_state`].
    fn upload_state(&self) -> crate::app::SavedAppState {
        crate::app::background_state(
            &self.saved_state_rx.borrow(),
            &self.tracker_auth.status().borrow(),
        )
    }

    /// Start or restart the "Capture done" countdown: once the login's bag
    /// and characters are in, it waits like a snapshot does (quiet for
    /// [`AUTOMATION_DEBOUNCE`], the exploration up to
    /// [`AUTOMATION_EXPLORE_WAIT`], [`AUTOMATION_MAX_WAIT`] at most). Done
    /// stays done until a new login replaces the data.
    fn note_capture_settling(&mut self) {
        if self.app_state.app_state.capture_done {
            return;
        }
        let updated = &self.app_state.app_state.updated;
        let Some(cycle) = self.automation_cycle_started_at else {
            return;
        };
        let since_cycle = |at: Option<Instant>| at.is_some_and(|at| at > cycle);
        if !since_cycle(updated.items_updated) || !since_cycle(updated.characters_updated) {
            return;
        }
        let now = Instant::now();
        match (self.settle_pending_since, self.settle_armed_at) {
            (Some(pending), Some(armed)) => {
                self.settle_pending_since = Some(rearmed(pending, armed, now));
            }
            _ => {
                self.settle_pending_since = Some(now);
                self.settle_armed_at = Some(now);
            }
        }
    }

    /// The amounts the resin alert goes at: the tracker's settings for this
    /// device while held, else the local setting.
    fn resin_targets(&self) -> [Option<u32>; 2] {
        if let Some(alerts) = self.tracker_alerts {
            return alerts.resin_targets();
        }
        let state = self.saved_state_rx.borrow();
        [
            state
                .resin_alert
                .then_some(state.resin_alert_at.clamp(1, RESIN_CAP)),
            None,
        ]
    }

    /// When the next resin alert is due (unix seconds) and its amount, or
    /// `None`: off, no resin clock, or every amount already sent for this
    /// clock. Never during a replay.
    fn resin_alert_due(&self) -> Option<(u64, u32)> {
        if !uploads_allowed(&self.capture_source) {
            return None;
        }
        let resin = self.app_state.app_state.tasks.as_ref()?.resin?;
        next_resin_alert(resin, self.resin_targets(), self.resin_alerted)
    }

    fn resin_alert_time(&self) -> Option<u64> {
        self.resin_alert_due().map(|(at, _)| at)
    }

    /// Send the resin alert: "Resin 160/200 · Full at 14:32".
    fn fire_resin_alert(&mut self) {
        let Some((at, target)) = self.resin_alert_due() else {
            return;
        };
        let now = chrono::Utc::now().timestamp().max(0) as u64;
        if at > now {
            return;
        }
        let Some(resin) = self
            .app_state
            .app_state
            .tasks
            .as_ref()
            .and_then(|t| t.resin)
        else {
            return;
        };
        self.resin_alerted = Some((resin, target));
        let title = trf("Resin {0}/{1}", &[&target, &RESIN_CAP]);
        let text = match resin_reaches(resin, RESIN_CAP).filter(|full| *full > now) {
            Some(full) => {
                let at = chrono::DateTime::from_timestamp(full as i64, 0)
                    .map(|at| {
                        at.with_timezone(&chrono::Local)
                            .format(crate::clock::hm())
                            .to_string()
                    })
                    .unwrap_or_default();
                trf("Full at {0}", &[&at])
            }
            None => tr("Resin is full").to_owned(),
        };
        tracing::info!(target, "resin alert");
        tokio::task::spawn_blocking(move || {
            if let Err(e) = crate::notify::show(&title, &text) {
                tracing::warn!("the resin alert could not be shown: {e:#}");
            }
        });
    }

    fn check_automation_trigger(&mut self) {
        if !uploads_allowed(&self.capture_source) {
            return;
        }
        let saved_state = self.upload_state();
        // The tracker half uses the same predicate the manual upload button
        // does. This used to omit `tracker_verified`, so a key the dashboard
        // had revoked still got a POST on every login.
        let plan = AutomationPlan::for_state(&saved_state);

        if plan.is_empty() {
            self.automation_pending_since = None;
            return;
        }

        let updated = &self.app_state.app_state.updated;

        let Some(cycle_started) = self.automation_cycle_started_at else {
            return;
        };
        // Only the data classes the plan's exports will actually write, and
        // all of them captured since the current capture cycle began.
        // Demanding achievements as well -- as this and both manual export
        // gates used to -- meant an account whose achievement packet was never
        // identified got no automated export at all, not even of the
        // characters and artifacts it did capture.
        let classes: Vec<_> = plan
            .settings()
            .flat_map(|settings| crate::app::export_data_classes(settings, updated))
            .collect();
        if classes.is_empty()
            || !classes
                .iter()
                .all(|class| class.captured_at.is_some_and(|at| at > cycle_started))
        {
            return;
        }

        let signature = (
            updated.items_updated,
            updated.characters_updated,
            updated.achievements_updated,
        );

        // What stops an export repeating is this signature, not a capture
        // restart: the cycle only advances when genuinely new data arrives.
        if self.automation_last_signature == Some(signature) {
            return;
        }

        if self.automation_pending_since.is_none() {
            tracing::info!(
                "All automation triggers met! Starting {} second countdown...",
                AUTOMATION_DEBOUNCE.as_secs()
            );
            let now = Instant::now();
            self.automation_pending_since = Some(now);
            self.automation_armed_at = Some(now);
        }
    }

    /// Let a snapshot that failed to reach its destination be exported again.
    fn release_automation_signature(&mut self, signature: AutomationSignature) {
        if release_signature(&mut self.automation_last_signature, signature) {
            tracing::info!("automation export did not complete; it will be retried");
        }
    }

    async fn execute_automation_export(&mut self) {
        tracing::info!("Executing background automation export!");
        self.automation_pending_since = None;
        let saved_state = self.upload_state();
        let plan = AutomationPlan::for_state(&saved_state);

        let signature = (
            self.app_state.app_state.updated.items_updated,
            self.app_state.app_state.updated.characters_updated,
            self.app_state.app_state.updated.achievements_updated,
        );

        // Both exports are built before anything is written or sent. The
        // signature is deliberately not recorded when either fails: a failed
        // export should be retried when the next data arrives.
        let mut file_export = None;
        if let Some(settings) = &plan.file {
            let Some(export) = self.automation_export(settings, "automation file") else {
                return;
            };
            file_export = Some(export);
        }
        // Live sync: in live mode the tracker gets a snapshot only for the
        // game's full sends (batches carry the rest); otherwise every change,
        // as before, labelled with why.
        let live_reason = self
            .live
            .automation_reason(live_enabled(&saved_state), now_ms());
        let mut tracker_export = None;
        if plan.tracker.is_some()
            && let Some(reason) = live_reason
        {
            tracker_export = Some(self.live_snapshot_export(reason));
        }
        // One toast, not one per export: the tracker's export leaves nothing
        // out by choice, so every gap in the file's is in the tracker's too.
        if let Some(report) = tracker_export
            .as_ref()
            .map(|(_, report, _)| report)
            .or(file_export.as_ref().map(|(_, report)| report))
        {
            self.report_export_gaps(report);
        }
        // Recorded now so a second trigger for the same data cannot start a
        // second export while this one is in flight, and given back below (or
        // by `release_automation_signature`) if any half of it failed.
        self.automation_last_signature = Some(signature);

        if let Some((json, _)) = file_export {
            match self.save_to_automation_file(&saved_state, &json) {
                Ok(path) => {
                    let _ = self.toast_tx.send(
                        Event::ok(
                            Topic::Export,
                            trf("Auto-exported {0}", &[&file_name(&path)]),
                        )
                        .with_path(path),
                    );
                }
                Err(e) => {
                    let _ = self.toast_tx.send(Event::error(
                        Topic::Export,
                        trf("Auto-export failed: {0}", &[&e]),
                    ));
                    self.release_automation_signature(signature);
                }
            }
        }

        if let Some((json, report, ticket)) = tracker_export
            && let Some(credential) = crate::app::upload_credential(&saved_state)
        {
            let host = destination_host(&saved_state);
            let counts = crate::app::counts_line(&report.counts());
            let _ = self.toast_tx.send(Event::info(
                Topic::Destination,
                trf("Uploading to {0} · {1}", &[&host, &counts]),
            ));
            self.spawn_tracker_upload(
                credential,
                json,
                UploadReport::Toast {
                    retry_tx: self.automation_retry_tx.clone(),
                    signature,
                },
                Some(ticket),
            );
        }

        // Capture is deliberately left running. Restarting it here used to make
        // the first automated export the last one of the game session: KCP
        // delivery is strictly ordered and this app can never ask for a
        // retransmit, so every segment missed during the restart stalled that
        // direction permanently -- with the UI still showing "Capturing".
        self.ctx.request_repaint();
    }

    /// Build one of an automation run's exports, or toast why it could not be
    /// built. `purpose` names it in the log line that gives its counts.
    fn automation_export(
        &self,
        settings: &ExportSettings,
        purpose: &str,
    ) -> Option<(String, ExportReport)> {
        match self
            .player_data
            .export_genshin_optimizer_with_report(settings)
        {
            Ok((json, report)) => {
                tracing::info!("{purpose} export: {}", report.counts());
                Some((json, report))
            }
            Err(e) => {
                let what = purpose_label(purpose);
                let _ = self.toast_tx.send(Event::error(
                    Topic::Export,
                    trf("Couldn't build the {0} export: {1}", &[&what, &e]),
                ));
                None
            }
        }
    }

    fn save_to_automation_file(
        &self,
        saved_state: &crate::app::SavedAppState,
        json: &str,
    ) -> Result<std::path::PathBuf> {
        let output_dir = if let Some(folder) = &saved_state.save_result_folder {
            folder.clone()
        } else {
            let exe_path =
                std::env::current_exe().context("Unable to locate current executable")?;
            exe_path
                .parent()
                .map(|path| path.to_path_buf())
                .unwrap_or(std::env::current_dir()?)
        };

        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Unable to create output directory {:?}", output_dir))?;
        let file_name =
            teyvatme_core::export_file_name(chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"));
        let path = output_dir.join(file_name);

        // `fs::write` rather than a `BufWriter`: the buffered version reported
        // success from `write_all`, leaving whatever was still in the buffer to
        // a `Drop` that discards its error.
        std::fs::write(&path, json).with_context(|| format!("Unable to write file {:?}", path))?;

        Ok(path)
    }

    fn spawn_tracker_upload(
        &self,
        credential: UploadCredential,
        json: String,
        report: UploadReport,
        live_ticket: Option<u64>,
    ) {
        let live_tx = self.live_tx.clone();
        let client = self.http.clone();
        let auth = self.tracker_auth.clone();
        let ctx = self.ctx.clone();
        let toast_tx = self.toast_tx.clone();
        let host = destination_host(&self.saved_state_rx.borrow());
        let captured_at = self.capture_timestamp_ms;
        let allowed = uploads_allowed(&self.capture_source);

        tokio::spawn(async move {
            let result = if !allowed {
                tracing::warn!("{REPLAY_UPLOAD_REFUSED}");
                #[rustfmt::skip]
                let refused = tr("Uploads are off while replaying a recording (-r); use --replay-export to inspect one");
                Err(refused.to_string())
            } else {
                match credential {
                    UploadCredential::ImportKey { url, key } => {
                        upload_to_tracker(&client, &url, &key, json, captured_at).await
                    }
                    UploadCredential::SignedIn { base } => {
                        auth.upload_answer(
                            &client,
                            &base,
                            json,
                            captured_at,
                            TRACKER_UPLOAD_TIMEOUT,
                        )
                        .await
                    }
                }
            };
            if let Some(ticket) = live_ticket {
                let answer = result.as_ref().ok().map(|body| ImportAnswer::parse(body));
                let _ = live_tx.send(LiveEvent::Snapshot { ticket, answer });
            }
            let result = result.map(|_| ());
            match report {
                UploadReport::Reply(reply_tx) => {
                    let _ = reply_tx.send(result);
                }
                UploadReport::Toast {
                    retry_tx,
                    signature,
                } => {
                    let _ = match result {
                        Ok(()) => toast_tx.send(Event::ok(
                            Topic::Destination,
                            trf("Snapshot stored on {0}", &[&host]),
                        )),
                        Err(e) => {
                            let _ = retry_tx.send(signature);
                            toast_tx.send(Event::error(
                                Topic::Destination,
                                trf("{0} upload failed · {1}", &[&host, &e]),
                            ))
                        }
                    };
                }
                UploadReport::Asked => {
                    let _ = toast_tx.send(match result {
                        Ok(()) => {
                            Event::ok(Topic::Destination, trf("Snapshot stored on {0}", &[&host]))
                        }
                        Err(e) => Event::error(
                            Topic::Destination,
                            trf("{0} upload failed · {1}", &[&host, &e]),
                        ),
                    });
                }
                UploadReport::Quiet => {
                    let _ = toast_tx.send(
                        match result {
                            Ok(()) => Event::ok(
                                Topic::Destination,
                                trf("Live sync · snapshot stored on {0}", &[&host]),
                            ),
                            Err(e) => Event::error(
                                Topic::Destination,
                                trf("{0} upload failed · {1}", &[&host, &e]),
                            ),
                        }
                        // Live sync stores these by itself; a failure still toasts.
                        .routine(),
                    );
                }
            }
            ctx.request_repaint();
        });
    }
}

impl Monitor {
    /// When live sync next has something to decide, in Unix ms.
    fn live_next_tick(&self) -> Option<u64> {
        let state = self.upload_state();
        if !uploads_allowed(&self.capture_source) || !crate::app::want_tracker_upload(&state) {
            return None;
        }
        self.live.next_tick_ms(live_enabled(&state), now_ms())
    }

    /// Let live sync send what is due: a batch, or a snapshot it asked for.
    fn live_tick(&mut self) {
        if !uploads_allowed(&self.capture_source) {
            return;
        }
        let state = self.upload_state();
        let Some(credential) = crate::app::upload_credential(&state) else {
            return;
        };
        if !state.auto_export_to_tracker {
            return;
        }
        let now = now_ms();
        let player_data = &self.player_data;
        let action = self.live.tick(live_enabled(&state), now, || {
            let (good, _, keyed) = player_data.live_export(now);
            Some((good, keyed))
        });
        match action {
            Action::Nothing => {}
            Action::Snapshot(reason) => {
                let (json, report, ticket) = self.live_snapshot_export(reason);
                tracing::info!(
                    reason = reason.as_str(),
                    "live sync: snapshot: {}",
                    report.counts()
                );
                self.spawn_tracker_upload(credential, json, UploadReport::Quiet, Some(ticket));
            }
            Action::Batch(delta) => self.spawn_live_batch(&state, credential, *delta),
        }
    }

    /// The tracker export for a snapshot taken for `reason`, labelled with
    /// its `gi_capture`, and the live sync ticket that follows its answer.
    fn live_snapshot_export(&mut self, reason: Reason) -> (String, ExportReport, u64) {
        let (mut good, report, keyed) = self.player_data.live_export(now_ms());
        tracing::info!(
            "tracker snapshot ({}): {}",
            reason.as_str(),
            report.counts()
        );
        good.gi_capture = self.live.capture(reason);
        let json = serde_json::to_string(&good).unwrap_or_default();
        let ticket = self.live.snapshot_started(reason, keyed);
        (json, report, ticket)
    }

    /// The UI's upload button: its export, labelled `manual`, and the live
    /// sync ticket that follows its answer (`None` when it is not a GOOD
    /// export this can read).
    fn manual_snapshot(&mut self, json: String) -> (String, Option<u64>) {
        let Ok(mut good) = serde_json::from_str::<crate::good::Good>(&json) else {
            return (json, None);
        };
        let Some(capture) = self.live.capture(Reason::Manual) else {
            return (json, None);
        };
        good.gi_capture = Some(capture);
        let Ok(labelled) = serde_json::to_string(&good) else {
            return (json, None);
        };
        let keyed = self.player_data.keyed(&good);
        let ticket = self.live.snapshot_started(Reason::Manual, keyed);
        (labelled, Some(ticket))
    }

    /// POST one batch; its answer comes back as a [`LiveEvent::Batch`].
    fn spawn_live_batch(
        &self,
        state: &crate::app::SavedAppState,
        credential: UploadCredential,
        delta: crate::delta::GoodDelta,
    ) {
        tracing::info!(
            seq = delta.seq,
            changes = delta.changes.len(),
            "live sync: sending a batch"
        );
        let client = self.http.clone();
        let auth = self.tracker_auth.clone();
        let live_tx = self.live_tx.clone();
        let toast_tx = self.toast_tx.clone();
        let host = destination_host(state);
        let ctx = self.ctx.clone();
        let api = state.tracker_api_url.clone();
        tokio::spawn(async move {
            let answer = send_live_batch(&client, &auth, &api, credential, &delta).await;
            match &answer {
                BatchAnswer::Stored { .. } => {
                    tracing::info!(seq = delta.seq, ?answer, "live sync: batch stored");
                    let changes = delta.changes.len();
                    let _ = toast_tx.send(
                        Event::ok(
                            Topic::Destination,
                            trf("Live sync · {0} changes sent to {1}", &[&changes, &host]),
                        )
                        .routine(),
                    );
                }
                BatchAnswer::Duplicate => {
                    tracing::info!(seq = delta.seq, ?answer, "live sync: batch stored");
                }
                other => {
                    tracing::warn!(seq = delta.seq, answer = ?other, "live sync: batch not stored")
                }
            }
            let _ = live_tx.send(LiveEvent::Batch(answer));
            ctx.request_repaint();
        });
    }

    fn handle_live_event(&mut self, event: LiveEvent) {
        let now = now_ms();
        match event {
            LiveEvent::Snapshot { ticket, answer } => {
                if let Some(answer) = &answer {
                    tracing::info!(
                        live = answer.base.is_some(),
                        watching = answer.watching,
                        "live sync: snapshot stored"
                    );
                }
                self.live.snapshot_answered(ticket, answer, now);
            }
            LiveEvent::Batch(answer) => self.live.batch_answered(answer, now),
        }
    }
}

impl Monitor {
    /// What the next heartbeat says: the game's state and the account's UID.
    fn presence_now(&self) -> (PresenceState, Option<u32>) {
        let app_state = &self.app_state.app_state;
        (
            presence_state(app_state.game_status),
            app_state.player.as_ref().and_then(|player| player.uid),
        )
    }

    /// Follow the credential: a new link sends the state at once; without
    /// one nothing is sent. Signing out drops the tracker's alert settings.
    fn link_check(&mut self) {
        let state = self.upload_state();
        let linked = uploads_allowed(&self.capture_source)
            && crate::app::upload_credential(&state).is_some();
        if matches!(
            *self.tracker_auth.status().borrow(),
            AuthStatus::SignedOut { .. }
        ) && self.tracker_alerts.is_some()
        {
            tracing::info!("signed out: the resin alert follows the local setting again");
            self.tracker_alerts = None;
            self.app_state.update_tracker_alerts(None);
            self.ctx.request_repaint();
        }
        if linked == self.linked {
            return;
        }
        self.linked = linked;
        self.presence.reset();
    }

    /// When a heartbeat is next due, in Unix ms.
    fn link_next_ms(&self) -> Option<u64> {
        if !self.linked {
            return None;
        }
        let (state, uid) = self.presence_now();
        self.presence.due_at(state, uid)
    }

    /// Send a heartbeat when one is due.
    fn link_tick(&mut self) {
        if !self.linked {
            return;
        }
        let state = self.upload_state();
        let Some(credential) = crate::app::upload_credential(&state) else {
            return;
        };
        let now = now_ms();
        let (presence, uid) = self.presence_now();
        if self
            .presence
            .due_at(presence, uid)
            .is_some_and(|at| at <= now)
        {
            self.presence.sending(presence, uid, now);
            tracing::debug!(state = presence.as_str(), "presence heartbeat");
            let body =
                crate::link::presence_body(presence, uid, env!("CARGO_PKG_VERSION"), PLATFORM);
            self.spawn_link_request(
                &state.tracker_api_url,
                credential,
                reqwest::Method::POST,
                crate::link::PRESENCE_PATH.to_owned(),
                Some(body),
                |status, body| LinkEvent::Presence(PresenceOutcome::of(status, body)),
            );
        }
    }

    /// One request of the link, in the background; `event` turns its answer
    /// (status 0: none) into what comes back on `link_tx`.
    fn spawn_link_request(
        &self,
        api: &str,
        credential: UploadCredential,
        method: reqwest::Method,
        path: String,
        body: Option<String>,
        event: fn(u16, &str) -> LinkEvent,
    ) {
        let client = self.http.clone();
        let auth = self.tracker_auth.clone();
        let link_tx = self.link_tx.clone();
        let ctx = self.ctx.clone();
        let api = api.to_owned();
        tokio::spawn(async move {
            let (status, answer) =
                link_request(&client, &auth, &api, credential, method, &path, body).await;
            let _ = link_tx.send(event(status, &answer));
            ctx.request_repaint();
        });
    }

    fn handle_link_event(&mut self, event: LinkEvent) {
        let now = now_ms();
        match event {
            LinkEvent::Presence(outcome) => {
                self.presence.answered(&outcome, now);
                let answer = match outcome {
                    PresenceOutcome::Answered(answer) => answer,
                    PresenceOutcome::Gone => {
                        tracing::info!("the tracker has no presence route; presence is off");
                        return;
                    }
                    PresenceOutcome::Retry => return,
                };
                let alerts = match answer.alerts {
                    AlertsAnswer::Absent => self.tracker_alerts,
                    AlertsAnswer::Unset => None,
                    AlertsAnswer::Set(alerts) => Some(alerts),
                };
                if alerts != self.tracker_alerts && self.linked {
                    tracing::info!(?alerts, "the tracker's alert settings for this device");
                    self.tracker_alerts = alerts;
                    self.app_state.update_tracker_alerts(alerts);
                    self.ctx.request_repaint();
                }
                for command in answer.commands {
                    tracing::info!(?command, "the tracker asked for something");
                    match command {
                        Command::Wishes => self.sync_wishes_asked(),
                        Command::Snapshot => self.snapshot_asked(),
                    }
                }
            }
        }
    }

    /// The website's "Sync wishes": this login's wish key syncs now, when
    /// there is one.
    fn sync_wishes_asked(&self) {
        let synced = self.wish_hub.as_ref().is_some_and(|hub| hub.sync_now());
        if !synced {
            tracing::info!("no wish key this login; the wish sync asked for is skipped");
        }
    }

    /// The website's "Send a capture": the whole account now (reason
    /// `manual`), when there is data.
    fn snapshot_asked(&mut self) {
        let updated = &self.app_state.app_state.updated;
        if updated.items_updated.is_none() || updated.characters_updated.is_none() {
            tracing::info!("nothing captured; the snapshot asked for is skipped");
            return;
        }
        let state = self.upload_state();
        let Some(credential) = crate::app::upload_credential(&state) else {
            return;
        };
        let (json, report, ticket) = self.live_snapshot_export(Reason::Manual);
        self.report_export_gaps(&report);
        let _ = self.toast_tx.send(Event::info(
            Topic::Destination,
            trf(
                "Uploading to {0} · {1}",
                &[
                    &destination_host(&state),
                    &crate::app::counts_line(&report.counts()),
                ],
            ),
        ));
        self.spawn_tracker_upload(credential, json, UploadReport::Asked, Some(ticket));
    }
}

/// One link request with the upload credential: its status (0: no answer,
/// or signed out) and body.
async fn link_request(
    client: &reqwest::Client,
    auth: &TrackerAuth,
    api: &str,
    credential: UploadCredential,
    method: reqwest::Method,
    path: &str,
    body: Option<String>,
) -> (u16, String) {
    let url = |base: &str| format!("{}/{path}", base.trim_end_matches('/'));
    let build = |builder: reqwest::RequestBuilder| {
        let builder = builder.timeout(LINK_TIMEOUT);
        match &body {
            Some(body) => builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone()),
            None => builder,
        }
    };
    let response = match credential {
        UploadCredential::ImportKey { key, .. } => build(
            client
                .request(method.clone(), url(api))
                .header("x-import-key", key),
        )
        .send()
        .await
        .map_err(|e| {
            tracing::debug!("link request failed: {}", e.without_url());
            (0, String::new())
        }),
        UploadCredential::SignedIn { base } => auth
            .send_signed_in(client, &base, |token| {
                build(
                    client
                        .request(method.clone(), url(&base))
                        .bearer_auth(token),
                )
            })
            .await
            .map_err(|error| match error {
                // A 401 the sign-in's own retry could not cure.
                crate::signin::AuthError::Http { status, body } => (status.as_u16(), body),
                other => {
                    tracing::debug!("link request not sent: {other}");
                    (0, String::new())
                }
            }),
    };
    match response {
        Ok(response) => {
            let status = response.status().as_u16();
            (status, response.text().await.unwrap_or_default())
        }
        Err(answer) => answer,
    }
}

/// Send one batch with an import key, and read the answer.
pub(crate) async fn send_live_batch_with_key(
    client: &reqwest::Client,
    api: &str,
    key: &str,
    delta: &crate::delta::GoodDelta,
) -> BatchAnswer {
    let response = client
        .post(changes_url(api))
        .timeout(LIVE_BATCH_TIMEOUT)
        .header("x-import-key", key)
        .json(delta)
        .send()
        .await;
    match response {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            BatchAnswer::of(status, &body)
        }
        Err(e) => {
            tracing::warn!("live sync: batch request failed: {}", e.without_url());
            BatchAnswer::Retry
        }
    }
}

/// Send one batch with the upload credential, and read the answer.
async fn send_live_batch(
    client: &reqwest::Client,
    auth: &TrackerAuth,
    api: &str,
    credential: UploadCredential,
    delta: &crate::delta::GoodDelta,
) -> BatchAnswer {
    let response = match credential {
        UploadCredential::ImportKey { key, .. } => {
            return send_live_batch_with_key(client, api, &key, delta).await;
        }
        UploadCredential::SignedIn { base } => auth
            .send_signed_in(client, &base, |token| {
                client
                    .post(changes_url(&base))
                    .timeout(LIVE_BATCH_TIMEOUT)
                    .bearer_auth(token)
                    .json(delta)
            })
            .await
            .map_err(|error| match error {
                crate::signin::AuthError::Http { status, body } => {
                    BatchAnswer::of(status.as_u16(), &body)
                }
                crate::signin::AuthError::SignedOut => BatchAnswer::Refused,
                crate::signin::AuthError::Network(_) => BatchAnswer::Retry,
                other => {
                    tracing::warn!("live sync: batch not sent: {other}");
                    BatchAnswer::Retry
                }
            }),
    };
    match response {
        Ok(response) => {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            BatchAnswer::of(status, &body)
        }
        Err(answer) => answer,
    }
}

/// `Instant::now()`, but strictly after `floor`.
///
/// The automation gate only counts data captured strictly after the current
/// cycle began, and a cycle can begin in the same call that then stamps the
/// first data of a new connection; on a coarse clock the two reads can be
/// equal, which would leave that data uncounted until it arrived again.
fn instant_after(floor: Option<Instant>) -> Instant {
    let now = Instant::now();
    match floor {
        Some(floor) if now <= floor => floor + Duration::from_nanos(1),
        _ => now,
    }
}

/// Forget the recorded signature of an export that did not actually land.
///
/// The signature is what suppresses a repeat export, so leaving it recorded for
/// a failed one means the user gets no snapshot at all for that login:
/// `check_automation_trigger` refuses every later attempt for the same data, and
/// the data itself only changes at the next login.
///
/// How long a pending countdown still waits: [`AUTOMATION_DEBOUNCE`] after
/// the last new data and, until the world's exploration points are held,
/// [`AUTOMATION_EXPLORE_WAIT`] after it began.
fn automation_wait(
    pending_since: Instant,
    armed_at: Instant,
    explore_held: bool,
    now: Instant,
) -> Duration {
    let quiet = AUTOMATION_DEBOUNCE.saturating_sub(now.saturating_duration_since(pending_since));
    if explore_held {
        return quiet;
    }
    quiet.max(AUTOMATION_EXPLORE_WAIT.saturating_sub(now.saturating_duration_since(armed_at)))
}

/// A pending countdown's start after new data: now, while the countdown is
/// younger than [`AUTOMATION_MAX_WAIT`].
fn rearmed(pending_since: Instant, armed_at: Instant, now: Instant) -> Instant {
    if now.saturating_duration_since(armed_at) < AUTOMATION_MAX_WAIT {
        now
    } else {
        pending_since
    }
}

/// Original Resin's cap and regeneration (one per 8 minutes).
const RESIN_CAP: u32 = 200;
const RESIN_SECS: u64 = 8 * 60;

/// When a resin clock reaches `target` (unix seconds): `None` when it already
/// had that much (the player saw it in the game) or more than the cap.
fn resin_reaches(resin: crate::good::GiResin, target: u32) -> Option<u64> {
    if resin.value >= target || target > RESIN_CAP {
        return None;
    }
    Some(u64::from(resin.next_at) + u64::from(target - resin.value - 1) * RESIN_SECS)
}

/// The next resin alert on `resin`'s clock, given the amounts to alert at
/// (lowest first) and the highest amount already sent on that clock: when it
/// is due (unix seconds), and its amount. Amounts the clock already had at
/// the capture are skipped.
fn next_resin_alert(
    resin: crate::good::GiResin,
    targets: [Option<u32>; 2],
    alerted: Option<(crate::good::GiResin, u32)>,
) -> Option<(u64, u32)> {
    targets
        .into_iter()
        .flatten()
        .filter(|target| !alerted.is_some_and(|(clock, sent)| clock == resin && *target <= sent))
        .find_map(|target| resin_reaches(resin, target).map(|at| (at, target)))
}

/// Only clears the signature it was given. A newer export may already have
/// replaced it, and that one is not this failure's to cancel. `true` when
/// something was actually released.
fn release_signature(
    last_signature: &mut Option<AutomationSignature>,
    signature: AutomationSignature,
) -> bool {
    if *last_signature == Some(signature) {
        *last_signature = None;
        return true;
    }
    false
}

/// Await the capture task, or wait forever when there is none.
///
/// `select!` needs a future on every arm. The handle is cleared by the arm that
/// receives this, because polling a `JoinHandle` that has already completed
/// panics.
async fn join_capture(
    handle: &mut Option<JoinHandle<Result<()>>>,
) -> std::result::Result<Result<()>, JoinError> {
    match handle.as_mut() {
        Some(handle) => handle.await,
        None => std::future::pending().await,
    }
}

/// The tracker's snapshot import endpoint for a configured base URL.
pub(crate) fn import_url(base_url: &str) -> String {
    format!(
        "{}/genshin-accounts-public/import-by-key",
        base_url.trim_end_matches('/')
    )
}

/// POST a GOOD snapshot to the tracker.
///
/// The failure string is `HTTP {status} - {body}`, and that shape is load
/// bearing: `app.rs::tracker_error_status` parses the status back out of it to
/// notice a 401/403 and re-verify the key.
pub(crate) async fn upload_to_tracker(
    client: &reqwest::Client,
    url: &str,
    key: &str,
    json: String,
    captured_at_ms: Option<i64>,
) -> std::result::Result<String, String> {
    let form = snapshot_form(json, captured_at_ms)?;

    let response = client
        .post(url)
        // Overrides the client-wide verify budget: see
        // [`TRACKER_UPLOAD_TIMEOUT`].
        .timeout(TRACKER_UPLOAD_TIMEOUT)
        .header("x-import-key", key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Tracker upload request failed: {e}");
            e.to_string()
        })?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status.is_success() {
        tracing::info!("Successfully uploaded data to tracker");
        Ok(body)
    } else {
        tracing::error!("Tracker upload failed ({}): {}", status, body);
        Err(format!("HTTP {} - {}", status, body))
    }
}

/// The multipart body of a snapshot upload, shared by the import-key path and
/// the signed-in one (`signin.rs`), whose routes take the same body.
pub(crate) fn snapshot_form(
    json: String,
    captured_at_ms: Option<i64>,
) -> std::result::Result<reqwest::multipart::Form, String> {
    let part = reqwest::multipart::Part::bytes(json.into_bytes())
        .file_name("irminsul_capture.json")
        .mime_str("application/json")
        .map_err(|e| format!("could not build the upload body: {e}"))?;

    // Sent before the file part, because the backend reads it while streaming
    // the upload. Without it a snapshot is stamped with the server's receive
    // time, which both misdates a delayed upload and stops the server's
    // duplicate check from ever firing on a re-upload of unchanged data.
    let mut form = reqwest::multipart::Form::new();
    if let Some(captured_at_ms) = captured_at_ms {
        form = form.text("timestamp", captured_at_ms.to_string());
    }
    Ok(form.part("file", part))
}

/// What the tracker says about an import key (`verify-key`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackerAccount {
    pub name: String,
    pub uid: String,
    pub server: String,
    /// The account's page in the tracker's web app, when the tracker reports
    /// one (the hosted tracker does; the old backend does not). Only http(s)
    /// URLs are kept: the UI opens this in the user's browser.
    pub dashboard_url: Option<String>,
}

/// Ask the tracker who an import key belongs to.
async fn verify_tracker_key(
    client: &reqwest::Client,
    url: &str,
    key: &str,
) -> Result<TrackerAccount> {
    let response = client
        .get(url)
        .header("x-import-key", key)
        .send()
        .await
        .map_err(|e| anyhow!("Request failed: {}", e))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| anyhow!("Failed to read response: {}", e))?;

    tracing::info!("Verify key response ({}): {}", status, body);
    if !status.is_success() {
        return Err(anyhow!("Verify failed: HTTP {}", status));
    }
    parse_verify_response(&body)
}

/// Reads a successful `verify-key` body. The old backend wraps it in
/// `{ data: { ... } }`; the hosted tracker answers at the top level.
fn parse_verify_response(body: &str) -> Result<TrackerAccount> {
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|_| anyhow!("Invalid JSON response"))?;
    let inner = json.get("data").unwrap_or(&json);
    let name = inner
        .get("accountName")
        .and_then(|v| v.as_str())
        .unwrap_or(tr("Unknown"))
        .to_string();
    let uid = inner
        .get("uid")
        .map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            _ => "N/A".to_string(),
        })
        .unwrap_or_else(|| "N/A".to_string());
    let server = inner
        .get("server")
        .and_then(|v| v.as_str())
        .unwrap_or("N/A")
        .to_string();
    let dashboard_url = inner
        .get("dashboardUrl")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|url| is_web_url(url))
        .map(str::to_owned);

    Ok(TrackerAccount {
        name,
        uid,
        server,
        dashboard_url,
    })
}

/// Whether a server-supplied link is safe to hand to the browser: an absolute
/// http(s) URL with a host, nothing like `file:` or `javascript:`.
fn is_web_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

async fn get_database(
    app_state: &mut AppStateManager,
    _ui_message_rx: &mut mpsc::UnboundedReceiver<Message>,
) -> Result<AnimeGameData> {
    app_state.update_app_state(State::CheckingForData);
    load_game_data()
}

/// The game data `build.rs` embedded in this binary.
pub(crate) fn load_game_data() -> Result<AnimeGameData> {
    static DATABASE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/game_data.gz"));
    let reader = GzDecoder::new(DATABASE);
    let db = teyvatme_game_data::AnimeGameData::new_from_reader(reader)?;
    tracing::info!(
        dump = embedded_game_data_sha().unwrap_or("unknown"),
        "loaded the embedded game data"
    );

    Ok(db)
}

/// The Dimbreath dump commit the embedded game data was built from.
///
/// `build.rs` reads it back out of the `game_data.gz` it embeds; `None` when it
/// could not. A binary built before a game patch carries an older dump, and
/// this is what says so next to the `unknown_*` gaps in its exports.
pub fn embedded_game_data_sha() -> Option<&'static str> {
    Some(env!("TEYVATME_GAME_DATA_SHA")).filter(|sha| !sha.is_empty())
}

async fn capture_task(
    cancel_token: CancellationToken,
    packet_tx: mpsc::UnboundedSender<CaptureEvent>,
    backend: BackendType,
    capture_source: capture::CaptureSource,
) -> Result<()> {
    let mut capture = match create_capture(backend, capture_source) {
        Ok(capture) => capture,
        Err(e) => {
            // Reported through the channel rather than out of the task: the
            // `JoinHandle` is watched now, but this is the arm that clears
            // `capturing` and toasts, and "not elevated" or "no such device" is
            // the most common first-run failure there is.
            let error = anyhow!("Error creating packet capture using {:?}: {e}", backend);
            tracing::error!("{error}");
            let _ = packet_tx.send(CaptureEvent::Failed(error));
            return Ok(());
        }
    };
    tracing::info!("starting capture");
    // Only now is there a backend to capture with. The monitor sets `capturing`
    // from this, not from having spawned the task.
    let _ = packet_tx.send(CaptureEvent::Started);

    loop {
        let packet = tokio::select!(
            packet = capture.next_packet() => packet,
            _ = cancel_token.cancelled() => break,
        );
        let packet = match packet {
            Ok(packet) => packet,
            Err(e) => {
                tracing::error!("Error receiving packet: {e}");
                let _ = packet_tx.send(CaptureEvent::Failed(anyhow!(
                    "Capture stream closed or errored: {e}"
                )));
                break;
            }
        };

        if let Err(e) = packet_tx.send(CaptureEvent::Packet(packet)) {
            tracing::error!("Error sending captured packet to monitor: {e}");
            break;
        }
    }
    tracing::info!("ending capture");
    Ok(())
}

pub(crate) fn load_keys() -> Result<HashMap<u16, Vec<u8>>> {
    let keys: HashMap<u16, String> = serde_json::from_slice(include_bytes!("../keys/gi.json"))?;

    keys.iter()
        .map(|(key, value)| -> Result<_, _> { Ok((*key, BASE64_STANDARD.decode(value)?)) })
        .collect::<Result<HashMap<_, _>>>()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_resin_alert_is_due_when_the_clock_reaches_it() {
        let resin = crate::good::GiResin {
            value: 150,
            next_at: 1_000,
        };
        // 151 at 1,000, then one per 8 minutes: 160 nine later.
        assert_eq!(resin_reaches(resin, 151), Some(1_000));
        assert_eq!(resin_reaches(resin, 160), Some(1_000 + 9 * 480));
        assert_eq!(resin_reaches(resin, 200), Some(1_000 + 49 * 480));
        // Already there at the capture, or past the cap: nothing to wait for.
        assert_eq!(resin_reaches(resin, 150), None);
        assert_eq!(resin_reaches(resin, 120), None);
        assert_eq!(resin_reaches(resin, 201), None);
    }

    #[test]
    fn resin_alerts_go_once_per_amount_and_clock() {
        let resin = crate::good::GiResin {
            value: 150,
            next_at: 1_000,
        };
        let both = [Some(160), Some(200)];
        assert_eq!(
            next_resin_alert(resin, both, None),
            Some((1_000 + 9 * 480, 160))
        );
        // 160 sent on this clock: full is next, then nothing.
        assert_eq!(
            next_resin_alert(resin, both, Some((resin, 160))),
            Some((1_000 + 49 * 480, 200))
        );
        assert_eq!(next_resin_alert(resin, both, Some((resin, 200))), None);
        // A new clock (a new capture) starts over.
        let later = crate::good::GiResin {
            value: 120,
            next_at: 9_000,
        };
        assert_eq!(
            next_resin_alert(later, both, Some((resin, 200))),
            Some((9_000 + 39 * 480, 160))
        );
        // An amount the capture already had is skipped.
        assert_eq!(
            next_resin_alert(resin, [Some(140), Some(200)], None),
            Some((1_000 + 49 * 480, 200))
        );
        assert_eq!(next_resin_alert(resin, [None, None], None), None);
    }

    #[test]
    fn the_tracker_hears_closed_running_or_decoding() {
        use crate::game_watch::MissedLaunch;
        assert_eq!(
            presence_state(GameStatus::NotRunning),
            PresenceState::Stopped
        );
        assert_eq!(
            presence_state(GameStatus::Decoding),
            PresenceState::Decoding
        );
        for running in [
            GameStatus::LaunchCaptured,
            GameStatus::CaptureOff,
            GameStatus::AwaitingLogin,
            GameStatus::LaunchMissed(MissedLaunch::AlreadyRunning),
            GameStatus::LaunchMissed(MissedLaunch::CaptureStopped),
            GameStatus::KeyLost,
        ] {
            assert_eq!(
                presence_state(running),
                PresenceState::Running,
                "{running:?}"
            );
        }
    }

    #[test]
    fn a_countdown_waits_for_the_burst_and_the_exploration() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        // Exploration held: 5 s after the last data.
        assert_eq!(
            automation_wait(start, start, true, at(2)),
            Duration::from_secs(3)
        );
        assert_eq!(automation_wait(start, start, true, at(6)), Duration::ZERO);
        // Not yet: until 12 s from the start, whichever ends later.
        assert_eq!(
            automation_wait(start, start, false, at(6)),
            Duration::from_secs(6)
        );
        assert_eq!(
            automation_wait(at(10), start, false, at(11)),
            Duration::from_secs(4)
        );
        assert_eq!(automation_wait(start, start, false, at(13)), Duration::ZERO);
        // New data restarts it, for 20 s at most.
        assert_eq!(rearmed(start, start, at(7)), at(7));
        assert_eq!(rearmed(at(19), start, at(21)), at(19));
    }

    /// Automation on for both destinations, with Genshin Optimizer settings
    /// that leave out characters, everything under 5★ and the real 4th line.
    fn automation_state() -> crate::app::SavedAppState {
        crate::app::SavedAppState {
            save_result_to_file: true,
            auto_export_to_tracker: true,
            tracker_import_key: "gdt_import_1_abc".to_string(),
            tracker_verified: true,
            tracker_sign_in: crate::app::TrackerSignIn::SignedOut,
            export_settings: ExportSettings {
                include_characters: false,
                fake_initialize_4th_line: true,
                min_artifact_rarity: 5,
                min_weapon_rarity: 5,
                ..ExportSettings::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn the_automated_upload_exports_with_the_tracker_settings() {
        let state = automation_state();
        let plan = AutomationPlan::for_state(&state);

        assert_eq!(plan.tracker, Some(ExportSettings::for_tracker()));
        // The automation file is a Genshin Optimizer export, and keeps the
        // user's settings.
        assert_eq!(plan.file, Some(state.export_settings.clone()));
    }

    #[test]
    fn the_automation_plan_follows_what_is_switched_on() {
        let mut state = automation_state();
        state.tracker_verified = false;
        let plan = AutomationPlan::for_state(&state);
        assert_eq!(plan.tracker, None);
        assert!(plan.file.is_some());

        state.save_result_to_file = false;
        assert!(AutomationPlan::for_state(&state).is_empty());

        state.tracker_verified = true;
        let plan = AutomationPlan::for_state(&state);
        assert_eq!(plan.file, None);
        assert_eq!(plan.settings().count(), 1);
    }

    #[test]
    fn the_automation_waits_for_what_the_upload_writes() {
        // Characters are off for the file, but the upload holds them, so the
        // trigger waits for them too.
        let plan = AutomationPlan::for_state(&automation_state());
        let updated = DataUpdated::new();
        let names: Vec<&str> = plan
            .settings()
            .flat_map(|settings| crate::app::export_data_classes(settings, &updated))
            .map(|class| class.name)
            .collect();
        assert!(names.contains(&"character data"), "{names:?}");
    }

    #[test]
    fn a_replayed_recording_never_uploads_or_auto_exports() {
        assert!(!uploads_allowed(&CaptureSource::File(PathBuf::from(
            "old.pcapng"
        ))));
        assert!(uploads_allowed(&CaptureSource::Device(None)));
        assert!(uploads_allowed(&CaptureSource::Device(Some(
            PathBuf::from("rec.pcap")
        ))));
    }

    #[test]
    fn verify_reads_the_hosted_trackers_answer_and_its_dashboard_link() {
        let body = r#"{"accountId":11,"accountName":"Main","uid":"811111111","server":"ASIA","dashboardUrl":"https://teyvat.me/app/a/11"}"#;
        assert_eq!(
            parse_verify_response(body).unwrap(),
            TrackerAccount {
                name: "Main".into(),
                uid: "811111111".into(),
                server: "ASIA".into(),
                dashboard_url: Some("https://teyvat.me/app/a/11".into()),
            }
        );
    }

    #[test]
    fn verify_reads_the_old_backends_wrapped_answer_without_a_dashboard() {
        let body = r#"{"status":200,"message":"ok","data":{"accountName":"Alt","uid":800000000,"server":null}}"#;
        let account = parse_verify_response(body).unwrap();
        assert_eq!(account.name, "Alt");
        assert_eq!(account.uid, "800000000");
        assert_eq!(account.server, "N/A");
        assert_eq!(account.dashboard_url, None);
    }

    #[test]
    fn verify_only_keeps_web_links_for_the_dashboard() {
        for url in [
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "/app/a/11",
            "",
            "https://",
        ] {
            let body = serde_json::json!({ "accountName": "x", "dashboardUrl": url }).to_string();
            assert_eq!(
                parse_verify_response(&body).unwrap().dashboard_url,
                None,
                "{url}"
            );
        }
        let body = r#"{"accountName":"x","dashboardUrl":" http://localhost:5173/app/a/1 "}"#;
        assert_eq!(
            parse_verify_response(body)
                .unwrap()
                .dashboard_url
                .as_deref(),
            Some("http://localhost:5173/app/a/1")
        );
    }

    #[test]
    fn the_embedded_game_data_names_its_dump_commit() {
        // `build.rs` only emits something that looks like a commit, or nothing.
        if let Some(sha) = embedded_game_data_sha() {
            assert!((7..=64).contains(&sha.len()), "{sha}");
            assert!(
                sha.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{sha}"
            );
        }
    }

    #[test]
    fn verify_rejects_a_body_that_is_not_json() {
        assert!(parse_verify_response("<html>").is_err());
    }

    fn command(command_id: u16, header: &[u8], data: &[u8]) -> GameCommand {
        GameCommand {
            command_id,
            header_len: header.len() as u16,
            data_len: data.len() as u32,
            proto_header: header.to_vec(),
            proto_data: data.to_vec(),
            sent: false,
        }
    }

    #[test]
    fn a_log_record_keeps_the_header_and_the_payload_apart() {
        let record = encode_packet_log_record(&command(4242, &[1, 2, 3], &[9, 8]));

        assert_eq!(record.len(), 10 + 3 + 2);
        assert_eq!(&record[0..2], &4242u16.to_le_bytes());
        assert_eq!(&record[2..6], &3u32.to_le_bytes());
        assert_eq!(&record[6..10], &2u32.to_le_bytes());
        assert_eq!(&record[10..13], &[1, 2, 3]);
        assert_eq!(&record[13..15], &[9, 8]);
    }

    #[test]
    fn a_log_record_with_no_header_is_still_self_describing() {
        // `proto_header` is empty for a command whose `header_len` was 0, and a
        // reader has to be able to tell that from a one-byte header.
        let record = encode_packet_log_record(&command(1, &[], &[7]));

        assert_eq!(record.len(), 10 + 1);
        assert_eq!(&record[2..6], &0u32.to_le_bytes());
        assert_eq!(&record[6..10], &1u32.to_le_bytes());
        assert_eq!(&record[10..11], &[7]);
    }

    #[test]
    fn the_packet_log_appends_to_one_file_per_session() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = PacketLog::with_dir(Some(dir.path().to_path_buf()));

        log.append(&command(1, &[], &[7])).unwrap();
        log.append(&command(2, &[], &[8, 8])).unwrap();

        let files: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "one file per session, not per command");

        let written = std::fs::read(files[0].path()).unwrap();
        assert_eq!(written.len(), (10 + 1) + (10 + 2));
        assert_eq!(&written[0..2], &1u16.to_le_bytes());
        assert_eq!(&written[11..13], &2u16.to_le_bytes());
    }

    #[test]
    fn the_packet_log_reports_an_unusable_directory_instead_of_panicking() {
        let mut log = PacketLog::with_dir(None);
        assert!(log.append(&command(1, &[], &[7])).is_err());
    }

    // -- capture watchdog --------------------------------------------------------

    /// A watchdog whose capture last heard a frame at `at`, during a game.
    fn heard_at(at: Instant) -> CaptureSilence {
        let mut silence = CaptureSilence::default();
        silence.note_frame(at);
        silence
    }

    #[test]
    fn a_capture_that_went_silent_on_a_running_game_is_restarted() {
        let start = Instant::now();
        let mut silence = heard_at(start);
        let later = start + CAPTURE_SILENCE_LIMIT;
        assert_eq!(
            silence.stalled(true, true, later),
            Some(CAPTURE_SILENCE_LIMIT)
        );
    }

    #[test]
    fn the_watchdog_leaves_everything_else_alone() {
        let start = Instant::now();
        let long_after = start + CAPTURE_SILENCE_LIMIT;

        // Not silent long enough.
        let soon = start + CAPTURE_SILENCE_LIMIT - Duration::from_secs(1);
        assert_eq!(heard_at(start).stalled(true, true, soon), None);
        // No game to hear from.
        assert_eq!(heard_at(start).stalled(false, true, long_after), None);
        // Capture is not up; the supervisor owns that case.
        assert_eq!(heard_at(start).stalled(true, false, long_after), None);
        // Nothing has arrived since this capture started -- a game sitting on
        // the title screen, or the capture the watchdog itself just restarted.
        // Restarting again would only open more blind windows.
        let mut restarted = heard_at(start);
        restarted.capture_started();
        assert_eq!(restarted.stalled(true, true, long_after), None);
    }

    #[test]
    fn silence_while_the_game_was_closed_is_not_held_against_the_next_session() {
        // 2026-10-05/06: the game closed at 19:30 and started again at 05:48;
        // the watchdog restarted capture the moment it saw the game, calling
        // the whole night (37116 s) silence.
        let closed = Instant::now();
        let mut silence = heard_at(closed);

        // Every poll while the game is closed.
        for minutes in [1, 60, 600] {
            let at = closed + Duration::from_secs(minutes * 60);
            assert_eq!(silence.stalled(false, true, at), None);
        }

        // Genshin starts: no restart, however long the night was ...
        let started = closed + Duration::from_secs(37_116);
        assert_eq!(silence.stalled(true, true, started), None);
        // ... nor while it sits on the title screen, sending nothing.
        let title_screen = started + 10 * CAPTURE_SILENCE_LIMIT;
        assert_eq!(silence.stalled(true, true, title_screen), None);

        // Once this session's traffic flows, its silence counts as before.
        let login = title_screen + Duration::from_secs(5);
        silence.note_frame(login);
        assert_eq!(
            silence.stalled(true, true, login + Duration::from_secs(30)),
            None
        );
        assert_eq!(
            silence.stalled(true, true, login + CAPTURE_SILENCE_LIMIT),
            Some(CAPTURE_SILENCE_LIMIT)
        );
    }

    #[test]
    fn a_game_seen_closed_for_one_poll_forgets_the_silence_before_it() {
        let start = Instant::now();
        let mut silence = heard_at(start);
        let later = start + CAPTURE_SILENCE_LIMIT;
        // The scan missed the game once (it restarted, or a poll raced its
        // exit): the next running poll starts counting afresh.
        assert_eq!(silence.stalled(false, true, later), None);
        assert_eq!(
            silence.stalled(true, true, later + CAPTURE_SILENCE_LIMIT),
            None
        );
    }

    #[test]
    fn the_packet_log_prunes_old_sessions_when_it_opens_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        for second in 0..10 {
            std::fs::write(
                dir.path()
                    .join(format!("2024-01-01_00-00-{second:02}.000.bin")),
                b"old",
            )
            .unwrap();
        }
        // Anything that is not a packet log is none of the pruner's business.
        std::fs::write(dir.path().join("notes.txt"), b"keep me").unwrap();

        let mut log = PacketLog::with_dir(Some(dir.path().to_path_buf()));
        log.append(&command(1, &[], &[7])).unwrap();
        // Pruning happens when the session's file is opened, not per record.
        log.append(&command(2, &[], &[8])).unwrap();

        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".bin"))
            .collect();
        names.sort();

        assert_eq!(names.len(), PACKET_LOG_RETENTION, "kept {names:?}");
        assert_eq!(names[0], "2024-01-01_00-00-05.000.bin", "oldest kept");
        assert!(
            !names.contains(&"2024-01-01_00-00-04.000.bin".to_string()),
            "the oldest sessions should be gone"
        );
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn data_stamped_right_after_a_cycle_starts_counts_for_it() {
        let start = Instant::now() + Duration::from_secs(1);
        assert!(instant_after(Some(start)) > start);
        let past = Instant::now();
        assert!(instant_after(Some(past)) >= past);
        let _ = instant_after(None);
    }

    #[test]
    fn a_failed_export_releases_its_signature_so_the_same_data_is_retried() {
        let signature = (Some(Instant::now()), None, None);
        let mut last = Some(signature);

        assert!(release_signature(&mut last, signature));
        assert_eq!(last, None, "the same snapshot must be exportable again");
    }

    #[test]
    fn a_failed_export_does_not_cancel_a_newer_one() {
        // The upload runs off the monitor loop, so its failure can arrive after
        // fresher data has already been exported. Clearing the signature then
        // would re-export data that did land.
        let stale = (Some(Instant::now()), None, None);
        let newer = (None, Some(Instant::now()), None);
        let mut last = Some(newer);

        assert!(!release_signature(&mut last, stale));
        assert_eq!(last, Some(newer));
    }

    #[test]
    fn presence_goes_to_its_route_with_the_key() {
        use crate::wish_history::test_server::{Reply, TestServer};

        let replies = std::sync::Arc::new(std::sync::Mutex::new(vec![Reply::json(
            200,
            r#"{"next":15,"commands":[{"id":"k3f9","kind":"snapshot"}],"alerts":null}"#,
        )]));
        let server = {
            let replies = replies.clone();
            TestServer::start(move |_| replies.lock().unwrap().pop().unwrap())
        };
        let auth = TrackerAuth::new(
            &server.base,
            std::sync::Arc::new(crate::signin::MemoryStore::default()),
            "TEST-PC".to_owned(),
            std::sync::Arc::new(|_| {}),
            std::sync::Arc::new(|| {}),
        );
        let client = crate::http_client_builder().build().unwrap();
        let key = || UploadCredential::ImportKey {
            url: import_url(&server.base),
            key: "gdt_uk_test".to_owned(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let body = crate::link::presence_body(
            PresenceState::Running,
            Some(812_345_678),
            "1.0.0",
            PLATFORM,
        );
        let (status, answer) = runtime.block_on(link_request(
            &client,
            &auth,
            &server.base,
            key(),
            reqwest::Method::POST,
            crate::link::PRESENCE_PATH,
            Some(body.clone()),
        ));
        let PresenceOutcome::Answered(answer) = PresenceOutcome::of(status, &answer) else {
            panic!("not answered: {status}");
        };
        assert_eq!(answer.next_secs, 15);
        assert_eq!(answer.commands, [Command::Snapshot]);
        assert_eq!(answer.alerts, AlertsAnswer::Unset);

        let requests = server.requests();
        assert_eq!(requests[0].method, "POST");
        assert!(requests[0].path.ends_with("/irminsul/presence"));
        assert_eq!(requests[0].header("x-import-key"), Some("gdt_uk_test"));
        assert_eq!(requests[0].body, body);
        // Nothing listens here: no answer, tried again at the next tick.
        let (status, _) = runtime.block_on(link_request(
            &client,
            &auth,
            "http://127.0.0.1:9",
            key(),
            reqwest::Method::POST,
            crate::link::PRESENCE_PATH,
            Some(body),
        ));
        assert_eq!(PresenceOutcome::of(status, ""), PresenceOutcome::Retry);
    }

    #[test]
    fn a_live_batch_goes_to_the_changes_route_and_its_answer_is_read() {
        use crate::wish_history::test_server::{Reply, TestServer};

        let replies = std::sync::Arc::new(std::sync::Mutex::new(vec![
            Reply::json(500, ""),
            Reply::json(
                409,
                r#"{"error":{"code":"resync","message":"Send a snapshot first"}}"#,
            ),
            Reply::json(
                200,
                r#"{"ok":true,"seq":1,"collapse":false,"watching":true}"#,
            ),
        ]));
        let server = {
            let replies = replies.clone();
            TestServer::start(move |_| replies.lock().unwrap().pop().unwrap())
        };
        let auth = TrackerAuth::new(
            &server.base,
            std::sync::Arc::new(crate::signin::MemoryStore::default()),
            "TEST-PC".to_owned(),
            std::sync::Arc::new(|_| {}),
            std::sync::Arc::new(|| {}),
        );
        let client = crate::http_client_builder().build().unwrap();
        let mut delta = crate::delta::GoodDelta::new(812_345_678, "01J9ZK5Q8W3E7R2T", 1, "k1", 1);
        delta.changes.push(crate::delta::Change::Set(
            vec!["materials".into(), "Mora".into()],
            serde_json::json!(5),
        ));
        let key = || UploadCredential::ImportKey {
            url: import_url(&server.base),
            key: "gdt_uk_test".to_owned(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let answers: Vec<BatchAnswer> = (0..3)
            .map(|_| runtime.block_on(send_live_batch(&client, &auth, &server.base, key(), &delta)))
            .collect();
        assert_eq!(
            answers,
            [
                BatchAnswer::Stored {
                    collapse: false,
                    watching: true
                },
                BatchAnswer::Resync,
                BatchAnswer::Retry,
            ]
        );
        let requests = server.requests();
        assert_eq!(requests[0].method, "POST");
        assert!(
            requests[0].path.ends_with("/irminsul/changes"),
            "{}",
            requests[0].path
        );
        assert_eq!(requests[0].header("x-import-key"), Some("gdt_uk_test"));
        let sent: crate::delta::GoodDelta = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(sent, delta);
        // Nothing listens here: a batch that cannot be sent is kept.
        let nowhere = UploadCredential::ImportKey {
            url: String::new(),
            key: "gdt_uk_test".to_owned(),
        };
        assert_eq!(
            runtime.block_on(send_live_batch(
                &client,
                &auth,
                "http://127.0.0.1:9",
                nowhere,
                &delta
            )),
            BatchAnswer::Retry
        );
    }

    #[test]
    fn live_sync_needs_its_switch_and_automated_uploads() {
        let mut state = automation_state();
        assert!(live_enabled(&state));
        state.live_sync = false;
        assert!(!live_enabled(&state));
        state.live_sync = true;
        state.auto_export_to_tracker = false;
        assert!(!live_enabled(&state));
        assert_eq!(
            changes_url("https://tracker.invalid/api/"),
            "https://tracker.invalid/api/irminsul/changes"
        );
    }

    #[test]
    fn the_import_url_is_the_same_with_or_without_a_trailing_slash() {
        let expected = "http://localhost:49000/genshin-accounts-public/import-by-key";
        assert_eq!(import_url("http://localhost:49000"), expected);
        assert_eq!(import_url("http://localhost:49000/"), expected);
        assert_eq!(import_url("http://localhost:49000///"), expected);

        // A base with a path (the hosted tracker's `/api`) keeps it.
        let expected = "https://teyvat.me/api/genshin-accounts-public/import-by-key";
        assert_eq!(import_url("https://teyvat.me/api"), expected);
        assert_eq!(import_url("https://teyvat.me/api/"), expected);
    }
}
