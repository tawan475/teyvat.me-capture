// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! A login's wish history as a UIGF v4.0 file (<https://uigf.org>), the
//! interchange format wish trackers (paimon.moe, StarDB, Snap Hutao, the
//! tracker) import: `info` plus one `hk4e` account with its records, every
//! field a string as the API sends it.

use serde::Serialize;

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

impl<'a> From<&'a ApiRecord> for Record<'a> {
    fn from(record: &'a ApiRecord) -> Self {
        Self {
            uigf_gacha_type: uigf_gacha_type(&record.gacha_type),
            gacha_type: &record.gacha_type,
            // The API sends it empty for the global servers' records; kept as
            // sent (importers match by name then).
            item_id: &record.item_id,
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
/// seconds) by `app` `app_version`.
pub fn to_uigf(history: &History, export_timestamp: u64, app: &str, app_version: &str) -> String {
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
            list: history.records.iter().map(Record::from).collect(),
        }],
    };
    serde_json::to_string_pretty(&uigf).expect("plain strings and numbers serialise")
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
        let json = to_uigf(&history(), 1_792_000_000, "Teyvat.me Capture", "1.0.0");
        let file: Value = serde_json::from_str(&json).unwrap();
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
        let file: Value =
            serde_json::from_str(&to_uigf(&empty, 0, "Teyvat.me Capture", "1.0.0")).unwrap();
        assert_uigf_v4(&file);
        assert_eq!(file["hk4e"][0]["timezone"], 8);
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
