// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Substat {
    pub key: String,
    pub value: f32,
    pub initial_value: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub set_key: String,
    pub slot_key: String,
    pub level: u32,
    pub rarity: u32,
    pub main_stat_key: String,
    pub location: String,
    pub lock: bool,
    pub substats: Vec<Substat>,

    // GOOD v3 fields.
    pub total_rolls: u32,
    pub astral_mark: bool,
    pub elixer_crafted: bool,
    pub unactivated_substats: Vec<Substat>,

    /// The game's id of this piece, as a decimal string (a u64 does not fit a
    /// JavaScript number): what live sync names it by (`delta.rs`). Tracker
    /// uploads only (`ExportSettings::item_guids`); omitted elsewhere. Not
    /// camelCase: Teyvat.me's own keys keep their `gi_` names.
    #[serde(rename = "gi_guid", skip_serializing_if = "Option::is_none", default)]
    pub gi_guid: Option<String>,
    /// EXP toward the next level, as the game holds it (0 at +0 and at the
    /// cap). Tracker uploads only, like `gi_guid`.
    #[serde(rename = "gi_exp", skip_serializing_if = "Option::is_none", default)]
    pub gi_exp: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Weapon {
    pub key: String,
    pub level: u32,
    pub ascension: u32,
    pub refinement: u32,
    pub location: String,
    pub lock: bool,
    /// The game's id of this weapon; see [`Artifact::gi_guid`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_guid: Option<String>,
    /// EXP toward the next level; see [`Artifact::gi_exp`].
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_exp: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TalentLevel {
    pub auto: u32,
    pub skill: u32,
    pub burst: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Character {
    pub key: String,
    pub level: u32,
    pub constellation: u32,
    pub ascension: u32,
    pub talent: TalentLevel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Good {
    pub format: String,
    pub version: u32,
    pub source: String,
    pub characters: Vec<Character>,
    pub artifacts: Vec<Artifact>,
    pub weapons: Vec<Weapon>,
    pub materials: HashMap<String, u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gi_achievements: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,

    // Teyvat.me's own additions. Not GOOD -- Genshin Optimizer owns that schema
    // -- so they ride as top-level keys of their own, like `gi_achievements`,
    // and come after every GOOD field: with all of them `None` the JSON is
    // byte for byte what it was before they existed. The tracker's importers
    // ignore top-level keys they do not know, so each can ship before the
    // tracker reads it. Append only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gi_player: Option<GiPlayer>,
    /// When each achievement in `gi_achievements` was finished, in unix
    /// seconds, keyed by achievement id (a JSON object key, so a string). Only
    /// achievements the game sent a plausible finish time for: a subset of
    /// `gi_achievements`, never more.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gi_achievement_times: Option<BTreeMap<u32, u32>>,
    /// Per-character values GOOD has no place for, keyed by the same GOOD key
    /// the character has in `characters` (Traveler with its element suffix).
    /// Only characters with at least one known value appear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gi_characters: Option<BTreeMap<String, GiCharacter>>,
    /// The Travel Accounts book, as the game last sent it. Omitted when the
    /// book's data was not captured, never sent empty: an absent key leaves
    /// the tracker's copy alone and keeps the content hash of a file without
    /// it unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gi_anecdotes: Option<GiAnecdotes>,
    /// Why this snapshot was taken and which play session it belongs to
    /// (live sync, plan section 5.1). Tracker uploads only.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_capture: Option<GiCapture>,
    /// The planner's routine (commissions, the Abyss, expeditions...), as the
    /// game last sent each part. Omitted when none was captured, and each
    /// part omitted when it was not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gi_tasks: Option<GiTasks>,
    /// What the player has collected (the Radiant Spincrystals so far).
    /// Omitted when none of it was captured, and each part when it was not.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_collections: Option<GiCollections>,
    /// What the characters own and wear: outfits, wind gliders, weapon skins,
    /// sprint trails, and the saved teams. Omitted when none was captured.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_wardrobe: Option<GiWardrobe>,
    /// How far each unfinished achievement is (those with a total).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_achievement_progress: Option<BTreeMap<u32, GiAchievementProgress>>,
    /// The events running when the game was asked (right after login), by
    /// event id, with their progress counters.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_events: Option<BTreeMap<u32, GiEvent>>,
    /// The Serenitea Pot (its Trust Rank follows from `gi_tasks.realm.limit`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_teapot: Option<GiTeapot>,
    /// Miliastra Wonderland.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_miliastra: Option<GiMiliastra>,
    /// Exploration: the Statues of The Seven, offerings, map points,
    /// exploration progress and reputation, as the game last sent each part.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gi_exploration: Option<GiExploration>,
}

/// `gi_collections`: what the player has collected, each part as the game
/// last sent it in full (at login). Append fields only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiCollections {
    /// The Radiant Spincrystals in the Serenitea Pot's music list, by item id
    /// (141001 = "Radiant Spincrystal 1"), ascending.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub spincrystals: Option<Vec<u32>>,
    /// The Archive's unlocked entries by tab (`quests`, `weapons`,
    /// `livingBeings`, `materials`, `books`, `tutorials`, `viewpoints`,
    /// `artifacts`), codex ids ascending.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub archive: Option<BTreeMap<String, Vec<u32>>>,
    /// By Living Beings codex id: enemies defeated, creatures captured. Only
    /// the ones with a count; `captured` left out at 0.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub defeats: Option<BTreeMap<u32, GiDefeats>>,
    /// The Adventurer Handbook.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub handbook: Option<GiHandbook>,
    /// Aranara collections by collection type, then id: `none`,
    /// `collectable`, `collected` or `finished`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub aranara: Option<BTreeMap<u32, BTreeMap<u32, String>>>,
    /// Genius Invokation TCG.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tcg: Option<GiTcg>,
    /// Every recipe unlocked → its proficiency (0 when none yet; the most is
    /// in the game data).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recipes: Option<BTreeMap<u32, u32>>,
    /// Hangout Events by chapter.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hangouts: Option<BTreeMap<u32, GiHangout>>,
    /// Every monster seen (monster ids, ascending).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub monsters_seen: Option<Vec<u32>>,
    /// Furnishing blueprints learned (the furnishings' item ids, ascending).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub blueprints: Option<Vec<u32>>,
    /// Recipes unlocked at the crafting bench, the blacksmith and processing.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub crafting: Option<GiCrafting>,
    /// The music game's songs by beatmap id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub music_game: Option<BTreeMap<u32, GiSong>>,
    /// Furnishing sets learned (`FurnitureSuiteExcelConfigData` ids).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub furnishing_sets: Option<Vec<u32>>,
    /// Companions' rewards collected in the Serenitea Pot (`HomeWorldEventExcelConfigData` ids).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub companion_rewards: Option<Vec<u32>>,
    /// NPCs met (`NpcFirstMetExcelConfigData` ids).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub npcs_met: Option<Vec<u32>>,
    /// Every parent quest touched (Capture 1.17.0).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub quests: Option<GiQuests>,
}

/// `gi_collections.quests`: parent quest ids (`MainQuestExcelConfigData`),
/// ascending, from the login's list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiQuests {
    pub finished: Vec<u32>,
    /// Accepted, not finished; left out when there are none.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub active: Vec<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiSong {
    /// Best score; absent when never played.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub score: Option<u32>,
    /// Its reward taken.
    pub claimed: bool,
}

/// `gi_collections.crafting`: recipe ids unlocked, ascending, by bench.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiCrafting {
    /// `CombineExcelConfigData` ids.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub crafting: Option<Vec<u32>>,
    /// `ForgeExcelConfigData` ids.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub forging: Option<Vec<u32>>,
    /// `CompoundExcelConfigData` ids.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub processing: Option<Vec<u32>>,
}

