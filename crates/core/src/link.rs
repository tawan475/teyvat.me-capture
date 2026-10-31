// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The link with the tracker beyond uploads (the tracker's
//! `docs/plans/capture-link.md`, "Contract with Capture"): the presence
//! heartbeat and what its answers carry (commands from the website, this
//! device's alert settings), and the farm plan the Planner published for
//! the Today screen.
//!
//! Pure: no HTTP and no clock of its own. Each app sends the requests with
//! its own client and credential (the sign-in, else the verified import
//! key) and feeds the answers in; times are Unix milliseconds.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};

/// The presence route under the API base.
pub const PRESENCE_PATH: &str = "irminsul/presence";
/// The farm plan route under the API base.
pub const TODAY_PATH: &str = "irminsul/today";

/// The heartbeat's interval when the answer names none.
pub const DEFAULT_NEXT_SECS: u64 = 60;
/// The bounds a tracker's `next` is held to.
pub const MIN_NEXT_SECS: u64 = 10;
pub const MAX_NEXT_SECS: u64 = 300;

/// How often the plan is read while linked.
pub const TODAY_EVERY_MS: u64 = 10 * 60_000;
/// Opening the Today screen reads the plan again, at most this often.
pub const TODAY_OPEN_MIN_MS: u64 = 60_000;

/// Original Resin's cap: `resinFull` is an alert at this.
pub const RESIN_CAP: u32 = 200;

/// What Capture tells the tracker about the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresenceState {
    /// The game is closed. Sent once, when it closes.
    #[default]
    Stopped,
    /// The game runs, but nothing decodes yet (or the launch was missed).
    Running,
    /// In game, data flowing.
    Decoding,
}

impl PresenceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Running => "running",
            Self::Decoding => "decoding",
        }
    }

    /// The name [`Self::as_str`] gives, back.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "stopped" => Some(Self::Stopped),
            "running" => Some(Self::Running),
            "decoding" => Some(Self::Decoding),
            _ => None,
        }
    }
}

/// One heartbeat's body.
#[derive(Debug, Serialize)]
struct PresenceBody<'a> {
    v: u32,
    state: &'static str,
    uid: Option<u32>,
    app: &'a str,
    platform: &'a str,
}

/// The JSON body of `POST /irminsul/presence`. `platform` is `windows`,
/// `linux`, `macos` or `android`.
pub fn presence_body(state: PresenceState, uid: Option<u32>, app: &str, platform: &str) -> String {
    serde_json::to_string(&PresenceBody {
        v: 1,
        state: state.as_str(),
        uid,
        app,
        platform,
    })
    .unwrap_or_default()
}

/// This device's alert settings on the tracker, as far as Capture's own
/// resin notification goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alerts {
    /// Alert when resin reaches this (1–200); `None`: off.
    #[serde(default, deserialize_with = "lenient_u32")]
    pub resin: Option<u32>,
    /// Alert when resin is full.
    #[serde(default, rename = "resinFull")]
    pub resin_full: bool,
}

impl Alerts {
    /// The amounts to alert at, lowest first, without repeats.
    pub fn resin_targets(&self) -> [Option<u32>; 2] {
        let at = self.resin.map(|at| at.clamp(1, RESIN_CAP));
        let full = self.resin_full.then_some(RESIN_CAP);
        match (at, full) {
            (Some(at), Some(full)) if at == full => [Some(at), None],
            (at, full) => [at, full],
        }
    }
}

/// A command the website sent the running Capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Run the wish history sync now (when this login has a wish key).
    Wishes,
    /// Send a full snapshot now (reason `manual`), when there is data.
    Snapshot,
}

/// What the answer says about the alert settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlertsAnswer {
    /// No `alerts` field (an older tracker): keep what is remembered.
    #[default]
    Absent,
    /// `null`: none set for this device (or an import key): forget them.
    Unset,
    /// This device's settings.
    Set(Alerts),
}

/// A 2xx answer to a heartbeat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresenceAnswer {
    /// Seconds to the next heartbeat, within the bounds.
    pub next_secs: u64,
    /// Known commands, in order; unknown kinds are skipped.
    pub commands: Vec<Command>,
    pub alerts: AlertsAnswer,
}

