// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

/**
 * Teyvat.me Capture's export format, as TypeScript. See export-format.md.
 *
 *   1. TeyvatmeExport   the export file / snapshot upload (GOOD + Teyvat.me's own keys)
 *   2. GoodXDelta       live sync batches between snapshots
 *   3. WishImport       wish history sync
 *
 * Status tags on every part:
 *   [GOOD]      Genshin Optimizer's format. Not ours to change.
 *   [shipped]   In released versions.
 *   [next]      In the next release.
 *
 * The source of truth is `crates/core/src/good.rs`; a test there pins the key
 * names. Known quirks are listed at the end (§4).
 */

// ═════════════════════════════════════════════════════════════════════════════
// Conventions (what every Teyvat.me part follows; §4 lists where today's don't)
// ═════════════════════════════════════════════════════════════════════════════

/** Unix seconds. Every Teyvat.me time is this, except where noted. */
type UnixSeconds = number
/** Unix milliseconds: only GOOD's own `timestamp` and the live batch's `at`. */
type UnixMs = number
/** A GOOD key: the English name stripped and PascalCased ("Hu Tao" → "HuTao"). */
type GoodKey = string
/** A character's GOOD key; the Traveler carries the element ("TravelerCryo"). */
type CharacterKey = GoodKey
/** The game's own numeric id (ExcelBinOutput row id); the tracker names it from the game data. */
type GameId = number
/** A JSON object key holding a GameId (JSON keys are strings). */
type IdKey = `${number}`
/** A u64 the game uses (item guids): a decimal string, since it doesn't fit a JS number. */
type U64String = string

// Rules:
// - Every Teyvat.me key is optional and append-only: never renamed or removed, only added.
// - A value that wasn't captured, or failed its check, is left out: never `null`, never guessed.
// - Top-level keys are `gi_<snake_case>` (so they can't collide with GOOD's); fields inside are camelCase.
// - Something with a unique id is a Record keyed by that id; a list only when order or repeats matter.
// - Enumerations are lowercase strings, not the game's numbers.  (§4.3: anecdotes don't yet)
// - Readers treat an absent number as 0 (the game itself leaves zeros off the wire).

// ═════════════════════════════════════════════════════════════════════════════
// 1. The export file
// ═════════════════════════════════════════════════════════════════════════════

export interface TeyvatmeExport {
  // ── GOOD ──────────────────────────────────────────────────────────── [GOOD]
  format: 'GOOD'
  version: 3
  /** What wrote the file: "Teyvat.me" from 1.3.0; earlier versions and irminsul wrote "Irminsul". */
  source: 'Teyvat.me' | 'Irminsul'
  characters: GoodCharacter[]
  artifacts: (GoodArtifact & ItemExtras)[]
  weapons: (GoodWeapon & ItemExtras)[]
  /** Material GOOD key → count. */
  materials: Record<GoodKey, number>
  /** When the data was captured. GOOD's field, so milliseconds. */
  timestamp?: UnixMs

