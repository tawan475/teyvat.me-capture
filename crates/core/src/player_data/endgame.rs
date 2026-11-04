// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! `gi_endgame`: the Imaginarium Theater's season state and the Stygian
//! Onslaught's records, as the game last sent them. The protocol library
//! recognises the messages by shape and hands over candidates; this settles
//! them against the game data's season tables and the account, and turns
//! them into the export's shapes.

use std::collections::BTreeMap;

use teyvatme_game_data::Endgame;
use teyvatme_protocol::{StygianRecord, TheaterDetail};

use super::PlayerData;
use crate::good;

/// Each part as the game last sent it; `None` until it did.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct EndgameState {
    pub theater: Option<TheaterProgress>,
    /// By season and whether the run was co-op (a season has at most one
    /// record of each).
    pub stygian: Option<BTreeMap<(u32, bool), good::GiStygianRecord>>,
}

/// The Theater's state for the season running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TheaterProgress {
    pub season: u32,
    /// `RoleCombatDifficultyExcelConfigData.difficultyId`, if chosen yet.
    pub difficulty: Option<u32>,
    pub acts: u32,
}

/// The server regions' offsets from UTC, in seconds: Asia (and the Chinese
/// servers, TW/HK/MO) +8, Europe +1, America -5. A season opens at 04:00
/// server time, and the schedule table says so in that local time.
const SERVER_OFFSETS: [i64; 3] = [8 * 3600, 3600, -5 * 3600];

/// `YYYY-MM-DD HH:MM:SS` as seconds since the epoch, as if it were UTC.
fn parse_local(time: &str) -> Option<i64> {
    let mut parts = time.split([' ', '-', ':']).map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    let (hour, minute, second) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + minute * 60 + second)
}

/// The season whose opening, in some server's local time, is `start`.
fn season_opening_at(endgame: &Endgame, start: u64) -> Option<u32> {
    let start = i64::try_from(start).ok()?;
    endgame
        .theater_seasons
        .iter()
        .find(|(_, season)| {
            parse_local(&season.begin)
                .is_some_and(|local| SERVER_OFFSETS.iter().any(|offset| local - offset == start))
        })
        .map(|(id, _)| *id)
}

impl PlayerData {
    /// Keep the Theater's season state if `detail` is a season the game data
    /// knows: its window must open when that season does, and the message
    /// must name the season among its numbers. Returns whether it did.
    pub fn apply_theater(&mut self, detail: &TheaterDetail) -> bool {
        let Ok(endgame) = self.game_data.endgame() else {
            return false;
        };
        let Some(season) = season_opening_at(endgame, detail.start) else {
            return false;
        };
        if !detail.small.contains(&u64::from(season)) {
            return false;
        }
        let difficulties = endgame
            .theater_seasons
            .get(&season)
            .map(|s| &s.difficulties);
        let chosen: Vec<u32> = detail
            .small
            .iter()
            .filter_map(|v| u32::try_from(*v).ok())
            .filter(|v| difficulties.is_some_and(|d| d.contains(v)))
            .collect();
        let difficulty = match chosen.as_slice() {
            [one] => Some(*one),
            _ => None,
        };
        self.endgame.theater = Some(TheaterProgress {
            season,
            difficulty,
            acts: detail.acts_cleared(),
        });
        true
    }

    /// Keep the Stygian records of the seasons the game data knows, with the
    /// account's own team in each (a co-op record's other players are dropped,
    /// UIDs included). The message is the whole list, so it replaces what was
    /// held. Returns how many records were kept; 0 means "some other message"
    /// (or no account yet: the login's inventory, which names the UID, comes
    /// first).
    pub fn apply_stygian_records(&mut self, records: &[StygianRecord]) -> usize {
        let Ok(endgame) = self.game_data.endgame() else {
            return 0;
        };
        let Some(uid) = self.account_uid() else {
            return 0;
        };
        let mut kept = BTreeMap::new();
        for record in records {
            if !endgame.stygian_seasons.contains(&record.schedule) {
                continue;
            }
            let coop = record
                .fights
                .iter()
                .any(|f| f.teams.iter().any(|t| t.uid != u64::from(uid)));
            let key_of = |id: u32| -> Option<String> {
                self.game_data
                    .get_character(id)
                    .ok()
                    .map(|name| good::to_good_key(name))
                    .filter(|key| !key.is_empty())
            };
            let figure = |f: &Option<teyvatme_protocol::Figure>| {
                f.as_ref().map(|f| good::GiFigure {
                    value: f.value,
                    by: f.by.and_then(key_of),
                })
            };
            let fights = record
                .fights
                .iter()
                .map(|f| good::GiStygianFight {
                    time: f.time,
                    done: f.finished,
                    team: f
                        .teams
                        .iter()
                        .filter(|t| t.uid == u64::from(uid))
                        .flat_map(|t| t.avatars.iter())
                        .filter_map(|id| key_of(*id))
                        .collect(),
                    strike: figure(&f.strike),
                    damage: figure(&f.damage),
                })
                .collect();
            kept.insert(
                (record.schedule, coop),
                good::GiStygianRecord {
                    season: record.schedule,
                    difficulty: record.difficulty,
                    at: record.at_ms / 1000,
                    coop,
                    fights,
                },
            );
        }
        if kept.is_empty() {
            return 0;
        }
        let count = kept.len();
        self.endgame.stygian = Some(kept);
        count
    }

    /// `gi_endgame`, or `None` when none of it was captured.
    pub(super) fn export_endgame(&self) -> Option<good::GiEndgame> {
        let endgame = self.game_data.endgame().ok();
        let theater = self.endgame.theater.as_ref().map(|t| good::GiTheater {
            season: t.season,
            difficulty: t.difficulty,
            level: t
                .difficulty
                .and_then(|d| endgame.and_then(|e| e.theater_difficulties.get(&d).copied())),
            acts: t.acts,
        });
        let stygian = self
            .endgame
            .stygian
            .as_ref()
            .map(|records| good::GiStygian {
                records: records.values().cloned().collect(),
            });
        if theater.is_none() && stygian.is_none() {
            return None;
        }
        Some(good::GiEndgame { theater, stygian })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_times_parse_as_utc_seconds() {
        assert_eq!(parse_local("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_local("2026-11-01 04:00:00"), Some(1_793_505_600));
        assert_eq!(parse_local("2024-02-29 23:59:59"), Some(1_709_251_199));
        assert_eq!(parse_local("2026-13-01 04:00:00"), None);
        assert_eq!(parse_local("soon"), None);
    }
}
