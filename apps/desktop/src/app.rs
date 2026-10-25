// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

use std::fmt::Display;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use egui::{Context, OpenUrl};
use egui_file_dialog::FileDialog;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::autostart::Startup;
use crate::capture;
use crate::monitor::TrackerAccount;
use crate::signin::{
    self, AuthStatus, TrackerAuth, TrackerRequest, UploadCredential, UploadScope, Whoami,
};
use crate::ui::toast::Toasts;

type AsyncRuntimeHandles = (
    mpsc::UnboundedSender<Message>,
    mpsc::UnboundedSender<UpdateAnswer>,
    watch::Receiver<AppState>,
    crate::wish_history::WishView,
    crate::activity::Receiver,
    JoinHandle<()>,
);
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
#[cfg(not(target_os = "linux"))]
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::game_watch::{GameStatus, MissedLaunch};
use crate::monitor::Monitor;
use crate::player_data::{ExportCounts, ExportSettings};
use crate::update::{InstallLock, UpdateAnswer, check_for_app_update};
use crate::{AppState, DataUpdated, Message, ReloadHandle, TracingLevel};

/// How long a capture backend may take to release its device or ETW session.
///
/// A mirror of `monitor::CAPTURE_SHUTDOWN_TIMEOUT`, which is private to that
/// module. The two are checked against each other by
/// `the_monitor_budget_outlasts_the_capture_teardown_it_waits_for` below, so
/// they cannot drift apart unnoticed.
const CAPTURE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long the monitor thread is given to wind down before shutdown gives up
/// on it and lets the process exit anyway.
///
/// Derived from, and strictly larger than, the capture teardown it is waiting
/// on. Clicking Stop and then closing the window leaves `handle_ui_msg`
/// awaiting `stop_capture()` for up to a full CAPTURE_SHUTDOWN_TIMEOUT while
/// this timer runs concurrently: with both set to the same 5 s, a teardown
/// that legitimately uses its whole budget is detached at the exact moment it
/// would have succeeded, abandoning a pktmon ETW session or a pcap device that
/// the next launch then silently loses the race for.
const MONITOR_SHUTDOWN_TIMEOUT: Duration =
    Duration::from_secs(CAPTURE_SHUTDOWN_TIMEOUT.as_secs() + 3);

/// How often whoami is re-read while signed in, so an upload target changed
/// on the tracker's website shows up without a restart.
const WHOAMI_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Coming back to the window re-reads whoami (the user may have just changed
/// the target in the browser), but not more often than this.
const WHOAMI_FOCUS_MIN_INTERVAL: Duration = Duration::from_secs(10);

/// Whether the tracker sign-in (`signin.rs`) is signed in, as the upload
/// gates see it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrackerSignIn {
    /// The stored sign-in has not been read yet (start-up, or just after an
    /// address change). Nothing uploads: the import key must not stand in
    /// for a sign-in that is about to load.
    #[default]
    Unknown,
    /// Not signed in (or a sign-in is waiting on the browser): the import
    /// key, when one is saved and verified, is what uploads.
    SignedOut,
    /// Signed in: uploads use the sign-in and the key is not used.
    SignedIn,
}

