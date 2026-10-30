<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 tawan475 -->

# Leyline

The Teyvat record format: GOOD, plus everything else your account holds.

For people writing a reader: [teyvat.me/developers](https://teyvat.me/developers).

Teyvat.me Capture writes your account as Leyline JSON. Leyline is
GOOD-compatible: a [GOOD](https://frzyc.github.io/genshin-optimizer/#/doc)
file, the format Genshin Optimizer reads, with optional `gi_*` keys of
Teyvat.me's own beside GOOD's. Genshin Optimizer and any GOOD reader load it
and skip the extra keys. The file still says `"format": "GOOD"`.

Every field, with its type and meaning: [`teyvatme-export.ts`](teyvatme-export.ts).

## What it sends, and where

On the developer page: [what a file holds](https://teyvat.me/developers#file) and
[uploads](https://teyvat.me/developers#upload).

| | Save / Copy (a Leyline file) | Upload to a tracker |
|---|---|---|
| GOOD: `characters`, `weapons`, `artifacts`, `materials`, `timestamp` | yes, filtered by your export settings | yes, the whole account |
| `gi_achievements`, `gi_achievement_times`, `gi_player`, `gi_characters`, `gi_anecdotes`, `gi_tasks`, `gi_collections`, `gi_wardrobe`, `gi_exploration`, `gi_achievement_progress`, `gi_events`, `gi_teapot`, `gi_miliastra` | yes | yes |
| [`gi_guid` and `gi_exp`](https://teyvat.me/developers#gi_guid) on each weapon and artifact | no | yes |
| `gi_capture` (why the snapshot was taken) | no | yes |

A tracker also gets two more payloads, typed in the same file:

- **Live sync** (`GoodXDelta`, the [Leyline delta](https://teyvat.me/developers#live)):
  small batches of changes between snapshots, while you play. Each also
  carries the account's totals after it (`sum`).
- **Wish history** (`WishImport`): new wish records, after you open
  Wish → History in game.

## Keys of its own

Each key links to its entry on the [developer page](https://teyvat.me/developers#keys).

| Key | What it holds |
|---|---|
| [`gi_achievements`](https://teyvat.me/developers#gi_achievements) | Achievement ids done. |
| [`gi_achievement_times`](https://teyvat.me/developers#gi_achievement_times) | When each was done, for those the game timed. |
| [`gi_player`](https://teyvat.me/developers#gi_player) | Adventure Rank and EXP, World Level, resin at login, Story Keys, stamina, the UID, and the nickname, signature, name card, profile picture and birthday. |
| [`gi_characters`](https://teyvat.me/developers#gi_characters) | Per character: friendship, friendship EXP, when they joined, and EXP into the level. |
| [`gi_anecdotes`](https://teyvat.me/developers#gi_anecdotes) | The Travel Accounts book: every anecdote met and its state, areas, reward track, last wish. |
| [`gi_tasks`](https://teyvat.me/developers#gi_tasks) | What a planner can tick: commissions, the Spiral Abyss, expeditions, realm currency, Welkin Moon, the Epitomized Path, the resin clock, the resin spent today, and weekly boss discounts, boss respawns, Adventure Rank rewards claimed, and login event calendars. |
| [`gi_collections`](https://teyvat.me/developers#gi_collections) | What you've collected: the Radiant Spincrystals in your Serenitea Pot, the Archive, enemies defeated, the Adventurer Handbook, Aranara, your TCG cards and decks, your recipes, and Hangout Events, monsters seen, furnishing blueprints, and crafting, forging and processing recipes, TCG challenges, the music game, furnishing sets, companions' rewards and NPCs met, and the quests finished or in progress. |
| [`gi_achievement_progress`](https://teyvat.me/developers#gi_achievement_progress) | How far each unfinished achievement is. |
| [`gi_events`](https://teyvat.me/developers#gi_events) | The events running and your progress in each. |
| [`gi_teapot`](https://teyvat.me/developers#gi_teapot) | The Serenitea Pot's comfort per realm. |
| [`gi_miliastra`](https://teyvat.me/developers#gi_miliastra) | Miliastra Wonderland cosmetics owned. |
| [`gi_exploration`](https://teyvat.me/developers#gi_exploration) | Statues of The Seven, offerings, the map points and areas you've unlocked, exploration per area, reputation, domains cleared and Natlan's tribal chapters. |
| [`gi_wardrobe`](https://teyvat.me/developers#gi_wardrobe) | Outfits, wind gliders, weapon skins and sprint trails you own and who wears them, and your saved teams. |
| [`gi_capture`](https://teyvat.me/developers#gi_capture) | Uploads only: why this snapshot was taken, and which game login it belongs to. |

## Rules it keeps

Also on the [developer page](https://teyvat.me/developers#rules).

- **Only ever added to.** A key or field is never renamed or removed, so a
  reader written today keeps working. New ones are optional.
- **Left out, never guessed.** A value that wasn't captured, or doesn't pass
  its checks, is not there: never `null`, never a default made up.
- **Times are unix seconds,** except GOOD's own `timestamp` and the live
  batch's `at` (milliseconds).
- **Ids are the game's own;** characters and weapons use GOOD keys. Names in a
  language come from the game data, not the file.
- **Personal text stays yours.** The file holds your UID; nothing in it is sent
  anywhere unless you pick a destination.

A few older fields predate these rules and keep their shape, since installed
apps and stored data rely on them. They are listed at the end of the types
file (§4).
