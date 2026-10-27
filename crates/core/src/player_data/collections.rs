// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! `gi_collections` and `gi_wardrobe`: what the player has collected and
//! what their characters wear, each part as the game last sent it in full
//! (at login). The messages are recognised in the protocol library; this
//! keeps them and turns them into the export's shapes.

use std::collections::BTreeMap;

use teyvatme_protocol::{
    AranaraState, ArchiveTab, AvatarWardrobe, DefeatCount, Handbook, HandbookProgress,
    HandbookState, TcgCollection, WeaponSkin,
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
}

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

fn aranara_state(state: AranaraState) -> &'static str {
    match state {
        AranaraState::None => "none",
        AranaraState::Collectable => "collectable",
        AranaraState::Collected => "collected",
        AranaraState::Finished => "finished",
    }
}

impl PlayerData {
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
        }
    }

    /// `gi_collections`, or `None` when none of it was captured.
    pub(super) fn export_collections(&self) -> Option<good::GiCollections> {
        let state = &self.collections;
        let out = good::GiCollections {
            spincrystals: self.spincrystals.clone(),
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
            }),
        };
        (!out.is_empty()).then_some(out)
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
