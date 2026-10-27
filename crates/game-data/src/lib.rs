// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

// Without the download only the reading half is used: the parsing code is
// kept (and tested) for the builds that download.
#![cfg_attr(not(feature = "download"), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Result, anyhow};

#[cfg(feature = "download")]
mod dimbreath;
mod game_data;
mod pinned;
mod types;

#[cfg(feature = "download")]
use dimbreath::Dimbreath;
pub use pinned::Pinned;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
pub use types::*;

use crate::game_data::{
    AvatarExcelConfigDataEntry, AvatarSkillExcelConfigDataEntry, ConstValueExcelConfigDataEntry,
    MaterialExcelConfigDataEntry, ReliquaryMainPropExcelConfigDataEntry,
    WeaponExcelConfigDataEntry,
};

trait GameDataSource {
    async fn get_latest_hash(&self) -> Result<String>;
    async fn get_json_file<T: DeserializeOwned>(&self, git_ref: &str, path: &str) -> Result<T>;
}

fn lookup_text(text_map: &HashMap<u32, String>, id: u32) -> Option<&String> {
    let res = text_map.get(&id);
    if res.is_none() {
        tracing::debug!("Unable to lookup text for hash {id}");
    }
    res
}

// Const values are stored as strings and padded out to six slots, so scalar
// constants live in the first slot.
fn lookup_const_value<T: FromStr>(
    const_value_map: &HashMap<String, Vec<String>>,
    name: &str,
) -> Option<T> {
    let res = const_value_map
        .get(name)
        .and_then(|values| values.first())
        .and_then(|value| value.parse().ok());
    if res.is_none() {
        tracing::debug!("Unable to lookup const value {name}");
    }
    res
}

const DATABASE_VERSION: u32 = 9;

#[derive(Debug, Deserialize, Serialize)]
struct Database {
    version: u32,
    git_hash: String,
    affix_map: HashMap<u32, Affix>,
    artifact_map: HashMap<u32, Artifact>,
    character_map: HashMap<u32, String>,
    material_map: HashMap<u32, String>,
    property_map: HashMap<u32, Property>,
    set_map: HashMap<u32, String>,
    skill_element_map: HashMap<u32, Element>,
    skill_type_map: HashMap<u32, SkillType>,
    tps_avatar_id_female: Option<u32>,
    tps_avatar_id_male: Option<u32>,
    weapon_map: HashMap<u32, Weapon>,
    /// Radiant Spincrystals: the Serenitea Pot music id each one unlocks →
    /// its item id.
    #[serde(default)]
    spincrystals: BTreeMap<u32, u32>,
    /// Cooking recipes: id → the most proficiency it takes.
    #[serde(default)]
    recipes: BTreeMap<u32, u32>,
    /// Crafting, forging and processing recipe ids (`Combine`, `Forge`,
    /// `Compound`).
    #[serde(default)]
    crafting: Crafting,
    /// The ids of the tables that tell lone id lists apart.
    #[serde(default)]
    id_tables: IdTables,
    #[serde(default)]
    exploration: Exploration,
}

impl Database {
    pub fn new(git_hash: &str) -> Self {
        Self {
            version: DATABASE_VERSION,
            git_hash: git_hash.into(),
            affix_map: HashMap::new(),
            artifact_map: HashMap::new(),
            character_map: HashMap::new(),
            material_map: HashMap::new(),
            property_map: HashMap::new(),
            set_map: HashMap::new(),
            skill_element_map: HashMap::new(),
            skill_type_map: HashMap::new(),
            tps_avatar_id_female: None,
            tps_avatar_id_male: None,
            weapon_map: HashMap::new(),
            spincrystals: BTreeMap::new(),
            recipes: BTreeMap::new(),
            crafting: Crafting::default(),
            id_tables: IdTables::default(),
            exploration: Exploration::default(),
        }
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let db: Self = serde_json::from_reader(reader)?;
        if db.version != DATABASE_VERSION {
            return Err(anyhow!(
                "Database versions do not match {} != {}",
                db.version,
                DATABASE_VERSION
            ));
        }
        Ok(db)
    }
}

#[derive(Debug)]
pub struct AnimeGameData {
    cache_path: Option<PathBuf>,
    db: Option<Database>,
}

impl AnimeGameData {
    pub fn new() -> Self {
        Self {
            cache_path: None,
            db: None,
        }
    }

    pub fn new_from_reader<R: Read>(reader: R) -> Result<Self> {
        let db = serde_json::from_reader(reader)?;
        Ok(Self {
            cache_path: None,
            db,
        })
    }

    pub fn new_with_cache<P: AsRef<Path>>(cache_path: P) -> Self {
        let cache_path = cache_path.as_ref();

        // Try to load cached data ignoring errors and instead leave and empty
        // database.
        let db = Database::load_from_path(cache_path).ok();

        Self {
            cache_path: Some(cache_path.to_owned()),
            db,
        }
    }

    pub fn save_to_writer<W: Write>(&self, writer: W) -> Result<()> {
        serde_json::to_writer_pretty(writer, self.db()?)?;
        Ok(())
    }

    fn db(&self) -> Result<&Database> {
        self.db.as_ref().ok_or_else(|| anyhow!("No data loaded"))
    }

    pub fn get_affix(&self, id: u32) -> Result<&Affix> {
        self.db()?
            .affix_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch affix {id}"))
    }

