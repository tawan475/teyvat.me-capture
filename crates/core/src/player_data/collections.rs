// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! `gi_collections` and `gi_wardrobe`: what the player has collected and
//! what their characters wear, each part as the game last sent it in full
//! (at login). The messages are recognised in the protocol library; this
//! keeps them and turns them into the export's shapes.

use std::collections::BTreeMap;

use teyvatme_protocol::{
    AranaraState, ArchiveTab, AvatarWardrobe, DefeatCount, EndingState, EventInfo, Handbook,
    HandbookProgress, HandbookState, HangoutChapter, HomeComfort, MusicSong, RecordsMessage,
    SignIn, TcgChallenges, TcgCollection, WeaponSkin,
};

use super::PlayerData;
use crate::good;

/// Each part as the game last sent it; `None` until it did.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CollectionsState {
    pub archive: Option<BTreeMap<ArchiveTab, Vec<u32>>>,
    pub defeats: Option<BTreeMap<u32, DefeatCount>>,
    pub handbook: Option<Handbook>,
    pub tcg: Option<TcgCollection>,
    pub aranara: Option<BTreeMap<u32, BTreeMap<u32, AranaraState>>>,
    pub weapon_skins: Option<Vec<WeaponSkin>>,
    pub wardrobe: Option<AvatarWardrobe>,
    /// Unlocked recipes → proficiency (0 when none yet).
    pub recipes: Option<BTreeMap<u32, u32>>,
    pub hangouts: Option<BTreeMap<u32, HangoutChapter>>,
    pub monsters_seen: Option<Vec<u32>>,
    pub blueprints: Option<Vec<u32>>,
    pub ar_rewards: Option<Vec<u32>>,
    pub events: Option<BTreeMap<u32, EventInfo>>,
    pub comfort: Option<BTreeMap<u32, HomeComfort>>,
    pub tcg_challenges: Option<TcgChallenges>,
    pub tcg_tavern: Option<BTreeMap<u32, Vec<u32>>>,
    pub music_game: Option<BTreeMap<u32, MusicSong>>,
    pub beyond_cosmetics: Option<BTreeMap<u32, u32>>,
    pub sign_ins: Option<BTreeMap<u32, SignIn>>,
    /// Crafting, forging and processing recipes unlocked.
    pub crafting: Option<Vec<u32>>,
    pub forging: Option<Vec<u32>>,
    pub processing: Option<Vec<u32>>,
    pub furnishing_sets: Option<Vec<u32>>,
    pub companion_rewards: Option<Vec<u32>>,
    pub domains_cleared: Option<Vec<u32>>,
    pub npcs_met: Option<Vec<u32>>,
    /// Natlan's tribal chapters: id → (state 1-4, when it started).
    pub tribal_chapters: Option<BTreeMap<u32, (u32, Option<u32>)>>,
}

/// The share of a lone list's ids one recipe table must hold for the list to
/// be that table's (a newer game version may add a few the pinned data lacks).
const RECIPE_LIST_SHARE: f64 = 0.95;

/// The fewest ids a furnishing set list is believed with. The sets' ids are
/// small and dense, so several short server lists fit them; an account learns
/// dozens of sets with its first Serenitea Pot.
const FURNISHING_SETS_MIN: usize = 20;

/// The shortest recipe list believed (7.1: 44 processing, 46 forging, 578
/// crafting; every account starts with more than this).
const MIN_RECIPE_LIST: usize = 10;

/// Monster ids (7.1: 20 010 101-84 803 091).
const MONSTER_IDS: std::ops::RangeInclusive<u64> = 20_000_000..=99_999_999;

/// Furnishing ids, the blueprint list's (7.1: 360 101-374 823).
const FURNISHING_IDS: std::ops::RangeInclusive<u64> = 360_000..=379_999;

/// Adventure Ranks with a reward: AR 1 has none.
const AR_REWARD_LEVELS: std::ops::RangeInclusive<u64> = 2..=60;

/// Which lone list a records message was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoneList {
    MonstersSeen,
    Blueprints,
    ArRewards,
    Crafting,
    Forging,
    Processing,
    FurnishingSets,
    CompanionRewards,
    DomainsCleared,
    NpcsMet,
}

