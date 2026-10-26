// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! When a sync runs: a new key syncs every banner, a wish its own banner a
//! little later, an unfinished run carries on shortly. Nothing polls.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::wish::Banner;
use crate::wish::hub::BANNERS;

/// How long after a wish its banner is synced. The history shows a wish a
/// little after it was made, and a run of single wishes then costs one sync.
pub const WISH_DELAY: Duration = Duration::from_secs(20);

/// The least time between two syncs of every banner: reopening the history
/// a few times in a row (each time with a new key) syncs once.
pub const MIN_RESYNC: Duration = Duration::from_secs(60);

/// How long after a run that stopped at its page cap the next one carries on.
pub const CONTINUE_DELAY: Duration = Duration::from_secs(30);

/// When each banner is next due.
#[derive(Debug, Default)]
pub struct Schedule {
    due: BTreeMap<Banner, Instant>,
    /// When every banner was last taken for a run.
    last_full: Option<Instant>,
}

impl Schedule {
    /// A new key: every banner, now -- or once [`MIN_RESYNC`] has passed since
    /// the last such run.
    pub fn on_new_key(&mut self, now: Instant) {
        let at = match self.last_full {
            Some(last) if now < last + MIN_RESYNC => last + MIN_RESYNC,
            _ => now,
        };
        for banner in BANNERS {
            self.due.insert(banner, at);
        }
    }

    /// A wish: that banner (every one when unknown), [`WISH_DELAY`] after the
    /// last wish.
    pub fn on_wish(&mut self, now: Instant, banner: Option<Banner>) {
        let banners = match banner {
            Some(banner) => vec![banner],
            None => BANNERS.to_vec(),
        };
        for banner in banners {
            self.due.insert(banner, now + WISH_DELAY);
        }
    }

    /// Carry on with unfinished banners after [`CONTINUE_DELAY`], unless
    /// they are due sooner anyway.
    pub fn on_unfinished(&mut self, now: Instant, banners: &[Banner]) {
        for &banner in banners {
            let at = now + CONTINUE_DELAY;
            self.due
                .entry(banner)
                .and_modify(|due| *due = (*due).min(at))
                .or_insert(at);
        }
    }

    pub fn next_due(&self) -> Option<Instant> {
        self.due.values().min().copied()
    }

    /// Take the banners due at `now`, in [`BANNERS`] order.
    pub fn take_due(&mut self, now: Instant) -> Vec<Banner> {
        let due: Vec<Banner> = BANNERS
            .into_iter()
            .filter(|banner| self.due.get(banner).is_some_and(|&at| at <= now))
            .collect();
        for banner in &due {
            self.due.remove(banner);
        }
        if due.len() == BANNERS.len() {
            self.last_full = Some(now);
        }
        due
    }

    pub fn clear(&mut self) {
        self.due.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_key_syncs_every_banner_now_but_not_twice_a_minute() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.on_new_key(now);
        assert_eq!(schedule.next_due(), Some(now));
        assert_eq!(schedule.take_due(now), BANNERS.to_vec());
        assert_eq!(schedule.next_due(), None);

        let soon = now + Duration::from_secs(10);
        schedule.on_new_key(soon);
        assert_eq!(schedule.take_due(soon), Vec::<Banner>::new());
        assert_eq!(schedule.next_due(), Some(now + MIN_RESYNC));
        assert_eq!(schedule.take_due(now + MIN_RESYNC), BANNERS.to_vec());
    }

    #[test]
    fn a_wish_syncs_its_banner_after_the_last_wish_settles() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.on_wish(now, Some(302));
        let later = now + Duration::from_secs(5);
        schedule.on_wish(later, Some(302));
        assert_eq!(schedule.next_due(), Some(later + WISH_DELAY));
        assert!(schedule.take_due(now + WISH_DELAY).is_empty());
        assert_eq!(schedule.take_due(later + WISH_DELAY), vec![302]);
    }

    #[test]
    fn a_wish_on_an_unknown_banner_syncs_them_all() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.on_wish(now, None);
        assert_eq!(schedule.take_due(now + WISH_DELAY), BANNERS.to_vec());
    }

    #[test]
    fn unfinished_banners_carry_on_shortly_without_postponing_a_sooner_run() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        schedule.on_wish(now, Some(301));
        schedule.on_unfinished(now, &[301, 200]);
        assert_eq!(schedule.take_due(now + WISH_DELAY), vec![301]);
        assert_eq!(schedule.take_due(now + CONTINUE_DELAY), vec![200]);
    }
}
