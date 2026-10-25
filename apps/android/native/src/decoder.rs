// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The phone's decoder: the game packets the forwarder tees off go through
//! the same decode layer the desktop monitor and `--replay-export` run
//! (`teyvatme_core::decode`), into one [`PlayerData`] the app exports.
//!
//! It runs on its own thread for the life of the process, not of one VPN
//! session, so what was captured stays exportable after capture stops. The
//! app reads it through [`Decoder::summary`] (once a second, for the home
//! screen and the notification) and [`Decoder::export`].

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use flate2::read::GzDecoder;
use serde::Serialize;
use teyvatme_core::decode::{
    self, AppliedData, DataReplacement, DataVerdict, KeyReport, SnifferEvent, SnifferStats,
};
use teyvatme_core::player_data::{ExportSettings, PlayerData};
use teyvatme_core::recording;
use teyvatme_core::wish::hub::{WishHub, take_wish_events};
use teyvatme_game_data::AnimeGameData;
use teyvatme_protocol::{GamePacket, GameSniffer, KeyState};

use crate::wish::{WishSummary, Wishes};

/// `LINKTYPE_RAW`, what the tee carries: bare IP packets.
const LINKTYPE_RAW: u16 = 101;
/// Game packets still arriving this long after the last one the sniffer
/// could place mean it cannot follow the connection: its login was missed.
const STUCK_AFTER_MS: u64 = 5_000;
/// No game connection packets (UDP 22101–22102) for this long: the game is
/// closed, and the app waits for it again. In the world the connection is
/// never this quiet; a short trip to the background keeps the state.
const CONNECTION_QUIET_MS: u64 = 120_000;
/// The activity list keeps this many events.
const EVENTS: usize = 50;
/// The tiles' counts are rebuilt at most this often while data pours in.
const COUNTS_EVERY_MS: u64 = 1_000;

/// One line of the activity list.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    /// Unix milliseconds.
    pub at: u64,
    /// `ok`, `error` or `info`.
    pub kind: &'static str,
    pub text: String,
}

/// What the home screen shows about the game, decided from its connection
/// (Android cannot see other apps' processes). Only the game's own
/// connection (UDP 22101–22102) counts: something under the game's UID
/// keeps opening TCP connections after the game is closed, so its HTTP
/// traffic says nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// No game connection.
    Waiting,
    /// The game connected (it does at the title screen), no login yet.
    GameDetected,
    /// The login's token exchange began (the first message under the
    /// dispatch key, `GetPlayerTokenReq`); the session key is not known yet.
    LoginSeen,
    /// The session key is known: the account's data decrypts.
    Decrypting,
    /// Game traffic of a connection whose login was not seen.
    LaunchMissed,
    /// The session key search for this login gave up.
    KeyLost,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Counts {
    pub characters: usize,
    pub weapons: usize,
    pub artifacts: usize,
    pub materials: usize,
    pub achievements: usize,
}

/// When each class of data last changed (Unix ms), as the desktop's
/// `DataUpdated` stamps them: what an export needs and the auto-export's
/// signature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Updated {
    pub items: Option<u64>,
    pub characters: Option<u64>,
    pub achievements: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub ar: Option<u32>,
    pub wl: Option<u32>,
    /// `Asia`, `America`, `Europe`, `TW, HK, MO` or `China`.
    pub server: Option<&'static str>,
}

/// What [`Decoder::summary`] answers, as JSON for Kotlin.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// `None` until the game data is loaded (or when it failed: `error`).
    pub ready: bool,
    pub error: Option<String>,
    pub phase: Phase,
    /// The capture that is running lost the game's login before it.
    pub missed_by_restart: bool,
    /// The game's connection began before this capture (its handshake was
    /// not seen): "Game detected" for a game that was already open.
    pub already_open: bool,
    /// When this login's session key was found.
    pub key_at: Option<u64>,
    /// Bumped whenever the captured data changes.
    pub data_version: u64,
    pub updated: Updated,
    /// The newest data, of any class (properties included).
    pub data_at: Option<u64>,
    /// Which login the data is from, so the auto-export writes one file per
    /// login.
    pub login: u64,
    pub counts: Counts,
    pub player: Option<Player>,
    pub game_data: Option<&'static str>,
    pub events: Vec<Event>,
    /// The wish card ([`Wishes::summary`]), filled in by the decoder.
    pub wish: Option<WishSummary>,
}

