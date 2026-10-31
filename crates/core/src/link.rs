// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The link with the tracker beyond uploads (the tracker's
//! `docs/plans/capture-link.md`, "Contract with Capture"): the presence
//! heartbeat and what its answers carry (commands from the website, this
//! device's alert settings).
//!
//! Pure: no HTTP and no clock of its own. Each app sends the requests with
//! its own client and credential (the sign-in, else the verified import
//! key) and feeds the answers in; times are Unix milliseconds.

use serde::{Deserialize, Deserializer, Serialize};

/// The presence route under the API base.
pub const PRESENCE_PATH: &str = "irminsul/presence";

/// The heartbeat's interval when the answer names none.
pub const DEFAULT_NEXT_SECS: u64 = 60;
/// The bounds a tracker's `next` is held to.
pub const MIN_NEXT_SECS: u64 = 10;
pub const MAX_NEXT_SECS: u64 = 300;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