impl TrackerSignIn {
    fn from_status(status: &AuthStatus) -> Self {
        match status {
            AuthStatus::Loading => Self::Unknown,
            AuthStatus::SignedOut { .. } | AuthStatus::Waiting { .. } => Self::SignedOut,
            AuthStatus::SignedIn { .. } => Self::SignedIn,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct SavedAppState {
    pub export_settings: ExportSettings,
    /// Whether Windows starts Teyvat.me at sign-in: the last state read from
    /// Task Scheduler (see `autostart.rs`), which overrides this on every
    /// read. Only a cache, kept so the checkbox has something to show before
    /// the first read.
    #[serde(default)]
    pub start_on_startup: bool,
    #[serde(default)]
    pub save_result_to_file: bool,
    #[serde(default)]
    pub save_result_folder: Option<PathBuf>,
    pub log_raw_packets: bool,
    #[serde(default)]
    pub tracing_level: TracingLevel,
    /// A non-expiring bearer credential for this account's snapshot writes,
    /// stored in cleartext in eframe's `app.ron` alongside the rest of this
    /// struct. Anyone who gets a copy of that file can push arbitrary GOOD
    /// snapshots into the account's history until the key is rotated, so the
    /// tracker settings modal says so and offers a "Clear key" button.
    #[serde(default)]
    pub tracker_import_key: String,
    /// Persisted, not `serde(skip)`: it is editable in the tracker settings
    /// modal, and an edit that silently reverted on the next launch would be
    /// worse than no field at all. `default` keeps `app.ron` files written
    /// before the field was persisted loading cleanly.
    #[serde(default = "default_tracker_url")]
    pub tracker_api_url: String,
    #[serde(default)]
    pub auto_export_to_tracker: bool,
    /// Live sync (`live.rs`): between the game's full sends, send the
    /// tracker what changed instead of the whole account. On by default; the
    /// tracker still decides per account whether it takes batches. Off is
    /// the behaviour from before: every change uploads the whole account.
    #[serde(default = "default_live_sync")]
    pub live_sync: bool,
    /// Whether the stored key was accepted by the tracker *in this session*.
    ///
    /// `serde(skip)`, because it records a round trip that actually happened
    /// rather than a preference: persisting it would let a key revoked between
    /// runs start the next one looking verified. It lives on the saved state
    /// rather than on `TeyvatmeApp` because this struct is the channel
    /// `monitor.rs` reads, and the automation upload has to gate on the same
    /// thing the manual button does.
    #[serde(skip)]
    pub tracker_verified: bool,
    /// Whether the tracker sign-in is signed in, mirrored from its status by
    /// the UI every frame. Never persisted (the sign-in itself lives in the
    /// OS credential store); here because, like `tracker_verified`, this
    /// struct is what `monitor.rs`'s automation reads.
    #[serde(skip)]
    pub tracker_sign_in: TrackerSignIn,
    #[serde(default)]
    pub minimize_to_tray: Option<bool>,
    /// The first-run screens were done. A fresh install starts at `false`;
    /// settings saved by an older build load as `true`, since that user is
    /// past first run.
    #[serde(default = "default_true")]
    pub onboarded: bool,
    #[serde(default)]
    pub theme: ThemeChoice,
    /// Send the wish history to the linked tracker account (the wish card's
    /// switch). On by default; needs a sign-in or a verified import key.
    #[serde(default = "default_true")]
    pub send_wishes: bool,
}

/// The theme setting (Settings › App).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_live_sync() -> bool {
    true
}

/// Where the tracker lives when the build bakes in no `TRACKER_API_URL`: the
/// hosted Teyvat.me tracker. Its Worker mounts the API under `/api`, so the
/// endpoint paths are appended after that; without it the request reaches the
/// single-page app, which answers every unknown path with HTML.
const FALLBACK_TRACKER_URL: &str = "https://teyvat.me/api";

/// The default before the hosted tracker existed: the self-hosted NestJS
/// backend on its default port. `app.ron` files written by those builds hold
/// it, and [`migrated_tracker_url`] moves them to the current default.
const LEGACY_TRACKER_URL: &str = "http://localhost:49000";

/// The hosted tracker's address before it moved to teyvat.me (irminsul's
/// default). The same Worker still answers there, but saved copies of it
/// move to the current default too; the sign-in moves with them
/// (`signin::MOVED_ORIGINS`).
const OLD_HOSTED_TRACKER_URL: &str = "https://genshin-tracker.475.dev/api";

/// Resolve the compile-time `TRACKER_API_URL` against the hosted default.
///
/// `option_env!` yields `Some("")` for a variable that is *set but empty*, so
/// `unwrap_or` alone silently lost the fallback and left every request built
/// against a relative URL — reqwest then failed at `send()` with "builder
/// error", and with the field serde-skipped there was no way for a user to
/// repair it.
fn resolve_tracker_url(baked_in: Option<&str>) -> String {
    baked_in
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .unwrap_or(FALLBACK_TRACKER_URL)
        .to_string()
}

/// The tracker base URL baked in at compile time, or the hosted default.
fn default_tracker_url() -> String {
    resolve_tracker_url(option_env!("TRACKER_API_URL"))
}

/// `url` without surrounding whitespace and trailing slashes.
fn bare_url(url: &str) -> &str {
    url.trim().trim_end_matches('/')
}

/// Whether `url` is one of the old defaults ([`LEGACY_TRACKER_URL`],
/// [`OLD_HOSTED_TRACKER_URL`]), give or take surrounding whitespace and
/// trailing slashes.
fn is_old_default_tracker_url(url: &str) -> bool {
    [LEGACY_TRACKER_URL, OLD_HOSTED_TRACKER_URL].contains(&bare_url(url))
}

/// The URL a saved `tracker_api_url` should be replaced with on load, if any.
///
/// The saved value is persisted, so an install that never touched the field
/// would otherwise keep an old default forever. An empty value is repaired
/// too: it makes every request a relative URL that reqwest refuses to send.
/// Anything else was typed by the user and is kept.
///
/// There is no "already migrated" marker, so an old default is moved on every
/// load; pointing a build at a local backend on that port means spelling it
/// differently (`http://127.0.0.1:49000`). A build whose own default *is* that
/// URL (`TRACKER_API_URL=http://localhost:49000`) leaves it alone rather than
/// "migrating" it to itself.
fn migrated_tracker_url(saved: &str, default: &str) -> Option<String> {
    let stale = saved.trim().is_empty()
        || (is_old_default_tracker_url(saved) && bare_url(saved) != bare_url(default));
    (stale && saved != default).then(|| default.to_string())
}

impl SavedAppState {
    /// Bring state loaded from an older `app.ron` up to date.
    fn migrate(&mut self) {
        if let Some(url) = migrated_tracker_url(&self.tracker_api_url, &default_tracker_url()) {
            tracing::info!(
                "Moving tracker API URL from {:?} to the default {url}",
                self.tracker_api_url
            );
            self.tracker_api_url = url;
        }
    }
}

impl Default for SavedAppState {
    fn default() -> Self {
        Self {
            export_settings: ExportSettings::default(),
            start_on_startup: false,
            save_result_to_file: false,
            save_result_folder: None,
            log_raw_packets: false,
            tracing_level: Default::default(),
            tracker_import_key: String::new(),
            tracker_api_url: default_tracker_url(),
            auto_export_to_tracker: false,
            live_sync: default_live_sync(),
            tracker_verified: false,
            tracker_sign_in: TrackerSignIn::Unknown,
            minimize_to_tray: None,
            onboarded: false,
            theme: ThemeChoice::System,
            send_wishes: true,
        }
    }
}

#[derive(Clone, Debug)]
enum OptimizerExportTarget {
    None,
    Clipboard,
    File,
    TrackerManual,
}

impl OptimizerExportTarget {
    /// The settings an export for this target is built with.
    ///
    /// The clipboard and the file are Genshin Optimizer exports and follow the
    /// user's settings (`optimizer`). A tracker upload always carries the
    /// whole account, whatever they filter out: it used to share them, so the
    /// tracker never saw a 1★ or 2★ artifact or weapon. The automated upload
    /// in `monitor.rs` makes the same split (`AutomationPlan`).
    fn export_settings(&self, optimizer: &ExportSettings) -> ExportSettings {
        match self {
            Self::TrackerManual => ExportSettings::for_tracker(),
            Self::None | Self::Clipboard | Self::File => optimizer.clone(),
        }
    }
}

/// The outcome of polling a background reply channel.
enum Polled<T> {
    /// No answer yet; keep waiting.
    Pending,
    /// The background task answered.
    Ready(T),
    /// The sender went away without answering — a dead or restarted monitor
    /// thread. Treated as a failure so the UI cannot sit on "Verifying…"
    /// forever, which is what it used to do.
    Dropped,
}

/// Poll a oneshot without blocking, clearing `slot` once it can never produce
/// anything more.
fn poll_oneshot<T>(slot: &mut Option<oneshot::Receiver<T>>) -> Polled<T> {
    let Some(rx) = slot.as_mut() else {
        return Polled::Pending;
    };

    match rx.try_recv() {
        Ok(value) => {
            *slot = None;
            Polled::Ready(value)
        }
        Err(oneshot::error::TryRecvError::Empty) => Polled::Pending,
        Err(oneshot::error::TryRecvError::Closed) => {
            *slot = None;
            Polled::Dropped
        }
    }
}

/// Recover the HTTP status from the error string `monitor.rs` builds for a
/// failed tracker request, which is always `format!("HTTP {status} - {body}")`.
///
/// The old test searched the *whole* string — response body included — for
/// "401" or "403", so any non-2xx body that happened to contain those digits
/// (a numeric id, a byte count, a proxy error page) dropped the verified state
/// and fired an extra verification round trip.
fn tracker_error_status(error: &str) -> Option<u16> {
    let rest = error.strip_prefix("HTTP ")?;
    let digits = rest.split(|c: char| !c.is_ascii_digit()).next()?;
    digits.parse().ok()
}

/// Whether a tracker upload can be attempted at all.
///
/// The manual cloud-upload button gates on this. It used to share the
/// automation predicate below, which also requires the "Auto export to
/// tracker" checkbox, so on the default settings -- key verified, auto export
/// off -- clicking the *enabled* button ran a full export and then toasted
/// "Tracker key not verified. Open settings to re-link."
///
/// Signed in, the sign-in uploads; signed out, a saved and verified import
/// key does, exactly as before sign-in existed; while the sign-in is still
/// being read, nothing does.
pub(crate) fn can_upload_to_tracker(state: &SavedAppState) -> bool {
    upload_credential(state).is_some()
}

/// How the next upload authenticates, if it can at all. The one place both
/// upload paths (the button and the automation) choose between the sign-in
/// and the import key.
pub(crate) fn upload_credential(state: &SavedAppState) -> Option<UploadCredential> {
    match state.tracker_sign_in {
        TrackerSignIn::SignedIn => Some(UploadCredential::SignedIn {
            base: state.tracker_api_url.clone(),
        }),
        TrackerSignIn::SignedOut
            if !state.tracker_import_key.is_empty() && state.tracker_verified =>
        {
            Some(UploadCredential::ImportKey {
                url: crate::monitor::import_url(&state.tracker_api_url),
                key: state.tracker_import_key.clone(),
            })
        }
        TrackerSignIn::SignedOut | TrackerSignIn::Unknown => None,
    }
}

/// What applying an edited tracker address would do.
#[derive(Clone, Debug, PartialEq, Eq)]
enum AddressChange {
    /// The same address (ignoring surrounding space and a trailing slash).
    Same,
    /// Not an http(s) URL.
    Invalid,
    /// A new address, normalized, and its host for the confirmation.
    To { base: String, host: String },
}

fn address_change(current: &str, edited: &str) -> AddressChange {
    let base = signin::normalize_base(edited);
    if base == signin::normalize_base(current) {
        return AddressChange::Same;
    }
    match signin::host_of(&base) {
        Some(host) => AddressChange::To { base, host },
        None => AddressChange::Invalid,
    }
}

/// Whether whoami should be read again: never read yet, the window just
/// got the focus back (and the last read is not brand new), or it is due.
fn whoami_refresh_due(regained_focus: bool, since_last: Option<Duration>) -> bool {
    match since_last {
        None => true,
        Some(since) => {
            since >= WHOAMI_REFRESH_INTERVAL
                || (regained_focus && since >= WHOAMI_FOCUS_MIN_INTERVAL)
        }
    }
}

/// A whoami answer as the account the UID check compares against: only in
/// account mode, where every upload goes to that one account.
fn whoami_account(whoami: &Whoami) -> Option<TrackerAccount> {
    // No account id: the chosen account was deleted, nothing to compare.
    (whoami.scope == UploadScope::Account && whoami.account_id.is_some()).then(|| TrackerAccount {
        name: whoami
            .account_name
            .clone()
            .unwrap_or_else(|| "the account".to_string()),
        uid: whoami.uid.clone().unwrap_or_else(|| "N/A".to_string()),
        server: whoami.server.clone().unwrap_or_else(|| "N/A".to_string()),
        dashboard_url: whoami.dashboard_url.clone(),
    })
}

/// Whether the automation path should upload after an export.
///
/// `monitor.rs`'s automation trigger and `execute_automation_export` call this
/// rather than keeping their own copy of the condition, which is how the copy
/// came to omit `tracker_verified` -- a key the dashboard had revoked still got
/// a POST on every login.
/// Whether the wish history goes to the tracker: the wish card's switch and
/// a linked account (sign-in or verified key). Independent of snapshot
/// uploads, so the switch means what it says.
pub(crate) fn want_wish_sync(state: &SavedAppState) -> bool {
    state.send_wishes && can_upload_to_tracker(state)
}

pub(crate) fn want_tracker_upload(state: &SavedAppState) -> bool {
    state.auto_export_to_tracker && can_upload_to_tracker(state)
}

/// One captured data class a GOOD export writes, and when it last arrived.
pub(crate) struct ExportDataClass {
    /// How the class is named in the toast that says what is missing.
    pub name: &'static str,
    pub captured_at: Option<Instant>,
}

/// The captured data classes an export writes, given the user's settings.
///
/// The single place that answers "what does this export need?", shared by the
/// GOOD file/clipboard export, the manual tracker upload and the automation
/// trigger in `monitor.rs` -- the three sites that used to each demand
/// characters *and* items *and* achievements.
pub(crate) fn export_data_classes(
    settings: &ExportSettings,
    updated: &DataUpdated,
) -> Vec<ExportDataClass> {
    let mut classes = Vec::new();

    if settings.include_characters {
        classes.push(ExportDataClass {
            name: "character data",
            captured_at: updated.characters_updated,
        });
    }

    // Artifacts, weapons and materials all arrive in the one inventory burst
    // that stamps `items_updated`.
    if settings.include_artifacts || settings.include_weapons || settings.include_materials {
        classes.push(ExportDataClass {
            name: "inventory data (artifacts, weapons and materials)",
            captured_at: updated.items_updated,
        });
    }

    classes
}

/// The captured data classes an export still needs, named for a toast.
///
/// Only what the user actually asked to export is required. All three export
/// paths used to demand characters *and* items *and* achievements, so an
/// account whose achievement packet was never seen -- or never identified --
/// could not export its artifacts either, and the toast said only "Data not
/// found. Please open the game first."
///
/// Achievements are deliberately not in this list:
/// `export_genshin_optimizer_with_report` attaches whatever achievements have
/// been captured (`gi_achievements`, an empty list when none have) and there
/// is no setting to include or exclude them, so they can never *block* a GOOD
/// export. The achievement export button has its own check.
pub(crate) fn missing_export_data(
    settings: &ExportSettings,
    updated: &DataUpdated,
) -> Vec<&'static str> {
    export_data_classes(settings, updated)
        .into_iter()
        .filter(|class| class.captured_at.is_none())
        .map(|class| class.name)
        .collect()
}

/// Name what is missing instead of the old catch-all "Data not found".
fn missing_export_data_toast(missing: &[&'static str]) -> String {
    format!(
        "No {} captured yet. Log in to the game with the account you want to export.",
        missing.join(" or ")
    )
}

/// The captured UID, when uploads go to one account and it has another.
///
/// `None` whenever either side is unknown: the old backend may answer with no
/// UID ("N/A"), an account need not have one set, and no UID is claimed for a
/// capture whose guids disagree.
fn uid_mismatch(account: &TrackerAccount, captured: Option<u32>) -> Option<u32> {
    let captured = captured?;
    let linked: u64 = account.uid.trim().parse().ok()?;
    (linked != u64::from(captured)).then_some(captured)
}

/// What the UID warning says, in its toast and its tooltip.
fn uid_mismatch_message(account: &TrackerAccount, captured: u32) -> String {
    format!(
        "This capture is UID {captured}, but uploads go to {} (UID {}). Uploads still go to \
         that account: log in with it, or change where uploads go.",
        account.name,
        account.uid.trim()
    )
}

pub struct TeyvatmeApp {
    ui_message_tx: mpsc::UnboundedSender<Message>,
    /// Answers to the update prompt only. Keeping these off `ui_message_tx` is
    /// what stops the update check from eating unrelated startup messages.
    update_answer_tx: mpsc::UnboundedSender<UpdateAnswer>,
    state_rx: watch::Receiver<AppState>,
    /// The wish URL (from the game's key, or its web cache) and the sync.
    wishes: crate::wish_history::WishView,
    log_packets_tx: watch::Sender<bool>,
    saved_state_tx: watch::Sender<SavedAppState>,
    tracing_reload_handle: ReloadHandle,
    toast_rx: crate::activity::Receiver,

    monitor_handle: Option<JoinHandle<()>>,
    monitor_cancel_token: CancellationToken,
    /// Held by the update path while the running executable is being replaced.
    /// `Drop` waits on it before its bounded join gets to give up on anything.
    install_lock: Arc<InstallLock>,

    toasts: Toasts,

    automation_folder_dialog: Option<FileDialog>,

    optimizer_export_rx: Option<oneshot::Receiver<Result<(String, ExportCounts)>>>,
    achievements_export_rx: Option<oneshot::Receiver<Result<Vec<u32>>>>,
    pending_open_url: Option<String>,
    optimizer_save_dialog: Option<FileDialog>,
    optimizer_save_path: Option<PathBuf>,
    optimizer_export_target: OptimizerExportTarget,

    restarting: bool,
    /// Shared with `main`, which does the relaunch after `run_native` returns
    /// and the single-instance mutex has been released.
    restart_requested: Arc<AtomicBool>,

    saved_state: SavedAppState,

    tracker_account: Option<TrackerAccount>,
    tracker_verify_rx: Option<oneshot::Receiver<Result<TrackerAccount>>>,
    tracker_upload_rx: Option<oneshot::Receiver<Result<(), String>>>,
    /// The tracker sign-in's status (`signin.rs`), published by the monitor
    /// side.
    auth_status_rx: watch::Receiver<AuthStatus>,
    /// Sign-in pages to open in the browser, from the monitor side; opened
    /// here so they take the same `open_url` path as every other link.
    browser_rx: mpsc::UnboundedReceiver<String>,
    /// When whoami was last asked for.
    whoami_checked_at: Option<Instant>,
    /// Whether the window had the focus last frame (for the whoami re-read
    /// on return).
    window_focused: bool,
    /// The sign-in notice last toasted, so it is said once.
    auth_notice_toasted: Option<String>,
    /// The tracker address being edited under "Advanced"; applied only once
    /// confirmed.
    tracker_url_edit: String,
    /// An edited address waiting on "Uploads will now go to <host>".
    pending_address: Option<String>,
    /// The `(linked account UID, captured UID)` pair the mismatch toast was
    /// last raised for, so it appears once per occurrence rather than per
    /// frame. See `note_uid_mismatch`.
    uid_mismatch_warned: Option<(String, u32)>,
    /// The (linked UID, captured UID) pair last logged as agreeing, so the
    /// "UIDs match" line is written once rather than every frame.
    uid_match_logged: Option<(String, u32)>,

    /// The "Genshin is already running" modal.
    game_missed_modal_open: bool,
    /// The verdict the modal was last raised for, so it appears once per
    /// occurrence instead of on every two-second poll. Cleared whenever the
    /// game status leaves the problem state, so a later recurrence -- a second
    /// game session started while capture was off -- raises it again.
    game_missed_modal_shown_for: Option<MissedLaunch>,
    game_kill_rx: Option<oneshot::Receiver<usize>>,
    /// The start-up memory release has been scheduled.
    #[cfg(windows)]
    startup_memory_released: bool,
    /// The game status the last session memory release looked at.
    #[cfg(windows)]
    memory_release_status: GameStatus,

    #[allow(dead_code)]
    tray_icon: Option<TrayIcon>,
    /// Whether there is a tray icon with a menu the user can actually act on.
    ///
    /// On Linux `TrayIconEvent` is never delivered, so the menu is the only way
    /// back to a hidden window — hiding without one is a one-way trip that
    /// leaves killing the process as the only exit. Set from the Linux tray
    /// thread once its menu is up, hence the atomic.
    tray_menu_ready: Arc<AtomicBool>,

    minimize_modal_open: bool,
    minimize_modal_remember: bool,

    /// "Start Teyvat.me on startup": the scheduled task's state, read in the
    /// background, and the warning when it starts another copy.
    startup: Startup,

    /// Said once at startup when the previous run did not exit cleanly (see
    /// `crash.rs`), until dismissed.
    previous_run_notice: Option<String>,

    /// What the first frame does with the window, then [`StartWindow::AsIs`].
    start_window: StartWindow,

    /// Which screen the window shows.
    view: View,
    /// What the app did, for the footer and its popover.
    activity: crate::activity::Log,
    activity_open: bool,
    /// Background toasts that arrived while the window was out of sight:
    /// the newest success and the newest problem, for the return.
    held_notice: Option<crate::activity::Event>,
    held_problem: Option<crate::activity::Event>,
    /// When this session started decrypting ("Session key found · HH:mm").
    decoding_since: Option<chrono::DateTime<chrono::Local>>,
    last_game_status: GameStatus,
    /// Whether this process can capture (Windows: elevated).
    is_admin: bool,
    applied_theme: Option<ThemeChoice>,
    /// An update the user put off at start, for the footer pill and About.
    update_put_off: Option<String>,
    /// The folder of the last manual save, for "Open export folder".
    last_save_folder: Option<PathBuf>,
    started_at: Instant,
}

/// The screen the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Main,
    Settings(Page),
    FirstRun(Onboarding),
}

/// A settings page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    App,
    Export,
    Destinations,
    Teyvatme,
    Diagnostics,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Onboarding {
    Welcome,
    SetUp,
}

mod frame;
mod home;
#[cfg(debug_assertions)]
mod preview;
mod run;
mod screens;
mod settings;

/// How the window starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartWindow {
    /// As the system shows it.
    AsIs,
    /// `--minimized` ("Start minimized" on the startup task): minimized on the
    /// first frame, like the minimize button would.
    Minimized,
    /// `--after-update`, the relaunch after a self-update: restored and
    /// brought to the front. The user just clicked "Yes" on the update, but
    /// the new process is started by one whose window has already closed, so
    /// Windows may keep it behind other windows or show it only as a taskbar
    /// button.
    Front,
}

