// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The tracker link beyond uploads on the phone: the presence heartbeat's
//! and the plan reads' timing and answers, from `teyvatme_core::link` (the
//! desktop's own). The app sends the requests with its tracker client
//! (`Tracker.kt`) and hands the answers back here; what they say comes back
//! as small JSON for Kotlin.

use std::sync::{LazyLock, Mutex};

use serde_json::json;
use teyvatme_core::link::{
    AlertsAnswer, Command, Heartbeat, PresenceOutcome, PresenceState, TodayOutcome, TodayPoll,
};

#[derive(Default)]
struct Link {
    presence: Heartbeat,
    today: TodayPoll,
}

static LINK: LazyLock<Mutex<Link>> = LazyLock::new(Mutex::default);

fn uid(uid: i64) -> Option<u32> {
    u32::try_from(uid).ok().filter(|uid| *uid > 0)
}

/// Whether a heartbeat for `state` and `uid` (≤ 0: unknown) is due at
/// `now`; when it is, it counts as sent.
pub fn presence_due(state: &str, raw_uid: i64, now: u64) -> bool {
    let Some(state) = PresenceState::parse(state) else {
        return false;
    };
    let uid = uid(raw_uid);
    let mut link = LINK.lock().unwrap();
    if link.presence.due_at(state, uid).is_some_and(|at| at <= now) {
        link.presence.sending(state, uid, now);
        return true;
    }
    false
}

/// The heartbeat's answer (status 0: none): `{"gone"}` once the tracker
/// lacks the route, else the commands and, when the answer had the field,
/// `alerts` (null: none set).
pub fn presence_answered(status: i32, body: &str, now: u64) -> String {
    let outcome = PresenceOutcome::of(u16::try_from(status).unwrap_or(0), body);
    LINK.lock().unwrap().presence.answered(&outcome, now);
    answer_json(&outcome).to_string()
}

fn answer_json(outcome: &PresenceOutcome) -> serde_json::Value {
    let answer = match outcome {
        PresenceOutcome::Answered(answer) => answer,
        PresenceOutcome::Gone => {
            log::info!("the tracker has no presence route; presence is off");
            return json!({ "gone": true });
        }
        PresenceOutcome::Retry => return json!({}),
    };
    let commands: Vec<&str> = answer
        .commands
        .iter()
        .map(|command| match command {
            Command::Wishes => "wishes",
            Command::Snapshot => "snapshot",
        })
        .collect();
    let mut out = json!({ "commands": commands });
    match answer.alerts {
        AlertsAnswer::Absent => {}
        AlertsAnswer::Unset => out["alerts"] = serde_json::Value::Null,
        AlertsAnswer::Set(alerts) => out["alerts"] = json!(alerts),
    }
    out
}

/// Whether a plan read for `uid` is due at `now`; when it is, it counts as
/// sent.
pub fn today_due(raw_uid: i64, now: u64) -> bool {
    let uid = uid(raw_uid);
    let mut link = LINK.lock().unwrap();
    if link.today.due_at(uid).is_some_and(|at| at <= now) {
        link.today.sending(uid, now);
        return true;
    }
    false
}

/// The Today screen opened.
pub fn today_opened(now: u64) {
    LINK.lock().unwrap().today.opened(now);
}

/// The plan read's answer: `{"kind": "plan", "plan": {…}}`, `noPlan`,
/// `gone` or `retry`.
pub fn today_answered(status: i32, body: &str) -> String {
    let outcome = TodayOutcome::of(u16::try_from(status).unwrap_or(0), body);
    LINK.lock().unwrap().today.answered(&outcome);
    match outcome {
        TodayOutcome::Plan(digest) => json!({ "kind": "plan", "plan": digest }),
        TodayOutcome::NoPlan => json!({ "kind": "noPlan" }),
        TodayOutcome::Gone => json!({ "kind": "gone" }),
        TodayOutcome::Retry => json!({ "kind": "retry" }),
    }
    .to_string()
}

/// The link is gone (signed out, no key) or new: the next one starts over.
pub fn reset() {
    let mut link = LINK.lock().unwrap();
    link.presence.reset();
    link.today.reset();
}

/// Whether a plan made for `weekday` (< 0: unknown), published at `at`
/// (ms), is from a game day that is over for `uid`'s server.
pub fn plan_is_old(weekday: i32, at: i64, raw_uid: i64, now: u64) -> bool {
    let digest = teyvatme_core::link::TodayDigest {
        weekday: u32::try_from(weekday).ok(),
        at: u64::try_from(at).unwrap_or(0),
        ..Default::default()
    };
    teyvatme_core::link::plan_is_old(&digest, uid(raw_uid), now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_for_kotlin_keeps_absent_null_and_set_apart() {
        let of = |body| answer_json(&PresenceOutcome::of(200, body));
        assert_eq!(of("{}"), json!({ "commands": [] }));
        assert_eq!(
            of(r#"{"alerts": null, "commands": [{"id": "1", "kind": "wishes"}]}"#),
            json!({ "commands": ["wishes"], "alerts": null })
        );
        assert_eq!(
            of(r#"{"alerts": {"resin": 160, "resinFull": true, "quiet": true}}"#),
            json!({ "commands": [], "alerts": { "resin": 160, "resinFull": true } })
        );
        assert_eq!(answer_json(&PresenceOutcome::Gone), json!({ "gone": true }));
    }
}
