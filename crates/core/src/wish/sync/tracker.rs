// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The tracker's wish routes (`/wishes/cursor`, `/wishes/import`): what they
//! take and answer. Each app sends them with its own HTTP client and
//! credential ([`WishTracker`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::wish::Banner;
use crate::wish::api::ApiRecord;
pub use crate::wish::uigf::{RECORD_LANG, region_timezone};

/// Records per import request: the tracker's limit.
pub const MAX_RECORDS_PER_IMPORT: usize = 2_000;

/// One tracker, as a sync run talks to it.
pub trait WishTracker {
    /// What to tell the user about a refused or failed request.
    type Error;

    /// `GET /wishes/cursor?uid=`.
    fn cursor(&self, uid: &str) -> impl Future<Output = Result<Cursor, Self::Error>> + Send;

    /// `POST /wishes/import`, once per [`MAX_RECORDS_PER_IMPORT`] records, in
    /// the order given ([`import_bodies`]).
    fn import(
        &self,
        uid: &str,
        region: &str,
        records: &[ApiRecord],
    ) -> impl Future<Output = Result<Imported, Self::Error>> + Send;
}

/// What the tracker holds per banner: its newest real record id, and the
/// time of its newest wish (which a history imported from a paimon.moe
/// backup has without real ids).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub newest: BTreeMap<Banner, u64>,
    /// `"YYYY-MM-DD HH:MM:SS"`, in the records' own clock (the server's).
    pub newest_time: BTreeMap<Banner, String>,
}

impl Cursor {
    pub fn newest(&self, banner: Banner) -> Option<u64> {
        self.newest.get(&banner).copied()
    }

    /// Where a pass over `banner` meets what the tracker has.
    pub fn known(&self, banner: Banner) -> Known {
        Known {
            id: self.newest(banner),
            time: self.newest_time.get(&banner).cloned(),
        }
    }
}

/// What a banner's pass counts as already in the tracker: a record whose id
/// is at most `id`, or whose time is before `time`. A record of the same
/// second as `time` is not known (a ten-pull can straddle what the tracker
/// has; the tracker dedupes it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Known {
    pub id: Option<u64>,
    pub time: Option<String>,
}

impl Known {
    pub fn by_id(id: Option<u64>) -> Self {
        Self { id, time: None }
    }

    pub fn covers(&self, id: u64, time: &str) -> bool {
        self.id.is_some_and(|newest| id <= newest)
            || self
                .time
                .as_deref()
                .is_some_and(|newest| is_record_time(time) && time < newest)
    }
}

/// `YYYY-MM-DD HH:MM:SS`: what the wish history API and the tracker write,
/// which compares as text in time order.
pub fn is_record_time(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 19
        && bytes.iter().enumerate().all(|(at, &byte)| match at {
            4 | 7 => byte == b'-',
            10 => byte == b' ',
            13 | 16 => byte == b':',
            _ => byte.is_ascii_digit(),
        })
}

/// What an import did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub added: u64,
    pub known: u64,
}

/// One record as the import takes it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ImportRecord {
    pub id: String,
    pub gacha_type: String,
    pub time: String,
    pub name: String,
    pub item_type: String,
    pub rank_type: String,
    pub item_id: String,
}

impl From<&ApiRecord> for ImportRecord {
    fn from(record: &ApiRecord) -> Self {
        Self {
            id: record.id.clone(),
            gacha_type: record.gacha_type.clone(),
            time: record.time.clone(),
            name: record.name.clone(),
            item_type: record.item_type.clone(),
            rank_type: record.rank_type.clone(),
            item_id: record.item_id.clone(),
        }
    }
}

/// The body of `POST /wishes/import`.
#[derive(Debug, Serialize)]
pub struct ImportBody<'a> {
    pub uid: &'a str,
    pub region: &'a str,
    pub lang: &'a str,
    /// The UTC offset of the records' times (they are the server's local
    /// time).
    pub timezone: i32,
    pub records: Vec<ImportRecord>,
}

/// The import requests for `records`, in the order given.
pub fn import_bodies<'a>(
    uid: &'a str,
    region: &'a str,
    records: &'a [ApiRecord],
) -> impl Iterator<Item = ImportBody<'a>> {
    records
        .chunks(MAX_RECORDS_PER_IMPORT)
        .map(move |chunk| ImportBody {
            uid,
            region,
            lang: RECORD_LANG,
            timezone: region_timezone(region),
            records: chunk.iter().map(ImportRecord::from).collect(),
        })
}

/// Why a cursor answer could not be read.
#[derive(Debug)]
pub enum CursorError {
    Unreadable(serde_json::Error),
    /// The banner whose newest id is not a number.
    BadId(String),
}

#[derive(Deserialize)]
struct CursorAnswer {
    #[serde(default)]
    newest: BTreeMap<String, Option<String>>,
    /// Read leniently: missing, or anything but an object of banner →
    /// time strings, is an older tracker (ids only).
    #[serde(default, rename = "newestTime")]
    newest_time: serde_json::Value,
}