  // ── Teyvat.me ─────────────────────────────────────────────────────────────
  /** [shipped] Achievement ids done. (§4.1: an older irminsul name, outside the gi_ pattern's spirit) */
  gi_achievements?: GameId[]
  /** [shipped] When each of `gi_achievements` was done; only those the game timed. */
  gi_achievement_times?: Record<IdKey, UnixSeconds>
  /** [shipped] Account values GOOD has no place for. */
  gi_player?: GiPlayer
  /** [shipped] Per character, keyed like `characters[].key`; only characters with a known value. */
  gi_characters?: Record<CharacterKey, GiCharacter>
  /** [shipped] The Travel Accounts book. */
  gi_anecdotes?: GiAnecdotes
  /** [shipped] What the Planner ticks its tasks from. */
  gi_tasks?: GiTasks
  /** [shipped] Why this snapshot was taken (live sync). Tracker uploads only. */
  gi_capture?: GiCapture
  /** [shipped] What the player has collected. */
  gi_collections?: GiCollections
  /** [shipped] What the characters own and wear, and the saved teams. */
  gi_wardrobe?: GiWardrobe
  /** [shipped] Exploration: Statues, offerings, map points, exploration, reputation. */
  gi_exploration?: GiExploration
  /** [shipped] How far each unfinished achievement is, by achievement id (those with a total). */
  gi_achievement_progress?: Record<IdKey, { progress: number; total: number }>
  /**
   * [shipped] The events running when the game was asked (right after login), by event id
   * (NewActivityExcelConfigData): this run's schedule id, when it opened and closes, and
   * its progress counters by watcher id (absent when it has none).
   */
  gi_events?: Record<
    IdKey,
    {
      schedule: GameId
      start: UnixSeconds
      end: UnixSeconds
      progress?: Record<IdKey, { progress: number; total: number; claimed: boolean }>
    }
  >
  /** [shipped] The Serenitea Pot. Its Trust Rank follows from `gi_tasks.realm.limit` (HomeWorldLevel). */
  gi_teapot?: {
    /** Comfort by realm (HomeWorldModule id): the mansion's and each outdoor block's. */
    comfort?: Record<IdKey, { mansion: number; blocks: number[] }>
  }
  /** [shipped] Miliastra Wonderland. */
  gi_miliastra?: {
    /** Cosmetics owned by id (outfits, effects...): when obtained, 0 when the game sent no time. */
    cosmetics?: Record<IdKey, UnixSeconds | 0>
  }
}

// ── GOOD's own shapes ─────────────────────────────────────────────── [GOOD]

export interface GoodCharacter {
  key: CharacterKey
  level: number
  constellation: number
  ascension: number
  /** Base talent levels, without C3/C5. */
  talent: { auto: number; skill: number; burst: number }
}

export interface GoodWeapon {
  key: GoodKey
  level: number
  ascension: number
  refinement: number
  /** Who wears it: a character key; "Traveler" for any Traveler; "" when nobody. */
  location: CharacterKey | ''
  lock: boolean
}

export interface GoodSubstat {
  key: string
  value: number
  initialValue: number
}

export interface GoodArtifact {
  setKey: GoodKey
  slotKey: 'flower' | 'plume' | 'sands' | 'goblet' | 'circlet'
  level: number
  rarity: number
  mainStatKey: string
  location: CharacterKey | ''
  lock: boolean
  substats: GoodSubstat[]
  totalRolls: number
  astralMark: boolean
  /** GOOD's own spelling. */
  elixerCrafted: boolean
  unactivatedSubstats: GoodSubstat[]
}

/** Teyvat.me's additions on every weapon and artifact. Tracker uploads only (a GOOD file for Genshin Optimizer has none). */
export interface ItemExtras {
  /** [shipped] The game's item id. Live sync names items by it. */
  gi_guid?: U64String
  /** [shipped] EXP toward the next level: 0 at a cap or never fed. */
  gi_exp?: number
}

// ── gi_player ───────────────────────────────────────────────────── [shipped]

/** The property snapshot the game sends at login (not followed live, except Mora/Primogems via `materials`). */
export interface GiPlayer {
  /** From the item guids. (§4.4: a number here, a string in WishImport) */
  uid?: number
  /** Adventure Rank and its EXP toward the next. */
  ar?: number
  arExp?: number
  /** World Level, and the highest the account may pick. */
  wl?: number
  wlLimit?: number
  /** Original Resin at login. (§4.5: also in gi_tasks.resin, with its clock) */
  resin?: number
  storyKeys?: number
  /** In the game's units: 24000 = the 240 it shows. (§4.6) */
  maxStamina?: number
  /** The game data dump commit the GOOD keys came from. */
  gameData?: string
  /** [shipped] The in-game name. */
  nickname?: string
  /** [shipped] The profile's signature; "" when the player set none. */
  signature?: string
  /** [shipped] The name card on the profile: its item id (210001...; the wardrobe's `namecards`). */
  nameCard?: GameId
  /**
   * [shipped] The profile picture as the game sends it. 7.1 sends `id`
   * (ProfilePictureExcelConfigData) and `frameId`; older clients a character
   * (`avatarId`, 10000xxx) and its outfit (`costumeId`).
   */
  profilePicture?: { id?: GameId; avatarId?: GameId; costumeId?: GameId; frameId?: GameId }
  /** [shipped] The birthday set on the profile. */
  birthday?: { month: number; day: number }
}