/// One Hangout Event chapter.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiHangout {
    /// `closed`, `locked` (its conditions not met), `open` or `accepted`;
    /// absent when it could not be read.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub state: Option<String>,
    /// By ending: `locked` (not seen), `unclaimed` (seen, reward waiting) or
    /// `claimed`.
    pub endings: BTreeMap<u32, String>,
    /// Pictures unlocked, ascending.
    pub pictures: Vec<u32>,
    /// Pictures the chapter has.
    pub pictures_total: u32,
}

impl GiCollections {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiDefeats {
    pub defeated: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub captured: Option<u32>,
}

/// A Handbook chapter's or target's progress. `state`: `open`, `complete`
/// (done, reward not taken) or `claimed`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiProgress {
    pub progress: u32,
    pub total: u32,
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiHandbook {
    /// By chapter id.
    pub chapters: BTreeMap<u32, GiProgress>,
    /// By the target's quest id.
    pub targets: BTreeMap<u32, GiHandbookTarget>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiHandbookTarget {
    #[serde(flatten)]
    pub progress: GiProgress,
    pub chapter: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTcg {
    /// Cards owned by card id (character cards below 10000).
    pub cards: BTreeMap<u32, GiTcgCard>,
    /// Saved decks by id; `name` is the player's own text.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub decks: Option<BTreeMap<u32, GiTcgDeck>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub current_deck: Option<u32>,
    /// Challenge levels played, by level id (`GCGLevelExcelConfigData`): the
    /// challenge ids finished in each.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub levels: Option<BTreeMap<u32, Vec<u32>>>,
    /// Boss challenges unlocked (`GCGBossLevelExcelConfigData` ids).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bosses: Option<Vec<u32>>,
    /// The Tavern: by character, the level ids unlocked with them.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tavern: Option<BTreeMap<u32, Vec<u32>>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTcgCard {
    pub count: u32,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub proficiency: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTcgDeck {
    pub name: String,
    pub characters: Vec<u32>,
    pub cards: Vec<u32>,
    pub valid: bool,
}

/// `gi_miliastra`: Miliastra Wonderland. Append fields only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiMiliastra {
    /// Cosmetics owned (outfits, effects...) by id: when each was obtained,
    /// 0 when the game sent no time.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cosmetics: Option<BTreeMap<u32, u32>>,
}

/// `gi_teapot`: the Serenitea Pot. Append fields only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTeapot {
    /// Comfort by realm (`HomeWorldModuleExcelConfigData` id).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub comfort: Option<BTreeMap<u32, GiComfort>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiComfort {
    /// The mansion's comfort.
    pub mansion: u32,
    /// Each outdoor block's.
    pub blocks: Vec<u32>,
}

/// One event running.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiEvent {
    /// This run of it (the event id times 1000 plus the run).
    pub schedule: u32,
    pub start: u32,
    pub end: u32,
    /// Its progress counters by watcher id; absent when it has none.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub progress: Option<BTreeMap<u32, GiEventProgress>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiEventProgress {
    pub progress: u32,
    pub total: u32,
    /// Its reward taken.
    pub claimed: bool,
}

/// One unfinished achievement's progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiAchievementProgress {
    pub progress: u32,
    pub total: u32,
}

