// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! `gi_endgame`: the Imaginarium Theater's season state and the Stygian
//! Onslaught's records, as the game last sent them. The protocol library
//! recognises the messages by shape and hands over candidates; this settles
//! them against the game data's season tables and the account, and turns
//! them into the export's shapes.

use std::collections::BTreeMap;

use teyvatme_game_data::{Endgame, TheaterDifficulty};
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
    /// The Star Challenge Stellas earned and the acts they were earned on,
    /// when the message tells them apart (see [`theater_stars`]).
    pub stars: Option<TheaterStars>,
    /// The stars the difficulty offers (its acts and extra bouts), when the
    /// game data knows the row.
    pub max_stars: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TheaterStars {
    pub stars: u32,
    pub acts: Vec<u32>,
}

/// The fewest entries a list is believed to be the starred acts with, and
/// the most stars a run may be short of: the message also lists three
/// difficulty levels and the reward acts, which are short lists of act-sized
/// numbers.
const STARRED_MIN: u32 = 4;
const STARS_MISSING_MAX: u32 = 4;

/// Whether `list` is some acts in order, each at most `top`.
fn acts_within(list: &[u64], top: u32) -> bool {
    !list.is_empty()
        && list.windows(2).all(|pair| pair[0] < pair[1])
        && list.iter().all(|act| (1..=u64::from(top)).contains(act))
}

/// The Star Challenge Stellas of the season from the message's lists and
/// numbers, or `None` when they cannot be told.
///
/// The game lists the acts cleared (`1..=cleared`) and, on another field,
/// the acts whose star was earned; the hardest difficulty's extra bouts
/// come as two more lists (cleared, starred) numbered from 1. The field
/// numbers are not known, so the starred acts are the one list of acts within
/// the cleared ones, at least [`STARRED_MIN`] long and at most
/// [`STARS_MISSING_MAX`] short of them, that is not the cleared list itself;
/// with no such list every act has its star. The count must then appear
/// among the message's lone numbers (the game sends it too). A run short of
/// more stars than that reads as full, and a run of fewer than four acts
/// is left out: those lists cannot be told from the others.
fn theater_stars(
    detail: &TheaterDetail,
    cleared: u32,
    row: &TheaterDifficulty,
) -> Option<TheaterStars> {
    if row.acts == 0 || cleared < STARRED_MIN || cleared > row.acts {
        return None;
    }
    let min_len = STARRED_MIN.max(cleared - STARS_MISSING_MAX) as usize;
    let full: Vec<u64> = (1..=u64::from(cleared)).collect();
    let candidates: Vec<&Vec<u64>> = detail
        .lists
        .iter()
        .filter(|list| acts_within(list, cleared) && list.len() >= min_len)
        .collect();
    let partial: Vec<&Vec<u64>> = candidates
        .iter()
        .copied()
        .filter(|list| **list != full)
        .collect();
    let starred = match partial.as_slice() {
        [] if !candidates.is_empty() => &full,
        [one] => *one,
        _ => return None,
    };
    let extras = if row.extras == 0 {
        0
    } else {
        detail
            .lists
            .iter()
            .filter(|list| acts_within(list, row.extras))
            .map(Vec::len)
            .min()
            .unwrap_or(0)
    };
    let stars = starred.len() + extras;
    if !detail.small.contains(&(stars as u64)) {
        return None;
    }
    Some(TheaterStars {
        stars: stars as u32,
        acts: starred.iter().map(|act| *act as u32).collect(),
    })
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
        let acts = detail.acts_cleared();
        let row = difficulty.and_then(|d| endgame.theater_difficulties.get(&d));
        self.endgame.theater = Some(TheaterProgress {
            season,
            difficulty,
            acts,
            stars: row.and_then(|row| theater_stars(detail, acts, row)),
            max_stars: row
                .filter(|row| row.acts > 0)
                .map(|row| row.acts + row.extras),
        });
        true
    }

    /// The Theater stars held, for the log: `(stars, of)`.
    pub fn theater_stars(&self) -> Option<(u32, Option<u32>)> {
        let theater = self.endgame.theater.as_ref()?;
        Some((theater.stars.as_ref()?.stars, theater.max_stars))
    }

    /// Keep the Stygian records of the seasons the game data knows, with the
    /// account's own team in each and, by position, every player's characters
    /// and a co-op run's players (nickname and picture; their UIDs are never
    /// kept). The message is the whole list, so it replaces what was held. Returns how many records were kept; 0 means "some other message"
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
            // Positions by UID: every team's player must be in the list.
            let position = |uid: u64| {
                record
                    .players
                    .iter()
                    .find(|p| p.uid == uid)
                    .map(|p| p.index)
            };
            let by_player = |f: &teyvatme_protocol::StygianFight| {
                let mut teams = f
                    .teams
                    .iter()
                    .map(|t| {
                        Some(good::GiStygianPlayerTeam {
                            p: position(t.uid)?,
                            team: t
                                .members
                                .iter()
                                .filter_map(|m| {
                                    Some(good::GiStygianMember {
                                        key: key_of(m.id)?,
                                        level: m.level,
                                        constellation: m.constellation,
                                    })
                                })
                                .collect(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                teams.sort_by_key(|t| t.p);
                Some(teams)
            };
            let players = (coop && !record.players.is_empty()).then(|| {
                let mut players: Vec<_> = record
                    .players
                    .iter()
                    .map(|p| good::GiStygianPlayer {
                        p: p.index,
                        is_self: p.uid == u64::from(uid),
                        picture: p.picture,
                        nickname: p.nickname.clone(),
                    })
                    .collect();
                players.sort_by_key(|p| p.p);
                players
            });
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
                    players: by_player(f),
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
                    players,
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
                .and_then(|d| endgame.and_then(|e| e.theater_difficulties.get(&d)))
                .map(|row| row.level),
            acts: t.acts,
            stars: t.stars.as_ref().map(|s| s.stars),
            max_stars: t.max_stars,
            starred_acts: t.stars.as_ref().map(|s| s.acts.clone()),
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
