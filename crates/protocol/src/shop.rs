// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Paimon's Bargains, recognised by what it sells.
//!
//! The game sends a shop's goods only when the player opens it
//! (`GetShopRsp`, the shop one level down; 7.1: command 22081) and the goods
//! just bought after a purchase (`BuyGoodsRsp`, the goods on top). Like
//! everything else this library reads, their field numbers are reshuffled by
//! every game version, and a goods entry's small numbers (its level range,
//! its limit, how many were bought, a flag) cannot be told apart by their
//! values alone: on the four shops of the recorded 7.1 sessions, five pairs
//! of fields fit "bought, then the limit". So Paimon's Bargains is recognised
//! by its Fates (items 223 and 224 for Masterless Stardust and Starglitter),
//! its limit field is the one holding the game data's monthly limit of the
//! Stardust Fates, and the bought field the one left that only counts. What
//! cannot be told apart is left out, not guessed.

use std::collections::{BTreeMap, BTreeSet};

use protobuf::Message;
use protobuf::UnknownValueRef::{LengthDelimited, Varint};

use crate::r#gen::protos::Unk;

/// Intertwined Fate, the limited banners' wish.
pub const INTERTWINED_FATE: u32 = 223;

/// Acquaint Fate, the standard banner's wish.
pub const ACQUAINT_FATE: u32 = 224;

/// Masterless Starglitter, what 4★ wishes give back.
pub const MASTERLESS_STARGLITTER: u32 = 221;

/// Masterless Stardust, what 3★ wishes give back.
pub const MASTERLESS_STARDUST: u32 = 222;

/// How many of each Fate Paimon's Bargains sells for Stardust a month
/// (7.1 `ShopGoodsExcelConfigData`, goods 102501 and 102502).
pub const STARDUST_FATE_LIMIT: u64 = 5;

/// The most goods a shop list holds believed (7.1: 1,173 in the largest).
const MAX_GOODS: usize = 10_000;

/// Goods ids (7.1: 102001 to 101071301).
const GOODS_IDS: std::ops::RangeInclusive<u64> = 1_000..=999_999_999;

/// Item ids an item count names (7.1: 101 and up).
const ITEM_IDS: std::ops::RangeInclusive<u64> = 100..=999_999_999;

/// The most of one goods a month believed.
const MAX_LIMIT: u64 = 1_000;

/// Above this share of a shop's goods, a field that is 1 wherever present is
/// a flag, not a count (7.1: a flag on 1,169 of 1,173 goods).
const FLAG_SHARE: f64 = 0.9;

/// One goods entry of a shop list, as its shape gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShopEntry {
    /// Its lone numbers by field number: its id, limit, bought count, level
    /// range, times...
    pub varints: BTreeMap<u32, u64>,
    /// Its item counts (a message of an item id and a count) by field
    /// number: the goods item and what it costs.
    pub items: BTreeMap<u32, Vec<(u32, u32)>>,
}

/// An item count: a message of an item id and at most a count beside it (0
/// is left out on the wire).
fn item_count(bytes: &[u8]) -> Option<(u32, u32)> {
    let msg = Unk::parse_from_bytes(bytes).ok()?;
    let mut values = Vec::new();
    for (_, value) in msg.unknown_fields().iter() {
        let Varint(v) = value else {
            return None;
        };
        values.push(v);
    }
    match values.as_slice() {
        [id] if ITEM_IDS.contains(id) => Some((u32::try_from(*id).ok()?, 0)),
        [id, count] if ITEM_IDS.contains(id) => {
            Some((u32::try_from(*id).ok()?, u32::try_from(*count).ok()?))
        }
        _ => None,
    }
}

/// The most fields of other messages and lists a goods entry has beside its
/// item counts (7.1: four, mostly empty).
const MAX_OTHER_FIELDS: usize = 4;

/// The fewest lone numbers a goods entry has (7.1: its id, level range,
/// times and limit; a property's entry has one).
const MIN_NUMBERS: usize = 3;

