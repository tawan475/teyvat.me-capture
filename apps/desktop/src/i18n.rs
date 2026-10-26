// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The app's languages: every one the game ships, like the tracker's site.
//!
//! Strings are written in English in the code and looked up by that text:
//! [`tr`] for a plain label, [`trf`] for one with `{0}`, `{1}` … slots.
//! `locales/<code>.json` maps each English text to its translation; a text
//! a file lacks shows in English. The language follows the system's unless
//! Settings › App picks one ([`set`]). `locales/en.json` lists every text the
//! code uses (a test checks it), which is what translators work from.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// A language the app speaks, in the game's order of its language menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    En,
    ZhCn,
    ZhTw,
    Ja,
    Ko,
    Es,
    Fr,
    Ru,
    Th,
    Vi,
    De,
    Id,
    Pt,
    It,
    Tr,
}

impl Lang {
    pub const ALL: [Lang; 15] = [
        Lang::En,
        Lang::ZhCn,
        Lang::ZhTw,
        Lang::Ja,
        Lang::Ko,
        Lang::Es,
        Lang::Fr,
        Lang::Ru,
        Lang::Th,
        Lang::Vi,
        Lang::De,
        Lang::Id,
        Lang::Pt,
        Lang::It,
        Lang::Tr,
    ];

    /// The BCP 47 tag, as the tracker's site names its languages.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::ZhCn => "zh-CN",
            Lang::ZhTw => "zh-TW",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::Ru => "ru",
            Lang::Th => "th",
            Lang::Vi => "vi",
            Lang::De => "de",
            Lang::Id => "id",
            Lang::Pt => "pt",
            Lang::It => "it",
            Lang::Tr => "tr",
        }
    }

    /// The language's own name, for the picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::ZhCn => "简体中文",
            Lang::ZhTw => "繁體中文",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::Ru => "Русский",
            Lang::Th => "ภาษาไทย",
            Lang::Vi => "Tiếng Việt",
            Lang::De => "Deutsch",
            Lang::Id => "Bahasa Indonesia",
            Lang::Pt => "Português",
            Lang::It => "Italiano",
            Lang::Tr => "Türkçe",
        }
    }

    /// The language for a system locale tag (`th-TH`, `zh_Hant_TW`, `pt-BR`…);
    /// None for one the app doesn't speak.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        let tag = tag.replace('_', "-").to_ascii_lowercase();
        let mut parts = tag.split(['-', '.', '@']);
        let primary = parts.next().unwrap_or_default();
        let rest: Vec<&str> = parts.collect();
        Some(match primary {
            "en" => Lang::En,
            "zh" => {
                let traditional = rest
                    .iter()
                    .any(|p| matches!(*p, "hant" | "tw" | "hk" | "mo"));
                let simplified = rest.iter().any(|p| matches!(*p, "hans" | "cn" | "sg"));
                if traditional && !simplified {
                    Lang::ZhTw
                } else {
                    Lang::ZhCn
                }
            }
            "ja" => Lang::Ja,
            "ko" => Lang::Ko,
            "es" => Lang::Es,
            "fr" => Lang::Fr,
            "ru" => Lang::Ru,
            "th" => Lang::Th,
            "vi" => Lang::Vi,
            "de" => Lang::De,
            "id" | "in" => Lang::Id,
            "pt" => Lang::Pt,
            "it" => Lang::It,
            "tr" => Lang::Tr,
            _ => return None,
        })
    }

    fn index(self) -> usize {
        Lang::ALL.iter().position(|l| *l == self).unwrap_or(0)
    }

    /// The translations, English text → this language's (empty for English).
    fn source(self) -> &'static str {
        match self {
            Lang::En => "{}",
            Lang::ZhCn => include_str!("../locales/zh-CN.json"),
            Lang::ZhTw => include_str!("../locales/zh-TW.json"),
            Lang::Ja => include_str!("../locales/ja.json"),
            Lang::Ko => include_str!("../locales/ko.json"),
            Lang::Es => include_str!("../locales/es.json"),
            Lang::Fr => include_str!("../locales/fr.json"),
            Lang::Ru => include_str!("../locales/ru.json"),
            Lang::Th => include_str!("../locales/th.json"),
            Lang::Vi => include_str!("../locales/vi.json"),
            Lang::De => include_str!("../locales/de.json"),
            Lang::Id => include_str!("../locales/id.json"),
            Lang::Pt => include_str!("../locales/pt.json"),
            Lang::It => include_str!("../locales/it.json"),
            Lang::Tr => include_str!("../locales/tr.json"),
        }
    }
}

/// The system's language, or English when the app doesn't speak it.
/// `TEYVATME_LANG=<tag>` stands in for the system's (screenshots, tests).
pub fn system() -> Lang {
    if let Some(lang) = std::env::var("TEYVATME_LANG")
        .ok()
        .as_deref()
        .and_then(Lang::from_tag)
    {
        return lang;
    }
    sys_locale::get_locales()
        .find_map(|tag| Lang::from_tag(&tag))
        .unwrap_or(Lang::En)
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language every [`tr`] answers in now.
pub fn current() -> Lang {
    Lang::ALL[CURRENT.load(Ordering::Relaxed) as usize % Lang::ALL.len()]
}

/// Switch to `choice`, or to the system's language for None.
pub fn set(choice: Option<Lang>) -> Lang {
    let lang = choice.unwrap_or_else(system);
    CURRENT.store(lang.index() as u8, Ordering::Relaxed);
    lang
}

type Table = HashMap<&'static str, &'static str>;

/// One language's table, parsed the first time it is asked for and kept.
fn table(lang: Lang) -> &'static Table {
    static TABLES: [OnceLock<Table>; 15] = [const { OnceLock::new() }; 15];
    TABLES[lang.index()].get_or_init(|| parse(lang.source()))
}