/// `gi_wardrobe`: what the characters own and wear, as the game last sent it
/// (at login). Append fields only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiWardrobe {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outfits: Option<GiWardrobeKind>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub gliders: Option<GiWardrobeKind>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub weapon_skins: Option<GiWardrobeKind>,
    /// Sprint trails.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub trails: Option<GiWardrobeKind>,
    /// Saved teams by slot; `name` is the player's own text.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub teams: Option<BTreeMap<u32, GiTeam>>,
    /// The team slot in use.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub current_team: Option<u32>,
}

impl GiWardrobe {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiWardrobeKind {
    /// Owned ids, ascending.
    pub owned: Vec<u32>,
    /// Who wears which, by character key.
    pub worn: BTreeMap<String, u32>,
    /// Owned only until then (trial weapon skins), unix seconds.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expires: Option<BTreeMap<u32, u32>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTeam {
    pub name: String,
    pub characters: Vec<String>,
}

/// `gi_exploration` (see `docs/teyvatme-export.ts`). Ids are the game's; keys
/// are JSON strings. Each part omitted when it was not captured.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiExploration {
    /// By city (`CityConfigData`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statues: Option<BTreeMap<u32, GiStatue>>,
    /// Level by offering (`OfferingOpenStateConfigData`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offerings: Option<BTreeMap<u32, u32>>,
    /// By scene: map points unlocked and areas revealed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenes: Option<BTreeMap<u32, GiScene>>,
    /// Exploration points by area (`WorldAreaConfigData.areaID1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explore: Option<BTreeMap<u32, u32>>,
    /// Reputation level by city.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reputation: Option<BTreeMap<u32, u32>>,
    /// Domains whose first-clear reward was taken (`DungeonEntryExcelConfigData` ids).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub domains_cleared: Option<Vec<u32>>,
    /// Natlan's tribal chapters (`TribalQuestChapterExcelConfigData` ids).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tribal_chapters: Option<BTreeMap<u32, GiTribalChapter>>,
}

/// One tribal chapter: `notOpen`, `locked`, `open` or `finished`, and when it
/// started.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTribalChapter {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub started_at: Option<u32>,
}

impl GiExploration {
    pub fn is_empty(&self) -> bool {
        self.statues.is_none()
            && self.offerings.is_none()
            && self.scenes.is_none()
            && self.explore.is_none()
            && self.reputation.is_none()
            && self.domains_cleared.is_none()
            && self.tribal_chapters.is_none()
    }
}

/// One Statue of The Seven.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct GiStatue {
    pub level: u32,
    /// Oculi given toward the next level.
    pub oculi: u32,
}

/// One scene's map.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct GiScene {
    /// Map points unlocked (waypoints, statues, domains...), sorted.
    pub points: Vec<u32>,
    /// Map areas revealed, sorted.
    pub areas: Vec<u32>,
}

/// `gi_capture`: why a tracker snapshot was taken, and which game login it
/// belongs to. A wire contract with the tracker (`live/capture.ts`); append
/// fields and reasons only.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct GiCapture {
    /// Always 1.
    pub v: u32,
    /// `login`, `world`, `manual`, `collapse`, `logout`, `resync` or `change`.
    pub reason: String,
    /// Teyvat.me's id of this game login: 16+ characters of `[A-Za-z0-9_-]`.
    pub session: String,
    /// 1 = the first world of the login, 2+ = each later full send of the
    /// account in the same connection, by order (Teyvat.me cannot tell the
    /// player's own world from a co-op host's).
    pub world: u32,
    /// The journal this snapshot ends.
    pub journal: GiCaptureJournal,
}

/// `gi_capture.journal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct GiCaptureJournal {
    /// The last batch sent on the journal this snapshot ends (0 at login).
    pub seq: u64,
}

/// How `gi_player.uid` was read off the item guids (see
/// `PlayerData::uid_check`; logged with every export, not exported): the top 32 bits of each non-zero item guid are
/// counted, and the most common value is the UID when enough items agree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UidCheck {
    /// The UID reported in `gi_player.uid`, if any.
    pub uid: Option<u32>,
    /// Item guids whose top half is the most common one.
    pub agreeing: usize,
    /// Item guids counted (non-zero).
    pub total: usize,
    /// The most common item guid top halves, `[top, count]`, most first.
    pub item_top_halves: Vec<(u64, usize)>,
    /// The same for avatar guids, which don't vote (field unverified).
    pub avatar_top_halves: Vec<(u64, usize)>,
    /// Items whose guid top half is not the UID, grouped (most first, at most
    /// 100 groups): what they are, to explain where they came from.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub strays: Vec<UidStray>,
}

/// Items of one kind and id whose guids carry one foreign top half.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UidStray {
    /// The guid top half (another account's UID, if the scheme holds).
    pub top: u64,
    pub item_id: u32,
    /// `material`, `weapon`, `artifact`, `furniture` or `other`.
    pub kind: String,
    /// From the game data, when it knows the item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub count: usize,
}

/// One character's entry in `gi_characters`.
///
/// Each value is omitted, never null, when it was not captured or failed its
/// plausibility check -- and for every character at once when most of them
/// failed it, since that means the field is read from the wrong place in this
/// game version.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiCharacter {
    /// Friendship level, 1..=10.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendship: Option<u32>,
    /// When the character joined the account, in unix seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obtained_at: Option<u32>,
    /// EXP toward the next level (0 at the level cap).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u32>,
    /// Friendship EXP toward the next friendship level (0 at 10).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendship_exp: Option<u32>,
}

