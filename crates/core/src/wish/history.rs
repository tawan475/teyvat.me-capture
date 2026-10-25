// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Reading a login's whole wish history: every banner, page after page,
//! through the paced client (1 s per page, 2 s per banner, back-off on a
//! throttle), for a UIGF file. Unlike the tracker sync, which stops where
//! the tracker's history ends, this reads to the oldest record the API has
//! (about the last six months).

use std::collections::BTreeMap;
use std::fmt;

use super::Banner;
use super::api::{ApiRecord, FetchError, HttpGet, LogClient, Sleeper};
use super::hub::BANNERS;
use super::link::PAGE_SIZE;

/// More pages than any banner has (six months of wishes are a few hundred).
const MAX_PAGES_PER_BANNER: u32 = 2_000;

/// Where a read is, after each page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub banner: Banner,
    pub page: u32,
    /// Records read so far, every banner.
    pub records: usize,
}

/// A login's wish history, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub uid: u32,
    /// The API's `region` (`os_asia`, …); empty when it named none.
    pub region: String,
    pub records: Vec<ApiRecord>,
}

/// Why the history could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryError {
    Fetch(FetchError),
    /// A record of another UID: the key is not this login's.
    OtherAccount,
    /// A record whose id is not a number.
    BadRecord,
    /// The caller stopped it.
    Cancelled,
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(error) => write!(f, "{error}"),
            Self::OtherAccount => write!(f, "the wish history is another account's"),
            Self::BadRecord => write!(f, "the wish history API sent a record without an id"),
            Self::Cancelled => write!(f, "stopped"),
        }
    }
}

impl From<FetchError> for HistoryError {
    fn from(error: FetchError) -> Self {
        Self::Fetch(error)
    }
}