/// The fewest recipes believed (every account starts with a few).
const MIN_RECIPES: usize = 3;

/// The most recipes believed (7.1: 284 in the game, 252 unlocked).
const MAX_RECIPES: usize = 5_000;

/// One part of the collections, as a message brought it.
#[derive(Clone, Debug, PartialEq)]
pub enum CollectionPart {
    Archive(BTreeMap<ArchiveTab, Vec<u32>>),
    Defeats(BTreeMap<u32, DefeatCount>),
    Handbook(Handbook),
    Tcg(TcgCollection),
    Aranara(BTreeMap<u32, BTreeMap<u32, AranaraState>>),
    WeaponSkins(Vec<WeaponSkin>),
    Wardrobe(AvatarWardrobe),
    Hangouts(BTreeMap<u32, HangoutChapter>),
    Events(BTreeMap<u32, EventInfo>),
    Comfort(BTreeMap<u32, HomeComfort>),
    TcgChallenges(TcgChallenges),
    TcgTavern(BTreeMap<u32, Vec<u32>>),
    MusicGame(BTreeMap<u32, MusicSong>),
    BeyondCosmetics(BTreeMap<u32, u32>),
    SignIns(BTreeMap<u32, SignIn>),
}

fn tab_key(tab: ArchiveTab) -> &'static str {
    match tab {
        ArchiveTab::Quests => "quests",
        ArchiveTab::LivingBeings => "livingBeings",
        ArchiveTab::Artifacts => "artifacts",
        ArchiveTab::Weapons => "weapons",
        ArchiveTab::Materials => "materials",
        ArchiveTab::Books => "books",
        ArchiveTab::Tutorials => "tutorials",
        ArchiveTab::Viewpoints => "viewpoints",
    }
}

fn progress(p: &HandbookProgress) -> good::GiProgress {
    good::GiProgress {
        progress: p.progress,
        total: p.total,
        state: match p.state {
            HandbookState::InProgress => "open",
            HandbookState::Complete => "complete",
            HandbookState::RewardTaken => "claimed",
        }
        .to_string(),
    }
}

fn hangout(chapter: &HangoutChapter) -> good::GiHangout {
    good::GiHangout {
        state: chapter.state.and_then(|state| {
            ["closed", "locked", "open", "accepted"]
                .get(state as usize)
                .map(|s| s.to_string())
        }),
        endings: chapter
            .endings
            .iter()
            .map(|(id, state)| {
                let state = match state {
                    EndingState::Locked => "locked",
                    EndingState::Unclaimed => "unclaimed",
                    EndingState::Claimed => "claimed",
                };
                (*id, state.to_string())
            })
            .collect(),
        pictures: chapter
            .pictures
            .iter()
            .filter(|(_, unlocked)| **unlocked)
            .map(|(id, _)| *id)
            .collect(),
        pictures_total: chapter.pictures.len() as u32,
    }
}

fn aranara_state(state: AranaraState) -> &'static str {
    match state {
        AranaraState::None => "none",
        AranaraState::Collectable => "collectable",
        AranaraState::Collected => "collected",
        AranaraState::Finished => "finished",
    }
}