/// `gi_anecdotes`: the Travel Accounts book. The contract with the tracker is
/// `GI_ANECDOTES.md` (v1, 2026-10-12); append fields only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiAnecdotes {
    /// Every anecdote the player has met, keyed by id (a JSON object key, so a
    /// string). An id that is absent was never met -- not "not done".
    pub anecdotes: BTreeMap<u32, GiAnecdote>,
    /// The book's unlocked area tabs, sorted and unique.
    pub unlocked_areas: Vec<u32>,
    /// The reward track. Omitted when its counters could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<GiAnecdoteReward>,
    /// The last wish. Omitted when none was made (or none was read).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wish: Option<GiAnecdoteWish>,
}

/// One entry of `gi_anecdotes.anecdotes`. Both values are always written; a
/// state the game left off the wire is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiAnecdote {
    /// 3 = recorded, 2 = in progress, 0 = idle (inferred, see the contract).
    pub state: u32,
    /// Times completed; above 1 only for the repeatable loops.
    pub finish_count: u32,
}

/// `gi_anecdotes.reward`: every `target` recorded anecdotes trade for a reward.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiAnecdoteReward {
    /// Which exchange is next.
    pub index: u32,
    /// Anecdotes recorded toward it.
    pub progress: u32,
    /// Anecdotes it needs (10 in 7.1).
    pub target: u32,
}

/// `gi_anecdotes.wish`: the NPC the player asked the book about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiAnecdoteWish {
    /// Omitted when no wish is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npc_id: Option<u32>,
    /// When the last wish was made, in unix seconds.
    pub last_wish_time: u32,
}

/// `gi_tasks`: what the planner's tasks can be ticked from. The contract with
/// the tracker is `GI_TASKS.md` (v1, 2026-10-15); append fields only. Times
/// are unix seconds. Each part is the game's state when it last sent it (at
/// login, mostly), not a live value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiTasks {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commissions: Option<GiCommissions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abyss: Option<GiAbyss>,
    /// The characters out on an expedition (an empty list: nobody is out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expeditions: Option<Vec<GiExpedition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub realm: Option<GiRealm>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub welkin: Option<GiWelkin>,
    /// The weapon banner's Epitomized Path, as the Wish screen last showed it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wish_path: Option<GiWishPath>,
    /// Original Resin and when the next one comes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resin: Option<GiResin>,
    /// The weekly bosses' resin discounts this week.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub weekly_bosses: Option<GiWeeklyBosses>,
    /// Adventure Ranks whose reward was claimed (2-60, ascending).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ar_rewards: Option<Vec<u32>>,
    /// Login events' calendars by config id.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sign_ins: Option<BTreeMap<u32, GiSignIn>>,
    /// When each boss and elite of the Adventurer Handbook respawns, by its
    /// Handbook id (`InvestigationMonsterConfigData`): one time per spot, unix
    /// seconds, ascending; a time already past means it is up.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub respawns: Option<BTreeMap<u32, Vec<u32>>>,
    /// The Original Resin spent today (Capture 1.16.0).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resin_spent: Option<GiResinSpent>,
    /// Paimon's Bargains as the game last showed it (Capture 1.17.2).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bargains: Option<GiBargains>,
}

/// `gi_tasks.bargains`: Paimon's Bargains as opening it (or a purchase
/// there) last showed it. Its counts are this month's, as of `at`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiBargains {
    /// When the game said it (unix seconds).
    pub at: u32,
    /// Whether anything at all was bought there this month.
    pub bought: bool,
    /// The Fates on sale for Masterless Stardust and Starglitter.
    pub fates: Vec<GiFateStock>,
}

/// One Fate on sale in Paimon's Bargains.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiFateStock {
    /// `IntertwinedFate` or `AcquaintFate`.
    pub item: String,
    /// `MasterlessStardust` or `MasterlessStarglitter`.
    pub currency: String,
    /// How much of it one costs.
    pub cost: u32,
    /// Bought this month.
    pub bought: u32,
    /// How many a month; absent when it has no limit.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub limit: Option<u32>,
}

/// `gi_tasks.resinSpent`: the Original Resin spent on one game day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiResinSpent {
    pub value: u32,
    /// That day of the game's week, 1 (Monday) to 7.
    pub day: u32,
    /// When the game first said it (unix seconds): the day it belongs to is
    /// the game day this falls in.
    pub at: u32,
}

/// `gi_tasks.signIns`: one login event's calendar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiSignIn {
    /// Days signed in.
    pub days: u32,
    /// Days whose reward was taken.
    pub claimed: Vec<u32>,
    pub last_at: u32,
    pub start: u32,
    pub end: u32,
}

/// `gi_tasks.weeklyBosses`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiWeeklyBosses {
    /// Discounts left this week, of `limit`. Exports without `used` (1.13.2
    /// and before) held the count used here instead.
    pub left: u32,
    /// Discounts used this week; its presence says `left` is the count left.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub used: Option<u32>,
    pub limit: u32,
    /// What a weekly boss costs with the discount, and without it.
    pub cost: u32,
    pub full_cost: u32,
}

impl GiTasks {
    pub fn is_empty(&self) -> bool {
        self.commissions.is_none()
            && self.abyss.is_none()
            && self.expeditions.is_none()
            && self.realm.is_none()
            && self.welkin.is_none()
            && self.wish_path.is_none()
            && self.resin.is_none()
            && self.weekly_bosses.is_none()
            && self.ar_rewards.is_none()
            && self.sign_ins.is_none()
            && self.respawns.is_none()
            && self.resin_spent.is_none()
            && self.bargains.is_none()
    }
}

/// `gi_tasks.wishPath`: the weapon charted on the Epitomized Path and its Fate
/// Points.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiWishPath {
    /// The weapon's GOOD key.
    pub weapon: String,
    pub points: u32,
    pub max: u32,
}

