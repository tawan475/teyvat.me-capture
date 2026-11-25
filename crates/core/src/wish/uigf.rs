// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! A login's wish history as a UIGF v4.0 file (<https://uigf.org>), the
//! interchange format wish trackers (paimon.moe, StarDB, Snap Hutao, the
//! tracker) import: `info` plus one `hk4e` account with its records, every
//! field a string as the API sends it.
//!
//! UIGF v4 requires `item_id`, and importers that check it refuse a file
//! (or the whole account) with an empty one. The API sends it empty for the
//! global servers' records, so an empty id is filled from Capture's own game
//! data by the record's English name ([`ItemIds`]); one the API sent is
//! written as it came.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use serde::Serialize;
use teyvatme_game_data::AnimeGameData;

use super::api::ApiRecord;
use super::history::History;

/// The UIGF version written.
pub const UIGF_VERSION: &str = "v4.0";

/// The language the records were read in (the link's `lang=en`).
pub const RECORD_LANG: &str = "en-us";

/// The UTC offset a region's record times are in: the UIGF table.
pub fn region_timezone(region: &str) -> i32 {
    match region {
        "os_usa" => -5,
        "os_euro" => 1,
        _ => 8,
    }
}

/// The UIGF banner of a record's `gacha_type`: the second character event
/// banner (400) is one banner with the first (301).
pub fn uigf_gacha_type(gacha_type: &str) -> &str {
    match gacha_type {
        "400" => "301",
        other => other,
    }
}

/// The playable characters' avatar ids. The data's other avatars (test
/// bodies, 11000000 up) never come from a wish.
const CHARACTER_IDS: std::ops::Range<u32> = 10_000_000..11_000_000;

/// Item ids by English name, for the records the API sent without one.
/// Names match exactly (case and spaces included): the records are read in
/// English ([`RECORD_LANG`]) and carry the game's own names, and a loose
/// match could give a record another item's id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemIds {
    characters: HashMap<String, String>,
    weapons: HashMap<String, String>,
}

impl ItemIds {
    /// From the game data's characters and weapons.
    pub fn from_game_data(data: &AnimeGameData) -> Self {
        Self::new(
            data.characters()
                .filter(|(id, _)| CHARACTER_IDS.contains(id)),
            data.weapons(),
        )
    }

    /// From `(id, name)` pairs. A name the data gives several ids keeps the
    /// lowest: the game's later copies of a name are trial, quest or test
    /// variants (Primordial Jade Cutter 11505 and its copy 11506), never
    /// the item a wish gives.
    pub fn new<'n>(
        characters: impl IntoIterator<Item = (u32, &'n str)>,
        weapons: impl IntoIterator<Item = (u32, &'n str)>,
    ) -> Self {
        fn by_name<'n>(pairs: impl IntoIterator<Item = (u32, &'n str)>) -> HashMap<String, String> {
            let mut lowest: HashMap<&str, u32> = HashMap::new();
            for (id, name) in pairs {
                match lowest.entry(name) {
                    Entry::Occupied(mut held) => {
                        if id < *held.get() {
                            held.insert(id);
                        }
                    }
                    Entry::Vacant(slot) => {
                        slot.insert(id);
                    }
                }
            }
            lowest
                .into_iter()
                .map(|(name, id)| (name.to_owned(), id.to_string()))
                .collect()
        }
        Self {
            characters: by_name(characters),
            weapons: by_name(weapons),
        }
    }

    /// The id of the item a record names, by its `item_type`.
    fn id(&self, item_type: &str, name: &str) -> Option<&str> {
        let table = match item_type {
            "Character" => &self.characters,
            "Weapon" => &self.weapons,
            _ => return None,
        };
        table.get(name).map(String::as_str)
    }
}

/// A written UIGF file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UigfFile {
    pub json: String,
    /// Records written with an empty `item_id`: the API sent none and the
    /// game data does not know the name (an item newer than this build).
    pub unknown_items: usize,
}

