// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! "Save history": this login's whole wish history as a UIGF file, read on a
//! thread of its own through the shared client (`teyvatme_core::wish`), the
//! same way the Android app does. Nothing goes anywhere but the game's own
//! wish API and the file the user picked.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use teyvatme_core::wish::api::{FetchError, LogClient, Pacing};
use teyvatme_core::wish::history::{self, HistoryError, Progress};
use teyvatme_core::wish::uigf;
use tokio::sync::watch;

use super::Banner;
use super::gacha_log::{ReqwestGet, TokioSleeper, WishKey};
use crate::i18n::{tr, trf};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a save stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum SaveState {
    #[default]
    Idle,
    Reading {
        banner: Banner,
        page: u32,
        records: usize,
    },
    Done {
        path: PathBuf,
        records: usize,
    },
    Failed(String),
}

/// The banner's name, as the game's wish screen says it.
pub(crate) fn banner_name(banner: Banner) -> &'static str {
    match banner {
        301 => tr("Character event"),
        302 => tr("Weapon event"),
        500 => tr("Chronicled"),
        200 => tr("Standard"),
        100 => tr("Beginners'"),
        _ => tr("Wishes"),
    }
}

impl SaveState {
    /// The wish card's line for it.
    #[rustfmt::skip]
    pub(crate) fn line(&self) -> Option<String> {
        match self {
            SaveState::Idle => None,
            SaveState::Reading {
                banner,
                page,
                records,
            } => Some(trf("Reading · {0}, page {1} · {2} wishes so far", &[&banner_name(*banner), &(page + 1), &records])),
            SaveState::Done { records, .. } => Some(trf("Saved {0} wishes", &[&records])),
            SaveState::Failed(why) => Some(trf("Save failed · {0}", &[&why])),
        }
    }
}

/// One save at a time; the UI polls [`HistorySave::state`].
pub(crate) struct HistorySave {
    state: Arc<Mutex<SaveState>>,
    cancel: Arc<AtomicBool>,
    /// A new login (or another account) stops a read under the old key.
    login_rx: watch::Receiver<u64>,
}

impl HistorySave {
    pub(crate) fn new(login_rx: watch::Receiver<u64>) -> Self {
        Self {
            state: Arc::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            login_rx,
        }
    }

    pub(crate) fn state(&self) -> SaveState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn reading(&self) -> bool {
        matches!(self.state(), SaveState::Reading { .. })
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Forget a finished or failed save (the card goes back to its line).
    pub(crate) fn clear(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(*state, SaveState::Reading { .. }) {
            *state = SaveState::Idle;
        }
    }

    /// Read every banner with `key` and write it to `path`.
    pub(crate) fn start(&self, key: WishKey, path: PathBuf, ctx: egui::Context) {
        let Some(uid) = key.uid() else {
            self.set(SaveState::Failed(
                tr("the account isn't known yet").to_owned(),
            ));
            return;
        };
        if self.reading() {
            return;
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.set(SaveState::Reading {
            banner: teyvatme_core::wish::hub::BANNERS[0],
            page: 0,
            records: 0,
        });
        let state = Arc::clone(&self.state);
        let cancel = Arc::clone(&self.cancel);
        let mut login_rx = self.login_rx.clone();
        login_rx.mark_unchanged();
        let spawned = std::thread::Builder::new()
            .name("teyvatme-wish-save".into())
            .spawn(move || {
                let outcome = read_and_write(key, uid, &path, &state, &cancel, &login_rx, &ctx);
                let next = match outcome {
                    Ok(records) => {
                        tracing::info!(records, "wish history saved");
                        SaveState::Done { path, records }
                    }
                    Err(HistoryError::Cancelled) => SaveState::Idle,
                    Err(error) => {
                        tracing::warn!("wish history save failed: {error}");
                        SaveState::Failed(error.to_string())
                    }
                };
                *state.lock().unwrap_or_else(|e| e.into_inner()) = next;
                ctx.request_repaint();
            });
        if let Err(e) = spawned {
            self.set(SaveState::Failed(trf("could not start: {0}", &[&e])));
        }
    }

    fn set(&self, next: SaveState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = next;
    }
}

fn read_and_write(
    key: WishKey,
    uid: u32,
    path: &std::path::Path,
    state: &Mutex<SaveState>,
    cancel: &AtomicBool,
    login_rx: &watch::Receiver<u64>,
    ctx: &egui::Context,
) -> Result<usize, HistoryError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| failed(trf("could not start: {0}", &[&e])))?;
    let http = crate::http_client_builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| failed(trf("could not start the HTTP client: {0}", &[&e])))?;
    let endpoint = key.endpoint().to_owned();
    let mut log = LogClient::new(
        ReqwestGet(http),
        endpoint,
        key,
        TokioSleeper,
        Pacing::default(),
    );
    let history = runtime.block_on(history::read_all(&mut log, uid, |progress: Progress| {
        *state.lock().unwrap_or_else(|e| e.into_inner()) = SaveState::Reading {
            banner: progress.banner,
            page: progress.page,
            records: progress.records,
        };
        ctx.request_repaint();
        !cancel.load(Ordering::Relaxed) && !login_rx.has_changed().unwrap_or(true)
    }))?;
    let exported = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let item_ids = item_ids();
    let file = uigf::to_uigf(
        &history,
        &item_ids,
        exported,
        "Teyvat.me Capture",
        env!("CARGO_PKG_VERSION"),
    );
    if file.unknown_items > 0 {
        tracing::info!(
            records = file.unknown_items,
            "wish history: records without an item id (names this build's game data lacks)"
        );
    }
    std::fs::write(path, file.json)
        .map_err(|e| failed(trf("couldn't write the file: {0}", &[&e])))?;
    Ok(history.records.len())
}