/// `gi_tasks.resin`: Original Resin when it was read, and when the next comes
/// (unix seconds; one every 8 minutes below the cap).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiResin {
    pub value: u32,
    pub next_at: u32,
}

/// `gi_tasks.commissions`: today's Daily Commissions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiCommissions {
    pub finished: u32,
    pub total: u32,
    /// Whether Katheryne's reward for the day was taken; absent when not
    /// known (Capture 1.16.0).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub claimed: Option<bool>,
}

/// `gi_tasks.abyss`: the Spiral Abyss period running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiAbyss {
    pub start: u32,
    /// When the next period begins.
    pub end: u32,
    pub stars: u32,
    pub max_stars: u32,
}

/// One entry of `gi_tasks.expeditions`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiExpedition {
    /// The character's GOOD key. Omitted when the roster was not captured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    /// The game's expedition id.
    pub id: u32,
    pub start: u32,
    pub hours: u32,
    /// When it is done: `start + hours`, less what a talent takes off.
    pub done_at: u32,
}

/// `gi_tasks.realm`: the Serenitea Pot's realm currency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiRealm {
    pub value: u32,
    pub limit: u32,
}

/// `gi_tasks.welkin`: the Blessing of the Welkin Moon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiWelkin {
    pub days: u32,
}

/// Account values GOOD has no place for: `gi_player`.
///
/// The numbers are the property snapshot the game sends at login, not live
/// values (resin keeps regenerating after it). Every field is omitted, never
/// null, when it was not captured or failed its plausibility check, and the
/// whole key is omitted when nothing about the account is known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiPlayer {
    /// The account UID, from the item and avatar guids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    /// Adventure Rank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ar: Option<u32>,
    /// Adventure EXP toward the next rank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ar_exp: Option<u32>,
    /// World Level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wl: Option<u32>,
    /// The highest World Level the account may choose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wl_limit: Option<u32>,
    /// Original Resin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resin: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub story_keys: Option<u32>,
    /// Maximum stamina in the game's units: 24000 is the 240 the game shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_stamina: Option<u32>,
    /// The Dimbreath dump commit the export's GOOD keys were derived from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_data: Option<String>,
    /// The in-game name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub nickname: Option<String>,
    /// The profile's signature: "" when the profile card came without one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub signature: Option<String>,
    /// The name card on the profile (an item id, 210001...).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name_card: Option<u32>,
    /// The profile picture, as the game sends it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub profile_picture: Option<GiProfilePicture>,
    /// The birthday set on the profile.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub birthday: Option<GiBirthday>,
}

/// `gi_player.profilePicture`: every part optional. 7.1 sends `id` (a
/// `ProfilePictureExcelConfigData` id) and `frameId`; older clients a
/// character (`avatarId`) and its outfit (`costumeId`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GiProfilePicture {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub avatar_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub costume_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub frame_id: Option<u32>,
}

/// `gi_player.birthday`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct GiBirthday {
    pub month: u32,
    pub day: u32,
}

/// GOOD key for the Traveler before their element is appended.
pub const TRAVELER_KEY: &str = "Traveler";

pub fn to_good_key(value: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;

    for c in value.chars() {
        if c.is_ascii_alphanumeric() {
            if capitalize_next {
                result.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                result.push(c);
            }
        } else if c == ' ' {
            capitalize_next = true;
        }
    }

    result
}