fn parse(source: &'static str) -> Table {
    let map: HashMap<String, String> = serde_json::from_str(source).unwrap_or_default();
    map.into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| -> (&'static str, &'static str) {
            (Box::leak(k.into()), Box::leak(v.into()))
        })
        .collect()
}

/// `english` in the current language.
pub fn tr(english: &'static str) -> &'static str {
    let lang = current();
    if lang == Lang::En {
        return english;
    }
    table(lang).get(english).copied().unwrap_or(english)
}

/// `english` in the current language with its `{0}`, `{1}` … filled in.
pub fn trf(english: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    fill(tr(english), args)
}

fn fill(template: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after
            .find('}')
            .and_then(|close| after[..close].parse::<usize>().ok().map(|i| (i, close)))
        {
            Some((i, close)) => {
                if let Some(arg) = args.get(i) {
                    use std::fmt::Write;
                    let _ = write!(out, "{arg}");
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use super::*;

    #[test]
    fn system_tags_map_to_the_games_languages() {
        assert_eq!(Lang::from_tag("th-TH"), Some(Lang::Th));
        assert_eq!(Lang::from_tag("zh-Hant-TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::from_tag("zh_HK"), Some(Lang::ZhTw));
        assert_eq!(Lang::from_tag("zh-Hans-CN"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_tag("zh"), Some(Lang::ZhCn));
        assert_eq!(Lang::from_tag("pt-BR"), Some(Lang::Pt));
        assert_eq!(Lang::from_tag("in-ID"), Some(Lang::Id));
        assert_eq!(Lang::from_tag("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_tag("nl-NL"), None);
    }

    #[test]
    fn slots_fill_in_any_order() {
        assert_eq!(
            fill("Saved {0} · {1}", &[&"a.json", &3]),
            "Saved a.json · 3"
        );
        assert_eq!(fill("{1}に{0}", &[&"x", &"y"]), "yにx");
        assert_eq!(fill("{x} {0}", &[&1]), "{x} 1");
        assert_eq!(fill("{5}", &[&1]), "");
    }

    /// Every `tr("…")`/`trf("…"` text in the source, from the files on disk.
    fn texts_in_source() -> BTreeSet<String> {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut found = BTreeSet::new();
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    // Split so this file's own source doesn't match. rustfmt
                    // may break a long call after the parenthesis.
                    for call in [concat!("tr", "("), concat!("trf", "(")] {
                        let mut rest = text.as_str();
                        while let Some(at) = rest.find(call) {
                            let before = rest[..at].chars().last();
                            rest = &rest[at + call.len()..];
                            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                                continue;
                            }
                            let Some(literal) = rest.trim_start().strip_prefix('"') else {
                                continue;
                            };
                            rest = literal;
                            let mut value = String::new();
                            let mut chars = rest.chars();
                            while let Some(c) = chars.next() {
                                match c {
                                    '\\' => match chars.next() {
                                        Some('n') => value.push('\n'),
                                        Some(other) => value.push(other),
                                        None => break,
                                    },
                                    '"' => break,
                                    c => value.push(c),
                                }
                            }
                            found.insert(value);
                        }
                    }
                }
            }
        }
        found.remove("…");
        found
    }

    fn slots(text: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let after = &rest[open + 1..];
            if let Some(close) = after.find('}')
                && after[..close].parse::<usize>().is_ok()
            {
                out.insert(after[..close].to_owned());
            }
            rest = after;
        }
        out
    }

    #[test]
    fn en_json_lists_every_text_in_the_code() {
        let en: HashMap<String, String> =
            serde_json::from_str(include_str!("../locales/en.json")).unwrap();
        let listed: BTreeSet<String> = en.keys().cloned().collect();
        let used = texts_in_source();
        let missing: Vec<_> = used.difference(&listed).collect();
        let stale: Vec<_> = listed.difference(&used).collect();
        assert!(
            missing.is_empty() && stale.is_empty(),
            "locales/en.json is out of date (run `python apps/desktop/locales/extract.py`)\n\
             missing: {missing:#?}\nstale: {stale:#?}"
        );
    }

    #[test]
    fn translations_keep_their_slots_and_known_texts() {
        let en: HashMap<String, String> =
            serde_json::from_str(include_str!("../locales/en.json")).unwrap();
        for lang in Lang::ALL.into_iter().skip(1) {
            let map: HashMap<String, String> = serde_json::from_str(lang.source())
                .unwrap_or_else(|e| panic!("locales/{}.json: {e}", lang.code()));
            for (english, translated) in &map {
                assert!(
                    en.contains_key(english),
                    "locales/{}.json translates a text the code no longer has: {english:?}",
                    lang.code()
                );
                if !translated.is_empty() {
                    assert_eq!(
                        slots(english),
                        slots(translated),
                        "locales/{}.json: {english:?} → {translated:?} changes its slots",
                        lang.code()
                    );
                }
            }
        }
    }
}
