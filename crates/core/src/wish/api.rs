// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The game's wish history API (`getGachaLog`): one page at a time, paced
//! and with back-off, through the HTTP client the app brings ([`HttpGet`]).
//!
//! Every URL here carries the key: none is logged unredacted, and an
//! [`HttpGet`] reports errors without their URL.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;
use url::Url;

use super::Banner;
use super::link::{PAGE_SIZE, WishKey, redact_url};

/// The `message` of the answer to an expired key.
const AUTHKEY_TIMEOUT: &str = "authkey timeout";

/// The retcode of an expired key (with [`AUTHKEY_TIMEOUT`]).
const RETCODE_AUTHKEY_TIMEOUT: i64 = -101;

/// The retcode of "visit too frequently".
const RETCODE_TOO_FREQUENT: i64 = -110;

/// One answer of the HTTP client an app brings.
#[derive(Clone, Debug)]
pub struct HttpReply {
    pub status: u16,
    /// The status as the client prints it (`503 Service Unavailable`).
    pub status_text: String,
    pub body: Vec<u8>,
}

/// Why the HTTP client got no whole answer. The text must carry no URL.
#[derive(Clone, Debug)]
pub enum HttpError {
    /// No answer at all (connect, TLS, timeout).
    Unreachable(String),
    /// The body broke off.
    CutOff(String),
}

/// A GET of the API, by the app's own HTTP client: the desktop's reqwest
/// with the system TLS, the Android app's with rustls.
pub trait HttpGet {
    fn get(&self, url: &Url) -> impl Future<Output = Result<HttpReply, HttpError>> + Send;
}

/// One wish as the API returns it. Every field is a string on the wire.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ApiRecord {
    pub uid: String,
    pub gacha_type: String,
    #[serde(default)]
    pub item_id: String,
    pub time: String,
    pub name: String,
    pub item_type: String,
    pub rank_type: String,
    pub id: String,
}

#[derive(Deserialize)]
struct Answer {
    retcode: i64,
    #[serde(default)]
    message: String,
    data: Option<AnswerData>,
}

#[derive(Deserialize)]
struct AnswerData {
    #[serde(default)]
    region: String,
    #[serde(default)]
    list: Vec<ApiRecord>,
}

/// One page of a banner's history, newest first.
#[derive(Debug)]
pub struct LogPage {
    pub region: String,
    pub records: Vec<ApiRecord>,
}

/// Why a page could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// "authkey timeout": the key no longer works.
    KeyExpired,
    /// Still throttled after every back-off.
    Throttled,
    /// Network or HTTP trouble, retried and still failing; or an answer that
    /// is not the API's. The text carries no URL.
    Failed(String),
    /// The API refused the request (a retcode other than the above).
    Refused { retcode: i64, message: String },
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyExpired => write!(f, "the wish key expired"),
            Self::Throttled => write!(f, "the wish history API is busy (too many requests)"),
            Self::Failed(why) => write!(f, "{why}"),
            Self::Refused { retcode, message } => {
                write!(f, "the wish history API refused: {message} ({retcode})")
            }
        }
    }
}

/// What one attempt came to: done, or worth another try after a back-off.
enum Attempt {
    Done(Result<LogPage, FetchError>),
    Retry(FetchError),
}

/// Waits, so tests can see the pacing without spending it.
pub trait Sleeper {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// The waits between requests.
#[derive(Clone, Debug)]
pub struct Pacing {
    /// Between two requests (pages of one banner).
    pub request: Duration,
    /// Before the first request of the next banner.
    pub banner: Duration,
    /// Before each retry of a throttled or failed request; its length is the
    /// number of retries.
    pub backoff: Vec<Duration>,
}

impl Default for Pacing {
    /// What paimon.moe and the other importers do (1 s per page, 2 s per
    /// banner), with a back-off that gives a throttle time to lift.
    fn default() -> Self {
        Self {
            request: Duration::from_secs(1),
            banner: Duration::from_secs(2),
            backoff: vec![
                Duration::from_secs(5),
                Duration::from_secs(15),
                Duration::from_secs(45),
            ],
        }
    }
}

/// Reads pages of the wish history with one key, keeping to [`Pacing`].
pub struct LogClient<H, S> {
    http: H,
    endpoint: String,
    key: WishKey,
    sleeper: S,
    pacing: Pacing,
    /// The wait owed before the next request; `None` before the first one.
    owed: Option<Duration>,
    requests: u32,
}

impl<H: HttpGet, S: Sleeper> LogClient<H, S> {
    pub fn new(http: H, endpoint: String, key: WishKey, sleeper: S, pacing: Pacing) -> Self {
        Self {
            http,
            endpoint,
            key,
            sleeper,
            pacing,
            owed: None,
            requests: 0,
        }
    }

    /// Requests sent so far, retries included.
    pub fn requests(&self) -> u32 {
        self.requests
    }