pub fn fake_uninitialized_4th_line(artifacts: Vec<Artifact>) -> Vec<Artifact> {
    artifacts
        .into_iter()
        .map(|mut arti| {
            if arti.unactivated_substats.is_empty() || arti.rarity != 5 {
                return arti;
            }
            arti.substats.push(arti.unactivated_substats.pop().unwrap());
            Artifact {
                level: 4,
                total_rolls: 4,
                ..arti
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_keys_strip_symbols_and_pascal_case() {
        assert_eq!(to_good_key("Chilled Meat"), "ChilledMeat");
        assert_eq!(
            to_good_key("Adventurer's Experience"),
            "AdventurersExperience"
        );
        assert_eq!(to_good_key("Traveler"), TRAVELER_KEY);
        assert_eq!(to_good_key("Gladiator's Finale"), "GladiatorsFinale");
    }

    #[test]
    fn names_with_no_ascii_alphanumerics_have_no_good_key() {
        // 16 items are literally named "？？？" (fullwidth question marks);
        // exporting them would add a `"": n` entry to GOOD `materials`.
        assert_eq!(to_good_key("？？？"), "");
        assert_eq!(to_good_key(""), "");
        assert_eq!(to_good_key("   "), "");
    }

    fn substat(key: &str) -> Substat {
        Substat {
            key: key.to_string(),
            value: 1.0,
            initial_value: 1.0,
        }
    }

    fn artifact(rarity: u32, unactivated: Vec<Substat>) -> Artifact {
        Artifact {
            set_key: "GladiatorsFinale".to_string(),
            slot_key: "flower".to_string(),
            level: 0,
            rarity,
            main_stat_key: "hp".to_string(),
            location: String::new(),
            lock: false,
            substats: vec![substat("critRate_")],
            total_rolls: 1,
            astral_mark: false,
            elixer_crafted: false,
            unactivated_substats: unactivated,
            gi_guid: Some("3488998120056947072".to_string()),
            gi_exp: Some(1_250),
        }
    }

    #[test]
    fn fake_4th_line_only_promotes_five_star_artifacts_with_an_unactivated_substat() {
        let faked = fake_uninitialized_4th_line(vec![
            artifact(5, vec![substat("critDMG_")]),
            artifact(5, Vec::new()),
            artifact(4, vec![substat("critDMG_")]),
        ]);

        assert_eq!(faked[0].substats.len(), 2);
        assert_eq!(faked[0].level, 4);
        assert_eq!(faked[0].total_rolls, 4);
        assert!(faked[0].unactivated_substats.is_empty());

        // Untouched: nothing to promote, and not a five star.
        assert_eq!(faked[1].substats.len(), 1);
        assert_eq!(faked[1].level, 0);
        assert_eq!(faked[2].substats.len(), 1);
        assert_eq!(faked[2].level, 0);
    }

    /// The JSON key names below are a cross-repo contract, not a style choice.
    ///
    /// `Good` is uploaded verbatim to the tracker's
    /// `POST /genshin-accounts-public/import-by-key`, which runs a bare
    /// `JSON.parse` and reads fields by name with no schema validation and no
    /// error on a miss (`genshin-accounts.service.ts::processImport`). So a
    /// rename here does not fail an import -- it silently drops whatever it
    /// renamed. The two easiest mistakes both look harmless in review:
    ///
    /// * adding `#[serde(rename_all = "camelCase")]` to `Good` turns
    ///   `gi_achievements` into `giAchievements`, and every snapshot then
    ///   imports zero achievements;
    /// * removing it from `Artifact` turns `setKey` into `set_key`, and every
    ///   artifact then hashes as `{"setKey":null,...}` -- one shared row for
    ///   the entire inventory.
    ///
    /// Neither raises anything anywhere. This test is the only thing that
    /// notices.
    #[test]
    fn good_json_keys_match_what_the_tracker_reads() {
        let good = Good {
            format: "GOOD".to_string(),
            version: 3,
            source: "Irminsul".to_string(),
            characters: vec![Character {
                key: "HuTao".to_string(),
                level: 90,
                constellation: 1,
                ascension: 6,
                talent: TalentLevel {
                    auto: 10,
                    skill: 9,
                    burst: 8,
                },
            }],
            artifacts: vec![artifact(5, vec![substat("critDMG_")])],
            weapons: vec![Weapon {
                key: "StaffOfHoma".to_string(),
                level: 90,
                ascension: 6,
                refinement: 1,
                location: "HuTao".to_string(),
                lock: true,
                gi_guid: Some("3488998120056947188".to_string()),
                gi_exp: Some(0),
            }],
            materials: HashMap::from([("ChilledMeat".to_string(), 12)]),
            gi_achievements: Some(vec![80001]),
            timestamp: Some(1_756_000_000_000),
            gi_player: Some(GiPlayer {
                uid: Some(811_111_111),
                ar: Some(60),
                ar_exp: Some(0),
                wl: Some(8),
                wl_limit: Some(9),
                resin: Some(124),
                story_keys: Some(3),
                max_stamina: Some(24_000),
                game_data: Some("792978e5503ecfba73dcb3562ed44a0d35a2abe2".to_string()),
                nickname: Some("player01".to_string()),
                signature: Some(String::new()),
                name_card: Some(210_001),
                profile_picture: Some(GiProfilePicture {
                    id: Some(1001),
                    ..Default::default()
                }),
                birthday: Some(GiBirthday { month: 1, day: 2 }),
            }),
            gi_achievement_times: Some(BTreeMap::from([(80001, 1_650_000_000)])),
            gi_characters: Some(BTreeMap::from([(
                "HuTao".to_string(),
                GiCharacter {
                    friendship: Some(10),
                    obtained_at: Some(1_646_092_800),
                    exp: Some(0),
                    friendship_exp: Some(0),
                },
            )])),
            gi_anecdotes: Some(GiAnecdotes {
                anecdotes: BTreeMap::from([
                    (
                        100_001,
                        GiAnecdote {
                            state: 3,
                            finish_count: 1,
                        },
                    ),
                    (
                        101_604,
                        GiAnecdote {
                            state: 0,
                            finish_count: 3,
                        },
                    ),
                ]),
                unlocked_areas: vec![1, 2, 3],
                reward: Some(GiAnecdoteReward {
                    index: 12,
                    progress: 4,
                    target: 10,
                }),
                wish: Some(GiAnecdoteWish {
                    npc_id: Some(9801),
                    last_wish_time: 1_791_000_000,
                }),
            }),
            gi_capture: Some(GiCapture {
                v: 1,
                reason: "login".to_string(),
                session: "01J9ZK5Q8W3E7R2T".to_string(),
                world: 1,
                journal: GiCaptureJournal { seq: 0 },
            }),
            gi_tasks: Some(GiTasks {
                commissions: Some(GiCommissions {
                    finished: 4,
                    total: 4,
                    claimed: None,
                }),
                abyss: Some(GiAbyss {
                    start: 1_789_502_400,
                    end: 1_792_094_399,
                    stars: 30,
                    max_stars: 36,
                }),
                expeditions: Some(vec![GiExpedition {
                    character: Some("Xiangling".to_string()),
                    id: 202,
                    start: 1_791_000_000,
                    hours: 20,
                    done_at: 1_791_072_000,
                }]),
                realm: Some(GiRealm {
                    value: 1_800,
                    limit: 2_400,
                }),
                welkin: Some(GiWelkin { days: 30 }),
                wish_path: Some(GiWishPath {
                    weapon: "StaffOfHoma".to_string(),
                    points: 1,
                    max: 1,
                }),
                resin: Some(GiResin {
                    value: 124,
                    next_at: 1_791_000_300,
                }),
                weekly_bosses: None,
                ar_rewards: None,
                sign_ins: None,
                respawns: None,
                resin_spent: None,
                bargains: None,
            }),
            gi_collections: Some(GiCollections {
                spincrystals: Some(vec![141001, 141002]),
                ..Default::default()
            }),
            gi_wardrobe: Some(GiWardrobe {
                current_team: Some(1),
                ..Default::default()
            }),
            gi_miliastra: Some(GiMiliastra {
                cosmetics: Some(BTreeMap::from([(260_087, 1_790_000_000)])),
            }),
            gi_teapot: Some(GiTeapot {
                comfort: Some(BTreeMap::from([(
                    2,
                    GiComfort {
                        mansion: 14_600,
                        blocks: vec![13_470],
                    },
                )])),
            }),
            gi_events: Some(BTreeMap::from([(
                2051,
                GiEvent {
                    schedule: 2_051_001,
                    start: 1_790_000_000,
                    end: 1_791_000_000,
                    progress: None,
                },
            )])),
            gi_achievement_progress: Some(BTreeMap::from([(
                80002,
                GiAchievementProgress {
                    progress: 3,
                    total: 10,
                },
            )])),
            gi_exploration: Some(GiExploration {
                statues: Some(BTreeMap::from([(
                    1,
                    GiStatue {
                        level: 10,
                        oculi: 0,
                    },
                )])),
                offerings: Some(BTreeMap::from([(1, 12)])),
                scenes: Some(BTreeMap::from([(
                    3,
                    GiScene {
                        points: vec![7, 12],
                        areas: vec![1],
                    },
                )])),
                explore: Some(BTreeMap::from([(1, 278)])),
                reputation: Some(BTreeMap::from([(1, 8)])),
                domains_cleared: None,
                tribal_chapters: None,
            }),
        };

        let json = serde_json::to_value(&good).expect("Good must serialize");

        let keys = |value: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = value
                .as_object()
                .expect("expected a JSON object")
                .keys()
                .cloned()
                .collect();
            keys.sort();
            keys
        };

        // `format`/`version`/`source` are inert for the tracker but are what
        // Genshin Optimizer identifies the file by, so they are part of the
        // contract too.
        assert_eq!(
            keys(&json),
            [
                "artifacts",
                "characters",
                "format",
                "gi_achievement_progress",
                "gi_achievement_times",
                "gi_achievements",
                "gi_anecdotes",
                "gi_capture",
                "gi_characters",
                "gi_collections",
                "gi_events",
                "gi_exploration",
                "gi_miliastra",
                "gi_player",
                "gi_tasks",
                "gi_teapot",
                "gi_wardrobe",
                "materials",
                "source",
                "timestamp",
                "version",
                "weapons",
            ]
        );

        // CHARACTER_SCHEMA in the backend's data-packer.util.ts.
        assert_eq!(
            keys(&json["characters"][0]),
            ["ascension", "constellation", "key", "level", "talent"]
        );
        assert_eq!(
            keys(&json["characters"][0]["talent"]),
            ["auto", "burst", "skill"]
        );

        // WEAPON_SCHEMA in the backend's data-packer.util.ts.
        assert_eq!(
            keys(&json["weapons"][0]),
            [
                "ascension",
                "gi_exp",
                "gi_guid",
                "key",
                "level",
                "location",
                "lock",
                "refinement"
            ]
        );

        // The first five feed the backend's SHA-256 artifact hash; `location`,
        // `lock` and `astralMark` are its mutable live state; `totalRolls`,
        // `elixerCrafted` and `substats` are stored on the row. Note
        // `elixerCrafted` really is spelled that way on both sides.
        assert_eq!(
            keys(&json["artifacts"][0]),
            [
                "astralMark",
                "elixerCrafted",
                "gi_exp",
                "gi_guid",
                "level",
                "location",
                "lock",
                "mainStatKey",
                "rarity",
                "setKey",
                "slotKey",
                "substats",
                "totalRolls",
                "unactivatedSubstats",
            ]
        );

        // `initialValue` is hashed alongside `key`/`value`, so renaming it
        // would re-hash every artifact in every account exactly once and
        // orphan the old rows.
        assert_eq!(
            keys(&json["artifacts"][0]["substats"][0]),
            ["initialValue", "key", "value"]
        );

        // Epoch milliseconds as a JSON number: the backend's
        // `resolveImportTimestamp` dates the snapshot by this when the
        // multipart `timestamp` field is absent.
        assert_eq!(json["timestamp"], serde_json::json!(1_756_000_000_000u64));
        assert_eq!(json["materials"]["ChilledMeat"], serde_json::json!(12));

        // Teyvat.me's own account values. The UID is a JSON number; the
        // tracker's account UID is a string of 9 or 10 digits.
        assert_eq!(
            keys(&json["gi_player"]),
            [
                "ar",
                "arExp",
                "birthday",
                "gameData",
                "maxStamina",
                "nameCard",
                "nickname",
                "profilePicture",
                "resin",
                "signature",
                "storyKeys",
                "uid",
                "wl",
                "wlLimit",
            ]
        );
        assert_eq!(json["gi_player"]["uid"], serde_json::json!(811_111_111));

        // Achievement id (a JSON object key, so a string) -> unix seconds.
        assert_eq!(
            json["gi_achievement_times"],
            serde_json::json!({ "80001": 1_650_000_000 })
        );

        // Keyed like `characters`; unix seconds for `obtainedAt`.
        assert_eq!(keys(&json["gi_characters"]), ["HuTao"]);
        assert_eq!(
            keys(&json["gi_characters"]["HuTao"]),
            ["exp", "friendship", "friendshipExp", "obtainedAt"]
        );

        // `GI_ANECDOTES.md`: anecdote ids are object keys (strings); the
        // state is written even when 0.
        assert_eq!(
            keys(&json["gi_anecdotes"]),
            ["anecdotes", "reward", "unlockedAreas", "wish"]
        );
        assert_eq!(
            json["gi_anecdotes"]["anecdotes"],
            serde_json::json!({
                "100001": { "state": 3, "finishCount": 1 },
                "101604": { "state": 0, "finishCount": 3 },
            })
        );
        assert_eq!(
            json["gi_anecdotes"]["unlockedAreas"],
            serde_json::json!([1, 2, 3])
        );
        assert_eq!(
            json["gi_anecdotes"]["reward"],
            serde_json::json!({ "index": 12, "progress": 4, "target": 10 })
        );
        assert_eq!(
            json["gi_anecdotes"]["wish"],
            serde_json::json!({ "npcId": 9801, "lastWishTime": 1_791_000_000 })
        );

        // Live sync (the tracker's `live/capture.ts` and `live/delta.ts`):
        // item guids are decimal strings, a u64 does not fit a JS number.
        assert_eq!(
            keys(&json["gi_capture"]),
            ["journal", "reason", "session", "v", "world"]
        );
        assert_eq!(keys(&json["gi_capture"]["journal"]), ["seq"]);
        assert_eq!(
            json["artifacts"][0]["gi_guid"],
            serde_json::json!("3488998120056947072")
        );
        assert_eq!(
            json["weapons"][0]["gi_guid"],
            serde_json::json!("3488998120056947188")
        );

        // `GI_TASKS.md`: unix seconds; each part omitted when not captured.
        assert_eq!(
            json["gi_tasks"],
            serde_json::json!({
                "commissions": { "finished": 4, "total": 4 },
                "abyss": {
                    "start": 1_789_502_400,
                    "end": 1_792_094_399,
                    "stars": 30,
                    "maxStars": 36,
                },
                "expeditions": [{
                    "character": "Xiangling",
                    "id": 202,
                    "start": 1_791_000_000,
                    "hours": 20,
                    "doneAt": 1_791_072_000,
                }],
                "realm": { "value": 1_800, "limit": 2_400 },
                "welkin": { "days": 30 },
                "wishPath": { "weapon": "StaffOfHoma", "points": 1, "max": 1 },
                "resin": { "value": 124, "nextAt": 1_791_000_300 },
            })
        );

        // `docs/teyvatme-export.ts` GiExploration: id-keyed objects.
        assert_eq!(
            json["gi_exploration"],
            serde_json::json!({
                "statues": { "1": { "level": 10, "oculi": 0 } },
                "offerings": { "1": 12 },
                "scenes": { "3": { "points": [7, 12], "areas": [1] } },
                "explore": { "1": 278 },
                "reputation": { "1": 8 },
            })
        );
    }

    #[test]
    fn absent_achievements_and_timestamp_are_omitted_rather_than_null() {
        // `skip_serializing_if` matters: the backend tests
        // `Array.isArray(parsedData.gi_achievements)` and falls back to `[]`,
        // and feeds `parsedData.timestamp` to `resolveImportTimestamp`, which
        // maps a null to "now". An explicit `null` would work, but only by
        // accident -- keep the omission the tracker was written against.
        let good = Good {
            format: "GOOD".to_string(),
            version: 3,
            source: "Irminsul".to_string(),
            characters: Vec::new(),
            artifacts: Vec::new(),
            weapons: Vec::new(),
            materials: HashMap::new(),
            gi_achievements: None,
            timestamp: None,
            gi_player: None,
            gi_achievement_times: None,
            gi_characters: None,
            gi_anecdotes: None,
            gi_capture: None,
            gi_tasks: None,
            gi_collections: None,
            gi_wardrobe: None,
            gi_achievement_progress: None,
            gi_events: None,
            gi_teapot: None,
            gi_miliastra: None,
            gi_exploration: None,
        };

        let json = serde_json::to_value(&good).expect("Good must serialize");
        let object = json.as_object().expect("expected a JSON object");
        assert!(!object.contains_key("gi_achievements"));
        assert!(!object.contains_key("timestamp"));
        assert!(!object.contains_key("gi_player"));
        assert!(!object.contains_key("gi_achievement_times"));
        assert!(!object.contains_key("gi_characters"));
        assert!(!object.contains_key("gi_anecdotes"));
        assert!(!object.contains_key("gi_capture"));
        assert!(!object.contains_key("gi_tasks"));
    }

    #[test]
    fn unknown_account_values_are_omitted_rather_than_null() {
        let player = GiPlayer {
            ar: Some(60),
            ..Default::default()
        };
        assert_eq!(serde_json::to_string(&player).unwrap(), r#"{"ar":60}"#);

        let character = GiCharacter {
            obtained_at: Some(1_646_092_800),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_string(&character).unwrap(),
            r#"{"obtainedAt":1646092800}"#
        );

        // A book with no reward read and no wish made, and a wish with no NPC.
        let book = GiAnecdotes {
            anecdotes: BTreeMap::from([(
                107_302,
                GiAnecdote {
                    state: 2,
                    finish_count: 0,
                },
            )]),
            unlocked_areas: vec![7],
            reward: None,
            wish: None,
        };
        assert_eq!(
            serde_json::to_string(&book).unwrap(),
            r#"{"anecdotes":{"107302":{"state":2,"finishCount":0}},"unlockedAreas":[7]}"#
        );
        let wish = GiAnecdoteWish {
            npc_id: None,
            last_wish_time: 1_791_000_000,
        };
        assert_eq!(
            serde_json::to_string(&wish).unwrap(),
            r#"{"lastWishTime":1791000000}"#
        );
    }
}
