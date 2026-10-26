// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The tracker's wish routes (`/wishes/cursor`, `/wishes/import`) and the
//! credential they are called with.

use std::fmt;

pub(crate) use teyvatme_core::wish::sync::{Cursor, Imported};
use teyvatme_core::wish::sync::{CursorError, WishTracker, error_code, import_bodies};

use super::gacha_log::ApiRecord;
use crate::i18n::{tr, trf};
use crate::signin::{AuthError, TrackerAuth};

/// How Teyvat.me proves who it uploads for: the same choice as a snapshot
/// upload (`app::upload_credential`). Both routes accept either kind and
/// resolve it to an owner exactly like a snapshot upload does.
#[derive(Clone)]
pub(crate) enum TrackerCredential {
    /// The import key from Settings (`gdt_import_…` / `gdt_uk_…`), sent as
    /// `x-import-key`. Used only while not signed in.
    ImportKey(String),
    /// The tracker sign-in: a bearer token from [`TrackerAuth`], which
    /// refreshes it (under its own lock) and retries once on a 401, and ends
    /// the sign-in on `session_revoked`. `base` is the address the token must
    /// belong to.
    SignedIn { auth: TrackerAuth, base: String },
}

impl fmt::Debug for TrackerCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImportKey(_) => f.write_str("ImportKey(<redacted>)"),
            Self::SignedIn { base, .. } => write!(f, "SignedIn({base})"),
        }
    }
}

/// Talks to one tracker with one credential.
pub(crate) struct Tracker {
    pub(crate) http: reqwest::Client,
    /// The API base (`SavedAppState.tracker_api_url`).
    pub(crate) base_url: String,
    pub(crate) credential: TrackerCredential,
}

impl Tracker {
    fn url(&self, path: &str) -> String {
        format!("{}/wishes/{path}", self.base_url.trim_end_matches('/'))
    }

    /// Send the request `build` makes with the credential; the body of a
    /// success, else what to tell the user. `build` may run twice (a
    /// signed-in retry after a refresh).
    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<String, String> {
        let response =
            match &self.credential {
                TrackerCredential::ImportKey(key) => build()
                    .header("x-import-key", key)
                    .send()
                    .await
                    .map_err(|e| trf("could not reach the tracker: {0}", &[&e.without_url()]))?,
                TrackerCredential::SignedIn { auth, base } => auth
                    .send_signed_in(&self.http, base, |token| build().bearer_auth(token))
                    .await
                    .map_err(|error| match error {
                        AuthError::SignedOut => describe_failure(401, "session_revoked", ""),
                        AuthError::Http { status, body } => failure(status.as_u16(), &body),
                        other => trf("tracker: {0}", &[&other]),
                    })?,
            };
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            return Ok(body);
        }
        Err(failure(status.as_u16(), &body))
    }
}

impl WishTracker for Tracker {
    type Error = String;

    async fn cursor(&self, uid: &str) -> Result<Cursor, String> {
        let url = self.url("cursor");
        let body = self
            .send(|| self.http.get(&url).query(&[("uid", uid)]))
            .await?;
        teyvatme_core::wish::sync::parse_cursor(&body).map_err(|error| match error {
            CursorError::Unreadable(e) => {
                trf("the tracker's wish cursor was unreadable: {0}", &[&e])
            }
            CursorError::BadId(banner) => trf(
                "the tracker's wish cursor holds a bad id for {0}",
                &[&banner],
            ),
        })
    }

    async fn import(
        &self,
        uid: &str,
        region: &str,
        records: &[ApiRecord],
    ) -> Result<Imported, String> {
        let mut total = Imported::default();
        for body in import_bodies(uid, region, records) {
            let url = self.url("import");
            let answer = self.send(|| self.http.post(&url).json(&body)).await?;
            let answer = teyvatme_core::wish::sync::parse_import(&answer)
                .map_err(|e| trf("the tracker's import answer was unreadable: {0}", &[&e]))?;
            total.added += answer.added;
            total.known += answer.known;
        }
        Ok(total)
    }
}

