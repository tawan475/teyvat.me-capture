// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Wish history sync to the tracker, as on the desktop
//! (`teyvatme_core::wish::sync`): a new key syncs every banner, a wish its
//! banner a little later, and only what the tracker does not have is sent.
//!
//! The tracker calls go through the app's sign-in (`Tracker.kt`, which holds
//! and refreshes the tokens): [`JavaTracker`] calls
//! `WishTrackerBridge.call(method, path, body)` over JNI. The app says whether
//! a sync may run ([`set_enabled`]): signed in, uploads on, wishes on.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use jni::JavaVM;
use jni::objects::{GlobalRef, JClass, JString, JValue};
use serde::Serialize;
use teyvatme_core::wish::Banner;
use teyvatme_core::wish::api::{ApiRecord, LogClient, Pacing};
use teyvatme_core::wish::hub::Trigger;
use teyvatme_core::wish::link::WishKey;
use teyvatme_core::wish::sync::{
    Cursor, Imported, PAGES_PER_RUN, Partial, Run, RunError, RunReport, Schedule, WishTracker,
    error_code, import_bodies, parse_cursor, parse_import,
};
use tokio::sync::{mpsc, watch};

use crate::wish::{RustlsGet, TokioSleeper};

/// Whole-request timeout for one page of the wish history.
const LOG_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The JVM and `me.teyvat.app.WishTrackerBridge`, kept by
/// `NativeBridge.init` (a thread of our own cannot find app classes).
static BRIDGE: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

/// Keep the JVM and the bridge class (from `NativeBridge.init`).
pub fn keep_bridge(vm: JavaVM, class: GlobalRef) {
    let _ = BRIDGE.set((vm, class));
}

/// What the wish card shows about the sync.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    /// `idle`, `syncing` or `off` (a key is held but the app does not sync).
    pub phase: &'static str,
    pub banner: Banner,
    pub page: u32,
    pub last: Option<LastSync>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSync {
    /// Unix ms.
    pub at: u64,
    /// `sent`, `upToDate`, `unfinished`, `keyExpired`, `otherAccount` or
    /// `failed`.
    pub outcome: &'static str,
    pub sent: usize,
    pub added: u64,
    /// For `failed`: what went wrong, in English.
    pub error: Option<String>,
}

/// The sync's handle: whether the app allows it, and what it shows.
pub struct WishSync {
    enabled_tx: watch::Sender<bool>,
    status: Arc<Mutex<SyncSummary>>,
}

impl WishSync {
    /// Start the sync thread over the hub's receivers.
    pub fn start(
        trigger_rx: mpsc::UnboundedReceiver<Trigger>,
        login_rx: watch::Receiver<u64>,
        key_tx: Arc<watch::Sender<Option<WishKey>>>,
    ) -> Self {
        let (enabled_tx, enabled_rx) = watch::channel(false);
        let status = Arc::new(Mutex::new(SyncSummary {
            phase: "idle",
            ..SyncSummary::default()
        }));
        let task = Task {
            trigger_rx,
            login_rx,
            key_tx,
            enabled_rx,
            status: Arc::clone(&status),
            schedule: Schedule::default(),
            partials: BTreeMap::new(),
        };
        let spawned = thread::Builder::new()
            .name("teyvatme-wish-sync".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(task.run()),
                    Err(e) => log::error!("wish sync: could not start: {e}"),
                }
            });
        if let Err(e) = spawned {
            log::error!("wish sync: could not start: {e}");
        }
        Self { enabled_tx, status }
    }

    /// Whether a captured key is synced (signed in, uploads and wishes on).
    pub fn set_enabled(&self, on: bool) {
        self.enabled_tx.send_if_modified(|enabled| {
            let changed = *enabled != on;
            *enabled = on;
            changed
        });
    }

    pub fn summary(&self) -> SyncSummary {
        self.status.lock().unwrap().clone()
    }
}

struct Task {
    trigger_rx: mpsc::UnboundedReceiver<Trigger>,
    login_rx: watch::Receiver<u64>,
    key_tx: Arc<watch::Sender<Option<WishKey>>>,
    enabled_rx: watch::Receiver<bool>,
    status: Arc<Mutex<SyncSummary>>,
    schedule: Schedule,
    partials: BTreeMap<Banner, Partial>,
}