/// A goods entry: no fixed-width numbers, a few numbers with a goods id
/// among them, one or two fields of item counts (the goods item, what it
/// costs) and few other fields. `None` if it is not one: a scene entity has
/// many more fields, a player property one number.
fn shop_entry(bytes: &[u8]) -> Option<ShopEntry> {
    let msg = Unk::parse_from_bytes(bytes).ok()?;
    let mut entry = ShopEntry::default();
    let mut repeated = BTreeSet::new();
    let mut other = BTreeSet::new();
    for (tag, value) in msg.unknown_fields().iter() {
        match value {
            Varint(v) => {
                // A repeated number is not one of its lone numbers.
                if entry.varints.insert(tag, v).is_some() {
                    repeated.insert(tag);
                }
            }
            LengthDelimited(inner) => match item_count(inner) {
                Some(count) => entry.items.entry(tag).or_default().push(count),
                None => {
                    other.insert(tag);
                }
            },
            _ => return None,
        }
    }
    entry.varints.retain(|tag, _| !repeated.contains(tag));
    let ok = (1..=2).contains(&entry.items.len())
        && other.len() <= MAX_OTHER_FIELDS
        && entry.varints.len() >= MIN_NUMBERS
        && entry.varints.values().any(|v| GOODS_IDS.contains(v));
    ok.then_some(entry)
}

/// The goods of every length-delimited value of one field, if each is one.
fn shop_entries(values: &[&[u8]]) -> Option<Vec<ShopEntry>> {
    if values.is_empty() || values.len() > MAX_GOODS {
        return None;
    }
    values.iter().map(|bytes| shop_entry(bytes)).collect()
}

/// A message's length-delimited values, by field number.
fn by_tag(msg: &Unk) -> BTreeMap<u32, Vec<&[u8]>> {
    let mut out: BTreeMap<u32, Vec<&[u8]>> = BTreeMap::new();
    for (tag, value) in msg.unknown_fields().iter() {
        if let LengthDelimited(inner) = value {
            out.entry(tag).or_default().push(inner);
        }
    }
    out
}

/// The goods list of a shop message: the longest list of goods entries on
/// top (`BuyGoodsRsp`) or one level down (`GetShopRsp`'s shop), or `None` if
/// it holds none. A candidate only: the caller tells which shop it is.
pub fn matches_shop_goods(data: &[u8]) -> Option<Vec<ShopEntry>> {
    let top = Unk::parse_from_bytes(data).ok()?;
    let mut best: Option<Vec<ShopEntry>> = None;
    let mut keep = |found: Vec<ShopEntry>| {
        if best.as_ref().is_none_or(|b| found.len() > b.len()) {
            best = Some(found);
        }
    };
    for values in by_tag(&top).values() {
        if let Some(found) = shop_entries(values) {
            keep(found);
        } else if let [one] = values.as_slice()
            && let Ok(inner) = Unk::parse_from_bytes(one)
        {
            for values in by_tag(&inner).values() {
                if let Some(found) = shop_entries(values) {
                    keep(found);
                }
            }
        }
    }
    best
}

/// The goods of a purchase (`BuyGoodsRsp`): one list of goods entries beside
/// numbers only (the shop type, the count bought), or `None` if it is not
/// one. A candidate only: the caller believes goods ids it knows.
pub fn matches_shop_purchase(data: &[u8]) -> Option<Vec<ShopEntry>> {
    let msg = Unk::parse_from_bytes(data).ok()?;
    if msg
        .unknown_fields()
        .iter()
        .any(|(_, value)| !matches!(value, Varint(_) | LengthDelimited(_)))
    {
        return None;
    }
    let lists = by_tag(&msg);
    let [values] = lists.values().collect::<Vec<_>>()[..] else {
        return None;
    };
    shop_entries(values)
}

/// One Fate on sale in Paimon's Bargains this month.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FateStock {
    pub goods_id: u32,
    /// [`INTERTWINED_FATE`] or [`ACQUAINT_FATE`].
    pub item: u32,
    /// [`MASTERLESS_STARDUST`] or [`MASTERLESS_STARGLITTER`].
    pub currency: u32,
    /// How much of it one costs.
    pub cost: u32,
    /// How many were bought this month.
    pub bought: u32,
    /// How many can be this month; `None` when it has no limit.
    pub limit: Option<u32>,
}

/// Where Paimon's Bargains keeps what, learned from its goods.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    /// The goods item's field and its cost's.
    item: u32,
    cost: u32,
    /// The goods id's field.
    id: u32,
    limit: u32,
    /// `None` until something was bought this month (an unbought goods
    /// carries no count), or while no Fate tells it from another field.
    bought: Option<u32>,
}