impl PresenceAnswer {
    /// Reads a 2xx body. Anything unreadable is an answer with nothing in it:
    /// the tracker got the heartbeat.
    pub fn parse(body: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let next_secs = value
            .get("next")
            .and_then(number)
            .map_or(DEFAULT_NEXT_SECS, |secs| {
                secs.clamp(MIN_NEXT_SECS, MAX_NEXT_SECS)
            });
        let commands = value
            .get("commands")
            .and_then(|list| list.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|command| match command.get("kind")?.as_str()? {
                        "wishes" => Some(Command::Wishes),
                        "snapshot" => Some(Command::Snapshot),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let alerts = match value.get("alerts") {
            None => AlertsAnswer::Absent,
            Some(serde_json::Value::Null) => AlertsAnswer::Unset,
            Some(alerts) => match serde_json::from_value::<Alerts>(alerts.clone()) {
                Ok(alerts) => AlertsAnswer::Set(alerts),
                Err(_) => AlertsAnswer::Absent,
            },
        };
        Self {
            next_secs,
            commands,
            alerts,
        }
    }
}

/// How a heartbeat went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresenceOutcome {
    Answered(PresenceAnswer),
    /// 404/405: a tracker without the route. Presence stays off until
    /// Capture restarts.
    Gone,
    /// No answer, or one that is not a success: try again at the next tick.
    Retry,
}

impl PresenceOutcome {
    /// The outcome for an HTTP status (0: no answer) and body.
    pub fn of(status: u16, body: &str) -> Self {
        match status {
            200..=299 => Self::Answered(PresenceAnswer::parse(body)),
            404 | 405 => Self::Gone,
            _ => Self::Retry,
        }
    }
}

/// When heartbeats go: at every change of state or UID, and every `next`
/// seconds while the game is not stopped. One request at a time.
#[derive(Debug)]
pub struct Heartbeat {
    /// The state and UID last sent (at first: stopped, so a closed game sends
    /// nothing at all).
    sent: (PresenceState, Option<u32>),
    /// The tracker took what was last sent.
    confirmed: bool,
    interval_ms: u64,
    next_at: Option<u64>,
    in_flight: bool,
    gone: bool,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            sent: (PresenceState::Stopped, None),
            confirmed: true,
            interval_ms: DEFAULT_NEXT_SECS * 1000,
            next_at: None,
            in_flight: false,
            gone: false,
        }
    }
}

impl Heartbeat {
    pub fn new() -> Self {
        Self::default()
    }

    /// When the next heartbeat is due for the game's `state` and the
    /// account's `uid` (Unix ms; 0 = now), or `None`: nothing to send.
    pub fn due_at(&self, state: PresenceState, uid: Option<u32>) -> Option<u64> {
        if self.gone || self.in_flight {
            return None;
        }
        if (state, uid) != self.sent {
            return Some(0);
        }
        if state == PresenceState::Stopped && self.confirmed {
            return None;
        }
        self.next_at
    }

    /// A heartbeat for `state`/`uid` goes now.
    pub fn sending(&mut self, state: PresenceState, uid: Option<u32>, now: u64) {
        self.sent = (state, uid);
        self.confirmed = false;
        self.in_flight = true;
        self.next_at = Some(now + self.interval_ms);
    }

    /// The heartbeat in flight finished.
    pub fn answered(&mut self, outcome: &PresenceOutcome, now: u64) {
        self.in_flight = false;
        match outcome {
            PresenceOutcome::Answered(answer) => {
                self.confirmed = true;
                self.interval_ms = answer.next_secs * 1000;
                self.next_at = Some(now + self.interval_ms);
            }
            PresenceOutcome::Gone => self.gone = true,
            PresenceOutcome::Retry => self.next_at = Some(now + self.interval_ms),
        }
    }

    /// The link is gone (signed out, no key): start over, so a new link
    /// hears the current state at once. A tracker without the route stays
    /// known.
    pub fn reset(&mut self) {
        *self = Self {
            gone: self.gone,
            ..Self::default()
        };
    }

    /// Whether the tracker lacks the route (presence is off).
    pub fn is_gone(&self) -> bool {
        self.gone
    }
}