/// Set while the window is hidden to the tray: from [`TeyvatmeApp::minimize`]
/// until [`restore_window`], which runs on the tray's thread.
static HIDDEN_TO_TRAY: AtomicBool = AtomicBool::new(false);

trait ToastError<T> {
    fn toast_error(self, app: &mut TeyvatmeApp) -> Option<T>;
}

impl<T, E: Display> ToastError<T> for std::result::Result<T, E> {
    fn toast_error(self, app: &mut TeyvatmeApp) -> Option<T> {
        match self {
            Ok(val) => Some(val),
            Err(e) => {
                tracing::error!("{e}");
                app.toasts.error(e.to_string());
                None
            }
        }
    }
}

/// Decode the bundled icon into the form `tray-icon` wants.
///
/// Built where the tray lives rather than passed across a thread boundary, so
/// no assumption is made about `tray_icon::Icon` being `Send`.
fn load_tray_icon() -> Option<tray_icon::Icon> {
    let icon_data = image::load_from_memory(include_bytes!("../assets/icon-256.png")).ok()?;
    let rgba = icon_data.into_rgba8();
    let (width, height) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), width, height).ok()
}

/// How long after the window is hidden or minimized its memory is handed back:
/// long enough for the frame that hid it to have been drawn.
#[cfg(windows)]
const RELEASE_HIDDEN_MEMORY_DELAY: Duration = Duration::from_secs(2);

/// Hand back the RAM a window that is out of sight was holding.
///
/// Most of a visible Teyvat.me's memory belongs to the graphics driver (on an
/// NVIDIA card its OpenGL context alone is over 100 MB, kept until the process
/// exits) and to egui, and none of it is touched while the window is in the
/// tray or minimized, because nothing is drawn. Windows takes such pages back
/// only once it runs short, so until then they count against a machine that is
/// also running the game. Trimming the working set returns them at once; the
/// capture and decoding keep the few MB they use, and the rest is read back in
/// (from RAM, if nothing has claimed it yet) when the window is shown again.
///
/// Skipped if the window is back on screen by the time the delay is up.
#[cfg(windows)]
fn release_memory_once_hidden() {
    let spawned = thread::Builder::new()
        .name("release-hidden-memory".into())
        .spawn(|| {
            thread::sleep(RELEASE_HIDDEN_MEMORY_DELAY);
            if !window_out_of_sight() {
                return;
            }
            match trim_working_set() {
                Ok(()) => tracing::info!("window hidden: released its memory"),
                Err(e) => tracing::warn!("could not release the hidden window's memory: {e}"),
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("could not start the thread that releases hidden memory: {e}");
    }
}

/// How long after start-up its one-off memory is handed back: by then the
/// first frames are drawn and the game data, fonts and TLS are set up.
#[cfg(windows)]
const RELEASE_STARTUP_MEMORY_DELAY: Duration = Duration::from_secs(10);

/// How long after a game session starts decoding its login burst (the whole
/// inventory, roster and achievements at once) is handed back.
#[cfg(windows)]
const RELEASE_SESSION_MEMORY_DELAY: Duration = Duration::from_secs(20);

/// Hand back memory a one-off burst of work touched and won't touch again.
///
/// A never-minimized window was measured holding 435 MB of private working set
/// for good, while the same window needed about 11 MB after one minimize and
/// restore: start-up (the graphics driver's first frames and shader compiles
/// above all) touches hundreds of MB once. Trimming after start-up and after a
/// session's login burst does what that minimize did, without the minimize;
/// pages still in use are simply read back in. Unlike
/// [`release_memory_once_hidden`] this runs whether or not the window shows.
#[cfg(windows)]
fn release_memory_after(delay: Duration, what: &'static str) {
    let spawned = thread::Builder::new()
        .name("release-settled-memory".into())
        .spawn(move || {
            thread::sleep(delay);
            match trim_working_set() {
                Ok(()) => tracing::info!("{what}: released memory it no longer touches"),
                Err(e) => tracing::warn!("could not release the memory {what} used: {e}"),
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("could not start the thread that releases settled memory: {e}");
    }
}

/// Which releases [`TeyvatmeApp::release_settled_memory`] starts this frame:
/// (start-up, game session). The start-up one once; the session one each time
/// the game status turns to decoding.
#[cfg(any(windows, test))]
fn memory_release_due(startup_released: bool, last: GameStatus, now: GameStatus) -> (bool, bool) {
    (
        !startup_released,
        now == GameStatus::Decoding && last != GameStatus::Decoding,
    )
}

/// Remove as many pages as possible from this process's working set.
#[cfg(windows)]
fn trim_working_set() -> windows::core::Result<()> {
    use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

    // (SIZE_T)-1 for both bounds is the documented "trim it" request; it sets
    // no limit on how far the working set may grow again.
    unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) }
}

/// Whether Teyvat.me's window is hidden or minimized. `false` when it cannot
/// be found, so nothing is released on a guess.
///
/// Only a window of this process counts: any other top-level window can be
/// titled "Teyvat.me" too, an Explorer window on a folder of that name for one.
#[cfg(windows)]
fn window_out_of_sight() -> bool {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    };
    use windows::core::PCWSTR;

    let title: Vec<u16> = std::ffi::OsStr::new("Teyvat.me")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut after = None;
    unsafe {
        while let Ok(hwnd) = FindWindowExW(None, after, PCWSTR::null(), PCWSTR(title.as_ptr()))
            && !hwnd.0.is_null()
        {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                return !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool();
            }
            after = Some(hwnd);
        }
    }
    false
}