/// Paimon's Bargains as the game last sent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bargains {
    /// The Fates on sale for Stardust and Starglitter, by currency then item.
    pub fates: Vec<FateStock>,
    /// Whether anything at all was bought there this month.
    pub bought_any: bool,
    layout: Layout,
    /// Every goods' numbers by goods id, to read a purchase against.
    goods: BTreeMap<u32, BTreeMap<u32, u64>>,
}

/// Its Fate and the Stardust or Starglitter it costs, if `entry` is a Fate
/// for one of them (fields `item` and `cost`).
fn fate_of(entry: &ShopEntry, item: u32, cost: u32) -> Option<(u32, u32, u32)> {
    let [(fate, 1)] = entry.items.get(&item)?.as_slice() else {
        return None;
    };
    let [(currency, price)] = entry.items.get(&cost)?.as_slice() else {
        return None;
    };
    let ok = matches!(*fate, INTERTWINED_FATE | ACQUAINT_FATE)
        && matches!(*currency, MASTERLESS_STARDUST | MASTERLESS_STARGLITTER);
    ok.then_some((*fate, *currency, *price))
}

/// The goods' field holding something on every entry with unique values in
/// [`GOODS_IDS`], if exactly one does.
fn id_field(goods: &[ShopEntry]) -> Option<u32> {
    let first = goods.first()?;
    let found: Vec<u32> = first
        .varints
        .keys()
        .copied()
        .filter(|tag| {
            let values: Option<Vec<u64>> =
                goods.iter().map(|g| g.varints.get(tag).copied()).collect();
            values.is_some_and(|values| {
                values.iter().all(|v| GOODS_IDS.contains(v))
                    && values.iter().collect::<BTreeSet<_>>().len() == values.len()
            })
        })
        .collect();
    match found.as_slice() {
        [tag] => Some(*tag),
        _ => None,
    }
}

/// The fields that could count what was bought, given the limit's: present
/// on some goods but not all, and wherever present a count from 1 to that
/// goods' limit; a field that is 1 on nearly every goods is a flag.
fn bought_fields(goods: &[ShopEntry], skip: &[u32], limit: u32) -> Vec<u32> {
    let tags: BTreeSet<u32> = goods
        .iter()
        .flat_map(|g| g.varints.keys().copied())
        .collect();
    tags.into_iter()
        .filter(|tag| !skip.contains(tag))
        .filter(|tag| {
            let with: Vec<&ShopEntry> = goods
                .iter()
                .filter(|g| g.varints.contains_key(tag))
                .collect();
            if with.is_empty() || with.len() == goods.len() {
                return false;
            }
            let counts = with.iter().all(|g| {
                let value = g.varints[tag];
                g.varints
                    .get(&limit)
                    .is_some_and(|max| (1..=*max).contains(&value))
            });
            let flag = with.len() as f64 > FLAG_SHARE * goods.len() as f64
                && with.iter().all(|g| g.varints[tag] == 1);
            counts && !flag
        })
        .collect()
}