impl PlayerData {
    /// Keep the recipes if `records` is the recipe list (7.1:
    /// `CookDataNotify`, at login), replacing the earlier one; returns how
    /// many, or `None` when this is another message.
    ///
    /// The list is records of one or two numbers (the recipe id and, once
    /// cooked, its proficiency) beside a lone number or two (the cooking
    /// grade), a shape a few other messages share. It is told apart by the
    /// game data: every entry's id is one of the recipe table's, once, and
    /// the proficiency never above that recipe's most.
    pub fn apply_recipes(&mut self, records: &RecordsMessage) -> Option<usize> {
        let table = self.game_data.recipes().ok()?;
        let entries = &records.records;
        if entries.len() < MIN_RECIPES
            || entries.len() > MAX_RECIPES
            || !records.lists.is_empty()
            || records.varints.len() > 2
            || entries
                .iter()
                .any(|e| !e.lists.is_empty() || e.varints.is_empty() || e.varints.len() > 2)
        {
            return None;
        }
        // The id: the tag on every entry whose values are all recipes, once.
        let tags: std::collections::BTreeSet<u32> = entries
            .iter()
            .flat_map(|e| e.varints.keys().copied())
            .collect();
        let id_tags: Vec<u32> = tags
            .iter()
            .copied()
            .filter(|tag| {
                let ids: Option<Vec<u64>> = entries
                    .iter()
                    .map(|e| e.varints.get(tag).copied())
                    .collect();
                ids.is_some_and(|ids| {
                    let unique: std::collections::BTreeSet<u64> = ids.iter().copied().collect();
                    unique.len() == ids.len()
                        && ids
                            .iter()
                            .all(|id| u32::try_from(*id).is_ok_and(|id| table.contains_key(&id)))
                })
            })
            .collect();
        let [id_tag] = id_tags.as_slice() else {
            return None;
        };
        let mut found = BTreeMap::new();
        for entry in entries {
            let id = entry.varints[id_tag] as u32;
            let proficiency = entry
                .varints
                .iter()
                .find(|(tag, _)| *tag != id_tag)
                .map_or(0, |(_, value)| *value);
            if proficiency > u64::from(table[&id]) {
                return None;
            }
            found.insert(id, proficiency as u32);
        }
        let count = found.len();
        self.collections.recipes = Some(found);
        Some(count)
    }

