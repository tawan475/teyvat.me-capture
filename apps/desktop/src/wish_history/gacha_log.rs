// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The game's wish history API (`getGachaLog`), from `teyvatme_core::wish`
//! (the link, the paced client), with what the desktop brings: reqwest with
//! the system TLS, and tokio's clock.
//!
//! The key in every URL here is a password while it lasts: every
//! `reqwest::Error` has its URL stripped before it is turned into text.

use std::time::Duration;

pub(crate) use teyvatme_core::wish::api::{
    ApiRecord, FetchError, HttpError, HttpGet, HttpReply, Pacing, Sleeper,
};
#[cfg(test)]
pub(crate) use teyvatme_core::wish::link::KEY_LIFETIME;
pub(crate) use teyvatme_core::wish::link::WishKey;

/// The paced client, over reqwest.
pub(crate) type LogClient<S> = teyvatme_core::wish::api::LogClient<ReqwestGet, S>;

/// The desktop's HTTP client for the API.
pub(crate) struct ReqwestGet(pub(crate) reqwest::Client);

impl HttpGet for ReqwestGet {
    async fn get(&self, url: &reqwest::Url) -> Result<HttpReply, HttpError> {
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

/// The real clock.
pub(crate) struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(duration)
    }
}