/// Paimon's Bargains, if `goods` (see [`matches_shop_goods`]) are its: both
/// Fates for Masterless Stardust are on sale and each field is told apart.
pub fn read_bargains(goods: &[ShopEntry]) -> Option<Bargains> {
    // The goods item and cost fields: where a Stardust Fate's Fate and
    // Stardust are, the same on every Fate.
    let first = goods.iter().find_map(|g| {
        g.items.iter().find_map(|(item, list)| {
            let [(fate, 1)] = list.as_slice() else {
                return None;
            };
            if !matches!(*fate, INTERTWINED_FATE | ACQUAINT_FATE) {
                return None;
            }
            g.items.iter().find_map(|(cost, list)| {
                matches!(list.as_slice(), [(MASTERLESS_STARDUST, _)])
                    .then_some((*item, *cost))
                    .filter(|(item, cost)| item != cost)
            })
        })
    })?;
    let (item, cost) = first;
    let stardust: Vec<&ShopEntry> = goods
        .iter()
        .filter(|g| {
            fate_of(g, item, cost).is_some_and(|(_, currency, _)| currency == MASTERLESS_STARDUST)
        })
        .collect();
    let fates_for_stardust: BTreeSet<u32> = stardust
        .iter()
        .filter_map(|g| fate_of(g, item, cost).map(|(fate, _, _)| fate))
        .collect();
    if stardust.len() != 2
        || fates_for_stardust != BTreeSet::from([INTERTWINED_FATE, ACQUAINT_FATE])
    {
        return None;
    }
    let id = id_field(goods)?;
    // The limit: the field holding the Stardust Fates' limit on both, on most
    // goods (an unlimited one has none).
    let limits: Vec<u32> = stardust[0]
        .varints
        .iter()
        .filter(|(tag, value)| {
            **tag != id
                && **value == STARDUST_FATE_LIMIT
                && stardust[1].varints.get(tag) == Some(&STARDUST_FATE_LIMIT)
                && 2 * goods.iter().filter(|g| g.varints.contains_key(tag)).count() >= goods.len()
                && goods
                    .iter()
                    .filter_map(|g| g.varints.get(tag))
                    .all(|v| (1..=MAX_LIMIT).contains(v))
        })
        .map(|(tag, _)| *tag)
        .collect();
    let [limit] = limits.as_slice() else {
        return None;
    };
    // The bought count. Another small field can fit too (7.1: a 1 on a
    // limit-1 goods only some accounts have); then the one on the Fates
    // decides, and when none is on them they bought none, but whether
    // anything else was is not known, so nothing is said.
    let candidates = bought_fields(goods, &[id, *limit], *limit);
    let on_fates: Vec<u32> = candidates
        .iter()
        .copied()
        .filter(|tag| {
            goods
                .iter()
                .any(|g| fate_of(g, item, cost).is_some() && g.varints.contains_key(tag))
        })
        .collect();
    let bought = match (candidates.as_slice(), on_fates.as_slice()) {
        ([], _) => None,
        ([tag], _) | (_, [tag]) => Some(*tag),
        (_, []) => None,
        _ => return None,
    };
    let layout = Layout {
        item,
        cost,
        id,
        limit: *limit,
        bought,
    };
    let goods_by_id = goods
        .iter()
        .filter_map(|g| Some((u32::try_from(*g.varints.get(&id)?).ok()?, g.varints.clone())))
        .collect();
    let mut bargains = Bargains {
        fates: Vec::new(),
        bought_any: bought.is_some(),
        layout,
        goods: goods_by_id,
    };
    bargains.fates = goods.iter().filter_map(|g| bargains.fate(g)).collect();
    bargains
        .fates
        .sort_by_key(|f| (f.currency, f.item, f.goods_id));
    Some(bargains)
}

impl Bargains {
    /// One goods' Fate stock, if it is a Fate for Stardust or Starglitter.
    fn fate(&self, entry: &ShopEntry) -> Option<FateStock> {
        let layout = self.layout;
        let (item, currency, cost) = fate_of(entry, layout.item, layout.cost)?;
        let count = |tag: Option<u32>| {
            tag.and_then(|tag| entry.varints.get(&tag))
                .and_then(|v| u32::try_from(*v).ok())
        };
        Some(FateStock {
            goods_id: u32::try_from(*entry.varints.get(&layout.id)?).ok()?,
            item,
            currency,
            cost,
            bought: count(layout.bought).unwrap_or(0),
            limit: count(Some(layout.limit)),
        })
    }

    /// Take a purchase (`BuyGoodsRsp`'s goods, see [`matches_shop_goods`]):
    /// `true` when every goods in it is one of these. A first purchase this
    /// month also tells which field counts: the one that appeared or grew
    /// on every goods bought.
    pub fn apply_purchase(&mut self, bought: &[ShopEntry]) -> bool {
        let layout = self.layout;
        let ids: Option<Vec<u32>> = bought
            .iter()
            .map(|g| u32::try_from(*g.varints.get(&layout.id)?).ok())
            .collect();
        let Some(ids) = ids else {
            return false;
        };
        if ids.is_empty() || !ids.iter().all(|id| self.goods.contains_key(id)) {
            return false;
        }
        if layout.bought.is_none() {
            let grew = |g: &ShopEntry, id: &u32, tag: &u32| {
                let before = self.goods[id].get(tag).copied().unwrap_or(0);
                let now = g.varints[tag];
                *tag != layout.id
                    && *tag != layout.limit
                    && now > before
                    && g.varints.get(&layout.limit).is_some_and(|max| now <= *max)
            };
            let first = &bought[0];
            let found: Vec<u32> = first
                .varints
                .keys()
                .copied()
                .filter(|tag| {
                    bought
                        .iter()
                        .zip(&ids)
                        .all(|(g, id)| g.varints.contains_key(tag) && grew(g, id, tag))
                })
                .collect();
            if let [tag] = found.as_slice() {
                self.layout.bought = Some(*tag);
            }
        }
        self.bought_any = true;
        for (g, id) in bought.iter().zip(&ids) {
            self.goods.insert(*id, g.varints.clone());
            if let Some(stock) = self.fate(g)
                && let Some(held) = self.fates.iter_mut().find(|f| f.goods_id == *id)
            {
                *held = stock;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return out;
            }
            out.push(byte | 0x80);
        }
    }