    /// Keep a lone id list if `records` is one of three (7.1, all at login):
    /// the monsters seen (`AllSeenMonsterNotify`: monster ids alone), the
    /// furnishing blueprints learned (`UnlockedFurnitureFormulaDataNotify`:
    /// furnishing ids beside a flag) or the Adventure Rank rewards claimed
    /// (`PlayerLevelRewardUpdateNotify`: ranks 2-60 alone, none above the
    /// account's). Over four recorded sessions no other message's lone list
    /// fit any of them (the other small lists all hold a 1).
    pub fn apply_lone_list(&mut self, records: &RecordsMessage) -> Option<LoneList> {
        if !records.records.is_empty() || records.lists.len() != 1 || records.varints.len() > 1 {
            return None;
        }
        let list = records.lists.values().next()?;
        let unique: std::collections::BTreeSet<u64> = list.iter().copied().collect();
        if unique.len() != list.len() {
            return None;
        }
        let sorted = || {
            let mut out: Vec<u32> = list.iter().map(|v| *v as u32).collect();
            out.sort_unstable();
            out
        };
        let state = &mut self.collections;
        // The client's own ask for defeat counts (`QueryCodexMonsterBeKilledNumReq`)
        // lists exactly the Archive's Living Beings, which arrived before it.
        let living = state
            .archive
            .as_ref()
            .and_then(|a| a.get(&ArchiveTab::LivingBeings));
        let is_codex_ask = living.is_some_and(|ids| {
            list.iter()
                .all(|v| u32::try_from(*v).is_ok_and(|v| ids.binary_search(&v).is_ok()))
        });
        if list.len() >= 50 && !is_codex_ask && list.iter().all(|v| MONSTER_IDS.contains(v)) {
            state.monsters_seen = Some(sorted());
            return Some(LoneList::MonstersSeen);
        }
        if list.len() >= 10 && list.iter().all(|v| FURNISHING_IDS.contains(v)) {
            state.blueprints = Some(sorted());
            return Some(LoneList::Blueprints);
        }
        // The crafting bench's, the blacksmith's and processing's unlocked
        // recipes (`CombineDataNotify`, `ForgeDataNotify` beside its queue
        // count, `CompoundDataNotify`): told apart by which recipe table holds
        // the ids. Crafting and forging share some ids, so a list is a
        // table's when nearly all its ids are that table's and no other
        // table holds as many.
        // At least 10: a few short lists elsewhere hold ids the crafting
        // table shares (7.1's six finished bonus activities, 50 001-50 006).
        if list.len() >= MIN_RECIPE_LIST
            && let Ok(tables) = self.game_data.crafting()
        {
            let share = |table: &std::collections::BTreeSet<u32>| {
                let held = list
                    .iter()
                    .filter(|v| u32::try_from(**v).is_ok_and(|v| table.contains(&v)))
                    .count();
                held as f64 / list.len() as f64
            };
            let scores = [
                (LoneList::Crafting, share(&tables.combine)),
                (LoneList::Forging, share(&tables.forge)),
                (LoneList::Processing, share(&tables.compound)),
            ];
            let best = scores
                .iter()
                .copied()
                .filter(|(_, score)| *score >= RECIPE_LIST_SHARE)
                .filter(|(kind, score)| scores.iter().all(|(k, s)| k == kind || s < score))
                .map(|(kind, _)| kind)
                .next();
            if let Some(kind) = best {
                let found = Some(sorted());
                let state = &mut self.collections;
                match kind {
                    LoneList::Crafting => state.crafting = found,
                    LoneList::Forging => state.forging = found,
                    _ => state.processing = found,
                }
                return Some(kind);
            }
        }
        let ar = self.properties.get(&super::PROP_ADVENTURE_RANK.id).copied();
        if records.varints.is_empty()
            && list.len() >= 3
            && list.iter().all(|v| AR_REWARD_LEVELS.contains(v))
            && ar.is_some_and(|ar| list.iter().all(|v| *v <= ar))
        {
            self.collections.ar_rewards = Some(sorted());
            return Some(LoneList::ArRewards);
        }
        // Furnishing sets learned (`UnlockedFurnitureSuiteDataNotify`), the
        // companions' rewards collected (`HomeAvatarAllFinishRewardNotify`),
        // domains cleared once (`_GetDungeonEntryFirstPassRewardStatusRsp`)
        // and NPCs met (`MetNpcIdListNotify`): small ids all, from tables
        // whose ranges overlap, so a list is a table's only when nearly all
        // its ids are that table's and no other table holds as many. Many
        // short lists of small numbers fit several tables equally; those
        // stay out.
        if list.len() >= 3
            && let Ok(tables) = self.game_data.id_tables()
        {
            let share = |table: &std::collections::BTreeSet<u32>| {
                let held = list
                    .iter()
                    .filter(|v| u32::try_from(**v).is_ok_and(|v| table.contains(&v)))
                    .count();
                held as f64 / list.len() as f64
            };
            let scores = [
                (LoneList::FurnishingSets, share(&tables.furnishing_sets)),
                (LoneList::CompanionRewards, share(&tables.companion_rewards)),
                (LoneList::DomainsCleared, share(&tables.domains)),
                (LoneList::NpcsMet, share(&tables.npcs_met)),
            ];
            let best = scores
                .iter()
                .copied()
                .filter(|(_, score)| *score >= RECIPE_LIST_SHARE)
                .filter(|(kind, score)| scores.iter().all(|(k, s)| k == kind || s < score))
                .filter(|(kind, _)| {
                    *kind != LoneList::FurnishingSets || list.len() >= FURNISHING_SETS_MIN
                })
                .map(|(kind, _)| kind)
                .next();
            if let Some(kind) = best {
                let found = Some(sorted());
                let state = &mut self.collections;
                match kind {
                    LoneList::FurnishingSets => state.furnishing_sets = found,
                    LoneList::CompanionRewards => state.companion_rewards = found,
                    LoneList::DomainsCleared => state.domains_cleared = found,
                    _ => state.npcs_met = found,
                }
                return Some(kind);
            }
        }
        None
    }