/// Read every banner of `uid`'s history with `log`. `on_page` sees the
/// progress after each page and stops the read by returning `false`.
pub async fn read_all<H: HttpGet, S: Sleeper>(
    log: &mut LogClient<H, S>,
    uid: u32,
    mut on_page: impl FnMut(Progress) -> bool,
) -> Result<History, HistoryError> {
    let uid_text = uid.to_string();
    let mut region = String::new();
    let mut records: BTreeMap<u64, ApiRecord> = BTreeMap::new();
    for banner in BANNERS {
        log.next_banner();
        let mut end_id = 0;
        for page in 1..=MAX_PAGES_PER_BANNER {
            let read = log.page(banner, page, end_id).await?;
            if region.is_empty() {
                region = read.region;
            }
            let full = read.records.len() >= PAGE_SIZE as usize;
            for record in read.records {
                if record.uid != uid_text {
                    return Err(HistoryError::OtherAccount);
                }
                let id: u64 = record.id.parse().map_err(|_| HistoryError::BadRecord)?;
                end_id = id;
                records.insert(id, record);
            }
            let progress = Progress {
                banner,
                page,
                records: records.len(),
            };
            if !on_page(progress) {
                return Err(HistoryError::Cancelled);
            }
            if !full {
                break;
            }
        }
        tracing::info!(banner, records = records.len(), "wish history: banner read");
    }
    Ok(History {
        uid,
        region,
        records: records.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use url::Url;

    use super::*;
    use crate::wish::api::{HttpError, HttpReply, Pacing};
    use crate::wish::link::{GLOBAL_LOG_URL, WishKey};

    const UID: u32 = 812_345_678;

    /// Answers each banner's pages from synthetic records, newest first.
    struct FakeApi {
        banners: HashMap<String, Vec<ApiRecord>>,
        uid_override: Option<String>,
        asked: Mutex<Vec<(String, String, String)>>,
    }

    impl HttpGet for FakeApi {
        async fn get(&self, url: &Url) -> Result<HttpReply, HttpError> {
            let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
            let banner = query["gacha_type"].clone();
            let end_id: u64 = query["end_id"].parse().unwrap();
            self.asked.lock().unwrap().push((
                banner.clone(),
                query["page"].clone(),
                query["end_id"].clone(),
            ));
            let list: Vec<_> = self
                .banners
                .get(&banner)
                .into_iter()
                .flatten()
                .filter(|record| end_id == 0 || record.id.parse::<u64>().unwrap() < end_id)
                .take(PAGE_SIZE as usize)
                .map(|record| {
                    let mut record = record.clone();
                    if let Some(uid) = &self.uid_override {
                        record.uid = uid.clone();
                    }
                    serde_json::json!({
                        "uid": record.uid, "gacha_type": record.gacha_type, "item_id": "",
                        "count": "1", "time": record.time, "name": record.name,
                        "lang": "en-us", "item_type": record.item_type,
                        "rank_type": record.rank_type, "id": record.id,
                    })
                })
                .collect();
            let body = serde_json::json!({
                "retcode": 0, "message": "OK",
                "data": {"page": query["page"], "size": "20", "region": "os_asia", "list": list},
            });
            Ok(HttpReply {
                status: 200,
                status_text: "200 OK".to_owned(),
                body: body.to_string().into_bytes(),
            })
        }
    }

    /// Spends no time; remembers each wait.
    #[derive(Default)]
    struct Clock(Mutex<Vec<Duration>>);

    impl Sleeper for &Clock {
        async fn sleep(&self, duration: Duration) {
            self.0.lock().unwrap().push(duration);
        }
    }

    /// `count` synthetic records on `banner`, newest (highest id) first.
    fn records(gacha_type: &str, first_id: u64, count: u64) -> Vec<ApiRecord> {
        (0..count)
            .rev()
            .map(|n| ApiRecord {
                uid: UID.to_string(),
                gacha_type: gacha_type.to_owned(),
                item_id: String::new(),
                time: format!("2026-10-{:02} 12:00:00", 1 + n % 28),
                name: format!("Synthetic Item {n}"),
                item_type: "Weapon".to_owned(),
                rank_type: "3".to_owned(),
                id: (first_id + n).to_string(),
            })
            .collect()
    }

    fn client<'a>(api: &'a FakeApi, clock: &'a Clock) -> LogClient<&'a FakeApi, &'a Clock> {
        let key = WishKey::new("SyNtHeTiCkEy", "hk4e_global", Instant::now()).with_uid(Some(UID));
        LogClient::new(
            api,
            GLOBAL_LOG_URL.to_owned(),
            key,
            clock,
            Pacing::default(),
        )
    }

    impl HttpGet for &FakeApi {
        fn get(&self, url: &Url) -> impl Future<Output = Result<HttpReply, HttpError>> + Send {
            (*self).get(url)
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn every_banner_is_read_to_its_end_oldest_first_and_paced() {
        let mut banners = HashMap::new();
        // The second character banner's records come under 301 too.
        let mut character = records("301", 1_000, 30);
        character.extend(records("400", 5_000, 5));
        character.sort_by_key(|record| std::cmp::Reverse(record.id.parse::<u64>().unwrap()));
        banners.insert("301".to_owned(), character);
        banners.insert("200".to_owned(), records("200", 9_000, 20));
        let api = FakeApi {
            banners,
            uid_override: None,
            asked: Mutex::default(),
        };
        let clock = Clock::default();
        let mut pages = Vec::new();
        let history = block_on(read_all(&mut client(&api, &clock), UID, |progress| {
            pages.push(progress);
            true
        }))
        .unwrap();

        assert_eq!(history.uid, UID);
        assert_eq!(history.region, "os_asia");
        assert_eq!(history.records.len(), 55);
        let ids: Vec<u64> = history
            .records
            .iter()
            .map(|r| r.id.parse().unwrap())
            .collect();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "oldest first");

        // 301: 35 records = a full page and a short one; 200: exactly a full
        // page, then an empty one; the other three banners: one empty page.
        let asked = api.asked.lock().unwrap().clone();
        let banners_asked: Vec<&str> = asked.iter().map(|(banner, _, _)| banner.as_str()).collect();
        assert_eq!(
            banners_asked,
            ["301", "301", "302", "500", "200", "200", "100"]
        );
        assert_eq!(
            asked[1],
            ("301".to_owned(), "2".to_owned(), "1015".to_owned())
        );
        assert_eq!(pages.last().unwrap().records, 55);

        // 1 s between pages of a banner, 2 s before the next banner.
        let waits = clock.0.lock().unwrap().clone();
        let s = Duration::from_secs;
        assert_eq!(waits, [s(1), s(2), s(2), s(2), s(1), s(2)]);
    }

    #[test]
    fn another_accounts_records_are_refused() {
        let mut banners = HashMap::new();
        banners.insert("301".to_owned(), records("301", 1, 3));
        let api = FakeApi {
            banners,
            uid_override: Some("823456789".to_owned()),
            asked: Mutex::default(),
        };
        let clock = Clock::default();
        let result = block_on(read_all(&mut client(&api, &clock), UID, |_| true));
        assert_eq!(result, Err(HistoryError::OtherAccount));
    }

    #[test]
    fn the_caller_can_stop_it() {
        let api = FakeApi {
            banners: HashMap::new(),
            uid_override: None,
            asked: Mutex::default(),
        };
        let clock = Clock::default();
        let result = block_on(read_all(&mut client(&api, &clock), UID, |_| false));
        assert_eq!(result, Err(HistoryError::Cancelled));
        assert_eq!(api.asked.lock().unwrap().len(), 1);
    }
}