// ── gi_characters ──────────────────────────────────────────────── [shipped]

export interface GiCharacter {
  /** [shipped] 1–10. The Traveler has none. */
  friendship?: number
  /** [shipped] When the character joined the account. */
  obtainedAt?: UnixSeconds
  /** [shipped] EXP toward the next level: 0 at the ascension cap and past 90. */
  exp?: number
  /** [shipped] Friendship EXP toward the next level: 0 at 10; absent for the Traveler. */
  friendshipExp?: number
}

// ── gi_anecdotes ────────────────────────────────────────────────── [shipped]

export interface GiAnecdotes {
  /** Every anecdote met. Absent = never met, not "not done". */
  anecdotes: Record<IdKey, { state: AnecdoteState; finishCount: number }>
  /** Unlocked area tabs, sorted. */
  unlockedAreas: GameId[]
  /** The reward track: every `target` recorded anecdotes trade for a reward. */
  reward?: { index: number; progress: number; target: number }
  /** The last wish made. */
  wish?: { npcId?: GameId; lastWishTime: UnixSeconds }
}
/** The game's number, inferred from one account: 3 recorded, 2 in progress, 0 idle. (§4.3) */
type AnecdoteState = 0 | 2 | 3

// ── gi_tasks ───────────────────────────────────────────────────── [shipped]

export interface GiTasks {
  /**
   * [shipped] Today's Daily Commissions. [shipped] `claimed` (1.16.0): Katheryne's reward taken;
   * absent when not known.
   */
  commissions?: { finished: number; total: number; claimed?: boolean }
  /** [shipped] The Spiral Abyss period running; `end` = when the next begins. */
  abyss?: { start: UnixSeconds; end: UnixSeconds; stars: number; maxStars: number }
  /** [shipped] Who is out on an expedition (empty = nobody), soonest done first. */
  expeditions?: {
    /** Absent when the roster wasn't captured. */
    character?: CharacterKey
    id: GameId
    start: UnixSeconds
    hours: number
    doneAt: UnixSeconds
  }[]
  /** [shipped] Serenitea Pot realm currency in the jar. */
  realm?: { value: number; limit: number }
  /** [shipped] Blessing of the Welkin Moon days left. */
  welkin?: { days: number }
  /** [shipped] The weapon banner's Epitomized Path, as the Wish screen last showed it. */
  wishPath?: { weapon: GoodKey; points: number; max: number }
  /** [shipped] Original Resin and when the next comes (one per 8 minutes; absent at 200+). */
  resin?: { value: number; nextAt: UnixSeconds }
  /**
   * [shipped] The weekly bosses' resin discounts: left this week of `limit`; what one costs with the
   * discount and without it. [shipped] `used`: discounts used. An export without `used` (1.13.2 and
   * before) held the count used in `left`: the discounts left there are `limit - left`.
   */
  weeklyBosses?: { left: number; used?: number; limit: number; cost: number; fullCost: number }
  /** [shipped] Adventure Ranks whose reward was claimed (2-60, ascending). */
  arRewards?: number[]
  /**
   * [shipped] When each boss and elite of the Adventurer Handbook respawns, by its Handbook id
   * (InvestigationMonsterConfigData): one time per spot, ascending; a time already past = up now.
   */
  respawns?: Record<IdKey, UnixSeconds[]>
  /** [shipped] Login events' calendars by config id: days signed in, days claimed, last sign-in, open/close. */
  signIns?: Record<IdKey, { days: number; claimed: number[]; lastAt: UnixSeconds; start: UnixSeconds; end: UnixSeconds }>
  /**
   * [shipped] (1.16.0) Original Resin spent on one game day: `day` 1 (Monday) to 7 of the game's
   * week, `at` when the game first said this value (the game day `at` falls in is the day it counts).
   */
  resinSpent?: { value: number; day: number; at: UnixSeconds }
}

// ── gi_collections ─────────────────────────────────────── [shipped]
// Every part is the game's state at login; a part not captured is absent.

