// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! A sync run (`teyvatme_core::wish::sync`), with the desktop's tracker and
//! words.

use crate::i18n::tr;

/// A run's error, with the desktop tracker's sentences.
pub(crate) type RunError = teyvatme_core::wish::sync::RunError<String>;
pub(crate) type RunReport = teyvatme_core::wish::sync::RunReport<String>;

/// What the wish card says about `error`.
pub(crate) fn describe(error: &RunError) -> String {
    match error {
        RunError::KeyExpired => tr("the wish key expired").to_owned(),
        RunError::OtherAccount => tr("the wish key belongs to another account").to_owned(),
        RunError::Api(why) | RunError::Tracker(why) | RunError::Data(why) => why.clone(),
    }
}