    fn v(tag: u32, value: u64) -> Vec<u8> {
        let mut out = varint(u64::from(tag) << 3);
        out.extend(varint(value));
        out
    }

    fn b(tag: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = varint((u64::from(tag) << 3) | 2);
        out.extend(varint(payload.len() as u64));
        out.extend_from_slice(payload);
        out
    }

    /// A goods entry in 7.1's layout: item 2, id 5, cost 8, bought 13, limit
    /// 14, max level 1, min level 11, times 4/12, a flag on 1293.
    fn goods(id: u64, item: u64, cost: Option<(u64, u64)>, bought: u64, limit: u64) -> Vec<u8> {
        let mut out = [
            v(1, 99),
            b(2, &[v(1, item), v(2, 1)].concat()),
            v(4, 1_575_129_600),
            v(5, id),
            v(11, 1),
            v(12, 2_051_193_600),
            v(1293, 1),
        ]
        .concat();
        if let Some((currency, count)) = cost {
            out.extend(b(8, &[v(1, currency), v(2, count)].concat()));
        }
        if bought > 0 {
            out.extend(v(13, bought));
        }
        if limit > 0 {
            out.extend(v(14, limit));
        }
        out
    }

    /// Paimon's Bargains' Fates and a few other goods; `bought` per goods id.
    fn bargains(bought: &[(u64, u64)]) -> Vec<Vec<u8>> {
        let of = |id: u64| bought.iter().find(|(g, _)| *g == id).map_or(0, |(_, n)| *n);
        vec![
            // The Fates for Primogems: no cost item, no limit.
            goods(102001, 223, None, 0, 0),
            goods(102002, 224, None, 0, 0),
            goods(102003, 223, Some((221, 5)), of(102003), 5),
            goods(102004, 224, Some((221, 5)), of(102004), 5),
            goods(102501, 223, Some((222, 75)), of(102501), 5),
            goods(102502, 224, Some((222, 75)), of(102502), 5),
            goods(102601, 202, Some((222, 10)), of(102601), 10),
            goods(102602, 104003, Some((222, 100)), of(102602), 20),
        ]
    }

    /// `GetShopRsp` in 7.1's layout: the shop on 9 (its goods on 4), the type on 7.
    fn shop_rsp(goods: &[Vec<u8>]) -> Vec<u8> {
        let shop: Vec<u8> = goods
            .iter()
            .flat_map(|g| b(4, g))
            .chain(v(8, 1001))
            .collect();
        [b(9, &shop), v(7, 1001)].concat()
    }

    #[test]
    fn the_fates_are_read_with_what_was_bought() {
        let rsp = shop_rsp(&bargains(&[(102501, 2), (102004, 5), (102602, 7)]));
        let found = read_bargains(&matches_shop_goods(&rsp).unwrap()).unwrap();
        assert!(found.bought_any);
        let stock: Vec<(u32, u32, u32, u32, Option<u32>)> = found
            .fates
            .iter()
            .map(|f| (f.currency, f.item, f.cost, f.bought, f.limit))
            .collect();
        assert_eq!(
            stock,
            vec![
                (221, 223, 5, 0, Some(5)),
                (221, 224, 5, 5, Some(5)),
                (222, 223, 75, 2, Some(5)),
                (222, 224, 75, 0, Some(5)),
            ]
        );
    }

    #[test]
    fn nothing_bought_reads_as_nothing_bought() {
        let rsp = shop_rsp(&bargains(&[]));
        let found = read_bargains(&matches_shop_goods(&rsp).unwrap()).unwrap();
        assert!(!found.bought_any);
        assert!(
            found
                .fates
                .iter()
                .all(|f| f.bought == 0 && f.limit == Some(5))
        );
    }

