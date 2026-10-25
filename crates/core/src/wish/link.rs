// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The game's wish history link: the key it carries, the game's own query
//! byte for byte, and the redaction every logged URL goes through.
//!
//! The key in every URL here is a password while it lasts: nothing in this
//! module logs a URL unredacted.

use std::fmt;
use std::time::{Duration, Instant};

use url::Url;

/// The global servers' wish history endpoint.
pub const GLOBAL_LOG_URL: &str =
    "https://public-operation-hk4e-sg.hoyoverse.com/gacha_info/api/getGachaLog";

/// The Chinese servers' (`hk4e_cn`) endpoint.
pub const CN_LOG_URL: &str = "https://public-operation-hk4e.mihoyo.com/gacha_info/api/getGachaLog";

/// Records per page. The API's maximum.
pub const PAGE_SIZE: u32 = 20;

/// How long a key is believed to last. The game does not say; keys have been
/// observed to stop working about a day after they were issued, and the API
/// answers "authkey timeout" when one has, which ends it sooner.
pub const KEY_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

/// Query parameters that carry the credential, or that only make sense
/// together with it. Blanked out before a URL is logged.
const CREDENTIAL_QUERY_PARAMS: [&str; 3] = ["authkey", "sign_type", "authkey_ver"];

/// The start of the link's query when no banner list was seen: the history
/// link's fixed parameters, as the game writes them, without the banner's
/// `gacha_id` and `timestamp` (the API does not need them).
pub const FALLBACK_RECORD_QUERY: &str = "win_mode=fullscreen&no_joypad_close=1&authkey_ver=1\
     &sign_type=2&auth_appid=webview_gacha&init_type=301";

/// What the game puts in the wish history link besides the key, as captured
/// before the key arrived. Nothing here is secret.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkParts {
    /// The history link query the server gave the banner (`GetGachaInfoRsp`):
    /// `win_mode=…&init_type=301&gacha_id=…&timestamp=…`, verbatim.
    pub record_query: Option<String>,
    /// The client's version (`game_version`), from its login.
    pub game_version: Option<String>,
    /// The server region (`region`): `os_asia`, `os_usa`, `os_euro`,
    /// `os_cht`, `cn_gf01`, `cn_qd01`.
    pub region: Option<String>,
}

/// Percent-encode a query value the way the game does: everything but
/// `A-Z a-z 0-9 - _ . ~`, with lowercase hex (`%2f`, `%2b`, `%3d`).
pub fn encode_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3 / 2);
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02x}"));
        }
    }
    out
}

/// A wish history key from the game, with when it arrived, the account it
/// belongs to and what the game's link carries besides it.
///
/// Memory only: never logged (its `Debug` prints no key), never persisted,
/// never sent anywhere but the game's own API.
#[derive(Clone, PartialEq, Eq)]
pub struct WishKey {
    authkey: String,
    /// Chooses the endpoint (`hk4e_cn` → the Chinese one). The link itself
    /// carries an empty `game_biz=`, as the game's does.
    game_biz: String,
    received_at: Instant,
    link: LinkParts,
    /// The UID of the login the key came after; `None` until it is known.
    /// The sync sends nothing for a key without one, and only records of it.
    uid: Option<u32>,
}

impl WishKey {
    pub fn new(authkey: &str, game_biz: &str, received_at: Instant) -> Self {
        Self {
            authkey: authkey.to_owned(),
            game_biz: game_biz.to_owned(),
            received_at,
            link: LinkParts::default(),
            uid: None,
        }
    }

    pub fn with_link(mut self, link: LinkParts) -> Self {
        self.link = link;
        self
    }

    pub fn with_uid(mut self, uid: Option<u32>) -> Self {
        self.uid = uid;
        self
    }

    /// The UID of the login the key came after.
    pub fn uid(&self) -> Option<u32> {
        self.uid
    }

    /// Whether this is the same key as `other`, whenever each arrived.
    pub fn same_key(&self, other: &Self) -> bool {
        self.authkey == other.authkey && self.game_biz == other.game_biz
    }

    /// How long the key is still believed to work at `now`.
    pub fn remaining(&self, now: Instant) -> Duration {
        KEY_LIFETIME.saturating_sub(now.saturating_duration_since(self.received_at))
    }

    pub fn is_fresh(&self, now: Instant) -> bool {
        !self.remaining(now).is_zero()
    }