export interface GiCollections {
  /**
   * [shipped] The Radiant Spincrystals in the Serenitea Pot's music list, by
   * item id (141001 = "Radiant Spincrystal 1"), ascending. From the list the
   * game sends at every login; absent while the Pot holds none.
   */
  spincrystals?: GameId[]
  /** [shipped] The Archive's unlocked entries by tab, codex ids ascending. */
  archive?: Partial<
    Record<
      'quests' | 'weapons' | 'livingBeings' | 'materials' | 'books' | 'tutorials' | 'viewpoints' | 'artifacts',
      GameId[]
    >
  >
  /**
   * [shipped] By Living Beings codex id: enemies defeated, creatures captured.
   * Only those with a count; `captured` absent at 0.
   */
  defeats?: Record<IdKey, { defeated: number; captured?: number }>
  /** [shipped] The Adventurer Handbook: its chapters and every target in them. */
  handbook?: {
    chapters: Record<IdKey, Progress>
    /** Keyed by the target's quest id. */
    targets: Record<IdKey, Progress & { chapter: GameId }>
  }
  /** [shipped] Aranara collections, by collection type then id (only type 1 in 7.1). */
  aranara?: Record<IdKey, Record<IdKey, 'none' | 'collectable' | 'collected' | 'finished'>>
  /** [shipped] Genius Invokation TCG. */
  tcg?: {
    /** Cards owned (character cards below 10000, action cards above). */
    cards: Record<IdKey, { count: number; proficiency?: number }>
    /** Saved decks by id; `name` is the player's own text. */
    decks?: Record<IdKey, { name: string; characters: GameId[]; cards: GameId[]; valid: boolean }>
    /** The deck in use. */
    currentDeck?: GameId
    /** [shipped] Challenge levels played (GCGLevelExcelConfigData) → the challenge ids finished in each. */
    levels?: Record<IdKey, GameId[]>
    /** [shipped] Boss challenges unlocked (GCGBossLevelExcelConfigData ids). */
    bosses?: GameId[]
    /** [shipped] The Tavern: by character, the level ids unlocked with them. */
    tavern?: Record<IdKey, GameId[]>
  }
  /** [shipped] Every recipe unlocked → its proficiency (0 when none yet; the most is in the game data). */
  recipes?: Record<IdKey, number>
  /** [shipped] Hangout Events by chapter (101401 = the first). */
  hangouts?: Record<
    IdKey,
    {
      /** Absent when it couldn't be read. */
      state?: 'closed' | 'locked' | 'open' | 'accepted'
      /** By ending (= its reward id): not seen, seen with its reward waiting, or claimed. */
      endings: Record<IdKey, 'locked' | 'unclaimed' | 'claimed'>
      /** Pictures unlocked, ascending, of `picturesTotal`. */
      pictures: GameId[]
      picturesTotal: number
    }
  >
  /** [shipped] Every monster seen (monster ids, ascending; more than the Archive's Living Beings). */
  monstersSeen?: GameId[]
  /** [shipped] Furnishing blueprints learned: the furnishings' item ids, ascending. */
  blueprints?: GameId[]
  /** [shipped] Recipes unlocked, ascending: at the crafting bench (Combine), the blacksmith (Forge), processing (Compound). */
  crafting?: { crafting?: GameId[]; forging?: GameId[]; processing?: GameId[] }
  /** [shipped] The music game's songs by beatmap id: best score (absent if never played) and reward taken. */
  musicGame?: Record<IdKey, { score?: number; claimed: boolean }>
  /** [shipped] Furnishing sets learned (FurnitureSuiteExcelConfigData ids, ascending). */
  furnishingSets?: GameId[]
  /** [shipped] Companions' rewards collected in the Serenitea Pot (HomeWorldEventExcelConfigData ids, ascending). */
  companionRewards?: GameId[]
  /** [shipped] NPCs met (NpcFirstMetExcelConfigData ids, ascending). */
  npcsMet?: GameId[]
  /**
   * [shipped] (1.17.0) Every parent quest touched (MainQuestExcelConfigData ids, ascending, all kinds:
   * Archon, Story, World, Event…), from the login's list: `active` = accepted, not finished
   * (absent when none). Login only: the next login's list replaces it.
   */
  quests?: { finished: GameId[]; active?: GameId[] }
}
interface Progress {
  progress: number
  total: number
  /** "complete" = done but its reward not taken yet. */
  state: 'open' | 'complete' | 'claimed'
}