    /// Keep Natlan's tribal chapters if `records` is their list (7.1:
    /// `GMKGAHNKOLA`, at login): records of a chapter id (the table's, every
    /// one once, at least half the table), a state 1-4 (not open, locked,
    /// open, finished) and maybe a start time, and nothing else.
    pub fn apply_tribal_chapters(&mut self, records: &RecordsMessage) -> Option<usize> {
        let table = &self.game_data.id_tables().ok()?.tribal_chapters;
        let entries = &records.records;
        if table.is_empty()
            || entries.len() * 2 < table.len()
            || entries.len() > table.len()
            || !records.lists.is_empty()
            || !records.varints.is_empty()
        {
            return None;
        }
        if entries
            .iter()
            .any(|e| !e.lists.is_empty() || e.varints.len() > 3)
        {
            return None;
        }
        // By field across the records: the id is distinct and the table's on
        // every one; the state 1-4 on every one; the start time, when sent.
        let tags: std::collections::BTreeSet<u32> = entries
            .iter()
            .flat_map(|e| e.varints.keys().copied())
            .collect();
        let column = |tag: u32| entries.iter().map(move |e| e.varints.get(&tag).copied());
        let on_all = |tag: u32| column(tag).all(|v| v.is_some());
        let id_tags: Vec<u32> = tags
            .iter()
            .copied()
            .filter(|t| on_all(*t))
            .filter(|t| {
                let ids: Vec<u64> = column(*t).flatten().collect();
                let unique: std::collections::BTreeSet<u64> = ids.iter().copied().collect();
                unique.len() == ids.len()
                    && ids
                        .iter()
                        .all(|v| u32::try_from(*v).is_ok_and(|v| table.contains(&v)))
            })
            .collect();
        let [id_tag] = id_tags.as_slice() else {
            return None;
        };
        let state_tags: Vec<u32> = tags
            .iter()
            .copied()
            .filter(|t| {
                t != id_tag && on_all(*t) && column(*t).flatten().all(|v| (1..=4).contains(&v))
            })
            .collect();
        let [state_tag] = state_tags.as_slice() else {
            return None;
        };
        let time_tag = tags.iter().copied().find(|t| t != id_tag && t != state_tag);
        if time_tag.is_some_and(|t| {
            !column(t)
                .flatten()
                .all(|v| (1_577_836_800..=u64::from(u32::MAX)).contains(&v))
        }) {
            return None;
        }
        let out: BTreeMap<u32, (u32, Option<u32>)> = entries
            .iter()
            .map(|e| {
                (
                    e.varints[id_tag] as u32,
                    (
                        e.varints[state_tag] as u32,
                        time_tag.and_then(|t| e.varints.get(&t)).map(|v| *v as u32),
                    ),
                )
            })
            .collect();
        let count = out.len();
        self.collections.tribal_chapters = Some(out);
        Some(count)
    }

    /// Keep one part of the collections, replacing what the game sent for it
    /// before (each message is the whole of its part).
    pub fn process_collection(&mut self, part: CollectionPart) {
        let state = &mut self.collections;
        match part {
            CollectionPart::Archive(found) => state.archive = Some(found),
            CollectionPart::Defeats(found) => state.defeats = Some(found),
            CollectionPart::Handbook(found) => state.handbook = Some(found),
            CollectionPart::Tcg(found) => state.tcg = Some(found),
            CollectionPart::Aranara(found) => state.aranara = Some(found),
            CollectionPart::WeaponSkins(found) => state.weapon_skins = Some(found),
            CollectionPart::Wardrobe(found) => state.wardrobe = Some(found),
            CollectionPart::Hangouts(found) => state.hangouts = Some(found),
            CollectionPart::Events(found) => state.events = Some(found),
            CollectionPart::Comfort(found) => state.comfort = Some(found),
            CollectionPart::TcgChallenges(found) => state.tcg_challenges = Some(found),
            CollectionPart::TcgTavern(found) => state.tcg_tavern = Some(found),
            CollectionPart::MusicGame(found) => state.music_game = Some(found),
            CollectionPart::BeyondCosmetics(found) => state.beyond_cosmetics = Some(found),
            CollectionPart::SignIns(found) => state.sign_ins = Some(found),
        }
    }