/// When the plan is read: at once when linked (or the UID changes), every
/// [`TODAY_EVERY_MS`], and when the Today screen opens (at most once per
/// [`TODAY_OPEN_MIN_MS`]).
#[derive(Debug, Default)]
pub struct TodayPoll {
    /// When the last read began, and for which UID.
    last: Option<(u64, Option<u32>)>,
    asked: bool,
    in_flight: bool,
    gone: bool,
}

impl TodayPoll {
    pub fn new() -> Self {
        Self::default()
    }

    /// When the next read is due for `uid` (Unix ms; 0 = now), or `None`.
    pub fn due_at(&self, uid: Option<u32>) -> Option<u64> {
        if self.gone || self.in_flight {
            return None;
        }
        match self.last {
            None => Some(0),
            Some((_, last_uid)) if last_uid != uid => Some(0),
            Some(_) if self.asked => Some(0),
            Some((at, _)) => Some(at + TODAY_EVERY_MS),
        }
    }

    /// The Today screen opened.
    pub fn opened(&mut self, now: u64) {
        if self
            .last
            .is_none_or(|(at, _)| now >= at + TODAY_OPEN_MIN_MS)
        {
            self.asked = true;
        }
    }

    /// A read for `uid` goes now.
    pub fn sending(&mut self, uid: Option<u32>, now: u64) {
        self.last = Some((now, uid));
        self.asked = false;
        self.in_flight = true;
    }

    pub fn answered(&mut self, outcome: &TodayOutcome) {
        self.in_flight = false;
        if *outcome == TodayOutcome::Gone {
            self.gone = true;
        }
    }

    /// The link is gone: the next one reads at once.
    pub fn reset(&mut self) {
        *self = Self {
            gone: self.gone,
            ..Self::default()
        };
    }
}

/// How a plan read went.
#[derive(Clone, Debug, PartialEq)]
pub enum TodayOutcome {
    Plan(TodayDigest),
    /// 404 `no_plan`: the Planner has published nothing for the account.
    NoPlan,
    /// 404 without `no_plan`, or 405: a tracker without the route.
    Gone,
    /// No answer, or one that is not a success: keep what is shown.
    Retry,
}

impl TodayOutcome {
    pub fn of(status: u16, body: &str) -> Self {
        match status {
            200..=299 => match serde_json::from_str(body) {
                Ok(digest) => Self::Plan(digest),
                Err(e) => {
                    tracing::warn!("the tracker's plan could not be read: {e}");
                    Self::Retry
                }
            },
            404 if error_code(body).as_deref() == Some("no_plan") => Self::NoPlan,
            404 | 405 => Self::Gone,
            _ => Self::Retry,
        }
    }
}

/// What the Today screen shows from the tracker.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum PlanState {
    /// Not linked, or not read yet: nothing at all.
    #[default]
    None,
    /// Linked, nothing published: the hint to open the Planner.
    NoPlan,
    Plan(Arc<TodayDigest>),
}

/// The farm plan the Planner last published (`GET /irminsul/today`). Text
/// is in the language the Planner was opened in. Unknown fields are
/// ignored; missing ones take their defaults.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TodayDigest {
    pub v: u32,
    pub account: Option<PlanAccount>,
    /// When it was published (Unix ms).
    pub at: u64,
    pub lang: String,
    /// The server's game day it was made for (0 = Sunday).
    #[serde(deserialize_with = "lenient_u32")]
    pub weekday: Option<u32>,
    pub sections: Vec<PlanSection>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PlanAccount {
    #[serde(deserialize_with = "lenient_u32")]
    pub id: Option<u32>,
    pub name: String,
}

/// The plan's cards of one kind and resin cost ("Domains", 20).
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PlanSection {
    pub key: String,
    /// The resin a run costs, as the Planner labels it.
    #[serde(deserialize_with = "lenient_text")]
    pub resin: String,
    pub label: String,
    pub cards: Vec<PlanCard>,
}

/// One place to farm today.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlanCard {
    pub name: String,
    pub detail: String,
    #[serde(deserialize_with = "lenient_u32")]
    pub runs: Option<u32>,
    #[serde(deserialize_with = "lenient_u32")]
    pub resin: Option<u32>,
    /// `runs` is at most that many.
    pub upper_bound: bool,
    pub items: Vec<PlanItem>,
    pub who: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PlanItem {
    pub key: String,
    pub name: String,
    #[serde(deserialize_with = "lenient_u32")]
    pub missing: Option<u32>,
}

