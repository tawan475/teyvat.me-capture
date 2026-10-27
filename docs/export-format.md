<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 2026 tawan475 -->

# Export format

Teyvat.me Capture writes your account as
[GOOD](https://frzyc.github.io/genshin-optimizer/#/doc) JSON, the format
Genshin Optimizer reads, with a few keys of its own beside GOOD's. Anything
that reads GOOD can read the file and skip the rest.

Every field, with its type and meaning: [`teyvatme-export.ts`](teyvatme-export.ts).

## What it sends, and where

| | Save / Copy (a GOOD file) | Upload to a tracker |
|---|---|---|
| GOOD: `characters`, `weapons`, `artifacts`, `materials`, `timestamp` | yes, filtered by your export settings | yes, the whole account |
| `gi_achievements`, `gi_achievement_times`, `gi_player`, `gi_characters`, `gi_anecdotes`, `gi_tasks`, `gi_collections`, `gi_wardrobe`, `gi_exploration`, `gi_achievement_progress`, `gi_events`, `gi_teapot` | yes | yes |
| `gi_guid` and `gi_exp` on each weapon and artifact | no | yes |
| `gi_capture` (why the snapshot was taken) | no | yes |

A tracker also gets two more payloads, typed in the same file:

- **Live sync** (`GoodXDelta`): small batches of changes between snapshots,
  while you play.
- **Wish history** (`WishImport`): new wish records, after you open
  Wish → History in game.

## Keys of its own

| Key | What it holds |
|---|---|
| `gi_achievements` | Achievement ids done. |
| `gi_achievement_times` | When each was done, for those the game timed. |
| `gi_player` | Adventure Rank and EXP, World Level, resin at login, Story Keys, stamina, and the UID. |
| `gi_characters` | Per character: friendship, friendship EXP, when they joined, and EXP into the level. |
| `gi_anecdotes` | The Travel Accounts book: every anecdote met and its state, areas, reward track, last wish. |
| `gi_tasks` | What a planner can tick: commissions, the Spiral Abyss, expeditions, realm currency, Welkin Moon, the Epitomized Path, the resin clock, and weekly boss discounts, boss respawns and Adventure Rank rewards claimed. |
| `gi_collections` | What you've collected: the Radiant Spincrystals in your Serenitea Pot, the Archive, enemies defeated, the Adventurer Handbook, Aranara, your TCG cards and decks, your recipes, and Hangout Events, monsters seen, furnishing blueprints, and (next release) crafting, forging and processing recipes and TCG challenges. |
| `gi_achievement_progress` | How far each unfinished achievement is. |
| `gi_events` | The events running and your progress in each. |
| `gi_teapot` | (next release) The Serenitea Pot's comfort per realm. |
| `gi_exploration` | Statues of The Seven, offerings, the map points and areas you've unlocked, exploration per area, and reputation. |
| `gi_wardrobe` | Outfits, wind gliders, weapon skins and sprint trails you own and who wears them, and your saved teams. |
| `gi_capture` | Uploads only: why this snapshot was taken, and which game login it belongs to. |

## Rules it keeps

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