    /// `gi_collections`, or `None` when none of it was captured.
    pub(super) fn export_collections(&self) -> Option<good::GiCollections> {
        let state = &self.collections;
        let out = good::GiCollections {
            spincrystals: self.spincrystals.clone(),
            recipes: state.recipes.clone(),
            monsters_seen: state.monsters_seen.clone(),
            furnishing_sets: state.furnishing_sets.clone(),
            companion_rewards: state.companion_rewards.clone(),
            npcs_met: state.npcs_met.clone(),
            music_game: state.music_game.as_ref().map(|songs| {
                songs
                    .iter()
                    .map(|(id, song)| {
                        (
                            *id,
                            good::GiSong {
                                score: (song.score > 0).then_some(song.score),
                                claimed: song.claimed,
                            },
                        )
                    })
                    .collect()
            }),
            blueprints: state.blueprints.clone(),
            crafting: (state.crafting.is_some()
                || state.forging.is_some()
                || state.processing.is_some())
            .then(|| good::GiCrafting {
                crafting: state.crafting.clone(),
                forging: state.forging.clone(),
                processing: state.processing.clone(),
            }),
            hangouts: state.hangouts.as_ref().map(|hangouts| {
                hangouts
                    .iter()
                    .map(|(id, chapter)| (*id, hangout(chapter)))
                    .collect()
            }),
            archive: state.archive.as_ref().map(|archive| {
                archive
                    .iter()
                    .map(|(tab, ids)| (tab_key(*tab).to_string(), ids.clone()))
                    .collect()
            }),
            defeats: state.defeats.as_ref().map(|defeats| {
                defeats
                    .iter()
                    .map(|(id, count)| {
                        (
                            *id,
                            good::GiDefeats {
                                defeated: count.defeated,
                                captured: (count.captured > 0).then_some(count.captured),
                            },
                        )
                    })
                    .collect()
            }),
            handbook: state.handbook.as_ref().map(|handbook| good::GiHandbook {
                chapters: handbook
                    .chapters
                    .iter()
                    .map(|(id, p)| (*id, progress(p)))
                    .collect(),
                targets: handbook
                    .targets
                    .iter()
                    .map(|(quest, target)| {
                        (
                            *quest,
                            good::GiHandbookTarget {
                                progress: progress(&target.progress),
                                chapter: target.chapter,
                            },
                        )
                    })
                    .collect(),
            }),
            aranara: state.aranara.as_ref().map(|aranara| {
                aranara
                    .iter()
                    .map(|(kind, states)| {
                        (
                            *kind,
                            states
                                .iter()
                                .map(|(id, state)| (*id, aranara_state(*state).to_string()))
                                .collect(),
                        )
                    })
                    .collect()
            }),
            tcg: state.tcg.as_ref().map(|tcg| good::GiTcg {
                cards: tcg
                    .cards
                    .iter()
                    .map(|(id, card)| {
                        (
                            *id,
                            good::GiTcgCard {
                                count: card.count,
                                proficiency: (card.proficiency > 0).then_some(card.proficiency),
                            },
                        )
                    })
                    .collect(),
                decks: (!tcg.decks.is_empty()).then(|| {
                    tcg.decks
                        .iter()
                        .map(|(id, deck)| {
                            (
                                *id,
                                good::GiTcgDeck {
                                    name: deck.name.clone(),
                                    characters: deck.characters.clone(),
                                    cards: deck.cards.clone(),
                                    valid: deck.valid,
                                },
                            )
                        })
                        .collect()
                }),
                current_deck: tcg.current_deck,
                levels: state.tcg_challenges.as_ref().map(|c| c.levels.clone()),
                bosses: state
                    .tcg_challenges
                    .as_ref()
                    .map(|c| c.bosses.keys().copied().collect()),
                tavern: state.tcg_tavern.clone(),
            }),
        };
        (!out.is_empty()).then_some(out)
    }

    /// `gi_miliastra`, or `None` when none of it was captured.
    pub(super) fn export_miliastra(&self) -> Option<good::GiMiliastra> {
        let cosmetics = self.collections.beyond_cosmetics.clone()?;
        Some(good::GiMiliastra {
            cosmetics: Some(cosmetics),
        })
    }

    /// `gi_tasks.signIns`, or `None` when not captured.
    pub(super) fn export_sign_ins(&self) -> Option<BTreeMap<u32, good::GiSignIn>> {
        let sign_ins = self.collections.sign_ins.as_ref()?;
        Some(
            sign_ins
                .iter()
                .map(|(id, s)| {
                    (
                        *id,
                        good::GiSignIn {
                            days: s.days,
                            claimed: s.claimed.clone(),
                            last_at: s.last_at,
                            start: s.start,
                            end: s.end,
                        },
                    )
                })
                .collect(),
        )
    }