/// Read the body of a successful `GET /wishes/cursor`.
pub fn parse_cursor(body: &str) -> Result<Cursor, CursorError> {
    let answer: CursorAnswer = serde_json::from_str(body).map_err(CursorError::Unreadable)?;
    let mut cursor = Cursor::default();
    for (banner, newest) in answer.newest {
        let (Ok(banner), Some(newest)) = (banner.parse::<Banner>(), newest) else {
            continue;
        };
        let newest = newest
            .parse::<u64>()
            .map_err(|_| CursorError::BadId(banner.to_string()))?;
        cursor.newest.insert(banner, newest);
    }
    if let serde_json::Value::Object(times) = answer.newest_time {
        for (banner, time) in times {
            if let (Ok(banner), Some(time)) = (banner.parse::<Banner>(), time.as_str())
                && is_record_time(time)
            {
                cursor.newest_time.insert(banner, time.to_owned());
            }
        }
    }
    Ok(cursor)
}

#[derive(Deserialize)]
struct ImportAnswer {
    #[serde(default)]
    added: u64,
    #[serde(default)]
    known: u64,
}

/// Read the body of a successful `POST /wishes/import`.
pub fn parse_import(body: &str) -> Result<Imported, serde_json::Error> {
    let answer: ImportAnswer = serde_json::from_str(body)?;
    Ok(Imported {
        added: answer.added,
        known: answer.known,
    })
}

#[derive(Deserialize)]
struct ErrorAnswer {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

/// The tracker's `{"error": {"code", "message"}}` of a refused request, or
/// two empty strings.
pub fn error_code(body: &str) -> (String, String) {
    match serde_json::from_str::<ErrorAnswer>(body) {
        Ok(ErrorAnswer { error }) => (error.code, error.message),
        Err(_) => (String::new(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_known_by_id_or_by_an_earlier_time_but_not_the_same_second() {
        let newest = "2026-10-01 09:44:55";
        let by_time = Known {
            id: None,
            time: Some(newest.to_owned()),
        };
        assert!(by_time.covers(1, "2026-10-01 09:44:54"));
        assert!(!by_time.covers(1, newest), "the same second is sent");
        assert!(!by_time.covers(1, "2026-10-01 09:44:56"));
        assert!(
            !by_time.covers(1, "not a time"),
            "an odd time is never known"
        );

        let by_id = Known::by_id(Some(100));
        assert!(by_id.covers(100, "2030-01-01 00:00:00"));
        assert!(!by_id.covers(101, "2000-01-01 00:00:00"));
        assert!(!Known::default().covers(1, "2000-01-01 00:00:00"));

        let both = Known {
            id: Some(100),
            time: Some(newest.to_owned()),
        };
        assert!(both.covers(100, "2030-01-01 00:00:00"));
        assert!(both.covers(500, "2026-09-30 00:00:00"));
        assert!(!both.covers(500, newest));
    }

    #[test]
    fn record_times_are_the_apis_shape() {
        assert!(is_record_time("2026-10-12 21:04:11"));
        for odd in [
            "",
            "2026-10-12T21:04:11",
            "2026-10-12 21:04",
            "2026-10-12 21:04:11Z",
            "2026/10/12 21:04:11",
            "２026-10-12 21:04:1",
        ] {
            assert!(!is_record_time(odd), "{odd}");
        }
    }

    #[test]
    fn region_time_zones_follow_uigf() {
        assert_eq!(region_timezone("os_usa"), -5);
        assert_eq!(region_timezone("os_euro"), 1);
        assert_eq!(region_timezone("os_asia"), 8);
        assert_eq!(region_timezone("os_cht"), 8);
        assert_eq!(region_timezone("cn_gf01"), 8);
    }

    #[test]
    fn a_cursor_reads_ids_and_times_and_skips_what_it_does_not_know() {
        let cursor = parse_cursor(
            r#"{"newest":{"301":"100","302":null,"x":"1"},"newestTime":{"301":"2026-10-12 21:04:11","200":"soon"}}"#,
        )
        .unwrap();
        assert_eq!(cursor.newest(301), Some(100));
        assert_eq!(cursor.newest(302), None);
        assert_eq!(
            cursor.known(301).time.as_deref(),
            Some("2026-10-12 21:04:11")
        );
        assert_eq!(cursor.known(200).time, None);
        assert!(
            matches!(parse_cursor(r#"{"newest":{"301":"x"}}"#), Err(CursorError::BadId(b)) if b == "301")
        );
        assert!(
            parse_cursor(r#"{"newest":{}, "newestTime": 5}"#).is_ok(),
            "an older tracker"
        );
    }

    #[test]
    fn imports_go_in_chunks_of_the_trackers_limit() {
        let record = ApiRecord {
            uid: "811111111".into(),
            gacha_type: "301".into(),
            item_id: String::new(),
            time: "2026-10-12 21:04:11".into(),
            name: "Item".into(),
            item_type: "Weapon".into(),
            rank_type: "3".into(),
            id: "1".into(),
        };
        let records = vec![record; MAX_RECORDS_PER_IMPORT + 1];
        let sizes: Vec<usize> = import_bodies("811111111", "os_asia", &records)
            .map(|body| body.records.len())
            .collect();
        assert_eq!(sizes, [MAX_RECORDS_PER_IMPORT, 1]);
        assert_eq!(
            error_code(r#"{"error":{"code":"rate_limited"}}"#).0,
            "rate_limited"
        );
        assert_eq!(error_code("<html>"), (String::new(), String::new()));
    }
}
