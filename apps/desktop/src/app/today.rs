// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The Today card's figures (spec §4.2b), from the capture's `gi_tasks`:
//! what each mini stat shows at a given moment. Pure, so the clocks can be
//! tested; the card itself is in `home.rs`.

use chrono::{DateTime, Datelike, Local, TimeZone};

use crate::good::{GiAbyss, GiExpedition, GiResin, GiTasks};
use crate::i18n::{tr, trf};

/// Original Resin's cap (7.1).
pub(super) const RESIN_CAP: u32 = 200;
/// One Original Resin every 8 minutes below the cap.
const RESIN_SECS: u64 = 8 * 60;

/// What one mini stat shows: the value, its "/ total", a 0–1 bar, and a
/// small line (and whether it says "done").
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Figure {
    pub value: Option<(String, Option<String>)>,
    pub bar: Option<f32>,
    pub line: Option<(String, bool)>,
}

/// The six figures, in the card's order: Resin, Realm currency,
/// Commissions, Expeditions, Weekly bosses, Spiral Abyss.
pub(super) fn figures(tasks: Option<&GiTasks>, login_resin: Option<u32>, now: u64) -> [Figure; 6] {
    let tasks = tasks.cloned().unwrap_or_default();
    [
        resin(tasks.resin, login_resin, now),
        realm(&tasks),
        commissions(&tasks),
        expeditions(tasks.expeditions.as_deref(), now),
        weekly_bosses(&tasks),
        abyss(tasks.abyss, now),
    ]
}

/// Resin at `now`, counted on from the capture's clock.
pub(super) fn resin_now(resin: GiResin, now: u64) -> u32 {
    let next = u64::from(resin.next_at);
    if resin.value >= RESIN_CAP || now < next {
        return resin.value;
    }
    let gained = 1 + (now - next) / RESIN_SECS;
    (u64::from(resin.value) + gained).min(u64::from(RESIN_CAP)) as u32
}

/// When Resin reaches the cap (unix seconds), or `None` once it has.
pub(super) fn resin_full_at(resin: GiResin, now: u64) -> Option<u64> {
    if resin_now(resin, now) >= RESIN_CAP {
        return None;
    }
    let missing = u64::from(RESIN_CAP - resin.value);
    Some(u64::from(resin.next_at) + (missing - 1) * RESIN_SECS)
}

fn resin(resin: Option<GiResin>, login_resin: Option<u32>, now: u64) -> Figure {
    let (value, line) = match resin {
        Some(resin) => {
            let value = resin_now(resin, now);
            let line = match resin_full_at(resin, now) {
                None => (tr("Full").to_owned(), true),
                Some(at) => (full_at(at, now), false),
            };
            (value, Some(line))
        }
        // No clock: the login's value, without a time.
        None => match login_resin {
            Some(value) => (value, None),
            None => return Figure::default(),
        },
    };
    Figure {
        value: Some((group(value), Some(group(RESIN_CAP)))),
        bar: Some(value as f32 / RESIN_CAP as f32),
        line,
    }
}

/// The jar and its bar. No "Full at": the comfort's hourly rate is not in
/// the pinned game data yet (spec §10), and the line is never a guess.
fn realm(tasks: &GiTasks) -> Figure {
    let Some(realm) = tasks.realm else {
        return Figure::default();
    };
    let full = realm.limit > 0 && realm.value >= realm.limit;
    Figure {
        value: Some((group(realm.value), Some(group(realm.limit)))),
        bar: (realm.limit > 0).then(|| realm.value as f32 / realm.limit as f32),
        line: full.then(|| (tr("Full").to_owned(), true)),
    }
}

fn commissions(tasks: &GiTasks) -> Figure {
    let Some(c) = tasks.commissions else {
        return Figure::default();
    };
    Figure {
        value: Some((c.finished.to_string(), Some(c.total.to_string()))),
        bar: None,
        line: (c.finished >= c.total).then(|| (tr("Done today").to_owned(), true)),
    }
}