/// What a frame from the tee is.
enum Input {
    Packet(SystemTime, Vec<u8>),
    /// A VPN session began: traffic from here on is that capture's.
    CaptureStarted,
    /// Answered once everything sent before it is decoded.
    #[cfg(test)]
    Flush(mpsc::Sender<()>),
}

/// The connection the newest packets belong to.
#[derive(Debug, Default)]
struct Connection {
    /// Its login was seen (the sniffer concluded a new connection began).
    began_here: bool,
    key: Option<KeyState>,
    key_at: Option<u64>,
    recovery_failed: bool,
}

struct State {
    player_data: Option<PlayerData>,
    error: Option<String>,
    replacement: DataReplacement,
    connection: Option<Connection>,
    /// The capture began while data or a connection was already held.
    restarted_mid_session: bool,
    /// The newest game packet, and the newest one the sniffer placed.
    last_game_packet: Option<u64>,
    last_placed: Option<u64>,
    /// When the running capture's first game packet came.
    capture_first_game_packet: Option<u64>,
    capture_started: Option<u64>,
    updated: Updated,
    data_at: Option<u64>,
    data_version: u64,
    login: u64,
    counts: Counts,
    counts_version: u64,
    counts_at: u64,
    events: VecDeque<Event>,
}

impl State {
    fn new() -> Self {
        Self {
            player_data: None,
            error: None,
            replacement: DataReplacement::default(),
            connection: None,
            restarted_mid_session: false,
            last_game_packet: None,
            last_placed: None,
            capture_first_game_packet: None,
            capture_started: None,
            updated: Updated::default(),
            data_at: None,
            data_version: 0,
            login: 0,
            counts: Counts::default(),
            counts_version: 0,
            counts_at: 0,
            events: VecDeque::new(),
        }
    }

    fn event(&mut self, kind: &'static str, text: impl Into<String>) {
        let text = text.into();
        log::info!("activity: {text}");
        if self.events.len() == EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(Event {
            at: now_ms(),
            kind,
            text,
        });
    }

    /// The captured account's UID, once its data says.
    fn uid(&self) -> Option<u32> {
        self.player_data
            .as_ref()
            .and_then(|data| data.gi_player())
            .and_then(|player| player.uid)
    }

    fn phase(&self, now: u64) -> Phase {
        let connected = self
            .last_game_packet
            .is_some_and(|at| now.saturating_sub(at) < CONNECTION_QUIET_MS);
        if !connected {
            return Phase::Waiting;
        }
        let stuck = self.stuck(now);
        if let Some(connection) = &self.connection {
            if connection.key == Some(KeyState::Session) && !stuck {
                return Phase::Decrypting;
            }
            if connection.recovery_failed {
                return Phase::KeyLost;
            }
            if connection.began_here && connection.key == Some(KeyState::Dispatch) && !stuck {
                return Phase::LoginSeen;
            }
        }
        if stuck {
            Phase::LaunchMissed
        } else {
            Phase::GameDetected
        }
    }

    /// Game packets keep coming in this capture but the sniffer places none
    /// of them: the connection's login happened outside the capture.
    fn stuck(&self, now: u64) -> bool {
        let (Some(first), Some(last)) = (self.capture_first_game_packet, self.last_game_packet)
        else {
            return false;
        };
        if now.saturating_sub(last) > STUCK_AFTER_MS {
            return false;
        }
        let placed = self.last_placed.filter(|placed| *placed >= first);
        match placed {
            Some(placed) => last.saturating_sub(placed) > STUCK_AFTER_MS,
            None => last.saturating_sub(first) > STUCK_AFTER_MS,
        }
    }

    fn summary(&self) -> Summary {
        let now = now_ms();
        let player = self.player_data.as_ref().and_then(|data| data.gi_player());
        Summary {
            ready: self.player_data.is_some(),
            error: self.error.clone(),
            phase: self.phase(now),
            missed_by_restart: self.restarted_mid_session,
            already_open: !self.connection.as_ref().is_some_and(|c| c.began_here),
            key_at: self.connection.as_ref().and_then(|c| c.key_at),
            data_version: self.data_version,
            updated: self.updated,
            data_at: self.data_at,
            login: self.login,
            counts: self.counts,
            player: player.map(|player| Player {
                ar: player.ar,
                wl: player.wl,
                server: player.uid.and_then(server_of),
            }),
            game_data: embedded_game_data_sha(),
            events: self.events.iter().cloned().collect(),
            wish: None,
        }
    }