impl Task {
    async fn run(mut self) {
        loop {
            let due = self.schedule.next_due();
            let wait = async {
                match due {
                    Some(at) => tokio::time::sleep_until(at.into()).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                trigger = self.trigger_rx.recv() => match trigger {
                    Some(trigger) => self.on_trigger(trigger, Instant::now()),
                    None => break,
                },
                changed = self.enabled_rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    // Signing in (or turning wishes on) while a key is held
                    // syncs it, like a new key.
                    let on = *self.enabled_rx.borrow_and_update();
                    if on {
                        self.on_trigger(Trigger::NewKey, Instant::now());
                    } else if self.status.lock().unwrap().phase == "idle" && self.has_key() {
                        self.status.lock().unwrap().phase = "off";
                    }
                }
                () = wait => self.run_due(Instant::now()).await,
            }
        }
    }

    fn fresh_key(&self, now: Instant) -> Option<WishKey> {
        self.key_tx
            .borrow()
            .as_ref()
            .filter(|key| key.is_fresh(now))
            .cloned()
    }

    fn has_key(&self) -> bool {
        self.fresh_key(Instant::now()).is_some()
    }

    fn on_trigger(&mut self, trigger: Trigger, now: Instant) {
        if trigger == Trigger::Cleared {
            self.clear();
            return;
        }
        // Only a key of this login whose UID is known syncs (the hub sends
        // `NewKey` again once the UID binds a waiting key).
        if self.fresh_key(now).is_none_or(|key| key.uid().is_none()) {
            return;
        }
        if !*self.enabled_rx.borrow() {
            log::info!("wish sync: not synced (no sign-in, or uploads or wishes off)");
            self.status.lock().unwrap().phase = "off";
            return;
        }
        {
            let mut status = self.status.lock().unwrap();
            if status.phase == "off" {
                status.phase = "idle";
            }
        }
        match trigger {
            Trigger::NewKey => self.schedule.on_new_key(now),
            Trigger::Wish(banner) => self.schedule.on_wish(now, banner),
            Trigger::Cleared => unreachable!("handled above"),
        }
    }

    /// A new login: nothing of the previous one's sync is kept.
    fn clear(&mut self) {
        self.schedule.clear();
        self.partials.clear();
        *self.status.lock().unwrap() = SyncSummary {
            phase: "idle",
            ..SyncSummary::default()
        };
    }

    async fn run_due(&mut self, now: Instant) {
        let banners = self.schedule.take_due(now);
        if banners.is_empty() {
            return;
        }
        let (Some(key), true) = (self.fresh_key(now), *self.enabled_rx.borrow()) else {
            self.schedule.clear();
            return;
        };
        let Some(uid) = key.uid() else {
            self.schedule.clear();
            return;
        };
        let http = match reqwest::Client::builder()
            .timeout(LOG_REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
        {
            Ok(http) => http,
            Err(e) => {
                log::warn!("wish sync: could not start the HTTP client: {e}");
                return;
            }
        };
        let mut log = LogClient::new(
            RustlsGet(http),
            key.endpoint().to_owned(),
            key.clone(),
            TokioSleeper,
            Pacing::default(),
        );
        log::info!("wish sync: syncing banners {banners:?}");
        let status = Arc::clone(&self.status);
        let progress = move |banner: Banner, page: u32| {
            let mut status = status.lock().unwrap();
            status.phase = "syncing";
            status.banner = banner;
            status.page = page;
        };
        // A new login while this runs stops it before it sends: the key, and
        // whatever was read with it, are the previous login's.
        let login = *self.login_rx.borrow();
        let mut login_rx = self.login_rx.clone();
        let new_login = async move {
            if login_rx.wait_for(|now| *now != login).await.is_err() {
                std::future::pending::<()>().await;
            }
        };
        let tracker = JavaTracker;
        let run = Run::new(&mut log, &tracker, &progress, PAGES_PER_RUN)
            .for_uid(uid.to_string())
            .sync(&banners, &mut self.partials);
        let report = tokio::select! {
            report = run => Some(report),
            () = new_login => None,
        };
        let Some(report) = report else {
            log::info!("wish sync: a new login; stopped before sending anything");
            self.clear();
            return;
        };
        log::info!(
            "wish sync finished: {} requests, sent {}, added {}, known {}, unfinished {:?}{}",
            log.requests(),
            report.sent,
            report.imported.added,
            report.imported.known,
            report.unfinished,
            report
                .error
                .as_ref()
                .map(|e| format!(", error: {e}"))
                .unwrap_or_default(),
        );
        self.finish(&key, report, Instant::now());
    }

    /// Forget `key`, unless a newer key arrived during the run.
    fn drop_key(&self, key: &WishKey) {
        self.key_tx.send_if_modified(|current| {
            if current
                .as_ref()
                .is_some_and(|current| current.same_key(key))
            {
                *current = None;
                return true;
            }
            false
        });
    }

    fn finish(&mut self, key: &WishKey, report: RunReport, now: Instant) {
        let mut error = None;
        let outcome = match &report.error {
            Some(RunError::KeyExpired) => {
                self.drop_key(key);
                "keyExpired"
            }
            Some(RunError::OtherAccount) => {
                self.drop_key(key);
                "otherAccount"
            }
            Some(other) => {
                error = Some(other.to_string());
                "failed"
            }
            None if !report.unfinished.is_empty() => {
                self.schedule.on_unfinished(now, &report.unfinished);
                "unfinished"
            }
            None if report.sent > 0 => "sent",
            None => "upToDate",
        };
        // The app notes "N new wishes sent" in its own language.
        *self.status.lock().unwrap() = SyncSummary {
            phase: "idle",
            banner: 0,
            page: 0,
            last: Some(LastSync {
                at: crate::decoder::now_ms(),
                outcome,
                sent: report.sent,
                added: report.imported.added,
                error,
            }),
        };
    }
}

/// The tracker's wish routes through the app's sign-in.
struct JavaTracker;

impl WishTracker for JavaTracker {
    type Error = String;