    pub fn get_artifact(&self, id: u32) -> Result<&Artifact> {
        self.db()?
            .artifact_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch artifact {id}"))
    }

    pub fn get_character(&self, id: u32) -> Result<&String> {
        self.db()?
            .character_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch character {id}"))
    }

    pub fn get_material(&self, id: u32) -> Result<&String> {
        self.db()?
            .material_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch material {id}"))
    }

    pub fn get_property(&self, id: u32) -> Result<&Property> {
        self.db()?
            .property_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch property {id}"))
    }

    pub fn get_set(&self, id: u32) -> Result<&String> {
        self.db()?
            .set_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch set {id}"))
    }

    pub fn get_skill_element(&self, id: u32) -> Result<&Element> {
        self.db()?
            .skill_element_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch skill element {id}"))
    }

    pub fn get_skill_type(&self, id: u32) -> Result<&SkillType> {
        self.db()?
            .skill_type_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch skill type {id}"))
    }

    pub fn get_tps_avatar_id_female(&self) -> Result<u32> {
        self.db()?
            .tps_avatar_id_female
            .ok_or_else(|| anyhow!("Unable to fetch female TPS avatar id"))
    }

    pub fn get_tps_avatar_id_male(&self) -> Result<u32> {
        self.db()?
            .tps_avatar_id_male
            .ok_or_else(|| anyhow!("Unable to fetch male TPS avatar id"))
    }

    pub fn get_weapon(&self, id: u32) -> Result<&Weapon> {
        self.db()?
            .weapon_map
            .get(&id)
            .ok_or_else(|| anyhow!("Unable to fetch weapon {id}"))
    }

    /// Radiant Spincrystals by the Serenitea Pot music id each one unlocks
    /// (value: the item id).
    pub fn spincrystals(&self) -> Result<&BTreeMap<u32, u32>> {
        Ok(&self.db()?.spincrystals)
    }

    /// Cooking recipes by id (value: the most proficiency it takes).
    pub fn recipes(&self) -> Result<&BTreeMap<u32, u32>> {
        Ok(&self.db()?.recipes)
    }

    /// The crafting, forging and processing recipe ids.
    pub fn crafting(&self) -> Result<&Crafting> {
        Ok(&self.db()?.crafting)
    }

    /// Furnishing sets, companion reward events, domain entries, NPCs met
    /// and Natlan's tribal chapters, by id.
    pub fn id_tables(&self) -> Result<&IdTables> {
        Ok(&self.db()?.id_tables)
    }

    /// The exploration tables' ids (see [`Exploration`]).
    pub fn exploration(&self) -> Result<&Exploration> {
        Ok(&self.db()?.exploration)
    }

    pub fn has_data(&self) -> bool {
        self.db.is_some()
    }

    /// True when the loaded data is not the pinned data set's (or nothing is loaded).
    pub async fn needs_update_from(&self, pinned: &Pinned) -> Result<bool> {
        self.needs_update_impl(pinned).await
    }

    /// Builds the database from a pinned copy of the game data files (no network).
    pub async fn update_from(&mut self, pinned: &Pinned) -> Result<()> {
        self.update_impl(pinned).await
    }

    #[cfg(feature = "download")]
    pub async fn needs_update(&self) -> Result<bool> {
        self.needs_update_impl(&Dimbreath::new()?).await
    }

    async fn needs_update_impl<Source: GameDataSource>(&self, source: &Source) -> Result<bool> {
        let Some(db) = &self.db else {
            return Ok(true);
        };
        Ok(db.git_hash != source.get_latest_hash().await?)
    }

    #[cfg(feature = "download")]
    pub async fn update(&mut self) -> Result<()> {
        self.update_impl(&Dimbreath::new()?).await
    }

    async fn update_impl<Source: GameDataSource>(&mut self, source: &Source) -> Result<()> {
        tracing::info!("Checking for updated data");
        // Check if data is already up to date
        let latest_git_hash = source.get_latest_hash().await?;
        if let Some(db) = &self.db
            && db.git_hash == latest_git_hash
        {
            return Ok(());
        }
        tracing::info!("New git hash detected {latest_git_hash}");

        // Index all data into a separate DB to ensure consistency.
        let mut db = Database::new(&latest_git_hash);

        tracing::info!("Downloading text map");
        let text_map = Self::fetch_text_map(source, &latest_git_hash).await?;

        tracing::info!("Downloading skill type map");
        db.skill_type_map = Self::fetch_skill_type_map(source, &latest_git_hash).await?;

        tracing::info!("Downloading skill element map");
        db.skill_element_map = Self::fetch_skill_element_map(source, &latest_git_hash).await?;

        tracing::info!("Downloading set map");
        db.set_map = Self::fetch_set_map(source, &latest_git_hash, &text_map).await?;

        tracing::info!("Downloading artifact map");
        db.artifact_map = Self::fetch_artifact_map(source, &latest_git_hash, &db.set_map).await?;

        tracing::info!("Downloading property map");
        db.property_map = Self::fetch_property_map(source, &latest_git_hash).await?;

        tracing::info!("Downloading affix map");
        db.affix_map = Self::fetch_affix_map(source, &latest_git_hash).await?;

        tracing::info!("Downloading weapon map");
        db.weapon_map = Self::fetch_weapon_map(source, &latest_git_hash, &text_map).await?;

        tracing::info!("Downloading material map");
        (db.material_map, db.spincrystals) =
            Self::fetch_material_map(source, &latest_git_hash, &text_map).await?;

        tracing::info!("Downloading character map");
        db.character_map = Self::fetch_character_map(source, &latest_git_hash, &text_map).await?;

        tracing::info!("Downloading recipes");
        db.recipes = Self::fetch_recipes(source, &latest_git_hash).await;
        db.crafting = Self::fetch_crafting(source, &latest_git_hash).await;
        db.id_tables = Self::fetch_id_tables(source, &latest_git_hash).await;

        tracing::info!("Downloading const value map");
        let const_value_map = Self::fetch_const_value_map(source, &latest_git_hash).await?;
        db.tps_avatar_id_female =
            lookup_const_value(&const_value_map, "CONST_VALUE_TPS_AVATAR_CONFIG_ID_FEMALE");
        db.tps_avatar_id_male =
            lookup_const_value(&const_value_map, "CONST_VALUE_TPS_AVATAR_CONFIG_ID_MALE");

        tracing::info!("Downloading exploration tables");
        db.exploration = Self::fetch_exploration(source, &latest_git_hash).await;

        self.db = Some(db);

        let _ = self.try_save_db();
        Ok(())
    }