    /// Rebuild the tiles' counts when the data changed, at most once a
    /// second (`force` at the end of a burst).
    fn refresh_counts(&mut self, force: bool) {
        let now = now_ms();
        if self.counts_version == self.data_version
            || (!force && now.saturating_sub(self.counts_at) < COUNTS_EVERY_MS)
        {
            return;
        }
        let Some(data) = &self.player_data else {
            return;
        };
        let (_, report) = data.export_at(&ExportSettings::for_tracker(), now);
        let counts = report.counts();
        self.counts = Counts {
            characters: counts.characters,
            weapons: counts.weapons,
            artifacts: counts.artifacts,
            materials: counts.materials,
            achievements: counts.achievements,
        };
        self.counts_version = self.data_version;
        self.counts_at = now;
    }
}

/// The server a UID is on, by its region digit (as the desktop's
/// `region_for_uid`).
fn server_of(uid: u32) -> Option<&'static str> {
    let digits = uid.to_string();
    let first = match digits.len() {
        9 => digits.as_bytes()[0],
        10 if digits.starts_with('1') => digits.as_bytes()[1],
        _ => return None,
    };
    match (digits.len(), first) {
        (_, b'6') => Some("America"),
        (_, b'7') => Some("Europe"),
        (_, b'8') => Some("Asia"),
        (_, b'9') => Some("TW, HK, MO"),
        (9, b'1' | b'2' | b'3' | b'5') => Some("China"),
        _ => None,
    }
}

pub fn now_ms() -> u64 {
    unix_ms(SystemTime::now())
}

fn unix_ms(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// The decoder thread's handle.
pub struct Decoder {
    tx: Mutex<mpsc::Sender<Input>>,
    state: Arc<Mutex<State>>,
    /// The wish key the decoder hands on, and the history reader.
    pub wishes: Arc<Wishes>,
}

static DECODER: OnceLock<Decoder> = OnceLock::new();

/// The process's decoder, started on first use.
pub fn get() -> &'static Decoder {
    DECODER.get_or_init(Decoder::spawn)
}

impl Decoder {
    fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(State::new()));
        let thread_state = state.clone();
        let wishes = Arc::new(Wishes::new());
        let thread_wishes = Arc::clone(&wishes);
        thread::Builder::new()
            .name("teyvatme-decode".into())
            .spawn(move || run(rx, thread_state, &thread_wishes))
            .expect("the decoder thread starts");
        Self {
            tx: Mutex::new(tx),
            state,
            wishes,
        }
    }

    /// A game packet the tee copied (bare IP).
    pub fn packet(&self, at: SystemTime, data: Vec<u8>) {
        let _ = self.tx.lock().unwrap().send(Input::Packet(at, data));
    }

    pub fn capture_started(&self) {
        let _ = self.tx.lock().unwrap().send(Input::CaptureStarted);
    }

    /// An event from the app (an export, a capture start or stop).
    pub fn note(&self, kind: &'static str, text: &str) {
        self.state.lock().unwrap().event(kind, text);
    }

    pub fn summary_json(&self) -> String {
        let mut summary = self.state.lock().unwrap().summary();
        summary.wish = Some(self.wishes.summary());
        serde_json::to_string(&summary).unwrap_or_default()
    }

    /// The GOOD export with `settings` (the desktop's Genshin Optimizer
    /// settings, as JSON; empty for the defaults). `None` when nothing is
    /// captured yet.
    pub fn export(&self, settings: &str) -> Result<Option<String>> {
        let settings: ExportSettings = if settings.trim().is_empty() {
            ExportSettings::default()
        } else {
            serde_json::from_str(settings)?
        };
        let state = self.state.lock().unwrap();
        let Some(data) = state
            .player_data
            .as_ref()
            .filter(|_| state.data_at.is_some())
        else {
            return Ok(None);
        };
        let (good, report) = data.export_at(&settings, now_ms());
        if !report.is_empty() {
            log::warn!("export dropped entities: {}", report.summary());
        }
        Ok(Some(serde_json::to_string(&good)?))
    }

    /// `{"gi_achievements": [...]}` as the desktop copies it for StarDB and
    /// Seelie.me; `None` before achievements were captured.
    pub fn achievements(&self) -> Option<String> {
        let state = self.state.lock().unwrap();
        state.updated.achievements?;
        let ids = state.player_data.as_ref()?.export_achievements().ok()?;
        Some(serde_json::json!({ "gi_achievements": ids }).to_string())
    }
}