/// The game day changes at 04:00 server time.
const RESET_HOUR: i64 = 4;

/// A UID's server time, as hours from UTC: America UTC−5, Europe UTC+1,
/// Asia, TW·HK·MO and China UTC+8 (the region from the UID's first digit).
pub fn server_utc_offset_hours(uid: u32) -> Option<i64> {
    match crate::wish::hub::region_for_uid(uid)? {
        "os_usa" => Some(-5),
        "os_euro" => Some(1),
        _ => Some(8),
    }
}

/// The server's game day at `now_ms`, counted in days from 1970-01-01, or
/// `None` for a UID whose server is not known.
pub fn game_day(uid: u32, now_ms: u64) -> Option<i64> {
    let offset = server_utc_offset_hours(uid)?;
    let secs = i64::try_from(now_ms / 1000).ok()?;
    Some((secs + (offset - RESET_HOUR) * 3600).div_euclid(86_400))
}

/// The game day's weekday (0 = Sunday; 1970-01-01 was a Thursday).
pub fn weekday_of(day: i64) -> u32 {
    (day + 4).rem_euclid(7) as u32
}

/// Ms until the server's next game day begins, for a redraw then.
pub fn ms_to_next_game_day(uid: u32, now_ms: u64) -> Option<u64> {
    let day = game_day(uid, now_ms)?;
    let offset = server_utc_offset_hours(uid)?;
    let next = (day + 1) * 86_400 - (offset - RESET_HOUR) * 3600;
    let next_ms = u64::try_from(next).ok()? * 1000;
    Some(next_ms.saturating_sub(now_ms))
}

/// Whether a plan is from a game day that is over: "from your plan of
/// <day>". Not when the UID (so the server) is not known.
pub fn plan_is_old(digest: &TodayDigest, uid: Option<u32>, now_ms: u64) -> bool {
    let Some(today) = uid.and_then(|uid| game_day(uid, now_ms)) else {
        return false;
    };
    let other_weekday = digest
        .weekday
        .is_some_and(|weekday| weekday != weekday_of(today));
    let earlier_day = digest.at > 0
        && uid
            .and_then(|uid| game_day(uid, digest.at))
            .is_some_and(|day| day < today);
    other_weekday || earlier_day
}

/// A whole number from a JSON number or a numeric string.
fn number(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `Option<u32>` from a number, a numeric string, or anything else (`None`).
fn lenient_u32<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(number(&value).and_then(|n| u32::try_from(n).ok()))
}

/// Text from a string or a number; anything else is empty.
fn lenient_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    })
}