// ── gi_wardrobe ──────────────────────────────────────────────────── [shipped]

export interface GiWardrobe {
  /** Owned ids, and who wears which. */
  outfits?: Wardrobe
  /** Wind gliders. */
  gliders?: Wardrobe
  weaponSkins?: Wardrobe
  /** Sprint trails. */
  trails?: Wardrobe
  /** Saved teams by slot; `name` is the player's own text. */
  teams?: Record<IdKey, { name: string; characters: WornBy[] }>
  /** The team slot in use. */
  currentTeam?: GameId
}
interface Wardrobe {
  owned: GameId[]
  worn: Record<WornBy, GameId>
  /** Owned only until then (trial weapon skins). */
  expires?: Record<IdKey, UnixSeconds>
}
/** A character key, the way GOOD's weapon `location` has it: "Traveler" for any Traveler. */
type WornBy = string

// ── gi_exploration ──────────────────────────────────────────────── [shipped]
// Every part is the game's state at login (or right after it); absent when not captured.
// [shipped] Statues, scenes and explore are left out while the player is in another player's world (co-op).

export interface GiExploration {
  /** Statues of The Seven by city (CityConfigData: 1 Mondstadt, 2 Liyue, 3 Inazuma, ...); level 1 = not yet levelled. */
  statues?: Record<IdKey, { level: number; /** Oculi given toward the next level. */ oculi: number }>
  /** Level by offering (OfferingOpenStateConfigData: Frostbearing Tree, Sacred Sakura, ...); [shipped] 0 = never levelled. */
  offerings?: Record<IdKey, number>
  /**
   * By scene (3 = Teyvat; the other maps have their own ids): the map points
   * unlocked (waypoints, Statues, domains... told apart by the scene's point
   * file) and the map areas revealed.
   */
  scenes?: Record<IdKey, { points: GameId[]; areas: GameId[] }>
  /** Exploration by area (WorldAreaConfigData), in the game's raw points; % = points / the area's total. */
  explore?: Record<IdKey, number>
  /** Reputation level by city (Mondstadt to Fontaine); [shipped] 0 = not started. */
  reputation?: Record<IdKey, number>
  /** [shipped] Domains whose first-clear reward was taken (DungeonEntryExcelConfigData ids, ascending). */
  domainsCleared?: GameId[]
  /** [shipped] Natlan's tribal chapters by TribalQuestChapterExcelConfigData id. */
  tribalChapters?: Record<
    IdKey,
    { state: 'notOpen' | 'locked' | 'open' | 'finished'; /** When it started (absent before). */ startedAt?: UnixSeconds }
  >
}

// ── gi_capture ──────────────────────────────────────────────────── [shipped]

export interface GiCapture {
  v: 1
  reason: 'login' | 'world' | 'manual' | 'collapse' | 'logout' | 'resync' | 'change'
  /** This game login's id: 16+ of [A-Za-z0-9_-]. */
  session: string
  /** 1 = the login's first world, then each later full send in the same connection. */
  world: number
  /** The live journal this snapshot ends. */
  journal: { seq: number }
}

// ═════════════════════════════════════════════════════════════════════════════
// 2. Live sync: one batch of changes since the newest snapshot   [shipped]
//    POST <api>/irminsul/changes
// ═════════════════════════════════════════════════════════════════════════════

export interface GoodXDelta {
  format: 'GOOD-X-delta'
  v: 1
  uid: number
  /** = gi_capture.session of the snapshot this journal follows. */
  session: string
  /** 1, 2, 3 … within the journal. */
  seq: number
  /** The `live.base` the tracker answered that snapshot with. */
  base: string
  /** The capture's clock when the batch was built. */
  at: UnixMs
  /** At most 2000 changes, 512 KB. Each is an absolute value, so re-applying is harmless. */
  changes: DeltaChange[]
}

