// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Wish history on the phone: the key the decoder's [`WishHub`] holds (the
//! game sends it when its Wish → History page opens), the game's own link for
//! Copy URL, and reading every banner into a UIGF v4 file for Save or Share
//! (`teyvatme_core::wish`).
//!
//! The key is a password while it lasts: memory only, never logged, and
//! only ever sent to the game's API. A new login clears it (the hub), and a
//! read in flight stops when the login changes.
//!
//! Syncing the history to the tracker is [`crate::wish_sync`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use teyvatme_core::wish::api::{HttpError, HttpGet, HttpReply, LogClient, Pacing, Sleeper};
use teyvatme_core::wish::history::{self, HistoryError, Progress};
use teyvatme_core::wish::hub::{HubReceivers, WishHub};
use teyvatme_core::wish::link::{KEY_LIFETIME, WishKey};
use teyvatme_core::wish::uigf;
use tokio::sync::watch;
use url::Url;

/// How long one API request may take (the desktop's).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The phone's HTTP client for the API: reqwest with rustls (Android has no
/// OpenSSL), outside the VPN (the app is not in its allowed list).
pub(crate) struct RustlsGet(pub(crate) reqwest::Client);

impl HttpGet for RustlsGet {
    async fn get(&self, url: &Url) -> Result<HttpReply, HttpError> {
        let response = self
            .0
            .get(url.clone())
            .send()
            .await
            .map_err(|error| HttpError::Unreachable(error.without_url().to_string()))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| HttpError::CutOff(error.without_url().to_string()))?;
        Ok(HttpReply {
            status: status.as_u16(),
            status_text: status.to_string(),
            body: body.to_vec(),
        })
    }
}

pub(crate) struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(duration)
    }
}

/// Where reading the history is, for the wish card.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Read {
    #[default]
    Idle,
    Running {
        banner: u32,
        page: u32,
        records: usize,
    },
    /// The UIGF file is ready ([`Wishes::take`]).
    Done {
        records: usize,
        /// Unix ms.
        at: u64,
    },
    Failed {
        error: String,
    },
}

/// What the wish card shows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WishSummary {
    /// `none`, `ready` or `expired`.
    pub key: &'static str,
    /// The key's login UID is known (reading needs it).
    pub uid_known: bool,
    /// When the held key arrived (Unix ms), for "URL ready · HH:mm".
    pub key_at: Option<u64>,
    pub read: Read,
    pub sync: crate::wish_sync::SyncSummary,
}

/// Why a read did not start.
#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    NoKey,
    NoUid,
    Running,
}

/// The phone's wish history: the hub's key and the history reader.
pub struct Wishes {
    hub: WishHub,
    key_rx: watch::Receiver<Option<WishKey>>,
    login_rx: watch::Receiver<u64>,
    read: Arc<Mutex<Read>>,
    /// The last finished file, until the next read.
    file: Arc<Mutex<Option<String>>>,
    cancel: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    pub sync: crate::wish_sync::WishSync,
}

impl Wishes {
    pub fn new() -> Self {
        // The phone polls the summary; nothing to repaint.
        let (
            hub,
            HubReceivers {
                key_rx,
                login_rx,
                trigger_rx,
                ..
            },
        ) = WishHub::new(Arc::new(|| {}));
        let sync =
            crate::wish_sync::WishSync::start(trigger_rx, login_rx.clone(), hub.key_sender());
        Self {
            sync,
            hub,
            key_rx,
            login_rx,
            read: Arc::default(),
            file: Arc::default(),
            cancel: Arc::default(),
        }
    }

    pub fn hub(&self) -> &WishHub {
        &self.hub
    }

    fn key(&self, now: Instant) -> Option<WishKey> {
        self.key_rx
            .borrow()
            .as_ref()
            .filter(|key| key.is_fresh(now))
            .cloned()
    }