    #[test]
    fn a_purchase_counts_and_teaches_the_field() {
        let rsp = shop_rsp(&bargains(&[]));
        let mut found = read_bargains(&matches_shop_goods(&rsp).unwrap()).unwrap();
        // `BuyGoodsRsp` in 7.1's layout: the goods on 5, the type on 15, the count on 7.
        let buy = [
            b(5, &goods(102501, 223, Some((222, 75)), 3, 5)),
            v(15, 1001),
            v(7, 3),
        ]
        .concat();
        assert!(found.apply_purchase(&matches_shop_purchase(&buy).unwrap()));
        assert!(found.bought_any);
        let fate = found.fates.iter().find(|f| f.goods_id == 102501).unwrap();
        assert_eq!(fate.bought, 3);
        // Another shop's goods are not these.
        let other = b(5, &goods(217346, 333118, Some((107021, 500)), 1, 2));
        assert!(!found.apply_purchase(&matches_shop_purchase(&other).unwrap()));
        // The opened shop is no purchase: its goods are one level down.
        assert_eq!(matches_shop_purchase(&rsp), None);
    }

    #[test]
    fn other_shops_and_lists_are_not_the_bargains() {
        // A shop without the Stardust Fates (the Realm Depot's layout).
        let depot: Vec<Vec<u8>> = (0..20)
            .map(|n| goods(217_300 + n, 333_000 + n, Some((107021, 500)), n % 3, 2))
            .collect();
        let goods = matches_shop_goods(&shop_rsp(&depot)).unwrap();
        assert_eq!(goods.len(), 20);
        assert_eq!(read_bargains(&goods), None);
        // Loot hints: item counts beside numbers, no shop.
        let hint = [b(13, &[v(1, 202), v(2, 500)].concat()), v(11, 37)].concat();
        assert_eq!(
            matches_shop_goods(&hint).and_then(|g| read_bargains(&g)),
            None
        );
        assert_eq!(matches_shop_goods(&[v(1, 5), v(2, 7)].concat()), None);
        // An entity-like entry: floats beside the numbers and an item count.
        let entity = b(
            1,
            &[
                v(5, 102501),
                b(2, &[v(1, 202), v(2, 5)].concat()),
                vec![0x0d, 0, 0, 0x80, 0x3f],
            ]
            .concat(),
        );
        assert_eq!(matches_shop_goods(&entity), None);
        assert_eq!(matches_shop_purchase(&entity), None);
    }

    /// A limit-1 goods for Starglitter with a 1 on field 7 beside its limit,
    /// as some 7.1 accounts' shops carry it.
    fn flagged_goods() -> Vec<u8> {
        let mut out = goods(102005, 1020, Some((221, 34)), 0, 1);
        out.extend(v(7, 1));
        out
    }

    #[test]
    fn a_field_beside_the_count_is_told_apart_by_the_fates() {
        let mut list = bargains(&[(102501, 5), (102502, 5), (102602, 20)]);
        list.push(flagged_goods());
        let found = read_bargains(&matches_shop_goods(&shop_rsp(&list)).unwrap()).unwrap();
        assert!(found.bought_any);
        let stardust: Vec<(u32, u32)> = found
            .fates
            .iter()
            .filter(|f| f.currency == MASTERLESS_STARDUST)
            .map(|f| (f.item, f.bought))
            .collect();
        assert_eq!(stardust, vec![(INTERTWINED_FATE, 5), (ACQUAINT_FATE, 5)]);
    }

    #[test]
    fn with_no_fate_bought_the_fates_are_read_and_the_rest_left_unsaid() {
        let mut list = bargains(&[(102602, 7)]);
        list.push(flagged_goods());
        let mut found = read_bargains(&matches_shop_goods(&shop_rsp(&list)).unwrap()).unwrap();
        assert!(!found.bought_any);
        assert!(found.fates.iter().all(|f| f.bought == 0));
        // A purchase still teaches the count.
        let buy = [
            b(5, &goods(102502, 224, Some((222, 75)), 1, 5)),
            v(15, 1001),
            v(7, 1),
        ]
        .concat();
        assert!(found.apply_purchase(&matches_shop_purchase(&buy).unwrap()));
        let fate = found.fates.iter().find(|f| f.goods_id == 102502).unwrap();
        assert_eq!(fate.bought, 1);
    }

    #[test]
    fn two_fields_that_both_count_are_not_guessed() {
        // A second count-like field beside the bought one: which is which
        // cannot be told, so nothing is read.
        let mut list = bargains(&[(102501, 2)]);
        list[6].extend(v(7, 3));
        list[4].extend(v(7, 1));
        let goods = matches_shop_goods(&shop_rsp(&list)).unwrap();
        assert_eq!(read_bargains(&goods), None);
    }
}