fn expeditions(out: Option<&[GiExpedition]>, now: u64) -> Figure {
    let Some(out) = out else {
        return Figure::default();
    };
    if out.is_empty() {
        return Figure {
            value: Some((tr("None").to_owned(), None)),
            ..Figure::default()
        };
    }
    let next = out
        .iter()
        .map(|e| u64::from(e.done_at))
        .filter(|at| *at > now)
        .min();
    Figure {
        value: Some((trf("{0} out", &[&out.len()]), None)),
        bar: None,
        line: Some(match next {
            Some(at) => (trf("First back {0}", &[&clock(at, now)]), false),
            None => (tr("All back").to_owned(), true),
        }),
    }
}

fn weekly_bosses(tasks: &GiTasks) -> Figure {
    let Some(b) = tasks.weekly_bosses else {
        return Figure::default();
    };
    Figure {
        value: Some((b.left.to_string(), Some(b.limit.to_string()))),
        bar: None,
        line: Some((tr("Discounts left").to_owned(), false)),
    }
}

fn abyss(abyss: Option<GiAbyss>, now: u64) -> Figure {
    let Some(a) = abyss else {
        return Figure::default();
    };
    let end = u64::from(a.end);
    let line = (end > now).then(|| {
        let days = (end - now).div_ceil(86_400);
        let text = if days <= 1 {
            tr("Resets within a day").to_owned()
        } else {
            trf("Resets in {0} days", &[&days])
        };
        (text, false)
    });
    Figure {
        value: Some((a.stars.to_string(), Some(a.max_stars.to_string()))),
        bar: None,
        line,
    }
}

/// "Full at 04:50", or "Full Thu 18:00" on another day.
fn full_at(at: u64, now: u64) -> String {
    match (local(at), local(now)) {
        (Some(at_time), Some(now_time)) if at_time.date_naive() == now_time.date_naive() => {
            trf("Full at {0}", &[&at_time.format("%H:%M")])
        }
        _ => trf("Full {0}", &[&clock(at, now)]),
    }
}

/// "22:10", or "Thu 22:10" on another day.
fn clock(at: u64, now: u64) -> String {
    match (local(at), local(now)) {
        (Some(at_time), Some(now_time)) => {
            let time = at_time.format("%H:%M").to_string();
            if at_time.date_naive() == now_time.date_naive() {
                time
            } else {
                format!("{} {time}", weekday(at_time.weekday()))
            }
        }
        _ => String::new(),
    }
}

fn local(secs: u64) -> Option<DateTime<Local>> {
    Local.timestamp_opt(i64::try_from(secs).ok()?, 0).single()
}

/// The short weekday name.
fn weekday(day: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match day {
        Mon => tr("Mon"),
        Tue => tr("Tue"),
        Wed => tr("Wed"),
        Thu => tr("Thu"),
        Fri => tr("Fri"),
        Sat => tr("Sat"),
        Sun => tr("Sun"),
    }
}