#[derive(Serialize)]
struct Uigf<'a> {
    info: Info<'a>,
    hk4e: [Account<'a>; 1],
}

#[derive(Serialize)]
struct Info<'a> {
    export_timestamp: u64,
    export_app: &'a str,
    export_app_version: &'a str,
    version: &'static str,
}

#[derive(Serialize)]
struct Account<'a> {
    uid: String,
    timezone: i32,
    lang: &'static str,
    list: Vec<Record<'a>>,
}

#[derive(Serialize)]
struct Record<'a> {
    uigf_gacha_type: &'a str,
    gacha_type: &'a str,
    item_id: &'a str,
    count: &'static str,
    time: &'a str,
    name: &'a str,
    item_type: &'a str,
    rank_type: &'a str,
    id: &'a str,
}

impl<'a> Record<'a> {
    /// `record` as written, with `item_id` (the one it came with, or the
    /// game data's).
    fn new(record: &'a ApiRecord, item_id: &'a str) -> Self {
        Self {
            uigf_gacha_type: uigf_gacha_type(&record.gacha_type),
            gacha_type: &record.gacha_type,
            item_id,
            count: "1",
            time: &record.time,
            name: &record.name,
            item_type: &record.item_type,
            rank_type: &record.rank_type,
            id: &record.id,
        }
    }
}