    /// The next request starts a new banner: wait the banner gap before it.
    pub fn next_banner(&mut self) {
        if self.owed.is_some() {
            self.owed = Some(self.pacing.banner.max(self.pacing.request));
        }
    }

    /// One page of `banner`, the one after the record `end_id` (0 = newest).
    pub async fn page(
        &mut self,
        banner: Banner,
        page: u32,
        end_id: u64,
    ) -> Result<LogPage, FetchError> {
        let banner = banner.to_string();
        let size = PAGE_SIZE.to_string();
        let page = page.to_string();
        let end_id = end_id.to_string();
        let url = self.key.url_at(
            &self.endpoint,
            &[
                ("gacha_type", &banner),
                ("size", &size),
                ("page", &page),
                ("end_id", &end_id),
            ],
        );

        let mut retries = self.pacing.backoff.clone().into_iter();
        loop {
            if let Some(wait) = self.owed.take() {
                self.sleeper.sleep(wait).await;
            }
            self.requests += 1;
            let attempt = self.attempt(url.clone()).await;
            self.owed = Some(self.pacing.request);
            match attempt {
                Attempt::Done(result) => return result,
                Attempt::Retry(error) => {
                    let Some(backoff) = retries.next() else {
                        return Err(error);
                    };
                    tracing::info!(
                        banner = %banner,
                        page = %page,
                        "wish history request failed ({error}); retrying in {}s",
                        backoff.as_secs()
                    );
                    self.owed = Some(backoff.max(self.pacing.request));
                }
            }
        }
    }

    async fn attempt(&self, url: Url) -> Attempt {
        let reply = match self.http.get(&url).await {
            Ok(reply) => reply,
            Err(HttpError::Unreachable(error)) => {
                tracing::debug!("request to {} failed: {error}", redact_url(url.as_str()));
                return Attempt::Retry(FetchError::Failed(format!(
                    "could not reach the wish history API: {error}"
                )));
            }
            Err(HttpError::CutOff(error)) => {
                return Attempt::Retry(FetchError::Failed(format!(
                    "the wish history answer was cut off: {error}"
                )));
            }
        };
        if reply.status == 429 {
            return Attempt::Retry(FetchError::Throttled);
        }
        if (500..600).contains(&reply.status) {
            return Attempt::Retry(FetchError::Failed(format!(
                "the wish history API answered HTTP {}",
                reply.status_text
            )));
        }
        if !(200..300).contains(&reply.status) {
            return Attempt::Done(Err(FetchError::Failed(format!(
                "the wish history API answered HTTP {}",
                reply.status_text
            ))));
        }
        match read_answer(&reply.body) {
            // "Visit too frequently" is a throttle like HTTP 429.
            Err(FetchError::Throttled) => Attempt::Retry(FetchError::Throttled),
            other => Attempt::Done(other),
        }
    }
}

/// Read one answer of the API. "Slow down" comes back as
/// [`FetchError::Throttled`], which [`LogClient::page`] retries.
pub fn read_answer(body: &[u8]) -> Result<LogPage, FetchError> {
    let answer: Answer = serde_json::from_slice(body).map_err(|error| {
        FetchError::Failed(format!(
            "the wish history API sent something unexpected: {error}"
        ))
    })?;
    if answer.retcode != 0 {
        if answer.retcode == RETCODE_AUTHKEY_TIMEOUT || answer.message == AUTHKEY_TIMEOUT {
            return Err(FetchError::KeyExpired);
        }
        if answer.retcode == RETCODE_TOO_FREQUENT {
            return Err(FetchError::Throttled);
        }
        return Err(FetchError::Refused {
            retcode: answer.retcode,
            message: answer.message,
        });
    }
    let data = answer.data.ok_or_else(|| {
        FetchError::Failed("the wish history API answered without data".to_owned())
    })?;
    Ok(LogPage {
        region: data.region,
        records: data.list,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read() {
        let page = read_answer(
            br#"{"retcode":0,"message":"OK","data":{"page":"1","size":"20","region":"os_asia",
               "list":[{"uid":"811111111","gacha_type":"400","item_id":"","count":"1",
               "time":"2026-10-12 21:04:11","name":"Some Character","lang":"en-us",
               "item_type":"Character","rank_type":"5","id":"1790000000000000002"}]}}"#,
        )
        .unwrap();
        assert_eq!(page.region, "os_asia");
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].gacha_type, "400");

        assert_eq!(
            read_answer(br#"{"retcode":-101,"message":"authkey timeout","data":null}"#)
                .unwrap_err(),
            FetchError::KeyExpired
        );
        assert_eq!(
            read_answer(br#"{"retcode":-110,"message":"visit too frequently","data":null}"#)
                .unwrap_err(),
            FetchError::Throttled
        );
        assert!(matches!(
            read_answer(br#"{"retcode":-100,"message":"authkey error","data":null}"#),
            Err(FetchError::Refused { retcode: -100, .. })
        ));
        assert!(matches!(read_answer(b"<html>"), Err(FetchError::Failed(_))));
    }
}