    async fn cursor(&self, uid: &str) -> Result<Cursor, String> {
        let body = call("GET", &format!("/wishes/cursor?uid={uid}"), "")?;
        parse_cursor(&body).map_err(|e| format!("the tracker's wish cursor was unreadable: {e:?}"))
    }

    async fn import(
        &self,
        uid: &str,
        region: &str,
        records: &[ApiRecord],
    ) -> Result<Imported, String> {
        let mut total = Imported::default();
        for body in import_bodies(uid, region, records) {
            let body = serde_json::to_string(&body).map_err(|e| e.to_string())?;
            let answer = call("POST", "/wishes/import", &body)?;
            let answer = parse_import(&answer)
                .map_err(|e| format!("the tracker's import answer was unreadable: {e}"))?;
            total.added += answer.added;
            total.known += answer.known;
        }
        Ok(total)
    }
}

/// One tracker request through `WishTrackerBridge.call`: the body of a
/// success, else what went wrong. Blocks (the sync thread's own runtime).
fn call(method: &str, path: &str, body: &str) -> Result<String, String> {
    let (vm, class) = BRIDGE
        .get()
        .ok_or_else(|| "the app's tracker is not ready".to_owned())?;
    let mut env = vm
        .attach_current_thread_permanently()
        .map_err(|e| format!("could not reach the app: {e}"))?;
    let answer = (|| -> jni::errors::Result<String> {
        let class: &JClass = class.as_obj().into();
        let method = env.new_string(method)?;
        let path = env.new_string(path)?;
        let body = env.new_string(body)?;
        let out = env
            .call_static_method(
                class,
                "call",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                &[
                    JValue::Object(&method),
                    JValue::Object(&path),
                    JValue::Object(&body),
                ],
            )?
            .l()?;
        Ok(env.get_string(&JString::from(out))?.into())
    })();
    let answer = match answer {
        Ok(answer) => answer,
        Err(e) => {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            return Err(format!("could not reach the app: {e}"));
        }
    };
    // "<status>\n<body>": 0 = unreachable (the body says why), -1 = signed out.
    let (status, body) = answer.split_once('\n').unwrap_or((&answer, ""));
    match status.parse::<i32>().unwrap_or(0) {
        200..=299 => Ok(body.to_owned()),
        -1 => Err("tracker: signed out; sign in again".to_owned()),
        0 => Err(format!("could not reach the tracker: {body}")),
        status => {
            let (code, message) = error_code(body);
            Err(match (code.is_empty(), message.is_empty()) {
                (true, _) => format!("tracker: HTTP {status}"),
                (false, true) => format!("tracker: HTTP {status} {code}"),
                (false, false) => format!("tracker: HTTP {status} {code} ({message})"),
            })
        }
    }
}
