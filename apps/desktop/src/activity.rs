// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! What the app did, for the footer's activity line, its popover, and the
//! toasts.
//!
//! Every background report (an auto-export written, an upload stored or
//! refused, live sync's batches, wishes sent) and every toast the UI raises
//! is an [`Event`]. Routine background work goes only to the activity line;
//! what the user just did, and every problem, is also a toast (spec §8).

use std::collections::VecDeque;
use std::path::PathBuf;

use chrono::{DateTime, Local};

/// How many events the popover keeps.
pub const KEPT: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ok,
    Info,
    Warn,
    Error,
}

/// What an event is about, so a card can show its latest one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    App,
    Capture,
    Export,
    Destination,
    Wishes,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub at: DateTime<Local>,
    pub kind: Kind,
    pub topic: Topic,
    pub text: String,
    /// Also raise a toast.
    pub toast: bool,
    /// A file it wrote (the auto-export), for "Show" and the Export row.
    pub path: Option<PathBuf>,
}

impl Event {
    fn new(kind: Kind, topic: Topic, text: impl Into<String>) -> Self {
        Self {
            at: Local::now(),
            kind,
            topic,
            text: text.into(),
            // Problems always reach the user.
            toast: matches!(kind, Kind::Error | Kind::Warn),
            path: None,
        }
    }

    /// Routine success: the activity line only.
    pub fn ok(topic: Topic, text: impl Into<String>) -> Self {
        Self::new(Kind::Ok, topic, text)
    }

    pub fn info(topic: Topic, text: impl Into<String>) -> Self {
        Self::new(Kind::Info, topic, text)
    }

    pub fn warn(topic: Topic, text: impl Into<String>) -> Self {
        Self::new(Kind::Warn, topic, text)
    }

    pub fn error(topic: Topic, text: impl Into<String>) -> Self {
        Self::new(Kind::Error, topic, text)
    }

    /// Raise a toast as well.
    pub fn toast(mut self) -> Self {
        self.toast = true;
        self
    }

    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    /// "21:06 · <text>".
    pub fn line(&self) -> String {
        format!("{} · {}", self.at.format("%H:%M"), self.text)
    }
}

/// Where the background sends its events.
pub type Sender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type Receiver = tokio::sync::mpsc::UnboundedReceiver<Event>;

/// The last [`KEPT`] events, newest first.
#[derive(Default)]
pub struct Log {
    events: VecDeque<Event>,
}

impl Log {
    pub fn push(&mut self, event: Event) {
        if self.events.len() == KEPT {
            self.events.pop_back();
        }
        self.events.push_front(event);
    }

    pub fn latest(&self) -> Option<&Event> {
        self.events.front()
    }

    /// The newest event about `topic`.
    pub fn latest_of(&self, topic: Topic) -> Option<&Event> {
        self.events.iter().find(|event| event.topic == topic)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_are_toasted_and_routine_work_is_not() {
        assert!(Event::error(Topic::Export, "x").toast);
        assert!(Event::warn(Topic::Export, "x").toast);
        assert!(!Event::ok(Topic::Export, "x").toast);
        assert!(Event::ok(Topic::Export, "x").toast().toast);
    }

    #[test]
    fn the_log_keeps_the_newest_fifty() {
        let mut log = Log::default();
        for i in 0..60 {
            log.push(Event::info(Topic::App, i.to_string()));
        }
        assert_eq!(log.iter().count(), KEPT);
        assert_eq!(log.latest().unwrap().text, "59");
        assert_eq!(log.iter().last().unwrap().text, "10");
    }

    #[test]
    fn the_latest_of_a_topic_skips_newer_events_of_others() {
        let mut log = Log::default();
        log.push(Event::ok(Topic::Export, "saved"));
        log.push(Event::ok(Topic::Destination, "stored"));
        assert_eq!(log.latest_of(Topic::Export).unwrap().text, "saved");
        assert!(log.latest_of(Topic::Wishes).is_none());
    }
}