fn load_game_data() -> Result<AnimeGameData> {
    static DATABASE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/game_data.gz"));
    AnimeGameData::new_from_reader(GzDecoder::new(DATABASE))
}

fn embedded_game_data_sha() -> Option<&'static str> {
    Some(env!("TEYVATME_GAME_DATA_SHA")).filter(|sha| !sha.is_empty())
}

/// The dispatch keys the desktop app embeds (apps/desktop/keys).
fn load_keys() -> Result<HashMap<u16, Vec<u8>>> {
    let keys: HashMap<u16, String> =
        serde_json::from_slice(include_bytes!("../../../desktop/keys/gi.json"))?;
    keys.iter()
        .map(|(id, key)| Ok((*id, BASE64_STANDARD.decode(key)?)))
        .collect()
}

/// The decoder thread: the game data is loaded first (packets queue
/// meanwhile), then every teed packet goes through the decode layer.
fn run(rx: mpsc::Receiver<Input>, state: Arc<Mutex<State>>, wishes: &Wishes) {
    let loaded = load_game_data().and_then(|data| Ok((data, load_keys()?)));
    let mut sniffer = match loaded {
        Ok((game_data, keys)) => {
            let mut state = state.lock().unwrap();
            state.player_data =
                Some(PlayerData::new(game_data).with_game_data_sha(embedded_game_data_sha()));
            log::info!(
                "decoder ready, game data {}",
                embedded_game_data_sha().unwrap_or("unknown")
            );
            GameSniffer::new().set_initial_keys(keys)
        }
        Err(e) => {
            log::error!("decoder: cannot load the game data: {e:#}");
            state.lock().unwrap().error = Some(format!("{e:#}"));
            return;
        }
    };
    let mut generation = sniffer.session_generation();
    let mut stats = SnifferStats::default();
    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();

    loop {
        let input = match rx.recv_timeout(Duration::from_millis(COUNTS_EVERY_MS)) {
            Ok(input) => input,
            Err(RecvTimeoutError::Timeout) => {
                state.lock().unwrap().refresh_counts(true);
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match input {
            Input::CaptureStarted => {
                let mut state = state.lock().unwrap();
                let now = now_ms();
                state.restarted_mid_session = state.connection.is_some();
                state.capture_started = Some(now);
                state.capture_first_game_packet = None;
            }
            #[cfg(test)]
            Input::Flush(done) => {
                state.lock().unwrap().refresh_counts(true);
                let _ = done.send(());
            }
            Input::Packet(at, data) => {
                let at = unix_ms(at);
                {
                    let mut state = state.lock().unwrap();
                    state.last_game_packet = Some(at);
                    state.capture_first_game_packet.get_or_insert(at);
                }
                let Some(frame) = recording::as_ethernet(LINKTYPE_RAW, data) else {
                    continue;
                };
                stats.note_frame(1);
                decode::decode_one_packet(
                    &mut sniffer,
                    &mut generation,
                    frame,
                    &events_tx,
                    &mut stats,
                );
                while let Ok(event) = events_rx.try_recv() {
                    handle(&mut state.lock().unwrap(), event, at, wishes.hub());
                }
                state.lock().unwrap().refresh_counts(false);
            }
        }
    }
}

/// What the monitor does with one sniffer event, minus the UI, uploads and
/// the automation (the app runs those from the summary).
fn handle(state: &mut State, event: SnifferEvent, at: u64, hub: &WishHub) {
    match event {
        SnifferEvent::SessionReset => {
            state.replacement.connection_reset();
            state.connection = Some(Connection {
                began_here: true,
                ..Connection::default()
            });
            state.restarted_mid_session = false;
            state.last_placed = Some(at);
            state.event("info", "Game connected");
        }
        SnifferEvent::Packet(packet, report) => {
            state.last_placed = Some(at);
            note_key(state, report, at);
            let GamePacket::Commands(commands) = packet else {
                return;
            };
            if commands.is_empty() {
                return;
            }
            let mut classified = decode::classify_commands(&commands);
            // Wish history messages are not account data: they go to the
            // wish key's hub, not into the batch that may replace the data.
            let events = take_wish_events(&mut classified);
            if !events.is_empty() {
                hub.forward(events, state.uid());
            }
            apply(state, report.state, classified, at, hub);
        }
    }
}

fn note_key(state: &mut State, report: KeyReport, at: u64) {
    let connection = state.connection.get_or_insert_with(Connection::default);
    let mut found = false;
    let mut login = false;
    if connection.key != Some(report.state) {
        // The first message under the dispatch key is the token exchange:
        // the login itself, not just the game connecting.
        login = report.state == KeyState::Dispatch
            && connection.key.is_none_or(|key| key == KeyState::None);
        connection.key = Some(report.state);
        if report.state == KeyState::Session && connection.key_at.is_none() {
            connection.key_at = Some(at);
            found = true;
        }
    }
    let lost = report.recovery_failed && !connection.recovery_failed;
    connection.recovery_failed |= report.recovery_failed;
    if login {
        state.event("info", "Login seen");
    }
    if found {
        state.event("ok", "Session key found");
    }
    if lost {
        state.event("error", "Session key not found");
    }
}

fn apply(
    state: &mut State,
    key: KeyState,
    classified: Vec<(u16, teyvatme_protocol::CommandMatch)>,
    at: u64,
    hub: &WishHub,
) {
    if classified.is_empty() {
        return;
    }
    match state.replacement.verdict_for_batch(key, &classified) {
        DataVerdict::Apply => {}
        DataVerdict::ReplaceThenApply => {
            // A new login: its key, if any, is not the old one's.
            hub.new_login();
            if let Some(data) = &mut state.player_data {
                data.reset();
            }
            state.updated = Updated::default();
            state.login += 1;
        }
        DataVerdict::Hold => return,
    }
    let Some(data) = &mut state.player_data else {
        return;
    };
    let applied: AppliedData = decode::apply_commands(data, classified);
    if !applied.any() {
        return;
    }
    let first = state.data_at.is_none() || state.updated == Updated::default();
    if applied.items {
        state.updated.items = Some(at);
    }
    if applied.characters {
        state.updated.characters = Some(at);
    }
    if applied.achievements {
        state.updated.achievements = Some(at);
    }
    state.data_at = Some(at);
    state.data_version += 1;
    // The login's UID binds its wish key; another UID is a new login.
    hub.note_uid(state.uid());
    if first && (applied.items || applied.characters) {
        state.event("ok", "Account data captured");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decodes a tee capture as the app would and writes its export with the
    /// tracker's settings (what `teyvatme --replay-export` writes), to
    /// compare the two. Personal data: run it on the phone by hand, with
    /// `TEYVATME_REPLAY_PCAP` and `TEYVATME_REPLAY_OUT` set.
    #[test]
    #[ignore]
    fn replays_a_tee_capture() {
        let pcap = std::env::var("TEYVATME_REPLAY_PCAP").expect("TEYVATME_REPLAY_PCAP");
        let out = std::env::var("TEYVATME_REPLAY_OUT").expect("TEYVATME_REPLAY_OUT");
        let decoder = Decoder::spawn();
        let mut recording = recording::Recording::open(std::path::Path::new(&pcap)).unwrap();
        let mut last = UNIX_EPOCH;
        while let Some(frame) = recording.next_frame().unwrap() {
            // The tee's own captures are bare IP; a desktop recording's
            // frames are Ethernet, whose header the tee never sees.
            let data = match frame.link_type {
                LINKTYPE_RAW => frame.data,
                1 if frame.data.len() > 14 => frame.data[14..].to_vec(),
                _ => continue,
            };
            let at = UNIX_EPOCH + Duration::from_nanos(frame.timestamp_ns.unwrap_or(0));
            last = at;
            decoder.packet(at, data);
        }
        let (done, flushed) = mpsc::channel();
        decoder.tx.lock().unwrap().send(Input::Flush(done)).unwrap();
        flushed.recv().unwrap();

        let state = decoder.state.lock().unwrap();
        let data = state.player_data.as_ref().expect("game data loaded");
        let (good, _) = data.export_at(&ExportSettings::for_tracker(), unix_ms(last));
        std::fs::write(&out, serde_json::to_string_pretty(&good).unwrap()).unwrap();
        println!(
            "phase {:?}, counts {:?}, events {:?}",
            state.phase(unix_ms(last)),
            state.counts,
            state.events.iter().map(|e| &e.text).collect::<Vec<_>>()
        );
        // Whether the wish key came through, never the URL itself.
        println!(
            "wish {:?}, url built: {}",
            decoder.wishes.summary(),
            decoder.wishes.url().is_some()
        );
    }

    #[test]
    fn servers_come_from_the_uid() {
        assert_eq!(server_of(812_345_678), Some("Asia"));
        assert_eq!(server_of(612_345_678), Some("America"));
        assert_eq!(server_of(1_812_345_678), Some("Asia"));
        assert_eq!(server_of(12_345), None);
    }

    #[test]
    fn the_phase_follows_the_traffic() {
        let mut state = State::new();
        let now = 1_000_000;
        assert_eq!(state.phase(now), Phase::Waiting);

        // The game connected at its title screen: no login yet.
        state.capture_first_game_packet = Some(now - 2_000);
        state.last_game_packet = Some(now - 100);
        state.last_placed = Some(now - 100);
        state.connection = Some(Connection {
            began_here: true,
            ..Connection::default()
        });
        assert_eq!(state.phase(now), Phase::GameDetected);
        state.connection.as_mut().unwrap().key = Some(KeyState::None);
        assert_eq!(state.phase(now), Phase::GameDetected);

        // The token exchange: the login, before its key.
        state.connection.as_mut().unwrap().key = Some(KeyState::Dispatch);
        assert_eq!(state.phase(now), Phase::LoginSeen);

        state.connection.as_mut().unwrap().key = Some(KeyState::Session);
        assert_eq!(state.phase(now), Phase::Decrypting);

        // A minute in the background is still the same login.
        assert_eq!(state.phase(now + 60_000), Phase::Decrypting);

        // The game closed: the data stays, the app waits for it again.
        let later = now + CONNECTION_QUIET_MS;
        assert_eq!(state.phase(later), Phase::Waiting);
    }

    #[test]
    fn login_seen_is_the_token_exchange_not_the_connection() {
        let mut state = State::new();
        state.connection = Some(Connection {
            began_here: true,
            ..Connection::default()
        });
        let report = |state: KeyState| KeyReport {
            state,
            recovery_failed: false,
        };
        note_key(&mut state, report(KeyState::None), 1);
        assert!(state.events.is_empty(), "connecting is not logging in");
        note_key(&mut state, report(KeyState::Dispatch), 2);
        note_key(&mut state, report(KeyState::Dispatch), 3);
        note_key(&mut state, report(KeyState::Session), 4);
        let events: Vec<_> = state.events.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(events, ["Login seen", "Session key found"]);
    }

    #[test]
    fn game_traffic_the_sniffer_cannot_place_is_a_missed_launch() {
        let mut state = State::new();
        let now = 1_000_000;
        state.capture_first_game_packet = Some(now - 8_000);
        state.last_game_packet = Some(now - 100);
        assert_eq!(state.phase(now), Phase::LaunchMissed);

        // Not before a few seconds of it, though.
        state.capture_first_game_packet = Some(now - 1_000);
        assert_eq!(state.phase(now), Phase::GameDetected);
    }

    #[test]
    fn a_lost_key_is_reported() {
        let mut state = State::new();
        state.connection = Some(Connection {
            began_here: true,
            key: Some(KeyState::Dispatch),
            recovery_failed: true,
            ..Connection::default()
        });
        state.last_game_packet = Some(999_000);
        assert_eq!(state.phase(1_000_000), Phase::KeyLost);
        // Until the game is gone: then it waits for the next launch.
        assert_eq!(state.phase(999_000 + CONNECTION_QUIET_MS), Phase::Waiting);
    }

    #[test]
    fn the_activity_list_keeps_the_newest_fifty() {
        let mut state = State::new();
        for n in 0..60 {
            state.event("info", format!("event {n}"));
        }
        assert_eq!(state.events.len(), EVENTS);
        assert_eq!(state.events.front().unwrap().text, "event 10");
    }
}