/// Bring a window hidden to the tray back.
fn restore_window(ctx: &Context) {
    HIDDEN_TO_TRAY.store(false, Ordering::Relaxed);
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
    #[cfg(windows)]
    show_window();
}

/// Quit from the tray menu.
fn quit_from_tray(ctx: &Context) {
    #[cfg(windows)]
    close_window();
    #[cfg(not(windows))]
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    ctx.request_repaint();
}

#[allow(clippy::too_many_arguments)]
fn start_async_runtime(
    cancel_token: CancellationToken,
    egui_ctx: Context,
    log_packets_rx: watch::Receiver<bool>,
    saved_state_rx: watch::Receiver<SavedAppState>,
    capture_backend: capture::BackendType,
    capture_source: capture::CaptureSource,
    install_lock: Arc<InstallLock>,
    tracker_auth: TrackerAuth,
) -> AsyncRuntimeHandles {
    tracing::info!("starting tokio async");
    let (ui_message_tx, ui_message_rx) = mpsc::unbounded_channel::<Message>();
    let (update_answer_tx, mut update_answer_rx) = mpsc::unbounded_channel::<UpdateAnswer>();
    let (toast_tx, toast_rx) = mpsc::unbounded_channel::<crate::activity::Event>();

    let (state_tx, state_rx) = watch::channel(AppState::new());
    let crate::wish_history::WishChannels {
        hub: wish_hub,
        view: wish_view,
        sync: wish_sync,
    } = crate::wish_history::channels(
        saved_state_rx.clone(),
        toast_tx.clone(),
        egui_ctx.clone(),
        tracker_auth.clone(),
    );
    let mut updater_state_rx = state_rx.clone();
    let updater_ctx = egui_ctx.clone();
    let monitor_handle = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let monitor_ctx = updater_ctx.clone();
            // Notify egui of state changes. This has to be running *before* the
            // update check: the check pushes the "update available" prompt
            // through `state_tx`, and without a repainter listening the window
            // sat on "Checking for Teyvat.me updates" until the user happened to
            // move the pointer over it.
            tokio::spawn(async move {
                loop {
                    let _ = updater_state_rx.changed().await;
                    updater_ctx.request_repaint();
                }
            });

            // Before starting the monitor, check for updates if not in debug mode
            tracing::info!("Checking for update");
            if let Err(e) = check_for_app_update(
                &state_tx,
                &mut update_answer_rx,
                &cancel_token,
                &toast_tx,
                &install_lock,
            )
            .await
            {
                tracing::error!("error checking for update: {e}");
            }

            // Wish history from the keys; ends with the monitor.
            tokio::spawn(wish_sync.run());

            tracing::info!("Starting monitor");
            let monitor = match Monitor::new(
                cancel_token,
                state_tx,
                ui_message_rx,
                log_packets_rx,
                capture_backend,
                capture_source,
                saved_state_rx,
                toast_tx,
                tracker_auth,
                monitor_ctx,
            )
            .await
            {
                Ok(monitor) => monitor.with_wish_hub(wish_hub),
                Err(e) => {
                    tracing::error!("error loading monitor task: {e}");
                    return;
                }
            };
            monitor.run().await;
        });
    });
    tracing::info!("started tokio");
    (
        ui_message_tx,
        update_answer_tx,
        state_rx,
        wish_view,
        toast_rx,
        monitor_handle,
    )
}

impl TeyvatmeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut tracing_reload_handle: ReloadHandle,
        capture_backend: capture::BackendType,
        capture_source: capture::CaptureSource,
        restart_requested: Arc<AtomicBool>,
        previous_run_notice: Option<String>,
        start_window: StartWindow,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        crate::ui::theme::install(&cc.egui_ctx);

        let mut saved_state: SavedAppState = if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        };
        saved_state.migrate();

        tracing::info!("Tracker API URL: {}", saved_state.tracker_api_url);

        let cancel_token = CancellationToken::new();
        let install_lock = Arc::new(InstallLock::new());
        tracing_reload_handle.set_filter(saved_state.tracing_level.get_filter());
        let (log_packets_tx, log_packets_rx) = watch::channel(saved_state.log_raw_packets);
        let (saved_state_tx, saved_state_rx) = watch::channel(saved_state.clone());

        // The tracker sign-in. Its refresh token lives in the OS credential
        // store; reading it is the first thing the monitor does with it
        // (`TrackerRequest::Load` below), off this thread.
        let (browser_tx, browser_rx) = mpsc::unbounded_channel::<String>();
        let opener_ctx = cc.egui_ctx.clone();
        let notify_ctx = cc.egui_ctx.clone();
        let tracker_auth = TrackerAuth::new(
            &saved_state.tracker_api_url,
            Arc::new(signin::KeyringStore::taking_over_irminsul()),
            signin::device_name(),
            Arc::new(move |url| {
                let _ = browser_tx.send(url);
                opener_ctx.request_repaint();
            }),
            Arc::new(move || notify_ctx.request_repaint()),
        );
        let auth_status_rx = tracker_auth.status();

        let (ui_message_tx, update_answer_tx, state_rx, wishes, toast_rx, monitor_handle) =
            start_async_runtime(
                cancel_token.clone(),
                cc.egui_ctx.clone(),
                log_packets_rx,
                saved_state_rx,
                capture_backend,
                capture_source,
                Arc::clone(&install_lock),
                tracker_auth,
            );

        if let Err(e) = ui_message_tx.send(Message::StartCapture) {
            tracing::error!("Failed to send auto start message: {e}");
        }
        let _ = ui_message_tx.send(Message::Tracker(TrackerRequest::Load));

        let toasts = Toasts::default();

        // Moves a Run entry left by an older version to a scheduled task, then
        // reads what Windows will start at sign-in.
        let startup = Startup::new(&cc.egui_ctx);

        // Auto-verify tracker key on startup
        let tracker_verify_rx = if !saved_state.tracker_import_key.is_empty() {
            let key = saved_state.tracker_import_key.clone();
            let url = format!(
                "{}/genshin-accounts-public/verify-key",
                saved_state.tracker_api_url.trim_end_matches('/')
            );
            let (tx, rx) = oneshot::channel();
            let _ = ui_message_tx.send(Message::VerifyTrackerKey(url, key, tx));
            Some(rx)
        } else {
            None
        };

        #[allow(unused_mut)]
        let mut tray_icon = None;
        let tray_menu_ready = Arc::new(AtomicBool::new(false));

        #[cfg(not(target_os = "linux"))]
        if let Some(icon) = load_tray_icon() {
            let tray_menu = Menu::new();
            let restore_i = MenuItem::new("Restore", true, None);
            let quit_i = MenuItem::new("Quit", true, None);
            let restore_id = restore_i.id().clone();
            let quit_id = quit_i.id().clone();
            let menu_populated = tray_menu.append_items(&[&restore_i, &quit_i]).is_ok();

            tray_icon = TrayIconBuilder::new()
                .with_tooltip("Teyvat.me")
                .with_icon(icon)
                .with_menu(Box::new(tray_menu))
                .build()
                .ok();

            if tray_icon.is_some() && menu_populated {
                tray_menu_ready.store(true, Ordering::Relaxed);
            }

            let ctx_clone1 = cc.egui_ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    restore_window(&ctx_clone1);
                }
            }));

            let ctx_clone2 = cc.egui_ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == restore_id {
                    restore_window(&ctx_clone2);
                } else if event.id == quit_id {
                    quit_from_tray(&ctx_clone2);
                }
            }));
        }

        // `Menu` and `MenuItem` are GTK objects and are not `Send`, so on Linux
        // the whole tray — icon, menu, items and the menu-event handler — has
        // to be built on the thread that owns the GTK main loop. Building the
        // menu on this thread and passing `Menu::new()` to the builder is what
        // left Linux users with an empty right-click menu and, since
        // `TrayIconEvent` is never delivered there, no way at all to get a
        // hidden window back.
        #[cfg(target_os = "linux")]
        {
            let tray_menu_ready = Arc::clone(&tray_menu_ready);
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                // `gtk::init().unwrap()` here took the whole app down on any
                // machine without a usable display.
                if let Err(e) = gtk::init() {
                    tracing::warn!("could not initialise GTK; the tray icon is disabled: {e}");
                    return;
                }

                let Some(icon) = load_tray_icon() else {
                    tracing::warn!("could not build the tray icon image; the tray is disabled");
                    return;
                };

                let tray_menu = Menu::new();
                let restore_i = MenuItem::new("Restore", true, None);
                let quit_i = MenuItem::new("Quit", true, None);
                let restore_id = restore_i.id().clone();
                let quit_id = quit_i.id().clone();
                if let Err(e) = tray_menu.append_items(&[&restore_i, &quit_i]) {
                    tracing::warn!("could not populate the tray menu; the tray is disabled: {e}");
                    return;
                }

                let _tray_icon = match TrayIconBuilder::new()
                    .with_tooltip("Teyvat.me")
                    .with_icon(icon)
                    .with_menu(Box::new(tray_menu))
                    .build()
                {
                    Ok(tray_icon) => tray_icon,
                    Err(e) => {
                        tracing::warn!("could not create the tray icon: {e}");
                        return;
                    }
                };

                MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                    if event.id == restore_id {
                        restore_window(&ctx);
                    } else if event.id == quit_id {
                        quit_from_tray(&ctx);
                    }
                }));

                // Only now is "minimize to tray" something the user can undo.
                tray_menu_ready.store(true, Ordering::Relaxed);

                gtk::main();
            });
        }

        let tracker_url_edit = saved_state.tracker_api_url.clone();
        Self {
            saved_state,
            ui_message_tx,
            update_answer_tx,
            restart_requested,
            tray_menu_ready,
            game_missed_modal_open: false,
            game_missed_modal_shown_for: None,
            game_kill_rx: None,
            #[cfg(windows)]
            startup_memory_released: false,
            #[cfg(windows)]
            memory_release_status: GameStatus::default(),
            log_packets_tx,
            saved_state_tx,
            tracing_reload_handle,
            toast_rx,
            toasts,
            automation_folder_dialog: None,
            optimizer_export_rx: None,
            achievements_export_rx: None,
            pending_open_url: None,
            optimizer_save_dialog: None,
            optimizer_save_path: None,
            optimizer_export_target: OptimizerExportTarget::None,
            restarting: false,
            tracker_account: None,
            tracker_verify_rx,
            tracker_upload_rx: None,
            auth_status_rx,
            browser_rx,
            whoami_checked_at: None,
            window_focused: true,
            auth_notice_toasted: None,
            tracker_url_edit,
            pending_address: None,
            uid_mismatch_warned: None,
            uid_match_logged: None,
            tray_icon,
            state_rx,
            wishes,
            minimize_modal_open: false,
            minimize_modal_remember: true,
            startup,
            previous_run_notice,
            start_window,
            view: View::Main,
            activity: crate::activity::Log::default(),
            activity_open: false,
            held_notice: None,
            held_problem: None,
            decoding_since: None,
            last_game_status: GameStatus::default(),
            is_admin: is_admin(),
            applied_theme: None,
            update_put_off: None,
            last_save_folder: None,
            started_at: Instant::now(),
            monitor_cancel_token: cancel_token,
            monitor_handle: Some(monitor_handle),
            install_lock,
        }
    }
}

