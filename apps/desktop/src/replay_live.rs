// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! `--replay-live` (debug builds): `--replay-journal`, but the snapshots and
//! batches really go to a tracker, so a tracker's live sync can be tested
//! with a recorded session instead of someone playing.
//!
//! Only to a tracker on this machine (`localhost` / `127.0.0.1`): a
//! recording is old personal data, and must never reach a real tracker's
//! history. The address and the import key come from the environment
//! (`TEYVATME_REPLAY_TRACKER`, `TEYVATME_REPLAY_IMPORT_KEY`); no app setting
//! or data directory is read or written, no lock is taken.
//!
//! Like the app, it checks the key first, which is also where the tracker
//! says whether it reads gzipped snapshots; `TEYVATME_REPLAY_PLAIN=1` sends
//! them plain all the same.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::delta::GoodDelta;
use crate::good::Good;
use crate::live::{AttachAnswer, AttachRequest, BatchAnswer, ImportAnswer};
use crate::monitor;
use crate::replay::{self, StandInTracker};

/// Run it and return the process exit code.
pub fn run(recording: &Path, out: &Path) -> i32 {
    replay::run_journal_with(recording, out, |_| {
        let tracker: Box<dyn StandInTracker> = Box::new(LocalTracker::from_env()?);
        Ok(tracker)
    })
}

/// A tracker on this machine, reached with an import key.
struct LocalTracker {
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
    api: String,
    key: String,
}

/// Whether `url` is an http(s) address on this machine.
fn is_local(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(host, _)| host);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

impl LocalTracker {
    fn from_env() -> Result<Self> {
        if !cfg!(debug_assertions) {
            bail!("--replay-live is for debug builds only");
        }
        let api = std::env::var("TEYVATME_REPLAY_TRACKER").context(
            "set TEYVATME_REPLAY_TRACKER to the local tracker's API, e.g. http://127.0.0.1:5173/api",
        )?;
        let api = api.trim().trim_end_matches('/').to_string();
        if !is_local(&api) {
            bail!("TEYVATME_REPLAY_TRACKER must be a localhost or 127.0.0.1 address, not {api}");
        }
        let key = std::env::var("TEYVATME_REPLAY_IMPORT_KEY")
            .context("set TEYVATME_REPLAY_IMPORT_KEY to an import key of the local tracker")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = crate::http_client_builder().build()?;
        if std::env::var_os("TEYVATME_REPLAY_PLAIN").is_some_and(|v| v == "1") {
            monitor::PLAIN_UPLOADS.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let verify = format!("{api}/genshin-accounts-public/verify-key");
        let checked = runtime.block_on(async {
            let response = client
                .get(&verify)
                .header("x-import-key", &key)
                .send()
                .await?;
            let status = response.status();
            anyhow::Ok((status, response.text().await.unwrap_or_default()))
        });
        match checked {
            Ok((status, body)) if status.is_success() => {
                monitor::note_upload_options(&verify, &body);
            }
            Ok((status, _)) => bail!("the local tracker refused the import key: HTTP {status}"),
            Err(e) => bail!("the local tracker did not answer: {e}"),
        }
        tracing::info!(
            %api,
            gzip = monitor::gzip_uploads(&api),
            final_batch = monitor::final_batches(&api),
            "replaying to a local tracker"
        );
        Ok(Self {
            runtime,
            client,
            api,
            key,
        })
    }
}

impl StandInTracker for LocalTracker {
    fn snapshot(&mut self, good: &Good) -> Option<ImportAnswer> {
        let json = good.upload_json().ok()?;
        let at = good.timestamp.map(|ms| ms as i64);
        let url = monitor::import_url(&self.api);
        match self.runtime.block_on(monitor::upload_to_tracker(
            &self.client,
            &url,
            &self.key,
            json,
            at,
        )) {
            Ok(body) => {
                let answer = ImportAnswer::parse(&body);
                tracing::info!(live = ?answer.base, "snapshot stored");
                Some(answer)
            }
            Err(e) => {
                tracing::warn!("snapshot not stored: {e}");
                None
            }
        }
    }

    fn batch(&mut self, delta: &GoodDelta) -> BatchAnswer {
        // As `monitor::send_live_batch_with_key`, with a refusal's status
        // and body in the log (a test run wants to see them).
        let url = monitor::changes_url(&self.api);
        let response = self.runtime.block_on(async {
            let response = self
                .client
                .post(&url)
                .header("x-import-key", &self.key)
                .json(delta)
                .send()
                .await?;
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            anyhow::Ok((status, body))
        });
        match response {
            Ok((status, body)) => {
                if !(200..300).contains(&status) {
                    tracing::warn!(seq = delta.seq, status, %body, "batch refused");
                } else {
                    tracing::info!(
                        seq = delta.seq,
                        changes = delta.changes.len(),
                        status,
                        "batch stored"
                    );
                }
                BatchAnswer::of(status, &body)
            }
            Err(e) => {
                tracing::warn!("batch request failed: {e:#}");
                BatchAnswer::Retry
            }
        }
    }

    fn attach(&mut self, request: &AttachRequest) -> AttachAnswer {
        self.runtime.block_on(monitor::send_attach_with_key(
            &self.client,
            &self.api,
            &self.key,
            request,
        ))
    }

    /// What the local tracker's own answers said (its verify-key and each
    /// import's, through `monitor::note_upload_options`).
    fn final_batch(&self) -> bool {
        monitor::final_batches(&self.api)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_tracker_on_this_machine_is_accepted() {
        assert!(is_local("http://127.0.0.1:5173/api"));
        assert!(is_local("http://localhost:8787/api"));
        assert!(is_local("http://[::1]:5173/api"));
        assert!(!is_local("https://teyvat.me/api"));
        assert!(!is_local("http://127.0.0.1.example.com/api"));
        assert!(!is_local("http://localhost.example.com:80/api"));
        assert!(!is_local("ftp://127.0.0.1/api"));
    }
}