    fn try_save_db(&self) -> Result<()> {
        let Some(cache_path) = &self.cache_path else {
            return Err(anyhow!("no cache path provided"));
        };

        let file = File::create(cache_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &self.db)?;

        Ok(())
    }

    /// The exploration tables' ids. A table that cannot be read leaves its set
    /// empty (logged): the rest of the game data does not depend on them, and
    /// an empty set only means that part of an export is left out.
    async fn fetch_exploration<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Exploration {
        async fn rows<Source: GameDataSource>(
            source: &Source,
            git_ref: &str,
            path: &str,
        ) -> Vec<serde_json::Value> {
            match source
                .get_json_file::<Vec<serde_json::Value>>(git_ref, path)
                .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!("could not read {path}; its ids are left out: {e}");
                    Vec::new()
                }
            }
        }
        let ids = |rows: &[serde_json::Value], field: &str| -> BTreeSet<u32> {
            rows.iter()
                .filter_map(|row| row.get(field)?.as_u64()?.try_into().ok())
                .collect()
        };

        let cities = ids(
            &rows(source, git_ref, "ExcelBinOutput/CityConfigData.json").await,
            "cityId",
        );
        let reputation_cities = ids(
            &rows(
                source,
                git_ref,
                "ExcelBinOutput/ReputationCityExcelConfigData.json",
            )
            .await,
            "cityId",
        );
        let offerings = ids(
            &rows(
                source,
                git_ref,
                "ExcelBinOutput/OfferingOpenStateConfigData.json",
            )
            .await,
            "offeringId",
        );
        let world_areas = rows(source, git_ref, "ExcelBinOutput/WorldAreaConfigData.json").await;
        let level1: Vec<serde_json::Value> = world_areas
            .iter()
            .filter(|row| row.get("areaType").and_then(|t| t.as_str()) == Some("LEVEL_1"))
            .cloned()
            .collect();
        let areas = ids(&level1, "areaID1");

        let mut scene_points = BTreeMap::new();
        for scene in ids(&world_areas, "sceneID") {
            let path = format!("BinOutput/Scene/Point/scene{scene}_point.json");
            match source
                .get_json_file::<serde_json::Value>(git_ref, &path)
                .await
            {
                Ok(file) => {
                    let points: BTreeSet<u32> = file
                        .get("points")
                        .and_then(|points| points.as_object())
                        .map(|points| points.keys().filter_map(|id| id.parse().ok()).collect())
                        .unwrap_or_default();
                    if !points.is_empty() {
                        scene_points.insert(scene, points);
                    }
                }
                Err(e) => tracing::debug!("no point file for scene {scene}: {e}"),
            }
        }

        Exploration {
            cities,
            reputation_cities,
            offerings,
            areas,
            scene_points,
        }
    }

    async fn fetch_affix_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Result<HashMap<u32, Affix>> {
        let data: Vec<game_data::ReliquaryAffixExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/ReliquaryAffixExcelConfigData.json")
            .await?;

        Ok(data
            .iter()
            .filter_map(|entry| {
                let property = entry.prop_type.parse::<Property>().ok()?;
                let value = if property.is_percentage() {
                    entry.prop_value * 100.
                } else {
                    entry.prop_value
                };
                Some((entry.id, Affix { property, value }))
            })
            .collect())
    }

    async fn fetch_artifact_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
        set_map: &HashMap<u32, String>,
    ) -> Result<HashMap<u32, Artifact>> {
        let data: Vec<game_data::ReliquaryExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/ReliquaryExcelConfigData.json")
            .await?;

        let map = data
            .iter()
            .filter_map(|entry| {
                let set = set_map.get(&entry.set_id?)?.to_string();
                let slot = ArtifactSlot::from_game_data_name(&entry.equip_type)?;
                Some((
                    entry.id,
                    Artifact {
                        set,
                        slot,
                        rarity: entry.rank_level,
                    },
                ))
            })
            .collect();

        Ok(map)
    }

    async fn fetch_character_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
        text_map: &HashMap<u32, String>,
    ) -> Result<HashMap<u32, String>> {
        let data: Vec<AvatarExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/AvatarExcelConfigData.json")
            .await?;

        Ok(data
            .iter()
            .filter_map(|entry| {
                Some((
                    entry.id,
                    lookup_text(text_map, entry.name_text_map_hash)?.clone(),
                ))
            })
            .collect())
    }

    async fn fetch_const_value_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Result<HashMap<String, Vec<String>>> {
        let data: Vec<ConstValueExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/ConstValueExcelConfigData.json")
            .await?;

        Ok(data
            .into_iter()
            .map(|entry| (entry.name, entry.value))
            .collect())
    }

    async fn fetch_material_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
        text_map: &HashMap<u32, String>,
    ) -> Result<(HashMap<u32, String>, BTreeMap<u32, u32>)> {
        let data: Vec<MaterialExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/MaterialExcelConfigData.json")
            .await?;

        let names = data
            .iter()
            .filter_map(|entry| {
                Some((
                    entry.id,
                    lookup_text(text_map, entry.name_text_map_hash)?.clone(),
                ))
            })
            .collect();
        // A Radiant Spincrystal is used up into the Serenitea Pot's music
        // list: its use names the music id it unlocks.
        let spincrystals = data
            .iter()
            .filter(|entry| entry.material_type == "MATERIAL_BGM")
            .filter_map(|entry| {
                let music = entry
                    .item_use
                    .iter()
                    .find(|item_use| item_use.use_op == "ITEM_USE_UNLOCK_HOME_BGM")?
                    .use_param
                    .first()?
                    .parse()
                    .ok()?;
                Some((music, entry.id))
            })
            .collect();
        Ok((names, spincrystals))
    }

    /// Cooking recipes and the most proficiency each takes. A table that
    /// cannot be read leaves it empty (logged): only `gi_collections.recipes`
    /// depends on it.
    async fn fetch_recipes<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> BTreeMap<u32, u32> {
        let rows: Vec<serde_json::Value> = match source
            .get_json_file(git_ref, "ExcelBinOutput/CookRecipeExcelConfigData.json")
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("could not read the recipe table; recipes are left out: {e}");
                return BTreeMap::new();
            }
        };
        rows.iter()
            .filter_map(|row| {
                let id = u32::try_from(row.get("id")?.as_u64()?).ok()?;
                let max = row
                    .get("maxProficiency")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|max| u32::try_from(max).ok())
                    .unwrap_or(0);
                Some((id, max))
            })
            .collect()
    }

    /// The crafting, forging and processing recipe ids. A table that cannot be
    /// read leaves its set empty (logged).
    async fn fetch_crafting<Source: GameDataSource>(source: &Source, git_ref: &str) -> Crafting {
        async fn ids<Source: GameDataSource>(
            source: &Source,
            git_ref: &str,
            table: &str,
            field: &str,
        ) -> BTreeSet<u32> {
            let path = format!("ExcelBinOutput/{table}ExcelConfigData.json");
            match source
                .get_json_file::<Vec<serde_json::Value>>(git_ref, &path)
                .await
            {
                Ok(rows) => rows
                    .iter()
                    .filter_map(|row| u32::try_from(row.get(field)?.as_u64()?).ok())
                    .collect(),
                Err(e) => {
                    tracing::warn!("could not read {path}; its ids are left out: {e}");
                    BTreeSet::new()
                }
            }
        }
        Crafting {
            combine: ids(source, git_ref, "Combine", "combineId").await,
            forge: ids(source, git_ref, "Forge", "id").await,
            compound: ids(source, git_ref, "Compound", "id").await,
        }
    }

    /// The id tables behind the lone id lists. A table that cannot be read
    /// leaves its set empty (logged).
    async fn fetch_id_tables<Source: GameDataSource>(source: &Source, git_ref: &str) -> IdTables {
        async fn ids<Source: GameDataSource>(
            source: &Source,
            git_ref: &str,
            table: &str,
            field: &str,
            keep: impl Fn(&serde_json::Value) -> bool,
        ) -> BTreeSet<u32> {
            let path = format!("ExcelBinOutput/{table}ExcelConfigData.json");
            match source
                .get_json_file::<Vec<serde_json::Value>>(git_ref, &path)
                .await
            {
                Ok(rows) => rows
                    .iter()
                    .filter(|row| keep(row))
                    .filter_map(|row| u32::try_from(row.get(field)?.as_u64()?).ok())
                    .collect(),
                Err(e) => {
                    tracing::warn!("could not read {path}; its ids are left out: {e}");
                    BTreeSet::new()
                }
            }
        }
        let all = |_: &serde_json::Value| true;
        IdTables {
            furnishing_sets: ids(source, git_ref, "FurnitureSuite", "suiteID", all).await,
            companion_rewards: ids(source, git_ref, "HomeWorldEvent", "eventID", |row| {
                row.get("eventType").and_then(serde_json::Value::as_str)
                    == Some("HOME_AVATAR_REWARD_EVENT")
            })
            .await,
            domains: ids(source, git_ref, "DungeonEntry", "id", all).await,
            npcs_met: ids(source, git_ref, "NpcFirstMet", "id", all).await,
            tribal_chapters: ids(source, git_ref, "TribalQuestChapter", "id", all).await,
        }
    }

    async fn fetch_property_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Result<HashMap<u32, Property>> {
        let data: Vec<ReliquaryMainPropExcelConfigDataEntry> = source
            .get_json_file(
                git_ref,
                "ExcelBinOutput/ReliquaryMainPropExcelConfigData.json",
            )
            .await?;

        Ok(data
            .iter()
            .filter_map(|entry| Some((entry.id, entry.prop_type.parse::<Property>().ok()?)))
            .collect())
    }

    async fn fetch_set_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
        text_map: &HashMap<u32, String>,
    ) -> Result<HashMap<u32, String>> {
        let affix_data: Vec<game_data::EquipAffixExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/EquipAffixExcelConfigData.json")
            .await?;

        // An affix has one entry per set bonus tier, all sharing the set's name.
        let affix_names: HashMap<u32, &String> = affix_data
            .iter()
            .filter_map(|entry| Some((entry.id, lookup_text(text_map, entry.name_text_map_hash)?)))
            .collect();

        let set_data: Vec<game_data::ReliquarySetExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/ReliquarySetExcelConfigData.json")
            .await?;

        Ok(set_data
            .iter()
            .filter_map(|entry| {
                // Sets that grant no bonuses omit equipAffixId and have no name.
                let name = *affix_names.get(&entry.equip_affix_id?)?;
                Some((entry.set_id, name.clone()))
            })
            .collect())
    }

    async fn fetch_skill_element_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Result<HashMap<u32, Element>> {
        let data: Vec<AvatarSkillExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/AvatarSkillExcelConfigData.json")
            .await?;

        Ok(data
            .iter()
            .filter_map(|entry| {
                // Skills that cost no elemental energy omit costElemType and
                // have no element.
                let element = entry.cost_elem_type.as_ref()?.parse::<Element>().ok()?;
                Some((entry.id, element))
            })
            .collect())
    }

    async fn fetch_skill_type_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Result<HashMap<u32, SkillType>> {
        let data: Vec<game_data::AvatarSkillDepotExcelConfigDataEntry> = source
            .get_json_file(
                git_ref,
                "ExcelBinOutput/AvatarSkillDepotExcelConfigData.json",
            )
            .await?;

        let mut type_map = HashMap::new();
        for config in data {
            // Depots without a burst (e.g. the elementless Traveler) have no
            // energy skill, and skill slots may be empty or missing.
            if let Some(skill) = config.energy_skill {
                type_map.insert(skill, SkillType::Burst);
            }
            if let Some(skill) = config.skill(0) {
                type_map.insert(skill, SkillType::Auto);
            }
            if let Some(skill) = config.skill(1) {
                type_map.insert(skill, SkillType::Skill);
            }
        }

        Ok(type_map)
    }

    async fn fetch_text_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
    ) -> Result<HashMap<u32, String>> {
        source
            .get_json_file(git_ref, "TextMap/TextMap_MediumEN.json")
            .await
    }

    async fn fetch_weapon_map<Source: GameDataSource>(
        source: &Source,
        git_ref: &str,
        text_map: &HashMap<u32, String>,
    ) -> Result<HashMap<u32, Weapon>> {
        let data: Vec<WeaponExcelConfigDataEntry> = source
            .get_json_file(git_ref, "ExcelBinOutput/WeaponExcelConfigData.json")
            .await?;

        Ok(data
            .iter()
            .filter_map(|entry| {
                let name = lookup_text(text_map, entry.name_text_map_hash)?;
                Some((
                    entry.id,
                    Weapon {
                        name: name.clone(),
                        rarity: entry.rank_level,
                    },
                ))
            })
            .collect())
    }
}