impl Drop for TeyvatmeApp {
    fn drop(&mut self) {
        self.monitor_cancel_token.cancel();

        // Before any deadline applies: a `self_replace` already under way has
        // to be allowed to finish. It renames the running exe aside and then
        // renames the replacement into place, so a process killed inside that
        // window leaves *nothing* at the install path -- the unrecoverable
        // brick the update guards exist to prevent, caused by the guard
        // against a wedged exit. This also stops one starting from here on;
        // the download half checks the cancel token and gives up on its own.
        self.install_lock.shutdown_and_wait();

        let Some(handle) = self.monitor_handle.take() else {
            return;
        };

        // Join with a deadline. This used to be an unconditional `join()`, so a
        // background task that never noticed the cancel token — the update
        // check, which ignored it entirely — wedged process exit *while still
        // holding the single-instance mutex*, and every relaunch afterwards
        // died silently until the user found the process in Task Manager.
        // Nothing here is allowed to be able to do that again: past the
        // timeout the thread is detached and `main` returning takes it down.
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let waiter = thread::spawn(move || {
            let result = handle.join();
            let _ = done_tx.send(result.is_ok());
        });

        match done_rx.recv_timeout(MONITOR_SHUTDOWN_TIMEOUT) {
            Ok(true) => {
                let _ = waiter.join();
            }
            Ok(false) => {
                tracing::error!("monitor thread panicked");
                let _ = waiter.join();
            }
            Err(_) => tracing::warn!(
                "monitor thread did not shut down within {:?}; detaching it and exiting anyway",
                MONITOR_SHUTDOWN_TIMEOUT
            ),
        }
    }
}

impl eframe::App for TeyvatmeApp {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.saved_state);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_frame(ctx);
    }
}

impl TeyvatmeApp {
    /// Collect everything the background thread has answered since the last
    /// frame. Called once at the top of `update`.
    fn poll_background_results(&mut self, ctx: &Context) {
        self.optimizer_handle_export(ctx).toast_error(self);
        self.achievements_handle_export(ctx).toast_error(self);
        self.poll_tracker_sign_in(ctx);
        self.poll_tracker_upload();
        self.poll_tracker_verify();
        self.startup.poll(
            ctx,
            &mut self.saved_state.start_on_startup,
            &mut self.toasts,
        );
    }

    fn poll_tracker_upload(&mut self) {
        let result = match poll_oneshot(&mut self.tracker_upload_rx) {
            Polled::Pending => return,
            Polled::Ready(result) => result,
            Polled::Dropped => {
                Err("the upload request was dropped before it completed".to_string())
            }
        };

        match result {
            Ok(()) => {
                let host = self.tracker_host();
                self.toasts.success(format!("Uploaded to {host}"));
            }
            Err(e) => {
                // A refused key is re-verified; a refused sign-in is the
                // sign-in's own business (it refreshes, or signs out).
                if self.saved_state.tracker_sign_in != TrackerSignIn::SignedIn
                    && matches!(tracker_error_status(&e), Some(401 | 403))
                {
                    self.saved_state.tracker_verified = false;
                    self.tracker_account = None;
                    self.request_tracker_verify();
                }
                let host = self.tracker_host();
                self.toasts
                    .error(format!("{host} upload failed · {e}"))
                    .action(crate::ui::toast::Action::RetryUpload);
            }
        }
    }

    fn poll_tracker_verify(&mut self) {
        let result = match poll_oneshot(&mut self.tracker_verify_rx) {
            Polled::Pending => return,
            Polled::Ready(result) => result,
            // Without this the panel stayed on "Verifying…" for the rest of the
            // session, with the cloud-upload button greyed out and auto-export
            // silently inert, and nothing told the user to hit refresh.
            Polled::Dropped => Err(anyhow!("the verification request was dropped")),
        };

        self.apply_tracker_verify_result(result);
    }

    /// Hide to the tray, or fall back to the taskbar when there is no tray menu
    /// to come back through.
    ///
    /// On Linux `TrayIconEvent` is never delivered, so a tray without a working
    /// menu is a one-way trip: the window disappears with no restore and no
    /// quit, and "Remember my choice" puts the next launch one click from the
    /// same trap.
    /// Schedule the start-up and game-session memory releases (see
    /// [`release_memory_after`]); cheap to call every frame.
    #[cfg(windows)]
    fn release_settled_memory(&mut self) {
        let now = self.state_rx.borrow().game_status;
        let (startup, session) = memory_release_due(
            self.startup_memory_released,
            self.memory_release_status,
            now,
        );
        if startup {
            self.startup_memory_released = true;
            release_memory_after(RELEASE_STARTUP_MEMORY_DELAY, "start-up");
        }
        if session {
            release_memory_after(RELEASE_SESSION_MEMORY_DELAY, "the game session's login");
        }
        self.memory_release_status = now;
    }