    /// The endpoint for this key's servers.
    pub fn endpoint(&self) -> &'static str {
        if self.game_biz == "hk4e_cn" {
            CN_LOG_URL
        } else {
            GLOBAL_LOG_URL
        }
    }

    /// The wish URL people paste into paimon.moe ("Manual" import) or
    /// stardb: the link the game itself writes when its history opens.
    pub fn wish_url(&self) -> String {
        format!("{}?{}", self.endpoint(), self.link_query())
    }

    /// The game's link query, in the game's order and encoding: the banner's
    /// history link query (or [`FALLBACK_RECORD_QUERY`]), then `lang=en`,
    /// `device_type=pc`, `game_version`, `region` (each left out when it was
    /// not captured), the key and an empty `game_biz`.
    fn link_query(&self) -> String {
        let link = &self.link;
        let mut query = link
            .record_query
            .clone()
            .unwrap_or_else(|| FALLBACK_RECORD_QUERY.to_owned());
        query.push_str("&lang=en&device_type=pc");
        if let Some(version) = &link.game_version {
            query.push_str("&game_version=");
            query.push_str(&encode_value(version));
        }
        if let Some(region) = &link.region {
            query.push_str("&region=");
            query.push_str(&encode_value(region));
        }
        query.push_str("&authkey=");
        query.push_str(&encode_value(&self.authkey));
        query.push_str("&game_biz=");
        query
    }

    /// The game's link on `endpoint`, then `extra` (what paimon.moe sets on
    /// it to page: `gacha_type`, `size`, `page`, `end_id`).
    pub fn url_at(&self, endpoint: &str, extra: &[(&str, &str)]) -> Url {
        let mut url = format!("{endpoint}?{}", self.link_query());
        for (name, value) in extra {
            url.push('&');
            url.push_str(name);
            url.push('=');
            url.push_str(&encode_value(value));
        }
        Url::parse(&url).expect("the wish history endpoints are valid URLs")
    }
}

impl fmt::Debug for WishKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WishKey")
            .field("authkey", &"<redacted>")
            .field("game_biz", &self.game_biz)
            .field("received_at", &self.received_at)
            .field("link", &self.link)
            .field("uid_known", &self.uid.is_some())
            .finish()
    }
}

