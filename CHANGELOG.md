# Changelog

Each release's section is its GitHub release notes (`.github/workflows/release.yaml`).

## 1.10.0

- Desktop: updates like on Android. **Settings → About** shows your version
  and has **Check for updates**; **Update now** downloads and installs the new
  version while capture keeps running, then **Restart now** switches to it
  (or it starts the next time you open Teyvat.me). No more full-screen update
  prompt at start: the app checks in the background at start and every few
  hours, and a banner on the home screen says when there's one.
- Android: the Quick Settings tile lights up while capture runs (and says
  "Starting capture" while it starts); it used to stay grey until the panel
  was closed and opened again.

## 1.9.0

- **Music game**: every song's best score and whether its reward is taken.
- **Miliastra Wonderland**: the cosmetics you own and when you got them.
- **Login events**: days signed in and rewards claimed on the event
  calendars running.

New optional export parts: `gi_collections.musicGame`, `gi_tasks.signIns`,
and a new `gi_miliastra` key.

- The "Open before the game" hint is gone from the home screen (desktop and
  Android).

## 1.8.0

- **Crafting, forging and processing**: which recipes you've unlocked at the
  crafting bench, the blacksmith and for processing.
- **Genius Invokation TCG**: the challenge levels you've cleared, boss
  challenges unlocked, and the Tavern.
- **Serenitea Pot**: comfort in each realm and the mansion.

New optional export parts: `gi_collections.crafting`, `gi_collections.tcg`'s
`levels` / `bosses` / `tavern`, and a new `gi_teapot` key.

## 1.7.0

More of your account, all from what the game sends at login:

- **Hangout Events**: each chapter's endings (seen, reward waiting, claimed)
  and pictures unlocked.
- **Weekly bosses**: discounts left this week, and when every boss and elite
  respawns.
- **Achievement progress** on the achievements you haven't finished.
- **Monsters seen**, **furnishing blueprints** learned, and the **Adventure
  Rank rewards** you've claimed.
- **Events**: the events running, when each closes, and your progress
  counters in them.

New optional export parts: `gi_collections.hangouts`, `.monstersSeen`,
`.blueprints`; `gi_tasks.weeklyBosses`, `.respawns`, `.arRewards`; and new
`gi_achievement_progress` and `gi_events` keys.

## 1.6.0

- Captures your **exploration**: Statue of The Seven levels (and Oculi toward
  the next), offering levels (Frostbearing Tree, Sacred Sakura, ...), the map
  points you've unlocked (waypoints, domains, ...) and areas revealed on each
  map, exploration per area, and reputation levels. Exported as the new
  `gi_exploration` key.

## 1.5.0

- Captures your **recipes**: every recipe you've unlocked and its cooking
  proficiency, from the list the game sends at login. Exported as
  `gi_collections.recipes`.

## 1.4.0

Captures what you've collected and what your characters wear, all from the
data the game sends at login:

- **Archive**: every tab's unlocked entries (books, viewpoints, Living
  Beings, weapons, artifacts, materials, tutorials, quests).
- **Enemies defeated** and creatures captured, per Living Being.
- **Adventurer Handbook**: each chapter and target, and whether its reward is
  taken.
- **Genius Invokation TCG**: the cards you own (count and proficiency) and
  your saved decks.
- **Aranara** collections.
- **Wardrobe**: outfits, wind gliders, weapon skins (trials with their end
  date) and sprint trails you own, who wears which, and your saved teams.

Exported as new parts of `gi_collections` and a new `gi_wardrobe` key; a
tracker can show what you have and what's left.

## 1.3.0

- Captures your **Radiant Spincrystals**: which of the 206 are in your
  Serenitea Pot's music list, read from the list the game sends at every
  login. Exported as the new `gi_collections.spincrystals` (item ids); a
  tracker can show which ones you're missing.