fn group(n: u32) -> String {
    crate::ui::widgets::group_thousands(n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::good::{GiCommissions, GiRealm, GiWeeklyBosses};

    /// A local noon, so "same day" holds for a few hours either way.
    fn noon() -> u64 {
        Local
            .with_ymd_and_hms(2026, 10, 27, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp() as u64
    }

    #[test]
    fn resin_counts_on_from_its_clock() {
        let now = noon();
        let resin = GiResin {
            value: 142,
            next_at: (now + 60) as u32,
        };
        assert_eq!(resin_now(resin, now), 142);
        // The next one at next_at, then one every 8 minutes.
        assert_eq!(resin_now(resin, now + 60), 143);
        assert_eq!(resin_now(resin, now + 60 + 8 * 60), 144);
        assert_eq!(resin_now(resin, now + 100_000), RESIN_CAP);
        // 58 missing: the last one 57 × 8 minutes after the next.
        assert_eq!(resin_full_at(resin, now), Some(now + 60 + 57 * 480));
        assert_eq!(resin_full_at(resin, now + 100_000), None);
    }

    #[test]
    fn resin_reads_full_at_the_cap() {
        let now = noon();
        let full = GiResin {
            value: 200,
            next_at: now as u32,
        };
        let figure = &figures(
            Some(&GiTasks {
                resin: Some(full),
                ..GiTasks::default()
            }),
            None,
            now,
        )[0];
        assert_eq!(figure.line, Some(("Full".to_owned(), true)));
        assert_eq!(figure.bar, Some(1.0));
    }

    #[test]
    fn resin_without_a_clock_shows_the_login_value_only() {
        let figure = &figures(None, Some(90), noon())[0];
        assert_eq!(
            figure.value,
            Some(("90".to_owned(), Some("200".to_owned())))
        );
        assert_eq!(figure.line, None);
    }

    #[test]
    fn a_part_not_captured_shows_no_value_and_no_line() {
        for figure in figures(None, None, noon()) {
            assert_eq!(figure, Figure::default());
        }
    }

    #[test]
    fn realm_currency_has_a_bar_and_no_full_time() {
        let tasks = GiTasks {
            realm: Some(GiRealm {
                value: 1_820,
                limit: 2_400,
            }),
            ..GiTasks::default()
        };
        let figure = &figures(Some(&tasks), None, noon())[1];
        assert_eq!(
            figure.value,
            Some(("1,820".to_owned(), Some("2,400".to_owned())))
        );
        assert_eq!(figure.line, None);
    }

    #[test]
    fn counters_read_as_the_spec_writes_them() {
        let now = noon();
        let tasks = GiTasks {
            commissions: Some(GiCommissions {
                finished: 4,
                total: 4,
                claimed: None,
            }),
            expeditions: Some(vec![
                GiExpedition {
                    done_at: (now + 3_600) as u32,
                    ..GiExpedition::default()
                },
                GiExpedition {
                    done_at: (now - 60) as u32,
                    ..GiExpedition::default()
                },
            ]),
            weekly_bosses: Some(GiWeeklyBosses {
                left: 2,
                used: Some(1),
                limit: 3,
                cost: 30,
                full_cost: 60,
            }),
            abyss: Some(GiAbyss {
                start: (now - 86_400) as u32,
                end: (now + 8 * 86_400 + 60) as u32,
                stars: 27,
                max_stars: 36,
            }),
            ..GiTasks::default()
        };
        let [_, _, commissions, expeditions, bosses, abyss] = figures(Some(&tasks), None, now);
        assert_eq!(commissions.line, Some(("Done today".to_owned(), true)));
        assert_eq!(expeditions.value, Some(("2 out".to_owned(), None)));
        assert_eq!(
            expeditions.line,
            Some(("First back 13:00".to_owned(), false))
        );
        assert_eq!(bosses.value, Some(("2".to_owned(), Some("3".to_owned()))));
        assert_eq!(abyss.line, Some(("Resets in 9 days".to_owned(), false)));
    }

    #[test]
    fn expeditions_all_back_or_none_out() {
        let now = noon();
        let back = [GiExpedition {
            done_at: (now - 1) as u32,
            ..GiExpedition::default()
        }];
        assert_eq!(
            expeditions(Some(&back), now).line,
            Some(("All back".to_owned(), true))
        );
        assert_eq!(
            expeditions(Some(&[]), now).value,
            Some(("None".to_owned(), None))
        );
    }

    #[test]
    fn a_time_on_another_day_names_the_day() {
        let now = noon();
        let tomorrow = now + 86_400;
        let name = weekday(local(tomorrow).unwrap().weekday());
        assert_eq!(clock(tomorrow, now), format!("{name} 12:00"));
        assert_eq!(full_at(now + 3_600, now), "Full at 13:00");
    }
}