/// The item ids the file fills in, from the embedded game data (read again
/// here: the monitor holds its copy, and the file is written once).
fn item_ids() -> uigf::ItemIds {
    match crate::monitor::load_game_data() {
        Ok(data) => uigf::ItemIds::from_game_data(&data),
        Err(e) => {
            tracing::warn!("wish history: no game data for the item ids: {e:#}");
            uigf::ItemIds::default()
        }
    }
}

fn failed(why: String) -> HistoryError {
    HistoryError::Fetch(FetchError::Failed(why))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_game_data_fills_wish_item_ids() {
        use teyvatme_core::wish::api::ApiRecord;
        use teyvatme_core::wish::history::History;

        let record = |item_type: &str, name: &str, id: u64| ApiRecord {
            uid: "812345678".to_owned(),
            gacha_type: "301".to_owned(),
            item_id: String::new(),
            time: "2026-10-12 21:04:11".to_owned(),
            name: name.to_owned(),
            item_type: item_type.to_owned(),
            rank_type: "5".to_owned(),
            id: id.to_string(),
        };
        let history = History {
            uid: 812_345_678,
            region: "os_euro".to_owned(),
            records: vec![
                record("Character", "Diluc", 1),
                record("Character", "Columbina", 2),
                record("Weapon", "Primordial Jade Cutter", 3),
                record("Weapon", "Amos' Bow", 4),
            ],
        };
        let file = uigf::to_uigf(&history, &item_ids(), 0, "Teyvat.me Capture", "1.0.0");
        assert_eq!(file.unknown_items, 0);
        let json: serde_json::Value = serde_json::from_str(&file.json).unwrap();
        let ids: Vec<&str> = json["hk4e"][0]["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record["item_id"].as_str().unwrap())
            .collect();
        // Columbina and the Jade Cutter have later copies in the data.
        assert_eq!(ids, ["10000016", "10000125", "11505", "15502"]);
    }

    #[test]
    fn the_card_line_says_where_a_save_stands() {
        assert_eq!(SaveState::Idle.line(), None);
        assert_eq!(
            SaveState::Reading {
                banner: 302,
                page: 2,
                records: 41
            }
            .line()
            .as_deref(),
            Some("Reading · Weapon event, page 3 · 41 wishes so far")
        );
        assert_eq!(
            SaveState::Done {
                path: PathBuf::from("w.json"),
                records: 7
            }
            .line()
            .as_deref(),
            Some("Saved 7 wishes")
        );
    }

    #[test]
    fn every_banner_has_a_name() {
        for banner in teyvatme_core::wish::hub::BANNERS {
            assert_ne!(banner_name(banner), "Wishes", "{banner}");
        }
    }
}