/// What to say about a refused tracker request.
fn failure(status: u16, body: &str) -> String {
    let (code, message) = error_code(body);
    describe_failure(status, &code, &message)
}

/// The status line's words for the tracker's error codes (the ones
/// `/irminsul/import` uses, which the wish routes share), else the code and
/// the tracker's own sentence.
#[rustfmt::skip]
fn describe_failure(status: u16, code: &str, message: &str) -> String {
    let known = match code {
        // Only after the refresh and retry did not help.
        "token_expired" => tr("the sign-in expired; sign in again"),
        "session_revoked" => tr("signed out of the tracker; sign in again"),
        "upload_target_missing" => {
            tr("the upload target no longer exists; choose one with Configure on the tracker")
        }
        "uid_mismatch" => tr("the tracker account uploads go to has another UID"),
        "uploads_blocked" => tr("uploads are blocked for this tracker account"),
        "account_suspended" => tr("this tracker account is suspended"),
        "rate_limited" => tr("the tracker is busy; the next key or wish tries again"),
        "daily_upload_limit" => tr("today's upload limit on the tracker is reached"),
        "storage_quota" => tr("the tracker storage is full"),
        _ => "",
    };
    if !known.is_empty() {
        return trf("tracker: {0}", &[&known]);
    }
    match (code.is_empty(), message.is_empty()) {
        (true, _) if status == 429 => {
            tr("tracker: busy (HTTP 429); the next key or wish tries again").to_owned()
        }
        (true, _) => trf("tracker: HTTP {0}", &[&status]),
        (false, true) => trf("tracker: HTTP {0} {1}", &[&status, &code]),
        (false, false) => trf("tracker: HTTP {0} {1} ({2})", &[&status, &code, &truncate(message, 160)]),
    }
}

fn truncate(text: &str, max: usize) -> &str {
    match text.char_indices().nth(max) {
        Some((at, _)) => &text[..at],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_credential_never_shows_in_debug_output() {
        let rendered = format!("{:?}", TrackerCredential::ImportKey("gdt_uk_secret".into()));
        assert!(!rendered.contains("secret"), "{rendered}");
    }

    #[test]
    fn the_trackers_error_codes_read_as_sentences() {
        let says = |status, code: &str| {
            failure(
                status,
                &serde_json::json!({ "error": { "code": code } }).to_string(),
            )
        };
        assert_eq!(
            says(409, "upload_target_missing"),
            "tracker: the upload target no longer exists; choose one with Configure on the tracker"
        );
        assert_eq!(
            says(409, "uid_mismatch"),
            "tracker: the tracker account uploads go to has another UID"
        );
        assert_eq!(
            says(403, "uploads_blocked"),
            "tracker: uploads are blocked for this tracker account"
        );
        assert_eq!(
            says(403, "account_suspended"),
            "tracker: this tracker account is suspended"
        );
        assert_eq!(
            says(429, "rate_limited"),
            "tracker: the tracker is busy; the next key or wish tries again"
        );
        assert_eq!(
            says(401, "session_revoked"),
            "tracker: signed out of the tracker; sign in again"
        );
        assert_eq!(
            says(401, "token_expired"),
            "tracker: the sign-in expired; sign in again"
        );
        assert_eq!(
            failure(429, "busy"),
            "tracker: busy (HTTP 429); the next key or wish tries again"
        );
        assert_eq!(failure(404, "<html>"), "tracker: HTTP 404");
        assert_eq!(
            failure(
                400,
                r#"{"error":{"code":"bad_records","message":"No records"}}"#
            ),
            "tracker: HTTP 400 bad_records (No records)"
        );
    }

    #[test]
    fn long_error_messages_are_cut_on_a_character_boundary() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("aéc", 2), "aé");
    }
}