    fn minimize(&self, ctx: &Context, to_tray: bool) {
        #[cfg(windows)]
        release_memory_once_hidden();

        if to_tray && self.tray_menu_ready.load(Ordering::Relaxed) {
            HIDDEN_TO_TRAY.store(true, Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        if to_tray {
            tracing::warn!("no usable tray icon; minimizing to the taskbar instead");
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }

    fn genshin_optimizer_request_export(&mut self, target: OptimizerExportTarget) {
        let (tx, rx) = oneshot::channel();
        let _ = self.ui_message_tx.send(Message::ExportGenshinOptimizer(
            target.export_settings(&self.saved_state.export_settings),
            tx,
        ));
        self.optimizer_export_target = target;
        self.optimizer_export_rx = Some(rx);
    }

    fn apply_tracker_verify_result(&mut self, result: Result<TrackerAccount>) {
        match result {
            Ok(info) => {
                self.tracker_account = Some(info);
                self.saved_state.tracker_verified = true;
            }
            Err(e) => {
                self.tracker_account = None;
                self.saved_state.tracker_verified = false;
                tracing::warn!("could not verify the import key: {e}");
                self.toasts.error("Couldn't verify the import key");
            }
        }
    }

    /// Warn, once per occurrence, that the captured account is not the one
    /// uploads go to.
    ///
    /// Only a warning: an upload still goes to that account, as it always
    /// has, and the UID scheme this relies on is not yet proven on every
    /// capture. Raised as soon as both UIDs are known, which is ahead of the
    /// automation export's debounce; the tracker section keeps saying it for as
    /// long as it is true.
    fn note_uid_mismatch(&mut self, captured_uid: Option<u32>) {
        let linked = self.upload_account();
        let mismatch = linked.as_ref().and_then(|account| {
            uid_mismatch(account, captured_uid).map(|captured| (account.clone(), captured))
        });
        let Some((account, captured)) = mismatch else {
            // Re-armed, so a later recurrence speaks up again.
            self.uid_mismatch_warned = None;
            // Say so when the two agree too: the UID is derived from guids by
            // a scheme that is still being verified, and a match is the proof.
            if let (Some(account), Some(captured)) = (&linked, captured_uid) {
                let pair = (account.uid.trim().to_string(), captured);
                if self.uid_match_logged.as_ref() != Some(&pair) {
                    tracing::info!(
                        captured,
                        linked = %pair.0,
                        "the captured UID matches the account uploads go to"
                    );
                    self.uid_match_logged = Some(pair);
                }
            }
            return;
        };

        let occurrence = (account.uid.clone(), captured);
        if self.uid_mismatch_warned.as_ref() == Some(&occurrence) {
            return;
        }
        tracing::warn!(
            captured,
            linked = %account.uid,
            "the captured account is not the one uploads go to"
        );
        self.toasts
            .warning(uid_mismatch_message(&account, captured))
            .duration(Some(Duration::from_secs(15)));
        self.uid_mismatch_warned = Some(occurrence);
    }

    fn request_tracker_verify(&mut self) {
        let key = self.saved_state.tracker_import_key.clone();
        if key.is_empty() {
            self.tracker_account = None;
            self.saved_state.tracker_verified = false;
            // Drop any in-flight request too, so its (now meaningless) answer
            // cannot land later and re-toast a failure for a key that is gone.
            self.tracker_verify_rx = None;
            return;
        }
        let url = format!(
            "{}/genshin-accounts-public/verify-key",
            self.saved_state.tracker_api_url.trim_end_matches('/')
        );
        let (tx, rx) = oneshot::channel();
        let _ = self
            .ui_message_tx
            .send(Message::VerifyTrackerKey(url, key, tx));
        self.tracker_verify_rx = Some(rx);
        self.tracker_account = None;
        self.saved_state.tracker_verified = false;
    }

    fn send_tracker(&self, request: TrackerRequest) {
        let _ = self.ui_message_tx.send(Message::Tracker(request));
    }

    /// Follow the sign-in: open the browser when it asks, mirror whether it
    /// is signed in into the saved state (which the upload gates read), and
    /// re-read whoami when the user comes back to the window and every few
    /// minutes, so a target changed on the tracker's website shows up.
    fn poll_tracker_sign_in(&mut self, ctx: &Context) {
        // The same path every other link takes (`open_url`), from the UI
        // thread. See `TrackerAuth::sign_in` on elevated browsers.
        while let Ok(url) = self.browser_rx.try_recv() {
            ctx.open_url(OpenUrl::new_tab(url));
        }

        let status = self.auth_status_rx.borrow().clone();
        let sign_in = TrackerSignIn::from_status(&status);
        if sign_in == TrackerSignIn::SignedIn && self.saved_state.tracker_sign_in != sign_in {
            // Signing in (or loading a saved sign-in) reads whoami itself.
            self.whoami_checked_at = Some(Instant::now());
        }
        self.saved_state.tracker_sign_in = sign_in;

        if let AuthStatus::SignedOut {
            notice: Some(notice),
        } = &status
        {
            if self.auth_notice_toasted.as_ref() != Some(notice) {
                self.toasts.error(notice.clone());
                self.auth_notice_toasted = Some(notice.clone());
            }
        } else {
            self.auth_notice_toasted = None;
        }

        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        let regained_focus = focused && !self.window_focused;
        self.window_focused = focused;
        if sign_in != TrackerSignIn::SignedIn {
            return;
        }
        let since = self.whoami_checked_at.map(|at| at.elapsed());
        if whoami_refresh_due(regained_focus, since) {
            self.whoami_checked_at = Some(Instant::now());
            self.send_tracker(TrackerRequest::Refresh);
            ctx.request_repaint_after(WHOAMI_REFRESH_INTERVAL);
        } else if let Some(since) = since {
            ctx.request_repaint_after(WHOAMI_REFRESH_INTERVAL.saturating_sub(since));
        }
    }

    /// The account uploads go to, when that is one known account: the
    /// sign-in's target in account mode, or the verified import key's while
    /// signed out. `None` in UID mode -- the tracker files each capture under
    /// its own UID, so there is nothing to mismatch -- and when nothing is
    /// linked.
    fn upload_account(&self) -> Option<TrackerAccount> {
        match self.saved_state.tracker_sign_in {
            TrackerSignIn::SignedIn => match &*self.auth_status_rx.borrow() {
                AuthStatus::SignedIn {
                    whoami: Some(whoami),
                    ..
                } => whoami_account(whoami),
                _ => None,
            },
            TrackerSignIn::SignedOut => self.tracker_account.clone(),
            TrackerSignIn::Unknown => None,
        }
    }

    /// Point uploads at another tracker, once the user confirmed it.
    fn apply_tracker_address(&mut self, base: String) {
        tracing::info!("Tracker API URL changed to {base}");
        self.saved_state.tracker_api_url = base.clone();
        self.tracker_url_edit = base.clone();
        // Nothing uploads until the sign-in for the new address is read.
        self.saved_state.tracker_sign_in = TrackerSignIn::Unknown;
        self.send_tracker(TrackerRequest::SetAddress(base));
        self.request_tracker_verify();
    }

    /// Collect the result of an `ExportGenshinOptimizer` request.
    ///
    /// Polled rather than awaited: `blocking_recv()` here stalled the egui
    /// update loop for the whole export.
    fn optimizer_handle_export(&mut self, ctx: &Context) -> Result<()> {
        let json = match poll_oneshot(&mut self.optimizer_export_rx) {
            Polled::Pending => return Ok(()),
            Polled::Ready(result) => result,
            Polled::Dropped => {
                self.optimizer_export_target = OptimizerExportTarget::None;
                return Err(anyhow!(
                    "the export request was dropped before it completed"
                ));
            }
        };
        let (json, counts) = match json {
            Ok(export) => export,
            Err(e) => {
                self.optimizer_export_target = OptimizerExportTarget::None;
                return Err(e);
            }
        };

        match self.optimizer_export_target {
            OptimizerExportTarget::None => {
                tracing::warn!("Unexpected json export");
            }
            OptimizerExportTarget::Clipboard => {
                self.optimizer_save_to_clipboard(ctx, json)?;
            }
            OptimizerExportTarget::File => {
                self.optimizer_save_to_file(json)?;
            }

            OptimizerExportTarget::TrackerManual => {
                if let Some(credential) = upload_credential(&self.saved_state) {
                    // Said again at the click, then uploaded anyway: see
                    // `note_uid_mismatch`.
                    let captured_uid = self
                        .state_rx
                        .borrow()
                        .player
                        .as_ref()
                        .and_then(|player| player.uid);
                    if let Some(account) = self.upload_account()
                        && let Some(captured) = uid_mismatch(&account, captured_uid)
                    {
                        self.toasts
                            .warning(uid_mismatch_message(&account, captured))
                            .duration(Some(Duration::from_secs(15)));
                    }
                    self.tracker_upload_json(json, counts, credential);
                } else if self.saved_state.tracker_sign_in == TrackerSignIn::Unknown {
                    self.toasts
                        .error("Still reading the tracker sign-in. Try again in a moment.");
                } else if !self.saved_state.tracker_import_key.is_empty() {
                    self.toasts
                        .error("Tracker key not verified. Open settings to re-link.");
                } else {
                    self.toasts.error("Not signed in to the tracker.");
                }
            }
        }

        self.optimizer_export_target = OptimizerExportTarget::None;
        Ok(())
    }

    fn optimizer_save_to_clipboard(&mut self, ctx: &Context, json: String) -> Result<()> {
        ctx.copy_text(json);
        self.toasts.info("GOOD JSON copied");
        Ok(())
    }

    fn optimizer_save_to_file(&mut self, json: String) -> Result<()> {
        let path = self
            .optimizer_save_path
            .take()
            .ok_or_else(|| anyhow!("No save file path set"))?;

        let file = File::create(&path).with_context(|| format!("Unable to open file {path:?}"))?;
        let mut writer = BufWriter::new(file);
        writer.write_all(json.as_bytes())?;

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.toasts
            .success(format!("Saved {name}"))
            .saved()
            .action(crate::ui::toast::Action::Show(path.clone()));
        Ok(())
    }

    fn tracker_upload_json(
        &mut self,
        json: String,
        counts: ExportCounts,
        credential: UploadCredential,
    ) {
        let host = self.tracker_host();
        self.toasts.info(format!("Uploading to {host} · {counts}"));

        let (tx, rx) = oneshot::channel();
        let _ = self
            .ui_message_tx
            .send(Message::UploadToTracker(json, credential, tx));
        self.tracker_upload_rx = Some(rx);
    }

    /// Collect the result of an `ExportAchievements` request.
    ///
    /// Polled rather than awaited, for the same reason as the other two.
    fn achievements_handle_export(&mut self, ctx: &Context) -> Result<()> {
        let achievements = match poll_oneshot(&mut self.achievements_export_rx) {
            Polled::Pending => return Ok(()),
            Polled::Ready(Ok(achievements)) => Some(achievements),
            Polled::Ready(Err(_)) | Polled::Dropped => None,
        };
        let open = self.pending_open_url.take();
        match achievements {
            Some(achievements) => {
                let json = serde_json::json!({ "gi_achievements": achievements }).to_string();
                ctx.copy_text(json);
                self.toasts
                    .info(format!("{} achievements copied", achievements.len()));
                if let Some(url) = open {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
            None => {
                let toast = self.toasts.error("Achievements not found");
                if let Some(url) = open {
                    toast.action(crate::ui::toast::Action::OpenAnyway(url));
                }
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn show_window() {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows::core::PCWSTR;

    let title: Vec<u16> = std::ffi::OsStr::new("Teyvat.me")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let hwnd = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr()));
        if let Ok(hwnd) = hwnd
            && !hwnd.0.is_null()
        {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(windows)]
fn close_window() {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    use windows::core::PCWSTR;

    let title: Vec<u16> = std::ffi::OsStr::new("Teyvat.me")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let hwnd = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr()));
        if let Ok(hwnd) = hwnd
            && !hwnd.0.is_null()
        {
            let _ = PostMessageW(
                Some(hwnd),
                WM_CLOSE,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

/// Whether this process can capture: elevated on Windows (the manifest asks
/// for it, so only a bypass such as `__COMPAT_LAYER=RunAsInvoker` lands here
/// without it); elsewhere the permission check runs before the window.
fn is_admin() -> bool {
    #[cfg(windows)]
    {
        unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().into() }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `monitor::CAPTURE_SHUTDOWN_TIMEOUT` is private to its module, so the
    /// mirror above is checked against the source rather than the item. Two
    /// agents picked 5 s independently and left the outer budget with exactly
    /// zero margin over the inner one.
    #[test]
    fn the_monitor_budget_outlasts_the_capture_teardown_it_waits_for() {
        const MONITOR_SOURCE: &str = include_str!("monitor.rs");
        const DECLARATION: &str = "const CAPTURE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(";

        let secs: u64 = MONITOR_SOURCE
            .split(DECLARATION)
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .unwrap_or_else(|| {
                panic!("could not find {DECLARATION} in monitor.rs; keep this test in sync")
            })
            .trim()
            .parse()
            .expect("the capture shutdown timeout is a whole number of seconds");

        assert_eq!(
            CAPTURE_SHUTDOWN_TIMEOUT.as_secs(),
            secs,
            "app.rs mirrors monitor.rs's capture shutdown timeout; they have drifted"
        );
        assert!(
            MONITOR_SHUTDOWN_TIMEOUT > CAPTURE_SHUTDOWN_TIMEOUT,
            "the monitor is joined with a deadline while it may still be spending the \
             capture teardown budget, so the outer one has to be strictly larger"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_working_set_can_be_trimmed() {
        // Harmless to the test process: the pages fault back in when touched.
        trim_working_set().unwrap();
    }

    #[test]
    fn settled_memory_is_released_after_start_up_and_each_session_start() {
        use GameStatus::{Decoding, LaunchCaptured, NotRunning};
        // The first frame schedules the start-up release, once.
        assert_eq!(
            memory_release_due(false, NotRunning, NotRunning),
            (true, false)
        );
        assert_eq!(
            memory_release_due(true, NotRunning, NotRunning),
            (false, false)
        );
        // A session that starts decoding schedules one release...
        assert_eq!(
            memory_release_due(true, LaunchCaptured, Decoding),
            (false, true)
        );
        // ...not one per frame while it keeps decoding...
        assert_eq!(memory_release_due(true, Decoding, Decoding), (false, false));
        // ...and again for the next session.
        assert_eq!(
            memory_release_due(true, NotRunning, Decoding),
            (false, true)
        );
    }

    /// The test process has no window, so whatever "Teyvat.me" windows exist on
    /// the machine (a running copy, an Explorer folder) are someone else's.
    #[cfg(windows)]
    #[test]
    fn another_process_window_never_counts_as_ours() {
        assert!(!window_out_of_sight());
    }

    fn verified_state() -> SavedAppState {
        SavedAppState {
            tracker_import_key: "gdt_import_1_abc".to_string(),
            tracker_verified: true,
            // Signed out: the key path. While the sign-in is still being read
            // (`Unknown`) nothing uploads at all.
            tracker_sign_in: TrackerSignIn::SignedOut,
            ..Default::default()
        }
    }

    #[test]
    fn the_manual_upload_does_not_need_the_automation_checkbox() {
        // The regression: with a verified key and "Auto export to tracker"
        // unchecked -- the default -- the enabled cloud-upload button ran a
        // full export and then toasted "Tracker key not verified".
        let state = verified_state();
        assert!(!state.auto_export_to_tracker);
        assert!(can_upload_to_tracker(&state));
        assert!(!want_tracker_upload(&state));
    }

    #[test]
    fn neither_upload_path_runs_without_a_verified_key() {
        let mut state = verified_state();
        state.auto_export_to_tracker = true;
        assert!(want_tracker_upload(&state));

        // A key the dashboard has revoked fails verification, and the
        // automation path must stop posting per login just like the button.
        state.tracker_verified = false;
        assert!(!can_upload_to_tracker(&state));
        assert!(!want_tracker_upload(&state));

        let mut state = verified_state();
        state.auto_export_to_tracker = true;
        state.tracker_import_key.clear();
        assert!(!can_upload_to_tracker(&state));
        assert!(!want_tracker_upload(&state));
    }

    #[test]
    fn an_export_needs_only_the_data_classes_it_writes() {
        let settings = SavedAppState::default().export_settings;
        let mut updated = DataUpdated::new();

        // Nothing captured: both classes are named, so the toast can say which.
        let missing = missing_export_data(&settings, &updated);
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing_export_data_toast(&missing).contains("character data"));

        updated.characters_updated = Some(Instant::now());
        updated.items_updated = Some(Instant::now());

        // Achievements are still missing, and used to block this. They are
        // attached best-effort and can never be asked for or turned off, so
        // they must not gate an export of the data that *is* there.
        assert!(updated.achievements_updated.is_none());
        assert!(missing_export_data(&settings, &updated).is_empty());
    }

    #[test]
    fn export_classes_carry_the_capture_time_the_automation_gate_compares() {
        // monitor.rs's automation trigger requires every class it will write to
        // have been captured *since the current capture cycle began*, so the
        // timestamps have to travel with the names.
        let settings = SavedAppState::default().export_settings;
        let cycle_started = Instant::now();

        let mut updated = DataUpdated::new();
        updated.characters_updated = Some(cycle_started);
        updated.items_updated = Some(cycle_started);

        let classes = export_data_classes(&settings, &updated);
        assert_eq!(classes.len(), 2);
        assert!(
            !classes
                .iter()
                .all(|class| class.captured_at.is_some_and(|at| at > cycle_started)),
            "data from before this cycle is the previous session's"
        );

        let fresh = Instant::now();
        updated.characters_updated = Some(fresh);
        updated.items_updated = Some(fresh);
        assert!(
            export_data_classes(&settings, &updated)
                .iter()
                .all(|class| class.captured_at.is_some_and(|at| at > cycle_started))
        );
    }

    #[test]
    fn an_export_of_characters_alone_does_not_wait_for_the_inventory() {
        let settings = ExportSettings {
            include_characters: true,
            include_artifacts: false,
            include_weapons: false,
            include_materials: false,
            ..SavedAppState::default().export_settings
        };
        let mut updated = DataUpdated::new();

        assert_eq!(
            missing_export_data(&settings, &updated),
            vec!["character data"]
        );

        updated.characters_updated = Some(Instant::now());
        assert!(missing_export_data(&settings, &updated).is_empty());
    }

    #[test]
    fn an_inventory_export_names_the_inventory_and_not_characters() {
        let settings = ExportSettings {
            include_characters: false,
            include_artifacts: true,
            ..SavedAppState::default().export_settings
        };
        let updated = DataUpdated::new();

        let missing = missing_export_data(&settings, &updated);
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert!(missing[0].contains("inventory"), "{missing:?}");
        assert!(!missing_export_data_toast(&missing).contains("character"));
    }

    /// Genshin Optimizer settings that leave out all they can: no characters,
    /// 5★ gear only, and the faked 4th line.
    fn narrow_optimizer_settings() -> ExportSettings {
        ExportSettings {
            include_characters: false,
            fake_initialize_4th_line: true,
            min_artifact_level: 20,
            min_artifact_rarity: 5,
            min_weapon_level: 90,
            min_weapon_rarity: 5,
            ..ExportSettings::default()
        }
    }

    #[test]
    fn a_manual_tracker_upload_exports_with_the_tracker_settings() {
        let optimizer = narrow_optimizer_settings();

        assert_eq!(
            OptimizerExportTarget::TrackerManual.export_settings(&optimizer),
            ExportSettings::for_tracker()
        );
        // The Genshin Optimizer exports keep the user's settings.
        for target in [
            OptimizerExportTarget::File,
            OptimizerExportTarget::Clipboard,
        ] {
            assert_eq!(target.export_settings(&optimizer), optimizer);
        }
    }

    #[test]
    fn the_upload_button_waits_for_what_the_upload_writes() {
        // Characters are off for Genshin Optimizer, but the upload holds them,
        // so the button waits for them too.
        let optimizer = narrow_optimizer_settings();
        let mut updated = DataUpdated::new();
        updated.items_updated = Some(Instant::now());

        assert!(missing_export_data(&optimizer, &updated).is_empty());
        assert_eq!(
            missing_export_data(
                &OptimizerExportTarget::TrackerManual.export_settings(&optimizer),
                &updated
            ),
            vec!["character data"]
        );
    }

    /// The call sites, held to the two mappings the tests above pin: the UI's
    /// one export request passes its target's settings, and the automation in
    /// `monitor.rs` plans its trigger and its export with `AutomationPlan`
    /// and never exports with the user's settings directly.
    #[test]
    fn both_tracker_upload_paths_take_their_settings_from_the_split() {
        fn code(source: &'static str) -> &'static str {
            source
                .split("#[cfg(test)]")
                .next()
                .expect("the code comes first")
        }
        let app = code(include_str!("app.rs"));
        let monitor = code(include_str!("monitor.rs"));

        let request = app
            .split("fn genshin_optimizer_request_export")
            .nth(1)
            .and_then(|rest| rest.split("\n    }\n").next())
            .expect("app.rs has genshin_optimizer_request_export");
        assert!(
            request.contains("target.export_settings(&self.saved_state.export_settings)"),
            "{request}"
        );
        assert_eq!(app.matches("Message::ExportGenshinOptimizer(").count(), 1);

        assert_eq!(
            monitor
                .matches("AutomationPlan::for_state(&saved_state)")
                .count(),
            2,
            "the automation trigger and the automation export"
        );
        assert!(!monitor.contains("_with_report(&saved_state.export_settings"));
    }

    #[test]
    fn a_set_but_empty_tracker_url_falls_back_to_the_default() {
        // `option_env!` cannot tell "unset" from "set to nothing", and a build
        // with `TRACKER_API_URL=` used to bake in the empty string, which turns
        // every request into a relative URL reqwest refuses to send.
        assert_eq!(resolve_tracker_url(None), FALLBACK_TRACKER_URL);
        assert_eq!(resolve_tracker_url(Some("")), FALLBACK_TRACKER_URL);
        assert_eq!(resolve_tracker_url(Some("   ")), FALLBACK_TRACKER_URL);
        assert_eq!(resolve_tracker_url(Some("\n")), FALLBACK_TRACKER_URL);
    }

    #[test]
    fn the_default_tracker_is_the_hosted_api() {
        // The tracker Worker mounts its API under `/api`
        // (genshin-data-tracker/apps/tracker/worker/index.ts `basePath`).
        assert_eq!(FALLBACK_TRACKER_URL, "https://teyvat.me/api");
        assert_eq!(resolve_tracker_url(None), FALLBACK_TRACKER_URL);
    }

    #[test]
    fn a_saved_old_hosted_address_moves_to_teyvat_me() {
        for saved in [
            "https://genshin-tracker.475.dev/api",
            "https://genshin-tracker.475.dev/api/",
            " https://genshin-tracker.475.dev/api\n",
        ] {
            assert_eq!(
                migrated_tracker_url(saved, FALLBACK_TRACKER_URL).as_deref(),
                Some(FALLBACK_TRACKER_URL),
                "{saved:?}"
            );
        }
        // Other paths on that host were typed by hand.
        assert_eq!(
            migrated_tracker_url("https://genshin-tracker.475.dev", FALLBACK_TRACKER_URL),
            None
        );
        // A build that bakes the old address in keeps it.
        assert_eq!(
            migrated_tracker_url(OLD_HOSTED_TRACKER_URL, OLD_HOSTED_TRACKER_URL),
            None
        );
        assert_eq!(
            migrated_tracker_url(
                "https://genshin-tracker.475.dev/api/",
                OLD_HOSTED_TRACKER_URL
            ),
            None
        );
    }

    #[test]
    fn a_saved_legacy_default_moves_to_the_current_default() {
        for saved in [
            "http://localhost:49000",
            "http://localhost:49000/",
            "http://localhost:49000//",
            " http://localhost:49000/\n",
        ] {
            assert_eq!(
                migrated_tracker_url(saved, FALLBACK_TRACKER_URL).as_deref(),
                Some(FALLBACK_TRACKER_URL),
                "{saved:?}"
            );
        }
    }

    #[test]
    fn an_empty_saved_tracker_url_is_repaired() {
        for saved in ["", "   ", "\n"] {
            assert_eq!(
                migrated_tracker_url(saved, FALLBACK_TRACKER_URL).as_deref(),
                Some(FALLBACK_TRACKER_URL),
                "{saved:?}"
            );
        }
    }

    #[test]
    fn a_customised_tracker_url_is_never_rewritten() {
        for saved in [
            FALLBACK_TRACKER_URL,
            "https://gdt.example/api",
            "http://127.0.0.1:49000",
            "http://localhost:49001",
            "http://localhost:49000/api",
            "https://localhost:49000",
        ] {
            assert_eq!(
                migrated_tracker_url(saved, FALLBACK_TRACKER_URL),
                None,
                "{saved:?}"
            );
        }
    }

    #[test]
    fn a_build_that_bakes_in_the_legacy_url_keeps_it() {
        // `TRACKER_API_URL=http://localhost:49000` makes the legacy URL the
        // build's own default; there is nothing to move it to.
        assert_eq!(
            migrated_tracker_url("http://localhost:49000", LEGACY_TRACKER_URL),
            None
        );
        assert_eq!(
            migrated_tracker_url("http://localhost:49000/", LEGACY_TRACKER_URL),
            None
        );
        // An empty value is still repaired.
        assert_eq!(
            migrated_tracker_url("", LEGACY_TRACKER_URL).as_deref(),
            Some(LEGACY_TRACKER_URL)
        );
    }

    #[test]
    fn loading_state_saved_by_an_old_build_migrates_its_tracker_url() {
        // Round-trip through serde so the test sees state the way `new` does.
        // Compared against `default_tracker_url()`, not the constant, because
        // CI may bake in `TRACKER_API_URL`.
        let mut old = serde_json::to_value(SavedAppState::default()).unwrap();
        old["tracker_api_url"] = "http://localhost:49000".into();
        let mut state: SavedAppState = serde_json::from_value(old).unwrap();
        state.migrate();
        assert_eq!(state.tracker_api_url, default_tracker_url());

        let mut custom = SavedAppState {
            tracker_api_url: "https://gdt.example/api".to_string(),
            ..Default::default()
        };
        custom.migrate();
        assert_eq!(custom.tracker_api_url, "https://gdt.example/api");
    }

    #[test]
    fn a_configured_tracker_url_is_used_verbatim_after_trimming() {
        assert_eq!(
            resolve_tracker_url(Some("https://gdt.example/api")),
            "https://gdt.example/api"
        );
        // CI writes the value through a shell, so a stray newline is plausible.
        assert_eq!(
            resolve_tracker_url(Some("  https://gdt.example/api\n")),
            "https://gdt.example/api"
        );
    }

    #[test]
    fn tracker_errors_report_the_status_and_nothing_else() {
        // monitor.rs formats these as `HTTP {status} - {body}`.
        assert_eq!(
            tracker_error_status("HTTP 401 Unauthorized - {\"message\":\"bad key\"}"),
            Some(401)
        );
        assert_eq!(tracker_error_status("HTTP 403 Forbidden - nope"), Some(403));
        assert_eq!(
            tracker_error_status("HTTP 500 Internal Server Error - oops"),
            Some(500)
        );
    }

    #[test]
    fn digits_in_a_response_body_are_not_mistaken_for_a_status() {
        // The old check searched the whole string, so any of these dropped the
        // verified state and fired a spurious re-verification.
        for body in [
            "HTTP 500 Internal Server Error - upstream returned 401 for account 403",
            "HTTP 502 Bad Gateway - <html><title>403 error page</title></html>",
            "HTTP 413 Payload Too Large - 4030112 bytes",
        ] {
            assert!(
                !matches!(tracker_error_status(body), Some(401 | 403)),
                "{body} must not be read as an auth failure"
            );
        }

        // Transport failures are `reqwest::Error` strings with no status at
        // all, and must not be guessed at either.
        assert_eq!(
            tracker_error_status("error sending request for url (http://host:40100/x)"),
            None
        );
        assert_eq!(tracker_error_status("HTTP - no status"), None);
    }

    fn linked_account(uid: &str) -> TrackerAccount {
        TrackerAccount {
            name: "Main".to_string(),
            uid: uid.to_string(),
            server: "ASIA".to_string(),
            dashboard_url: None,
        }
    }

    #[test]
    fn a_capture_of_another_account_is_a_uid_mismatch() {
        let account = linked_account("811111111");
        assert_eq!(uid_mismatch(&account, Some(800_000_001)), Some(800_000_001));
        assert_eq!(uid_mismatch(&account, Some(811_111_111)), None);

        let message = uid_mismatch_message(&account, 800_000_001);
        assert!(message.contains("UID 800000001"), "{message}");
        assert!(message.contains("Main (UID 811111111)"), "{message}");
    }

    #[test]
    fn an_unknown_uid_on_either_side_is_no_mismatch() {
        // Nothing captured yet, or the guids disagreed.
        assert_eq!(uid_mismatch(&linked_account("811111111"), None), None);
        // The old backend's answer without a UID, or an account without one.
        assert_eq!(
            uid_mismatch(&linked_account("N/A"), Some(800_000_001)),
            None
        );
        assert_eq!(uid_mismatch(&linked_account(""), Some(800_000_001)), None);
        // A numeric answer read back as a string, with stray whitespace.
        assert_eq!(
            uid_mismatch(&linked_account(" 800000001 "), Some(800_000_001)),
            None
        );
    }

    #[test]
    fn a_sign_in_takes_over_from_the_import_key() {
        // Signed in: the sign-in uploads, whether or not a key is saved.
        let mut state = verified_state();
        state.tracker_sign_in = TrackerSignIn::SignedIn;
        assert_eq!(
            upload_credential(&state),
            Some(UploadCredential::SignedIn {
                base: state.tracker_api_url.clone()
            })
        );
        state.tracker_import_key.clear();
        state.tracker_verified = false;
        assert_eq!(
            upload_credential(&state),
            Some(UploadCredential::SignedIn {
                base: state.tracker_api_url.clone()
            })
        );
        state.auto_export_to_tracker = true;
        assert!(want_tracker_upload(&state));
    }

    #[test]
    fn signed_out_the_key_path_is_what_it_always_was() {
        let mut state = verified_state();
        state.tracker_api_url = "https://gdt.example/api/".to_string();
        assert_eq!(
            upload_credential(&state),
            Some(UploadCredential::ImportKey {
                url: "https://gdt.example/api/genshin-accounts-public/import-by-key".to_string(),
                key: "gdt_import_1_abc".to_string(),
            })
        );
    }

    #[test]
    fn nothing_uploads_while_the_sign_in_is_still_being_read() {
        let mut state = verified_state();
        state.tracker_sign_in = TrackerSignIn::Unknown;
        state.auto_export_to_tracker = true;
        assert_eq!(upload_credential(&state), None);
        assert!(!can_upload_to_tracker(&state));
        assert!(!want_tracker_upload(&state));
    }

    #[test]
    fn the_sign_in_status_maps_onto_the_upload_gate() {
        assert_eq!(
            TrackerSignIn::from_status(&AuthStatus::Loading),
            TrackerSignIn::Unknown
        );
        assert_eq!(
            TrackerSignIn::from_status(&AuthStatus::SignedOut { notice: None }),
            TrackerSignIn::SignedOut
        );
        // Waiting on the browser is still signed out: a saved key keeps
        // working until the sign-in completes.
        assert_eq!(
            TrackerSignIn::from_status(&AuthStatus::Waiting { link: None }),
            TrackerSignIn::SignedOut
        );
        assert_eq!(
            TrackerSignIn::from_status(&AuthStatus::SignedIn {
                whoami: None,
                error: None
            }),
            TrackerSignIn::SignedIn
        );
    }

    #[test]
    fn an_edited_address_is_a_change_only_when_it_points_elsewhere() {
        let current = "https://teyvat.me/api";
        assert_eq!(address_change(current, current), AddressChange::Same);
        assert_eq!(
            address_change(current, " https://teyvat.me/api/ "),
            AddressChange::Same
        );
        assert_eq!(
            address_change(current, "https://gdt.example/api"),
            AddressChange::To {
                base: "https://gdt.example/api".to_string(),
                host: "gdt.example".to_string(),
            }
        );
        // A lookalike is named by its host in the confirmation.
        assert_eq!(
            address_change(current, "https://teyvat.me.evil.example/api"),
            AddressChange::To {
                base: "https://teyvat.me.evil.example/api".to_string(),
                host: "teyvat.me.evil.example".to_string(),
            }
        );
        assert_eq!(
            address_change(current, "http://localhost:8787/api"),
            AddressChange::To {
                base: "http://localhost:8787/api".to_string(),
                host: "localhost:8787".to_string(),
            }
        );
        for invalid in [
            "",
            "tracker",
            "ftp://gdt.example/api",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                address_change(current, invalid),
                AddressChange::Invalid,
                "{invalid}"
            );
        }
    }

    #[test]
    fn whoami_is_reread_on_return_and_every_few_minutes() {
        assert!(whoami_refresh_due(false, None));
        assert!(!whoami_refresh_due(false, Some(Duration::from_secs(30))));
        assert!(whoami_refresh_due(true, Some(Duration::from_secs(30))));
        // Not on every focus flicker.
        assert!(!whoami_refresh_due(true, Some(Duration::from_secs(2))));
        assert!(whoami_refresh_due(false, Some(WHOAMI_REFRESH_INTERVAL)));
    }

    fn whoami(scope: UploadScope) -> Whoami {
        Whoami {
            scope,
            user: "traveler".to_string(),
            account_id: Some(12),
            account_name: Some("Main".to_string()),
            uid: Some("811111111".to_string()),
            server: Some("ASIA".to_string()),
            dashboard_url: None,
            config_url: None,
        }
    }

    #[test]
    fn only_an_account_target_can_mismatch_the_captured_uid() {
        let account = whoami_account(&whoami(UploadScope::Account)).unwrap();
        assert_eq!(uid_mismatch(&account, Some(800_000_001)), Some(800_000_001));
        assert_eq!(uid_mismatch(&account, Some(811_111_111)), None);
        // By UID, every capture goes to its own account.
        assert_eq!(whoami_account(&whoami(UploadScope::Uid)), None);
    }
}