/// The `error` code of a tracker error body: `{"error": "code"}` or
/// `{"error": {"code": "code"}}`.
fn error_code(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    error
        .as_str()
        .or_else(|| error.get("code")?.as_str())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-30 12:00 UTC, a Friday.
    const FRIDAY_NOON_UTC: u64 = 1_793_361_600_000;
    const HOUR: u64 = 3_600_000;

    #[test]
    fn the_body_is_the_contracts() {
        let body = presence_body(
            PresenceState::Running,
            Some(812_345_678),
            "1.17.4",
            "windows",
        );
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "v": 1, "state": "running", "uid": 812345678, "app": "1.17.4", "platform": "windows"
            })
        );
        let unknown = presence_body(PresenceState::Stopped, None, "1.17.4", "android");
        assert!(unknown.contains("\"uid\":null"), "{unknown}");
        for state in [
            PresenceState::Stopped,
            PresenceState::Running,
            PresenceState::Decoding,
        ] {
            assert_eq!(PresenceState::parse(state.as_str()), Some(state));
        }
    }

    #[test]
    fn an_answer_gives_its_interval_commands_and_alerts() {
        let answer = PresenceAnswer::parse(
            r#"{"next": 15, "commands": [{"id": "k3f9", "kind": "wishes"},
                {"id": "x", "kind": "dance"}, {"id": "y", "kind": "snapshot"}],
                "alerts": {"resin": 160, "resinFull": true, "expeditions": false}, "extra": 1}"#,
        );
        assert_eq!(answer.next_secs, 15);
        assert_eq!(answer.commands, vec![Command::Wishes, Command::Snapshot]);
        assert_eq!(
            answer.alerts,
            AlertsAnswer::Set(Alerts {
                resin: Some(160),
                resin_full: true
            })
        );
    }

    #[test]
    fn the_interval_is_held_to_its_bounds_and_defaults() {
        assert_eq!(PresenceAnswer::parse(r#"{"next": 1}"#).next_secs, 10);
        assert_eq!(PresenceAnswer::parse(r#"{"next": 9000}"#).next_secs, 300);
        assert_eq!(PresenceAnswer::parse(r#"{"next": "30"}"#).next_secs, 30);
        assert_eq!(PresenceAnswer::parse("{}").next_secs, 60);
        assert_eq!(PresenceAnswer::parse("not json").next_secs, 60);
    }

    #[test]
    fn alerts_absent_null_and_off() {
        assert_eq!(PresenceAnswer::parse("{}").alerts, AlertsAnswer::Absent);
        assert_eq!(
            PresenceAnswer::parse(r#"{"alerts": null}"#).alerts,
            AlertsAnswer::Unset
        );
        let off = PresenceAnswer::parse(r#"{"alerts": {"resin": null, "resinFull": false}}"#);
        assert_eq!(off.alerts, AlertsAnswer::Set(Alerts::default()));
        assert_eq!(Alerts::default().resin_targets(), [None, None]);
    }

    #[test]
    fn resin_targets_are_ordered_and_not_repeated() {
        let both = Alerts {
            resin: Some(160),
            resin_full: true,
        };
        assert_eq!(both.resin_targets(), [Some(160), Some(200)]);
        let same = Alerts {
            resin: Some(200),
            resin_full: true,
        };
        assert_eq!(same.resin_targets(), [Some(200), None]);
        let wild = Alerts {
            resin: Some(900),
            resin_full: false,
        };
        assert_eq!(wild.resin_targets(), [Some(200), None]);
    }

    #[test]
    fn outcomes_by_status() {
        assert!(matches!(
            PresenceOutcome::of(200, "{}"),
            PresenceOutcome::Answered(_)
        ));
        assert_eq!(PresenceOutcome::of(404, ""), PresenceOutcome::Gone);
        assert_eq!(PresenceOutcome::of(405, ""), PresenceOutcome::Gone);
        assert_eq!(PresenceOutcome::of(0, ""), PresenceOutcome::Retry);
        assert_eq!(PresenceOutcome::of(503, ""), PresenceOutcome::Retry);
        assert_eq!(PresenceOutcome::of(401, ""), PresenceOutcome::Retry);
    }

    #[test]
    fn a_closed_game_sends_nothing_until_it_runs() {
        let beat = Heartbeat::new();
        assert_eq!(beat.due_at(PresenceState::Stopped, None), None);
        assert_eq!(beat.due_at(PresenceState::Running, None), Some(0));
    }

    #[test]
    fn heartbeats_follow_changes_and_the_interval() {
        let mut beat = Heartbeat::new();
        let now = 1_000_000;
        beat.sending(PresenceState::Running, None, now);
        // One at a time.
        assert_eq!(beat.due_at(PresenceState::Decoding, None), None);
        beat.answered(&PresenceOutcome::of(200, r#"{"next": 15}"#), now + 200);
        assert_eq!(
            beat.due_at(PresenceState::Running, None),
            Some(now + 15_200)
        );
        // A change goes at once: the state, and the UID becoming known.
        assert_eq!(beat.due_at(PresenceState::Decoding, None), Some(0));
        assert_eq!(
            beat.due_at(PresenceState::Running, Some(812_345_678)),
            Some(0)
        );
        beat.sending(PresenceState::Decoding, Some(812_345_678), now + 1_000);
        beat.answered(&PresenceOutcome::of(200, "{}"), now + 1_100);
        assert_eq!(
            beat.due_at(PresenceState::Decoding, Some(812_345_678)),
            Some(now + 61_100)
        );
    }

    #[test]
    fn stopped_is_sent_once_and_retried_until_taken() {
        let mut beat = Heartbeat::new();
        beat.sending(PresenceState::Running, None, 0);
        beat.answered(&PresenceOutcome::of(200, "{}"), 10);
        assert_eq!(beat.due_at(PresenceState::Stopped, None), Some(0));
        beat.sending(PresenceState::Stopped, None, 20);
        beat.answered(&PresenceOutcome::Retry, 30);
        // Not taken: again at the next tick.
        assert_eq!(beat.due_at(PresenceState::Stopped, None), Some(60_030));
        beat.sending(PresenceState::Stopped, None, 60_030);
        beat.answered(&PresenceOutcome::of(204, ""), 60_040);
        assert_eq!(beat.due_at(PresenceState::Stopped, None), None);
    }

    #[test]
    fn a_tracker_without_the_route_stops_presence() {
        let mut beat = Heartbeat::new();
        beat.sending(PresenceState::Running, None, 0);
        beat.answered(&PresenceOutcome::Gone, 10);
        assert!(beat.is_gone());
        assert_eq!(beat.due_at(PresenceState::Decoding, None), None);
        beat.reset();
        assert_eq!(beat.due_at(PresenceState::Decoding, None), None);
    }

    #[test]
    fn a_reset_sends_the_current_state_again() {
        let mut beat = Heartbeat::new();
        beat.sending(PresenceState::Running, None, 0);
        beat.answered(&PresenceOutcome::of(200, "{}"), 10);
        beat.reset();
        assert_eq!(beat.due_at(PresenceState::Running, None), Some(0));
        assert_eq!(beat.due_at(PresenceState::Stopped, None), None);
    }

    #[test]
    fn the_plan_is_read_at_once_then_every_ten_minutes_and_on_open() {
        let mut poll = TodayPoll::new();
        let uid = Some(812_345_678);
        assert_eq!(poll.due_at(uid), Some(0));
        poll.sending(uid, 1_000);
        assert_eq!(poll.due_at(uid), None);
        poll.answered(&TodayOutcome::NoPlan);
        assert_eq!(poll.due_at(uid), Some(1_000 + TODAY_EVERY_MS));
        // Opening the screen within the minute does nothing; after it, reads.
        poll.opened(30_000);
        assert_eq!(poll.due_at(uid), Some(1_000 + TODAY_EVERY_MS));
        poll.opened(61_000);
        assert_eq!(poll.due_at(uid), Some(0));
        poll.sending(uid, 61_000);
        poll.answered(&TodayOutcome::Retry);
        // Another account: at once.
        assert_eq!(poll.due_at(Some(712_345_678)), Some(0));
        poll.answered(&TodayOutcome::Gone);
        assert_eq!(poll.due_at(uid), None);
    }

    #[test]
    fn the_contracts_digest_reads_and_unknown_fields_are_ignored() {
        let body = r#"{
          "v": 1, "account": {"id": 1, "name": "Main", "color": "red"},
          "at": 1760000000000, "lang": "en", "weekday": 5, "future": [1, 2],
          "sections": [{
            "key": "domain", "resin": "20", "label": "Domains", "icon": "x",
            "cards": [{
              "name": "Cecilia Garden", "detail": "Freedom", "runs": 6, "resin": 120,
              "upperBound": false, "region": "Mondstadt",
              "items": [{"key": "TeachingsOfFreedom", "name": "Teachings of Freedom", "missing": 9, "rarity": 2}],
              "who": ["Venti", "Sucrose"]
            }]
          }]
        }"#;
        let TodayOutcome::Plan(digest) = TodayOutcome::of(200, body) else {
            panic!("not a plan");
        };
        assert_eq!(digest.weekday, Some(5));
        assert_eq!(digest.account.as_ref().unwrap().name, "Main");
        let section = &digest.sections[0];
        assert_eq!(
            (section.label.as_str(), section.resin.as_str()),
            ("Domains", "20")
        );
        let card = &section.cards[0];
        assert_eq!(card.runs, Some(6));
        assert_eq!(card.resin, Some(120));
        assert_eq!(card.items[0].missing, Some(9));
        assert_eq!(card.who, ["Venti", "Sucrose"]);
    }

    #[test]
    fn a_sparse_or_odd_digest_still_reads() {
        let TodayOutcome::Plan(digest) = TodayOutcome::of(
            200,
            r#"{"sections": [{"resin": 40, "cards": [{"name": "Boss", "runs": "3", "resin": null, "items": [{}]}]}]}"#,
        ) else {
            panic!("not a plan");
        };
        let section = &digest.sections[0];
        assert_eq!(section.resin, "40");
        assert_eq!(section.cards[0].runs, Some(3));
        assert_eq!(section.cards[0].resin, None);
        assert_eq!(section.cards[0].items[0].missing, None);
        assert_eq!(digest.weekday, None);
        // Round trip (the Android bridge hands it on re-serialized).
        let again: TodayDigest =
            serde_json::from_str(&serde_json::to_string(&digest).unwrap()).unwrap();
        assert_eq!(again, digest);
    }

    #[test]
    fn plan_outcomes_by_status() {
        assert_eq!(
            TodayOutcome::of(404, r#"{"error": "no_plan"}"#),
            TodayOutcome::NoPlan
        );
        assert_eq!(
            TodayOutcome::of(404, r#"{"error": {"code": "no_plan"}}"#),
            TodayOutcome::NoPlan
        );
        assert_eq!(TodayOutcome::of(404, "Not Found"), TodayOutcome::Gone);
        assert_eq!(TodayOutcome::of(405, ""), TodayOutcome::Gone);
        assert_eq!(TodayOutcome::of(200, "[1"), TodayOutcome::Retry);
        assert_eq!(TodayOutcome::of(0, ""), TodayOutcome::Retry);
        assert_eq!(TodayOutcome::of(500, ""), TodayOutcome::Retry);
    }

    #[test]
    fn the_game_day_turns_at_four_server_time() {
        let asia = 812_345_678;
        let america = 612_345_678;
        let europe = 712_345_678;
        // Friday 12:00 UTC is Friday 20:00 in Asia, 07:00 in America, 13:00
        // in Europe: Friday everywhere.
        for uid in [asia, america, europe] {
            assert_eq!(
                weekday_of(game_day(uid, FRIDAY_NOON_UTC).unwrap()),
                5,
                "{uid}"
            );
        }
        // 20:00 UTC: Saturday 04:00 in Asia (a new day), 15:00 in America.
        let evening = FRIDAY_NOON_UTC + 8 * HOUR;
        assert_eq!(weekday_of(game_day(asia, evening).unwrap()), 6);
        assert_eq!(weekday_of(game_day(asia, evening - 1).unwrap()), 5);
        assert_eq!(weekday_of(game_day(america, evening).unwrap()), 5);
        // America's Friday ends at Saturday 09:00 UTC.
        let saturday_9 = FRIDAY_NOON_UTC + 21 * HOUR;
        assert_eq!(weekday_of(game_day(america, saturday_9 - 1).unwrap()), 5);
        assert_eq!(weekday_of(game_day(america, saturday_9).unwrap()), 6);
        assert_eq!(ms_to_next_game_day(asia, evening - 1_000), Some(1_000));
        assert_eq!(
            ms_to_next_game_day(america, FRIDAY_NOON_UTC),
            Some(21 * HOUR)
        );
        assert_eq!(game_day(12_345, FRIDAY_NOON_UTC), None);
    }

    #[test]
    fn a_plan_of_a_day_that_is_over_is_old() {
        let asia = Some(812_345_678);
        let plan = TodayDigest {
            weekday: Some(5),
            at: FRIDAY_NOON_UTC - HOUR,
            ..TodayDigest::default()
        };
        assert!(!plan_is_old(&plan, asia, FRIDAY_NOON_UTC));
        // Saturday's game day in Asia.
        assert!(plan_is_old(&plan, asia, FRIDAY_NOON_UTC + 8 * HOUR));
        // A week later, the same weekday: still another day.
        assert!(plan_is_old(&plan, asia, FRIDAY_NOON_UTC + 7 * 24 * HOUR));
        // No server known: never said.
        assert!(!plan_is_old(&plan, None, FRIDAY_NOON_UTC + 8 * HOUR));
        // Made for another weekday, whenever published.
        let thursday = TodayDigest {
            weekday: Some(4),
            ..TodayDigest::default()
        };
        assert!(plan_is_old(&thursday, asia, FRIDAY_NOON_UTC));
    }
}