- Exported files now name their source as `"Teyvat.me"` (it said
  `"Irminsul"`, the app's old name).

## 1.2.3

- Android: the Quick Settings tile only turns capture on and off. It no
  longer opens Genshin (or the app); open the game yourself once capture is
  on. Only the very first start still opens the app, because Android asks to
  allow the VPN there once.

## 1.2.2

- Android: the Quick Settings tile no longer opens Teyvat.me. A tap starts
  capture in the background, like a VPN app's tile, and opens Genshin;
  another tap stops capture. Only the very first start still opens the app,
  because Android asks to allow the VPN there once.

Everything in 1.2.1 and 1.2.0 below still applies.

## 1.2.1

- Android: a **Quick Settings tile**. Add "Teyvat.me" to your quick settings;
  a tap starts capture and opens Genshin once it is ready, another tap stops
  capture. (If Genshin was already running, close it first: it has to log in
  after capture starts.)
- Android: self-updating behaves.
  - **Install now** opens Android's update screen right away.
  - A downloaded update is kept when the app restarts, instead of being
    downloaded again.
  - Leaving Android's update screen with Home no longer leaves the card on
    "Installing"; it is ready again for Install now.
  - While your phone still has a copy you installed yourself, every update
    needs your tap, so it no longer pretends to install when you leave the
    app: a notification says the update is ready instead. After one update
    from inside the app, later ones install by themselves.

Google Play Protect may still ask to scan the app the first time it sees a
new version: it does that for any app installed outside the Play Store.

Everything in 1.2.0 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.2.0

New for the tracker (teyvat.me shows them; a GOOD file for Genshin Optimizer
is unchanged):

- EXP toward the next level for every character, weapon and artifact, and
  friendship EXP. The tracker shows how far into a level each one is, and its
  planner counts EXP you have already fed.
- The weapon banner's Epitomized Path: the charted weapon and its Fate Points,
  read when you open the Wish screen.
- The resin clock: when your next Original Resin comes, so the tracker's resin
  count follows the game's own timer.

Also:

- The export format is written down: [docs/export-format.md](https://github.com/tawan475/teyvat.me-capture/blob/main/docs/export-format.md),
  with every field typed in `docs/teyvatme-export.ts`. Fields are only ever
  added, so tools reading today's files keep working.
- Releases build faster.

Everything in 1.1.6 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.6

- Windows: minimizing to the tray with the title bar's minimize button no
  longer keeps one CPU core busy while the window is hidden. The button now
  hides the window straight to the tray. Minimizing another way (Win+M)
  leaves it on the taskbar.

Everything in 1.1.5 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.5

- Android: signing in, the import key and uploads work on networks whose
  DNS answers 0.0.0.0 for teyvat.me (some filters do that for new
  domains). The app then asks Cloudflare's or Google's DNS over HTTPS,
  as the browser already does. Seen as "Failed to connect to
  teyvat.me/0.0.0.0:443".

Everything in 1.1.4 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.4

- Desktop: switching between light and dark changes the whole window at
  once. The capture card faded on its own while the rest had already
  changed.
- Desktop: the Activity list uses its full width; lines were cut off
  halfway with the scrollbar in the middle.
- A Discord button: the desktop's bottom bar and Settings › About, and
  Android's Settings and About (https://discord.gg/W6GUY3UtAx).

Everything in 1.1.3 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.3

- Android: wish history syncs to teyvat.me by itself, like the desktop.
  Open Wish → History in game once per login; every banner's new wishes
  go up, and a wish you make syncs its banner a little later. Switch:
  Settings → Tracker → "Send wishes to teyvat.me".
- Android: live sync. Between snapshots the phone sends what changed as
  you play, so the tracker stays current without logging out (the
  tracker's per-account switch decides too). Switch: Settings → Tracker →
  "Live sync".
- Android: tracker uploads hold the whole account. They went through the
  export filters, so 1★ and 2★ artifacts and weapons were left out.
- Android: a failed automatic upload is tried again after 30 s, then less
  often (up to every 10 min), instead of every second.
- Android: a sign-in the tracker ended (signed out on the website) shows
  "Signed out, sign in again".
- Android, like the desktop:
  - the tracker card names the upload account ("name (UID …)"), says when
    the chosen account is gone, and warns when the capture is another UID
    than the account uploads go to;
  - the sign-in is checked again every 5 minutes while the app is open;
  - Settings → Tracker → Advanced: the tracker address, and an import key
    that uploads (snapshots, live sync, wishes) while signed out;
  - a language picker (Settings → App), every language the game ships;
  - "Export incomplete" in the activity list when an export leaves
    something out, and the activity lines in your language;
  - a "Crashed last time" card with Share log, and a Release notes link.

Everything in 1.1.2 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.2

- Desktop: uploads, live sync and wish history now go to teyvat.me while
  the window is hidden in the tray, also when Teyvat.me starts there with
  Windows. Before, they waited until you opened the window: the upload
  check read a copy of your sign-in that only a visible window refreshes.
- Android: a "Run in the background" step (Home, set-up and Settings).
  Android's battery optimisation, and the extra savers on Xiaomi, Samsung,
  Huawei and other phones, could pause exports and uploads while the game
  was in front. Allowing it keeps them going; Settings links to
  dontkillmyapp.com for phones that need more.

Everything in 1.1.1 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.1

- Desktop: Settings › Tracker shows the teyvat.me settings directly
  (sign-in, uploads, live sync, address and key), with no page to click
  through.
- Android: the first release that 1.1.0 installs by itself.

Everything in 1.1.0 below still applies.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.1.0

**Every language the game ships.** Teyvat.me speaks English, 简体中文,
繁體中文, 日本語, 한국어, Español, Français, Русский, ภาษาไทย, Tiếng Việt,
Deutsch, Bahasa Indonesia, Português, Italiano and Türkçe, the same words
as the teyvat.me site and the game's own names for its terms.

- It follows your system's language. On the desktop, Settings › App ›
  Language picks another one. On Android 13+, so does the phone's own
  per-app language setting.
- Chinese, Japanese, Korean and Thai use your system's font for that script.

**Android: the tracker works.** The "Destinations" card is now "Tracker".

- Sign in with teyvat.me in your browser.
- Each capture uploads by itself. "Upload now" sends one whenever you want.
- Choose where uploads go on the website, open your dashboard, or sign out.

**Android: updates itself**, like the desktop.

- New releases download by themselves and are checked against
  `SHA256SUMS.txt`.
- They install through Android's own installer. Android asks once to
  allow "Install unknown apps", then shows a one-tap Update screen, or no
  screen at all on Android 12+ once the app has updated itself.
- An update never interrupts a capture: it waits until capture stops, or
  you tap "Install now".
- Settings › Updates turns this off, or checks right away.

**Desktop: fixes**

- A left click on the tray icon brings the window back; the menu is on the
  right click. Restore and Quit in that menu work again.

**Android: fixes**

- The top bar no longer overlaps the page in a small or pop-up window.

**Both:** the tracker's Devices list names this app "Teyvat.me on <device>"
instead of "Irminsul".

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.0.1

Fixes the 1.0.0 desktop downloads, which drew most text with letters missing:
the app's fonts reached the build damaged (git had stored them as text). The
fonts are binary files in the repository now, and a test checks them byte for
byte. Android builds get the intact fonts too.

Everything in 1.0.0 below still applies. 1.0.0 offers this update by itself.

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Unofficial fan project, not
affiliated with HoYoverse.

## 1.0.0

The first release of Teyvat.me Capture, irminsul's successor under its own
name. It reads your Genshin Impact account from the game's own traffic and
saves it as GOOD JSON, with no account or sign-in needed. The
[teyvat.me](https://teyvat.me) tracker is one optional destination.

**Downloads**

| Platform | File |
|---|---|
| Windows 10/11 | `teyvatme-windows.exe` (built-in packet capture; asks for administrator rights) |
| Windows with Npcap | `teyvatme-windows-pcap.exe` |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` |
| Android 10+ | `teyvatme-android.apk` |

Check a download against `SHA256SUMS.txt`. Start Teyvat.me **before**
Genshin enters the world: it reads the login to find the session key.

**Desktop**

- A new design: the system's own window frame, title bar and file dialogs,
  light and dark themes (or follow the system), and a status card that walks
  you through Capture → Open Genshin → Enter world.
- Save GOOD JSON, copy it, or auto-export every capture to a folder, with
  per-category filters. Achievements go to paimon.moe, StarDB or Seelie.
- Wish history: copy the wish URL for paimon.moe or StarDB, or save every
  banner as a UIGF file, once you open Wish → History in game.
- Optional teyvat.me destination: sign in with your browser (or an import
  key), upload each capture and sync changes live as you play.
- Toasts for what happens, and the last 50 events under History.
- Starts with Windows if you like, minimized to the tray or the taskbar.
- Updates itself from GitHub releases.

**Android**

- A local VPN that sees only Genshin's traffic and decodes it on the phone,
  with nothing sent anywhere but where you choose.
- Save, share or copy the GOOD export, and save your wish history as UIGF.

**Coming from irminsul**

On its first start Teyvat.me takes over irminsul's settings, tracker sign-in
and start-with-Windows task. A saved `genshin-tracker.475.dev` address moves
to `teyvat.me`. irminsul itself will point its updater here in a later
release.

Unofficial fan project, not affiliated with HoYoverse.