/// `history` as UIGF v4.0 JSON, exported at `export_timestamp` (Unix
/// seconds) by `app` `app_version`, empty item ids filled from `item_ids`.
pub fn to_uigf<'a>(
    history: &'a History,
    item_ids: &'a ItemIds,
    export_timestamp: u64,
    app: &'a str,
    app_version: &'a str,
) -> UigfFile {
    let mut unknown_items = 0;
    let list = history
        .records
        .iter()
        .map(|record| {
            let item_id = if record.item_id.is_empty() {
                item_ids
                    .id(&record.item_type, &record.name)
                    .unwrap_or_else(|| {
                        unknown_items += 1;
                        ""
                    })
            } else {
                &record.item_id
            };
            Record::new(record, item_id)
        })
        .collect();
    let uigf = Uigf {
        info: Info {
            export_timestamp,
            export_app: app,
            export_app_version: app_version,
            version: UIGF_VERSION,
        },
        hk4e: [Account {
            uid: history.uid.to_string(),
            timezone: region_timezone(&history.region),
            lang: RECORD_LANG,
            list,
        }],
    };
    UigfFile {
        json: serde_json::to_string_pretty(&uigf).expect("plain strings and numbers serialise"),
        unknown_items,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    const UID: u32 = 812_345_678;

    fn record(gacha_type: &str, id: u64, rank: &str) -> ApiRecord {
        ApiRecord {
            uid: UID.to_string(),
            gacha_type: gacha_type.to_owned(),
            item_id: String::new(),
            time: "2026-10-12 21:04:11".to_owned(),
            name: format!("Synthetic {id}"),
            item_type: if rank == "5" { "Character" } else { "Weapon" }.to_owned(),
            rank_type: rank.to_owned(),
            id: id.to_string(),
        }
    }

    fn history() -> History {
        History {
            uid: UID,
            region: "os_euro".to_owned(),
            records: vec![
                record("100", 1_790_000_000_000_000_001, "4"),
                record("200", 1_790_000_000_000_000_002, "3"),
                record("301", 1_790_000_000_000_000_003, "5"),
                record("400", 1_790_000_000_000_000_004, "4"),
                record("302", 1_790_000_000_000_000_005, "3"),
                record("500", 1_790_000_000_000_000_006, "5"),
            ],
        }
    }

    fn is_string(value: &Value) -> bool {
        value.is_string()
    }

    /// The UIGF v4.0 schema's rules for a Genshin export, checked by hand:
    /// `info` and its four required fields; `hk4e` an array of accounts with
    /// `uid` (string or integer), `timezone` (integer) and `list`; every
    /// record with the required `uigf_gacha_type` (100, 200, 301, 302, 500),
    /// `gacha_type` (those and 400), `item_id`, `time` and `id`, and every
    /// field a string.
    fn assert_uigf_v4(file: &Value) {
        let top = file.as_object().expect("an object");
        let info = top["info"].as_object().expect("info");
        assert!(info["export_timestamp"].is_u64() || info["export_timestamp"].is_string());
        assert!(is_string(&info["export_app"]));
        assert!(is_string(&info["export_app_version"]));
        assert_eq!(info["version"], "v4.0");

        let accounts = top["hk4e"].as_array().expect("hk4e");
        assert!(!accounts.is_empty());
        for account in accounts {
            let account = account.as_object().unwrap();
            let uid = &account["uid"];
            assert!(uid.is_u64() || uid.as_str().is_some_and(|uid| uid.parse::<u64>().is_ok()));
            assert!((-12..=14).contains(&account["timezone"].as_i64().expect("timezone")));
            if let Some(lang) = account.get("lang") {
                assert!(is_string(lang));
            }
            for record in account["list"].as_array().expect("list") {
                let record = record.as_object().unwrap();
                for field in ["uigf_gacha_type", "gacha_type", "item_id", "time", "id"] {
                    assert!(record.contains_key(field), "{field} is required");
                }
                for (field, value) in record {
                    assert!(is_string(value), "{field} is a string");
                }
                assert!(
                    ["100", "200", "301", "302", "500"]
                        .contains(&record["uigf_gacha_type"].as_str().unwrap())
                );
                assert!(
                    ["100", "200", "301", "302", "400", "500"]
                        .contains(&record["gacha_type"].as_str().unwrap())
                );
                let time = record["time"].as_str().unwrap();
                assert_eq!(time.len(), 19, "YYYY-MM-DD HH:MM:SS");
                assert!(record["id"].as_str().unwrap().parse::<u64>().is_ok());
            }
        }
    }

    #[test]
    fn a_history_is_written_as_uigf_v4() {
        let written = to_uigf(
            &history(),
            &ItemIds::default(),
            1_792_000_000,
            "Teyvat.me Capture",
            "1.0.0",
        );
        assert_eq!(written.unknown_items, 6);
        let file: Value = serde_json::from_str(&written.json).unwrap();
        assert_uigf_v4(&file);

        assert_eq!(file["info"]["export_timestamp"], 1_792_000_000u64);
        assert_eq!(file["info"]["export_app"], "Teyvat.me Capture");
        let account = &file["hk4e"][0];
        assert_eq!(account["uid"], "812345678");
        assert_eq!(account["timezone"], 1, "os_euro");
        assert_eq!(account["lang"], "en-us");
        let list = account["list"].as_array().unwrap();
        assert_eq!(list.len(), 6);
        // Both character banners are one UIGF banner; the record keeps its own.
        assert_eq!(list[3]["gacha_type"], "400");
        assert_eq!(list[3]["uigf_gacha_type"], "301");
        assert_eq!(list[5]["uigf_gacha_type"], "500");
        assert_eq!(list[0]["count"], "1");
    }

    #[test]
    fn an_empty_history_is_still_a_valid_file() {
        let empty = History {
            uid: UID,
            region: String::new(),
            records: Vec::new(),
        };
        let file: Value = serde_json::from_str(
            &to_uigf(&empty, &ItemIds::default(), 0, "Teyvat.me Capture", "1.0.0").json,
        )
        .unwrap();
        assert_uigf_v4(&file);
        assert_eq!(file["hk4e"][0]["timezone"], 8);
    }

    fn named(item_type: &str, name: &str, item_id: &str, id: u64) -> ApiRecord {
        ApiRecord {
            item_id: item_id.to_owned(),
            name: name.to_owned(),
            item_type: item_type.to_owned(),
            ..record("301", 1_790_000_000_000_000_000 + id, "5")
        }
    }

    fn item_ids() -> ItemIds {
        ItemIds::new(
            [(10_000_002, "Kamisato Ayaka"), (10_000_016, "Diluc")],
            [
                (11_505, "Primordial Jade Cutter"),
                (11_506, "Primordial Jade Cutter"),
                (15_502, "Amos' Bow"),
                // The same name in both tables stays apart by item_type.
                (11_101, "Diluc"),
            ],
        )
    }

    /// The written records' `item_id`s, in order, and the unknown count.
    fn written_ids(records: Vec<ApiRecord>) -> (Vec<String>, usize) {
        let history = History {
            uid: UID,
            region: "os_asia".to_owned(),
            records,
        };
        let written = to_uigf(&history, &item_ids(), 0, "Teyvat.me Capture", "1.0.0");
        let file: Value = serde_json::from_str(&written.json).unwrap();
        assert_uigf_v4(&file);
        let ids = file["hk4e"][0]["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record["item_id"].as_str().unwrap().to_owned())
            .collect();
        (ids, written.unknown_items)
    }

    #[test]
    fn empty_item_ids_are_filled_from_the_game_data_by_name() {
        let (ids, unknown) = written_ids(vec![
            named("Character", "Kamisato Ayaka", "", 1),
            named("Weapon", "Amos' Bow", "", 2),
            named("Character", "Diluc", "", 3),
            named("Weapon", "Diluc", "", 4),
            // The lowest of a name's ids, the one a wish gives.
            named("Weapon", "Primordial Jade Cutter", "", 5),
        ]);
        assert_eq!(ids, ["10000002", "15502", "10000016", "11101", "11505"]);
        assert_eq!(unknown, 0);
    }

    #[test]
    fn an_id_the_api_sent_is_kept() {
        let (ids, unknown) = written_ids(vec![
            named("Character", "Kamisato Ayaka", "10000002", 1),
            // Even one that disagrees with the game data: never replaced.
            named("Weapon", "Amos' Bow", "99999", 2),
            named("Weapon", "Not In The Data", "15599", 3),
        ]);
        assert_eq!(ids, ["10000002", "99999", "15599"]);
        assert_eq!(unknown, 0);
    }

    #[test]
    fn an_unknown_name_stays_empty_and_is_counted() {
        let (ids, unknown) = written_ids(vec![
            named("Character", "Someone Newer", "", 1),
            named("Character", "Kamisato Ayaka", "", 2),
            // Names match exactly: case and spaces are not forgiven.
            named("Character", "kamisato ayaka", "", 3),
            named("Character", "Kamisato Ayaka ", "", 4),
            // A character's name read as a weapon is not that character.
            named("Weapon", "Kamisato Ayaka", "", 5),
            named("Mystery", "Diluc", "", 6),
        ]);
        assert_eq!(ids, ["", "10000002", "", "", "", ""]);
        assert_eq!(unknown, 5);
    }

    #[test]
    fn different_items_never_share_an_id() {
        let ids = item_ids();
        let mut seen = std::collections::HashMap::new();
        for (item_type, table) in [("Character", &ids.characters), ("Weapon", &ids.weapons)] {
            for (name, id) in table {
                if let Some(other) = seen.insert((item_type, id.clone()), name.clone()) {
                    panic!("{item_type} {id} is both {other} and {name}");
                }
            }
        }
        assert_eq!(ids.weapons.len(), 3);
    }

    #[test]
    fn only_playable_characters_come_from_the_game_data() {
        assert_eq!(
            ItemIds::from_game_data(&AnimeGameData::new()),
            ItemIds::default()
        );
        assert!(CHARACTER_IDS.contains(&10_000_002));
        assert!(!CHARACTER_IDS.contains(&11_000_017));
    }

    #[test]
    fn regions_have_the_uigf_timezones() {
        assert_eq!(region_timezone("os_usa"), -5);
        assert_eq!(region_timezone("os_euro"), 1);
        for region in ["os_asia", "os_cht", "cn_gf01", "cn_qd01"] {
            assert_eq!(region_timezone(region), 8);
        }
    }
}