impl Default for AnimeGameData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use tempfile::NamedTempFile;

    use super::*;

    struct TestDataSource;

    impl GameDataSource for TestDataSource {
        async fn get_latest_hash(&self) -> Result<String> {
            Ok("13be4fd7343fe4cee8fa0096fe854b1c5b01b124".into())
        }

        async fn get_json_file<T: DeserializeOwned>(
            &self,
            _git_ref: &str,
            path: &str,
        ) -> Result<T> {
            let json_string = match path {
                "ExcelBinOutput/ReliquaryAffixExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ReliquaryAffixExcelConfigData.json")
                }
                "ExcelBinOutput/ReliquaryExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ReliquaryExcelConfigData.json")
                }
                "ExcelBinOutput/AvatarExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/AvatarExcelConfigData.json")
                }
                "ExcelBinOutput/MaterialExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/MaterialExcelConfigData.json")
                }
                "ExcelBinOutput/ReliquaryMainPropExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ReliquaryMainPropExcelConfigData.json")
                }
                "ExcelBinOutput/EquipAffixExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/EquipAffixExcelConfigData.json")
                }
                "ExcelBinOutput/ReliquarySetExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ReliquarySetExcelConfigData.json")
                }
                "ExcelBinOutput/AvatarSkillDepotExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/AvatarSkillDepotExcelConfigData.json")
                }
                "ExcelBinOutput/AvatarSkillExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/AvatarSkillExcelConfigData.json")
                }
                "ExcelBinOutput/ConstValueExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ConstValueExcelConfigData.json")
                }
                "ExcelBinOutput/WeaponExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/WeaponExcelConfigData.json")
                }
                "ExcelBinOutput/CookRecipeExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/CookRecipeExcelConfigData.json")
                }
                "ExcelBinOutput/CombineExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/CombineExcelConfigData.json")
                }
                "ExcelBinOutput/ForgeExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ForgeExcelConfigData.json")
                }
                "ExcelBinOutput/CompoundExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/CompoundExcelConfigData.json")
                }
                "ExcelBinOutput/FurnitureSuiteExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/FurnitureSuiteExcelConfigData.json")
                }
                "ExcelBinOutput/HomeWorldEventExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/HomeWorldEventExcelConfigData.json")
                }
                "ExcelBinOutput/DungeonEntryExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/DungeonEntryExcelConfigData.json")
                }
                "ExcelBinOutput/NpcFirstMetExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/NpcFirstMetExcelConfigData.json")
                }
                "ExcelBinOutput/TribalQuestChapterExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/TribalQuestChapterExcelConfigData.json")
                }
                "TextMap/TextMap_MediumEN.json" => {
                    include_str!("test_data/TextMap/TextMap_MediumEN.json")
                }
                "ExcelBinOutput/CityConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/CityConfigData.json")
                }
                "ExcelBinOutput/ReputationCityExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ReputationCityExcelConfigData.json")
                }
                "ExcelBinOutput/OfferingOpenStateConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/OfferingOpenStateConfigData.json")
                }
                "ExcelBinOutput/WorldAreaConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/WorldAreaConfigData.json")
                }
                "BinOutput/Scene/Point/scene3_point.json" => {
                    include_str!("test_data/BinOutput/Scene/Point/scene3_point.json")
                }
                _ => return Err(anyhow!("no test data for {path}")),
            };

            let data = serde_json::from_str(json_string)?;
            Ok(data)
        }
    }

    struct TestDataSource2;

    impl GameDataSource for TestDataSource2 {
        async fn get_latest_hash(&self) -> Result<String> {
            Ok("13be4fd7343fe4cee8fa0096fe854b1c5b01b124-2".into())
        }

        async fn get_json_file<T: DeserializeOwned>(&self, git_ref: &str, path: &str) -> Result<T> {
            let json_string = match path {
                "ExcelBinOutput/ReliquaryAffixExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ReliquaryAffixExcelConfigData2.json")
                }
                _ => return TestDataSource {}.get_json_file(git_ref, path).await,
            };
            let data = serde_json::from_str(json_string)?;
            Ok(data)
        }
    }

    // A source whose const value data is missing the entries indexed below.
    struct TestDataSource3;

    impl GameDataSource for TestDataSource3 {
        async fn get_latest_hash(&self) -> Result<String> {
            Ok("13be4fd7343fe4cee8fa0096fe854b1c5b01b124-3".into())
        }

        async fn get_json_file<T: DeserializeOwned>(&self, git_ref: &str, path: &str) -> Result<T> {
            let json_string = match path {
                "ExcelBinOutput/ConstValueExcelConfigData.json" => {
                    include_str!("test_data/ExcelBinOutput/ConstValueExcelConfigData2.json")
                }
                _ => return TestDataSource {}.get_json_file(git_ref, path).await,
            };
            let data = serde_json::from_str(json_string)?;
            Ok(data)
        }
    }

    /// The test data written as a pinned folder (gzipped files and a manifest).
    fn pinned_test_data(id: &str) -> tempfile::TempDir {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/test_data");
        for sub in ["ExcelBinOutput", "TextMap", "BinOutput/Scene/Point"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
            for entry in std::fs::read_dir(root.join(sub)).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                let file = File::create(dir.path().join(sub).join(format!("{name}.gz"))).unwrap();
                let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
                gz.write_all(&std::fs::read(&path).unwrap()).unwrap();
                gz.finish().unwrap();
            }
        }
        std::fs::write(
            dir.path().join("manifest.json"),
            format!(r#"{{"id": "{id}"}}"#),
        )
        .unwrap();
        dir
    }

    #[tokio::test]
    async fn pinned_files_build_the_same_database() {
        let id = "13be4fd7343fe4cee8fa0096fe854b1c5b01b124";
        let dir = pinned_test_data(id);
        let pinned = Pinned::open(dir.path()).unwrap();
        assert_eq!(pinned.id(), id);

        let mut from_pinned = AnimeGameData::new();
        assert!(from_pinned.needs_update_from(&pinned).await.unwrap());
        from_pinned.update_from(&pinned).await.unwrap();
        assert!(!from_pinned.needs_update_from(&pinned).await.unwrap());

        let mut from_source = AnimeGameData::new();
        from_source.update_impl(&TestDataSource).await.unwrap();
        let json = |data: &AnimeGameData| {
            let mut out = Vec::new();
            data.save_to_writer(&mut out).unwrap();
            serde_json::from_slice::<serde_json::Value>(&out).unwrap()
        };
        assert_eq!(json(&from_pinned), json(&from_source));
    }

    #[test]
    fn a_pinned_folder_needs_a_well_formed_id() {
        let dir = pinned_test_data("not-an-id");
        let error = Pinned::open(dir.path()).unwrap_err().to_string();
        assert!(error.contains("not 40 hex digits"), "{error}");
        assert!(Pinned::open(dir.path().join("missing")).is_err());
    }

    #[tokio::test]
    async fn character_map_returns_correct_character() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        assert_eq!(data.get_character(10000061).unwrap(), &"Kirara".to_string());
    }

    #[tokio::test]
    async fn exploration_ids_come_from_their_tables() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        let exploration = data.exploration().unwrap();
        assert_eq!(exploration.cities, BTreeSet::from([1, 2, 99]));
        assert_eq!(exploration.reputation_cities, BTreeSet::from([1, 2]));
        assert_eq!(exploration.offerings, BTreeSet::from([1, 2]));
        // Level-1 areas only.
        assert_eq!(exploration.areas, BTreeSet::from([1, 6, 300]));
        // Scene 9 has no point file in the test data: left out, not an error.
        assert_eq!(
            exploration.scene_points,
            BTreeMap::from([(3, BTreeSet::from([1, 7, 12]))])
        );
    }

    #[tokio::test]
    async fn skill_type_map_returns_correct_type() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        assert_eq!(data.get_skill_type(10024).unwrap(), &SkillType::Auto);
        assert_eq!(data.get_skill_type(10018).unwrap(), &SkillType::Skill);
        assert_eq!(data.get_skill_type(10019).unwrap(), &SkillType::Burst);
    }

    #[tokio::test]
    async fn skill_element_map_returns_correct_element() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        assert_eq!(data.get_skill_element(10034).unwrap(), &Element::Anemo);
        assert_eq!(data.get_skill_element(10078).unwrap(), &Element::Geo);
        assert_eq!(data.get_skill_element(10014).unwrap(), &Element::Electro);
        assert_eq!(data.get_skill_element(10072).unwrap(), &Element::Hydro);
        assert_eq!(data.get_skill_element(10017).unwrap(), &Element::Pyro);
        assert_eq!(data.get_skill_element(10019).unwrap(), &Element::Cryo);
        assert_eq!(data.get_skill_element(10118).unwrap(), &Element::Dendro);
    }

    #[tokio::test]
    async fn skill_element_map_skips_skills_with_omitted_cost_elem_type() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        // Normal attacks and skills that cost no elemental energy omit
        // costElemType and are not indexed.
        assert!(data.get_skill_element(10024).is_err());
        assert!(data.get_skill_element(10018).is_err());
    }

    #[tokio::test]
    async fn skill_type_map_handles_omitted_energy_skill() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        // Depot 501 (elementless Traveler) has no energySkill field and only
        // a normal attack.  Its normal attack is still indexed...
        assert_eq!(data.get_skill_type(100540).unwrap(), &SkillType::Auto);

        // ...but the defaulted (0) energy skill and empty skill slots are not.
        assert!(data.get_skill_type(0).is_err());
    }

    #[tokio::test]
    async fn set_map_skips_sets_without_equip_affix() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        // Set 15000 grants no bonuses and omits equipAffixId.
        assert!(data.get_set(15000).is_err());
    }

    #[tokio::test]
    async fn artifact_map_skips_artifacts_with_omitted_set_id() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        // Reliquary 20002 belongs to no set and omits setId.
        assert!(data.get_artifact(20002).is_err());
    }

    #[tokio::test]
    async fn set_map_returns_correct_set() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        assert_eq!(
            data.get_set(15031).unwrap(),
            &"Marechaussee Hunter".to_string()
        );
        assert_eq!(
            data.get_set(10001).unwrap(),
            &"Resolution of Sojourner".to_string()
        );

        // Set 15048's display item names are missing from the medium text map,
        // so it is only reachable through its equip affix.
        assert_eq!(
            data.get_set(15048).unwrap(),
            &"Heart of the Furnace".to_string()
        );
    }

    #[tokio::test]
    async fn material_map_returns_correct_material() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        assert_eq!(data.get_material(100002).unwrap(), &"Sunsettia".to_string());
    }

    #[tokio::test]
    async fn id_tables_come_from_their_tables() {
        let mut data = AnimeGameData::new();
        data.update_impl(&TestDataSource).await.unwrap();
        let tables = data.id_tables().unwrap();
        assert_eq!(tables.furnishing_sets, BTreeSet::from([1, 2]));
        // Only the reward events.
        assert_eq!(tables.companion_rewards, BTreeSet::from([1002]));
        assert_eq!(tables.domains, BTreeSet::from([1006, 1112]));
        assert_eq!(tables.npcs_met, BTreeSet::from([2, 1052]));
        assert_eq!(tables.tribal_chapters, BTreeSet::from([1, 18]));
    }

    #[tokio::test]
    async fn crafting_ids_come_from_their_tables() {
        let mut data = AnimeGameData::new();
        data.update_impl(&TestDataSource).await.unwrap();
        let crafting = data.crafting().unwrap();
        assert_eq!(crafting.combine, BTreeSet::from([11001, 11002]));
        assert_eq!(crafting.forge, BTreeSet::from([11001, 14001]));
        assert_eq!(crafting.compound, BTreeSet::from([1001]));
    }

    #[tokio::test]
    async fn recipes_keep_their_most_proficiency() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        assert_eq!(
            data.recipes().unwrap(),
            &BTreeMap::from([(1001, 5), (5502, 25)])
        );
    }

    #[tokio::test]
    async fn spincrystals_map_music_ids_to_their_items() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        // Only the Radiant Spincrystal; the Sunsettia's uses unlock no music.
        assert_eq!(
            data.spincrystals().unwrap(),
            &BTreeMap::from([(101, 141001)])
        );
    }

    #[tokio::test]
    async fn affix_map_returns_correct_affix() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        // Flat affixes contain their vaule unmodified.
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );

        // Prercentage affixes get multipled by 100 from thier data value.
        assert_eq!(
            data.get_affix(982001).unwrap(),
            &Affix {
                property: Property::GeoDamage,
                value: 80.0
            }
        );
    }

    #[tokio::test]
    async fn artifact_map_returns_correct_artifact() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        assert_eq!(
            data.get_artifact(31534).unwrap(),
            &Artifact {
                set: "Marechaussee Hunter".into(),
                slot: ArtifactSlot::Circlet,
                rarity: 5
            }
        );
    }

    #[tokio::test]
    async fn proptery_map_returns_correct_property() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        assert_eq!(data.get_property(50960).unwrap(), &Property::PyroDamage);
    }

    #[tokio::test]
    async fn const_values_return_correct_tps_avatar_ids() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        assert_eq!(data.get_tps_avatar_id_female().unwrap(), 10000135);
        assert_eq!(data.get_tps_avatar_id_male().unwrap(), 10000134);
    }

    #[tokio::test]
    async fn missing_const_values_are_not_indexed() {
        let source = TestDataSource3;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        // The rest of the database still indexes...
        assert_eq!(data.get_character(10000061).unwrap(), &"Kirara".to_string());

        // ...but the absent const values have no value to return.
        assert!(data.get_tps_avatar_id_female().is_err());
        assert!(data.get_tps_avatar_id_male().is_err());
    }

    #[tokio::test]
    async fn weapon_map_returns_correct_weapon() {
        let source = TestDataSource;
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();

        assert_eq!(
            data.get_weapon(11505).unwrap(),
            &Weapon {
                name: "Primordial Jade Cutter".into(),
                rarity: 5
            }
        );
    }

    #[tokio::test]
    async fn data_is_cached_by_update() {
        let tempfile = NamedTempFile::new().unwrap();

        let source = TestDataSource;
        let mut data = AnimeGameData::new_with_cache(tempfile.path());

        // Affix does not exist before update
        assert!(data.get_affix(501022).is_err());

        data.update_impl(&source).await.unwrap();

        // Affix exists after update
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );

        drop(data);

        // Re-open data with valid cache.
        let data = AnimeGameData::new_with_cache(tempfile.path());

        // Affix exists after loading from cache
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );
    }

    #[tokio::test]
    async fn cached_data_is_updated_from_source() {
        let tempfile = NamedTempFile::new().unwrap();

        let source = TestDataSource;
        let mut data = AnimeGameData::new_with_cache(tempfile.path());

        // Affix does not exist before update
        assert!(data.get_affix(501022).is_err());

        data.update_impl(&source).await.unwrap();

        // Affix exists after update
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );

        drop(data);

        // Re-open data with valid cache.
        let mut data = AnimeGameData::new_with_cache(tempfile.path());

        // Affix exists after loading from cache
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );

        let source = TestDataSource2;
        data.update_impl(&source).await.unwrap();

        // Affix is updated with second source data
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 240.0
            }
        );
    }

    #[tokio::test]
    async fn needs_update_is_correct_across_caches_and_updates() {
        let tempfile = NamedTempFile::new().unwrap();

        let mut data = AnimeGameData::new_with_cache(tempfile.path());

        let source = TestDataSource;
        let source2 = TestDataSource2;
        // A new database always needs updating.
        assert!(data.needs_update_impl(&source).await.unwrap());

        // After updating an update is no longer needed.
        data.update_impl(&source).await.unwrap();
        assert!(!data.needs_update_impl(&source).await.unwrap());

        drop(data);

        let mut data = AnimeGameData::new_with_cache(tempfile.path());
        // After re-opening A new database doesn't need an update from the same
        // source.
        assert!(!data.needs_update_impl(&source).await.unwrap());

        // With a new source, it does need updating
        assert!(data.needs_update_impl(&source2).await.unwrap());

        // After updating an update is no longer needed.
        data.update_impl(&source2).await.unwrap();
        assert!(!data.needs_update_impl(&source2).await.unwrap());
    }

    #[tokio::test]
    async fn old_database_version_cache_is_ignored() {
        let tempfile = NamedTempFile::new().unwrap();

        let source = TestDataSource;

        // Force an old database version to be cached.
        let mut data = AnimeGameData::new_with_cache(tempfile.path());
        data.update_impl(&source).await.unwrap();
        data.db.as_mut().unwrap().version = 0;
        data.try_save_db().unwrap();
        drop(data);

        // Re-open with cache and ensure the old data is not loaded.
        let mut data = AnimeGameData::new_with_cache(tempfile.path());

        // Affix does not exist before update
        assert!(data.get_affix(501022).is_err());

        data.update_impl(&source).await.unwrap();

        // Affix exists after update
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );
    }

    #[tokio::test]
    async fn saving_to_and_reading_from_reader_works() {
        let tempfile = NamedTempFile::new().unwrap();

        let source = TestDataSource;

        // Save database to tempfile.
        let mut data = AnimeGameData::new();
        data.update_impl(&source).await.unwrap();
        let writer = File::create(tempfile.path()).unwrap();
        data.save_to_writer(writer).unwrap();
        drop(data);

        // Re-open database from tempfile
        let reader = File::open(tempfile.path()).unwrap();
        let data = AnimeGameData::new_from_reader(reader).unwrap();

        // Affix exists without updating
        assert_eq!(
            data.get_affix(501022).unwrap(),
            &Affix {
                property: Property::Hp,
                value: 239.0
            }
        );
    }
}