    /// `gi_teapot`, or `None` when none of it was captured.
    pub(super) fn export_teapot(&self) -> Option<good::GiTeapot> {
        let comfort = self.collections.comfort.as_ref()?;
        Some(good::GiTeapot {
            comfort: Some(
                comfort
                    .iter()
                    .map(|(id, c)| {
                        (
                            *id,
                            good::GiComfort {
                                mansion: c.mansion,
                                blocks: c.blocks.clone(),
                            },
                        )
                    })
                    .collect(),
            ),
        })
    }

    /// `gi_events`, or `None` when the events were not captured.
    pub(super) fn export_events(&self) -> Option<BTreeMap<u32, good::GiEvent>> {
        let events = self.collections.events.as_ref()?;
        Some(
            events
                .iter()
                .map(|(id, event)| {
                    let progress = (!event.watchers.is_empty()).then(|| {
                        event
                            .watchers
                            .iter()
                            .map(|(w, p)| {
                                (
                                    *w,
                                    good::GiEventProgress {
                                        progress: p.progress,
                                        total: p.total,
                                        claimed: p.claimed,
                                    },
                                )
                            })
                            .collect()
                    });
                    (
                        *id,
                        good::GiEvent {
                            schedule: event.schedule,
                            start: event.start,
                            end: event.end,
                            progress,
                        },
                    )
                })
                .collect(),
        )
    }

    /// `gi_wardrobe`, or `None` when none of it was captured. Who wears what
    /// comes from the characters held (outfits, gliders, trails) and from the
    /// weapon skins' own list of wearers.
    pub(super) fn export_wardrobe(&self) -> Option<good::GiWardrobe> {
        let state = &self.collections;
        let worn = |pick: fn(&super::AvatarInfo) -> u32| -> BTreeMap<String, u32> {
            self.characters
                .values()
                .filter(|avatar| pick(avatar) != 0)
                .filter_map(|avatar| Some((self.character_key_by_guid(avatar.guid)?, pick(avatar))))
                .collect()
        };
        let kind = |owned: &[u32], worn: BTreeMap<String, u32>| {
            (!owned.is_empty()).then(|| good::GiWardrobeKind {
                owned: owned.to_vec(),
                worn,
                expires: None,
            })
        };
        let wardrobe = state.wardrobe.as_ref();
        let out = good::GiWardrobe {
            outfits: wardrobe.and_then(|w| kind(&w.outfits, worn(|a| a.costume_id))),
            gliders: wardrobe.and_then(|w| kind(&w.gliders, worn(|a| a.wearing_flycloak_id))),
            trails: wardrobe.and_then(|w| kind(&w.trails, worn(|a| a.trace_effect_id))),
            weapon_skins: state.weapon_skins.as_ref().map(|skins| {
                let mut owned: Vec<u32> = skins.iter().map(|skin| skin.id).collect();
                owned.sort_unstable();
                good::GiWardrobeKind {
                    owned,
                    worn: skins
                        .iter()
                        .flat_map(|skin| {
                            skin.worn_by.iter().filter_map(|guid| {
                                Some((self.character_key_by_guid(*guid)?, skin.id))
                            })
                        })
                        .collect(),
                    expires: Some(
                        skins
                            .iter()
                            .filter_map(|skin| Some((skin.id, skin.expires?)))
                            .collect(),
                    )
                    .filter(|expires: &BTreeMap<u32, u32>| !expires.is_empty()),
                }
            }),
            teams: wardrobe.filter(|w| !w.teams.is_empty()).map(|w| {
                w.teams
                    .iter()
                    .map(|(slot, team)| {
                        (
                            *slot,
                            good::GiTeam {
                                name: team.name.clone(),
                                characters: team
                                    .avatar_guids
                                    .iter()
                                    .filter_map(|guid| self.character_key_by_guid(*guid))
                                    .collect(),
                            },
                        )
                    })
                    .collect()
            }),
            current_team: wardrobe.and_then(|w| w.current_team),
        };
        (!out.is_empty()).then_some(out)
    }
}