    pub fn summary(&self) -> WishSummary {
        let now = Instant::now();
        let held = self.key_rx.borrow().clone();
        let key = match &held {
            None => "none",
            Some(key) if key.is_fresh(now) => "ready",
            Some(_) => "expired",
        };
        WishSummary {
            key,
            uid_known: held.as_ref().and_then(|key| key.uid()).is_some(),
            key_at: held.as_ref().map(|key| {
                let age = KEY_LIFETIME.saturating_sub(key.remaining(now));
                crate::decoder::now_ms().saturating_sub(age.as_millis() as u64)
            }),
            read: self.read.lock().unwrap().clone(),
            sync: self.sync.summary(),
        }
    }

    /// The game's own wish history link for this login's key.
    pub fn url(&self) -> Option<String> {
        self.key(Instant::now()).map(|key| key.wish_url())
    }

    /// Start reading every banner into a UIGF file, on a thread of its own.
    pub fn start(&self, app_version: &str) -> Result<(), StartError> {
        let key = self.key(Instant::now()).ok_or(StartError::NoKey)?;
        let uid = key.uid().ok_or(StartError::NoUid)?;
        {
            let mut read = self.read.lock().unwrap();
            if matches!(*read, Read::Running { .. }) {
                return Err(StartError::Running);
            }
            *read = Read::Running {
                banner: teyvatme_core::wish::hub::BANNERS[0],
                page: 0,
                records: 0,
            };
        }
        *self.file.lock().unwrap() = None;
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().unwrap() = Some(Arc::clone(&cancel));

        let read = Arc::clone(&self.read);
        let file = Arc::clone(&self.file);
        let mut login_rx = self.login_rx.clone();
        login_rx.mark_unchanged();
        let app_version = app_version.to_owned();
        let spawned = thread::Builder::new()
            .name("teyvatme-wishes".into())
            .spawn(move || {
                let outcome = read_history(key, uid, &read, &cancel, &login_rx, &app_version);
                let mut state = read.lock().unwrap();
                *state = match outcome {
                    Ok((json, records)) => {
                        *file.lock().unwrap() = Some(json);
                        log::info!("wish history: read {records} wishes");
                        Read::Done {
                            records,
                            at: crate::decoder::now_ms(),
                        }
                    }
                    Err(HistoryError::Cancelled) => {
                        log::info!("wish history: read stopped");
                        Read::Idle
                    }
                    Err(error) => {
                        log::warn!("wish history: read failed: {error}");
                        Read::Failed {
                            error: error.to_string(),
                        }
                    }
                };
            });
        if let Err(e) = spawned {
            *self.read.lock().unwrap() = Read::Failed {
                error: format!("could not start: {e}"),
            };
        }
        Ok(())
    }

    pub fn cancel(&self) {
        if let Some(cancel) = self.cancel.lock().unwrap().as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The finished UIGF file (kept for another Save or Share).
    pub fn take(&self) -> Option<String> {
        self.file.lock().unwrap().clone()
    }
}

/// Read every banner with `key` and write the UIGF file: `(json, records)`.
fn read_history(
    key: WishKey,
    uid: u32,
    read: &Mutex<Read>,
    cancel: &AtomicBool,
    login_rx: &watch::Receiver<u64>,
    app_version: &str,
) -> Result<(String, usize), HistoryError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| failed(format!("could not start: {e}")))?;
    let http = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| failed(format!("could not start the HTTP client: {e}")))?;
    let endpoint = key.endpoint().to_owned();
    let mut log = LogClient::new(
        RustlsGet(http),
        endpoint,
        key,
        TokioSleeper,
        Pacing::default(),
    );
    log::info!("wish history: reading every banner");
    let history = runtime.block_on(history::read_all(&mut log, uid, |progress: Progress| {
        *read.lock().unwrap() = Read::Running {
            banner: progress.banner,
            page: progress.page,
            records: progress.records,
        };
        // A new login (or another account) ends the key's read.
        !cancel.load(Ordering::Relaxed) && !login_rx.has_changed().unwrap_or(true)
    }))?;
    let exported = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let records = history.records.len();
    Ok((
        uigf::to_uigf(&history, exported, "Teyvat.me Capture", app_version),
        records,
    ))
}

fn failed(why: String) -> HistoryError {
    HistoryError::Fetch(teyvatme_core::wish::api::FetchError::Failed(why))
}