/// Rebuild `url` with the credential-bearing parameters blanked out.
///
/// A wish link's `authkey` grants read access to the account's entire wish
/// history for roughly a day. `latest.log` is the file users are told to
/// attach to public bug reports, so the raw URL must never be written to it.
pub fn redact_url(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return "<unparsable wish url, redacted>".to_owned();
    };

    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(key, value)| {
            let key = key.into_owned();
            let value = if CREDENTIAL_QUERY_PARAMS.contains(&key.as_str()) {
                "REDACTED".to_owned()
            } else {
                value.into_owned()
            };
            (key, value)
        })
        .collect();

    if pairs.is_empty() {
        // `query_pairs_mut()` would leave a bare `?` behind.
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(pairs);
    }

    // Some web view URLs carry their parameters in the fragment instead.
    if parsed.fragment().is_some_and(|f| f.contains("authkey")) {
        parsed.set_fragment(Some("REDACTED"));
    }

    parsed.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTHKEY: &str = "SyNtHeTiC+tEsT/kEy==";

    fn key(biz: &str) -> WishKey {
        WishKey::new(AUTHKEY, biz, Instant::now())
    }

    /// What the game captured before the key: the banner's history link
    /// query, the client version, the region. Synthetic values.
    fn captured() -> LinkParts {
        LinkParts {
            record_query: Some(
                "win_mode=fullscreen&no_joypad_close=1&authkey_ver=1&sign_type=2\
                 &auth_appid=webview_gacha&init_type=301\
                 &gacha_id=0123456789abcdef0123456789abcdef01234567&timestamp=1790000000"
                    .to_owned(),
            ),
            game_version: Some("OSRELWin7.1.0_R11111111_S22222222_D33333333".to_owned()),
            region: Some("os_asia".to_owned()),
        }
    }

    #[test]
    fn the_url_is_the_games_own_link_in_its_order_and_encoding() {
        assert_eq!(
            key("hk4e_global").with_link(captured()).wish_url(),
            "https://public-operation-hk4e-sg.hoyoverse.com/gacha_info/api/getGachaLog\
             ?win_mode=fullscreen&no_joypad_close=1&authkey_ver=1&sign_type=2\
             &auth_appid=webview_gacha&init_type=301\
             &gacha_id=0123456789abcdef0123456789abcdef01234567&timestamp=1790000000\
             &lang=en&device_type=pc&game_version=OSRELWin7.1.0_R11111111_S22222222_D33333333\
             &region=os_asia&authkey=SyNtHeTiC%2btEsT%2fkEy%3d%3d&game_biz="
        );
    }

    #[test]
    fn what_was_not_captured_is_left_out_of_the_url() {
        assert_eq!(
            key("hk4e_global").wish_url(),
            "https://public-operation-hk4e-sg.hoyoverse.com/gacha_info/api/getGachaLog\
             ?win_mode=fullscreen&no_joypad_close=1&authkey_ver=1&sign_type=2\
             &auth_appid=webview_gacha&init_type=301&lang=en&device_type=pc\
             &authkey=SyNtHeTiC%2btEsT%2fkEy%3d%3d&game_biz="
        );
    }

    #[test]
    fn values_are_encoded_like_the_game_with_lowercase_hex() {
        assert_eq!(encode_value("a+b/c=d"), "a%2bb%2fc%3dd");
        assert_eq!(encode_value("Az09-_.~"), "Az09-_.~");
        assert_eq!(encode_value("a b&c"), "a%20b%26c");
    }

    #[test]
    fn a_chinese_key_uses_the_chinese_endpoint() {
        let url = key("hk4e_cn").wish_url();
        assert!(
            url.starts_with("https://public-operation-hk4e.mihoyo.com/gacha_info/api/getGachaLog?"),
            "{url}"
        );
        // Like the game's own link: the game is named by the host, and the
        // parameter stays empty.
        assert!(url.ends_with("&game_biz="), "{url}");
    }

    #[test]
    fn requests_keep_the_link_byte_for_byte_and_add_the_paging() {
        let key = key("hk4e_global").with_link(captured());
        let url = key.url_at(
            GLOBAL_LOG_URL,
            &[
                ("gacha_type", "301"),
                ("size", "20"),
                ("page", "2"),
                ("end_id", "17"),
            ],
        );
        assert_eq!(
            url.as_str(),
            format!("{}&gacha_type=301&size=20&page=2&end_id=17", key.wish_url())
        );
    }

    #[test]
    fn the_key_round_trips_through_the_url_encoding() {
        let url = Url::parse(&key("hk4e_global").wish_url()).unwrap();
        let authkey = url
            .query_pairs()
            .find(|(name, _)| name == "authkey")
            .map(|(_, value)| value.into_owned());
        assert_eq!(authkey.as_deref(), Some(AUTHKEY));
    }

    #[test]
    fn the_key_never_shows_in_debug_or_in_a_redacted_url() {
        let key = key("hk4e_global");
        let rendered = format!("{key:?} {}", redact_url(&key.wish_url()));
        assert!(!rendered.contains("SyNtHeTiC"), "{rendered}");
    }

    #[test]
    fn a_key_is_believed_for_a_day() {
        let at = Instant::now();
        let key = WishKey::new(AUTHKEY, "hk4e_global", at);
        assert_eq!(key.remaining(at), KEY_LIFETIME);
        assert!(key.is_fresh(at + Duration::from_secs(23 * 3600)));
        assert!(!key.is_fresh(at + KEY_LIFETIME));
    }

    #[test]
    fn redact_url_blanks_the_credential_but_keeps_the_rest() {
        let redacted = redact_url(&key("hk4e_global").with_link(captured()).wish_url());

        assert!(!redacted.contains("SyNtHeTiC"), "{redacted}");
        assert!(redacted.contains("authkey=REDACTED"), "{redacted}");
        assert!(redacted.contains("authkey_ver=REDACTED"), "{redacted}");
        assert!(redacted.contains("sign_type=REDACTED"), "{redacted}");
        // Everything that is not a credential survives, so the log line is
        // still useful in a bug report.
        assert!(redacted.contains("region=os_asia"), "{redacted}");
        assert!(redacted.contains("init_type=301"), "{redacted}");
    }

    #[test]
    fn redact_url_blanks_a_fragment_carrying_the_credential() {
        let redacted = redact_url("https://example.invalid/index.html#/log?authkey=Sup3rSecret");
        assert!(!redacted.contains("Sup3rSecret"), "{redacted}");
    }

    #[test]
    fn redact_url_leaves_a_url_without_a_query_alone() {
        assert_eq!(
            redact_url("https://example.invalid/index.html"),
            "https://example.invalid/index.html"
        );
    }

    #[test]
    fn redact_url_drops_anything_it_cannot_parse() {
        let redacted = redact_url("not a url at all authkey=Sup3rSecret");
        assert!(!redacted.contains("Sup3rSecret"), "{redacted}");
    }
}