/**
 * Paths, each part a string:
 *   materials.<key>                                   count; del = gone
 *   characters.<key>                                  add a whole GoodCharacter; del
 *   characters.<key>.level|ascension|constellation    number
 *   characters.<key>.talent.auto|skill|burst          number
 *   weapons.<guid> / artifacts.<guid>                 add a whole item; del (hint {rarity})
 *   weapons.<guid>.<field> / artifacts.<guid>.<field> that field's value (gi_exp included)
 *   gi_player.<field>                                 number
 *   gi_characters.<key>.<field>                       number; del of the entry drops it
 *   achievements.<id>                                 finish time (0 = done, time unknown); del
 *   anecdotes.<id>                                    {state, finishCount}; del
 *   gi_anecdotes.unlockedAreas|reward|wish            that value; del
 *   gi_tasks.<part>                                   that part whole; del
 *   gi_wardrobe.teams|currentTeam                     the saved teams whole / the slot in use; del
 * Unknown paths are ignored, never an error.
 */
export type DeltaChange =
  | ['set', string[], unknown]
  | ['add', string[], Record<string, unknown>]
  | ['del', string[]]
  | ['del', string[], Record<string, unknown>]

// ═════════════════════════════════════════════════════════════════════════════
// 3. Wish history sync   [shipped]
//    GET  <api>/wishes/cursor  → WishCursor
//    POST <api>/wishes/import  ← WishImport
// ═════════════════════════════════════════════════════════════════════════════

/** The wish history API's own banner codes ("400" = the second character banner). */
type GachaType = '100' | '200' | '301' | '302' | '400' | '500'

export interface WishImport {
  /** (§4.4: a string here, a number in gi_player / GoodXDelta) */
  uid: string
  region: string
  lang: string
  /** The account server's UTC offset in hours. */
  timezone: number
  /** Records as the wish history API gives them, strings throughout. (§4.2) */
  records: {
    id: string
    gacha_type: GachaType
    /** "YYYY-MM-DD HH:MM:SS" on the server's clock. */
    time: string
    name: string
    item_type: string
    rank_type: '3' | '4' | '5'
    item_id: string
  }[]
}

export interface WishCursor {
  uid: string
  accountId: number
  /** The newest record id stored per banner, and its time ("YYYY-MM-DD HH:MM:SS"). */
  newest: Partial<Record<'301' | '302' | '500' | '200' | '100', string | null>>
  newestTime: Partial<Record<'301' | '302' | '500' | '200' | '100', string | null>>
}

// ═════════════════════════════════════════════════════════════════════════════
// 4. Known quirks: kept as they are, since installed apps and stored data rely on them
// ═════════════════════════════════════════════════════════════════════════════
//
// 4.1 Achievements are two keys: `gi_achievements` (ids) + `gi_achievement_times`
//     (id → time). One `gi_achievements_v2: Record<IdKey, UnixSeconds | 0>` would say both.
// 4.2 Wish sync keeps the wish API's snake_case strings (`gacha_type`, "5") instead of
//     our camelCase numbers. Deliberate: records pass through byte for byte, UIGF-style.
// 4.3 Anecdote `state` is the game's number (0/2/3); new parts use strings ('open'…).
// 4.4 The UID is a number in gi_player and GoodXDelta, a string in the wish payloads.
// 4.5 Resin is in two places: gi_player.resin (at login) and gi_tasks.resin (with its clock).
// 4.6 gi_player.maxStamina is in the game's units (24000), not what it shows (240).
// 4.7 Times: GOOD's `timestamp` and GoodXDelta.at are milliseconds; everything else is
//     seconds; wish times are server-clock strings.
// 4.8 `location` is "" for nobody (GOOD's rule) where our own parts leave a field out.
// 4.10 EXP (gi_exp, exp, friendshipExp) is written as 0 when the game sends 0;
//      most other parts leave a 0 out.
// 4.9 gi_tasks.expeditions is a list (sorted by doneAt) though each has a unique id;
//     by the Record rule it would be Record<IdKey, {...}>.
