# Changelog

Each release's section is its GitHub release notes (`.github/workflows/release.yaml`).

## 1.17.23

- Restarting Teyvat.me no longer needs a new login. Updating mid-session
  (and closing or quitting the app, then starting it again within two
  minutes) used to lose the game session: the key that decrypts it and the
  account data the later changes apply to lived only in the process, so
  nothing decoded until the game was logged in again. A clean exit now
  hands the running session to the next start: the key and conversation
  (with the packets held in flight and the decompressor's state), the
  account as captured and live sync's journal, in `session-handover.json`
  in the data folder, deleted once read. The next start carries on as if
  nothing happened; the first messages after the gap are checked against
  the key before anything is believed, and a file that does not match (a
  new login meanwhile, too old) is ignored. Verified on a recording cut at
  four points, inside the login burst too: the export equals the uncut
  one. A crash writes nothing.
- The KCP crate is vendored (`crates/mhy-kcp`, MIT) for the one setter the
  resume needs.

## 1.17.22

- The login's snapshot goes up sooner. The export used to wait 5 s of
  quiet after the last packet and at least 12 s for the exploration
  points, 20 s at most, so a login's snapshot started 12-20 s after the
  bag arrived while the login's own data was in after 7-10 s. Now Capture
  ticks off the parts a login sends (bag, characters, achievements, the
  world, commissions, the Travel Accounts book, the exploration points)
  and, once every one is held, exports 3 s after the last packet: about
  9-13 s after the bag. A login missing a part (an account without the
  book, a message the matchers did not recognise) keeps the old waits.
  Same on Android, for the upload and the auto-export file.
- Every profile picture is read. Capture told the picture from its frame
  by the size of the number, and many pictures' numbers looked like
  something else: the Traveler in an outfit (11, 21), characters below
  1000 (Amber, Kaeya, Lisa, Barbara, Xiangling, Xingqiu, Ningguang…) and
  the quest and event pictures (100010-100300, 200010-200011, the same
  size as frames and outfits). Those were dropped, and the tracker's
  profile card and share cards showed the plain Traveler instead. The
  picture is now read by its place in the message, which the game has
  kept the same since 6.3; a Stygian co-op record's players likewise,
  when a quest picture sits beside a frame.

## 1.17.21

- Stygian Onslaught records reach the tracker as soon as the record screen
  is opened. They arrive after the login's snapshot and the live sync's
  batches cannot carry them, so they went up only with the next login: a
  co-op record opened in a session was never seen until the day after.
  Now the records, the Spiral Abyss record book and the Imaginarium
  Theater's state when it moves after a run go up at once as one small
  request (`POST /irminsul/attach`, a few KB), which the tracker writes
  into this login's snapshot; nothing is re-uploaded, and opening the
  screen again with nothing new sends nothing. Live sync on or off alike.
- The Spiral Abyss's record book is captured when its Challenge Summary is
  opened (`gi_endgame.abyss`): per period the deepest descent, the stars,
  Most Defeats, Strongest Single Strike, Most Damage Taken, Elemental Skills
  and Bursts Unleashed with their characters, and the characters picked most
  with how often. The figures' labels are read off the 7.2 layout; a later
  version's reshuffle leaves them out (floor, stars and picks stay) until it
  is learned again.

## 1.17.20

- Stygian Onslaught co-op records name every player and their characters,
  as the game's record screen does: each player's position (1P, 2P…),
  profile picture and nickname (`gi_endgame.stygian.records[].players`),
  and per fight every player's characters with the level and constellation
  they had when the record was set (`fights[].players`; solo records get
  their one entry too). Other players' UIDs are still never exported.
- A Ko-fi button beside Discord, in the app bar and in Settings › About,
  for anyone who wants to support the project's running costs: the same
  link the tracker's top bar has. On Android too.

## 1.17.19

- The Imaginarium Theater's Star Challenge Stellas are captured: how many
  this season, of how many the difficulty offers, and which acts earned one
  (`gi_endgame.theater` `stars`, `maxStars`, `starredActs`). They are told
  from the season message's lists, so a run short of more than four stars
  reads as full and a run of fewer than four acts has no stars yet; the
  tracker shows them on the Endgame page.

## 1.17.18

- Pieces made by the Mystic Offering strongbox stay in the bag. The game's
  answer to the strongbox lists the pieces it just made, and Teyvat.me took
  that list for a delete: the 18 pieces offered left the bag, the new ones
  were removed again right after they arrived, until the next login.
- A weapon or artifact levelled or ascended while equipped keeps its wearer,
  and so does a piece you lock or unlock. The game's answers
  to those actions name the item by itself beside a level or a lock flag,
  which also passed for a delete: the item was removed with its wearer, came
  back with the next change without one, and exported as unequipped until
  the next login. Only the game's real delete message removes items now: a
  packed guid list beside the store it is deleted from, nothing else.
- Artifacts made with Sanctifying Elixir export `elixirCrafted`, GOOD's own
  spelling, so Genshin Optimizer keeps the flag. Teyvat.me wrote
  `elixerCrafted`, which Genshin Optimizer ignores; the tracker reads both.
- Nothing is uploaded or exported from a login caught halfway. A new login
  sends the bag a message before the character list; in between (and for
  good when that connection drops there) Teyvat.me held a bag with no
  characters. The automated upload already waited for both, but a live sync
  snapshot still wanted from before the relogin (a collapse, a resync),
  Android's Upload now and a toast's Retry could send that state, and the
  tracker would have recorded an account with no characters. Every upload
  and export now waits for the owned roster and the bag
  (`PlayerData::holds_account`). `--replay-export` of a recording that ends
  inside a login exports the account before it, saying so.

## 1.17.17

- Teyvat.me no longer asks the tracker who it is signed in as every five
  minutes. While the game runs, each presence heartbeat's answer already
  says who is signed in and where uploads go, so the signed-in panel follows
  it from there; with the game closed nothing uploads, and bringing the
  window back still reads it again. Copies left running in the tray all day
  made most of these requests. Android does the same.

## 1.17.16

- Quests finished during a session are captured as they happen: a parent quest
  the game reports finished (or newly accepted) with its sub-quests joins
  `gi_collections.quests` at once instead of at the next login, so the
  tracker's World Quests list is right after the session's own snapshot.
- A wish whose answer carried a counter equal to a banner number (a wish count
  of 100 or 200) is no longer logged without its banner: the field the banner
  sat on in an earlier wish settles it, and only that banner is re-synced.

## 1.17.15

- Closing Teyvat.me installs an update that is waiting first. A new version
  the hourly check has found and not installed yet (**Install updates by
  itself** off, or the check just ran) is downloaded and installed when the
  window is closed, from its close button or the tray's Quit, and the app
  then closes; one being installed finishes first. The bar says "Installing
  X · Teyvat.me closes when it is done" meanwhile, and the next start runs
  the new version. Nothing restarts by itself: closing means closing. A
  download that fails or takes over 30 s is logged and the app closes
  anyway, as does closing it a second time. Players kept running old
  versions because the window was never looked at; the moments the app is
  not in use are when updates go in now, this one among them.

## 1.17.14

- A lost graphics device no longer ends Teyvat.me. Direct3D reports the
  device lost after a driver reset or update, a sleep or a display change,
  and the window's renderer answers the next frame with a panic it cannot
  recover from inside the process (1.17.13 died with "Error in
  Surface::present"). Teyvat.me now starts itself again in its place, with
  the same command line and the window back where it was (minimized or in
  the tray); the crash report is still written and the next run says so. A
  device that fails at every start gets one restart, not a loop.
- Paimon's Bargains: the Fates bought this month are counted again. A Fate
  bought for Starglitter, which has no monthly limit, hid the shop's count
  field, and the Stardust Fates then read as unbought ("5 / 5 left" on
  teyvat.me for a player who had bought them out).

## 1.17.13

- Updates install by themselves. A new version found by the check is
  downloaded and installed while capture keeps running (**Settings → About →
  Install updates by itself**, on), and Teyvat.me restarts into it on its own
  once Genshin is closed and the window is minimized or in the tray
  (**Restart by itself**, on) — a window in sight keeps its **Restart now**.
  Until now a release waited for a click on a banner on the home page that
  most people never gave: logs sent in showed apps finding every new version
  at start and running the old one for days.
- The update bar is over every screen, not the home page alone; the tray
  icon carries an amber mark while an update waits, its tooltip says so, and
  one Windows notification per release says the version is installed (or is
  out, when installing by itself is off).
- The check runs hourly (it was six hours), on coming back to the window
  after an hour, and ten minutes after one that failed. It reads the newest
  release's version from the releases page's redirect, with no API quota, so
  a building full of players behind one address is never refused; GitHub's
  API is asked only once a newer version is known.

## 1.17.12

- Opening Teyvat.me while it is already running brings its window to the
  front, out of the tray or the taskbar, instead of doing nothing.
- A ten-digit UID (18…, 16…) showed its server as China; it is read from the
  digits before the account number now, so 18… is Asia again.
- Live sync no longer rebuilds the export several times a second while a lone
  change waits its two minutes; it looks again every 15 s, or when new data
  arrives.
- The log no longer gets a line per server ACK the KCP reassembler skips
  (141,000 lines in one evening); the stats line counts them (`kcp_dropped`),
  shows frames per direction (`sent`/`received`), and Windows' packet monitor
  lists the adapters and components it found at start.
- Wi-Fi: the packet monitor's own copies of a Wi-Fi adapter's packets are
  802.11 frames, which were thrown away; they are read now, so an incoming
  packet no longer depends on the one component that reports it as Ethernet.
- When a login goes by with none of the game server's packets seen (one PC's
  packet monitor delivered only the client's sends), capture switches to
  compatible mode by itself and says so; the next login is read.
- The log starts with the Windows edition and build, and the raw packet log
  names which packet monitor component delivered each frame.
- New in the export: `gi_endgame`. The Imaginarium Theater season you are
  in, the difficulty you picked and the acts you cleared (from the login),
  and your Stygian Onslaught records (difficulty, time per boss, your own
  team, and each fight's strongest single strike and highest total damage;
  read when you open its menu). A co-op record's other players are never
  exported. The seasons come with the game data; a season newer than
  the build is skipped until the next release.
- Genshin 7.2: the Miliastra Wonderland cosmetics are read again. The game
  now lists the emotes and poses beside them as entries instead of plain
  numbers, which made the whole message unrecognisable to 1.17.11. Every
  other part of a 7.2 login reads as before.
- Decoding a login takes about 40% less CPU: the message matchers read each
  payload in place instead of copying it into a field map two dozen times,
  and the account's UID is no longer recounted over the whole inventory for
  every message the matchers leave unclaimed.
- Compatible capture now notices a network interface or address that
  appears after it started (Wi-Fi joining, a VPN, a renumbering) and
  listens on it too; a reader that fails is reopened. It used to list the
  interfaces once, so a game started on a new one was never heard.
- A capture backend restart (the silence watchdog's, a mode switch's) no
  longer stops the automated snapshots for the rest of the login: they
  waited for a character list that only comes with the next login.
- A replaced capture task's late failure report no longer drops the running
  task's stop handle, which left both running and decoded every packet
  twice.
- A KCP segment with an unknown command no longer costs the segments after
  it in the same datagram; the packet log closes with one warning when it
  can't be written instead of logging every frame; the compatible-mode
  switch is not announced under `-b pcap`, where it changes nothing;
  recordings declare no snapshot length cap, as a merged datagram can be
  longer than the one they declared.

## 1.17.11

- Compatible capture now reads the game's packets through a raw socket
  (what RawCap uses) instead of Windows' packet monitor. On a PC that merges
  the game's incoming packets, the packet monitor's older path still lost
  3-6% of them in ways it never reported, so logins came through with gaps
  (the key read, characters or inventory missing). Windows hands the raw
  socket every packet once and whole. If no raw socket can be opened, the
  older path is used as before.

## 1.17.10

- Compatible capture keeps more of the game's data: on some PCs Windows
  reports part of the game's packets in a form compatible capture used to
  throw away, so logins still came through with gaps (the key read, but
  characters, inventory or achievements missing). They are kept now.
- The log's capture stats line says how many packets Windows' capture lost,
  if any, so the next report shows it.

## 1.17.9

- Fixed: on some PCs the app couldn't read the login ("Session key not
  found"), or read only part of the account. Windows 11 can merge incoming
  game packets into bigger ones, and the packet capture Windows gives apps
  drops any packet over 9,000 bytes, so up to a third of the game's data
  never arrived. When the app sees merged packets it now switches to
  Windows' older capture path, which keeps them, and remembers that for
  this PC. The first time, return to the title screen and enter again.
  Settings → Diagnostics has the switch ("Compatible capture").

## 1.17.8

- Lighter on the tracker: with live sync on, entering or leaving a domain,
  the Serenitea Pot or any other world no longer uploads your whole account
  (about 1 MB each time); what changed goes with the next small update
  instead. Logins still upload the whole account, as before.

## 1.17.7

- Fixed: on Linux and macOS, Teyvat.me closed at once instead of showing
  the "Packet capture needs permission" window ("semibold" is not bound
  to any fonts). Thanks to @MRDGH2821 for the report and the fix.
- "Log raw packets" (Settings → Diagnostics) now also records the game's
  traffic as it arrives, before decryption, so a login the app couldn't
  read ("Session key not found") can still be looked at. The folder and the
  session's files appear as soon as you turn it on, and a "Raw packet
  folder" row opens it.

## 1.17.6

- Fixed: picking characters for the Imaginarium Theater (or any mode that
  lends you copies of your own characters) took those characters' weapons
  and artifacts out of the capture until the next login, so teyvat.me
  showed them with no gear. Log in again after updating and the next
  capture puts it all back.
- The "From your plan" card on the home screen is gone, on desktop and
  Android: teyvat.me no longer publishes the plan.

## 1.17.5

- Android: an update never installs on its own any more. New releases
  still download by themselves (with "Update automatically" on), and the
  home screen says one is ready; it installs only when you open the app and
  tap Install now. Installing in the background made Play Protect's app scan
  pop up out of nowhere.

## 1.17.4

- Teyvat.me knows you're playing: while the game runs, the app tells
  teyvat.me (signed in, or with an import key), which shows "Playing now on
  <this PC>" with two buttons: Sync wishes and Send a capture. The app acts
  on them within a minute. Phone alerts from teyvat.me hold off while you
  play.
- From your plan: the home screen shows today's farm from your teyvat.me
  Planner (domains, bosses, what's missing and for whom), refreshed every
  10 minutes. Open the Planner on teyvat.me once to publish it.
- The resin notification can follow teyvat.me: set an amount (and resin
  full) for this device in teyvat.me's Settings › Devices, and the app uses
  it in place of its own setting.
- Fixed: on some accounts Paimon's Bargains was never read. Their shop
  carries one more small number beside the bought counts (on a goods that
  can be bought once), and Teyvat.me gave up rather than guess which one
  counts. Now the count on the Fates decides.

## 1.17.3

- Fixed: started while Genshin was already past its first login (in the
  world, or back at the title screen), Teyvat.me could not read any later
  login, so going back to the title screen and entering again never worked
  and only restarting the game did. The game draws every login's key from
  one generator seeded at its first login of the session; Teyvat.me now
  searches for that first login from the moment the game started (a second
  or so, at most a few seconds when the game has been open for hours), and
  every later login of that game is read at once. The Android app looks back
  from the login instead (it cannot see when the game started).
- Settings › App › Clock: 12-hour ("2:45 PM") or 24-hour ("14:45") times
  everywhere in the app. It follows Windows' own time format until you pick
  one. On Android times follow the phone's 24-hour setting.
- Finding a session key is faster: each candidate key is checked with a
  quarter of the work it took before.

## 1.17.2

- New `gi_tasks.bargains`: Paimon's Bargains as the shop showed it. Opening
  the shop in-game sends how many Intertwined and Acquaint Fates were bought
  this month for Masterless Stardust and Starglitter, and whether anything
  there was bought at all; a purchase there updates it. The tracker's
  Planner ticks Paimon's Bargains by itself once something was bought this
  month, and its Wishes page shows the Fates left. Live sync sends it as
  soon as the shop is opened. Needs the tracker of 2026-10-31 or later (an
  older one ignores it).
- The export format has a name: Leyline, the Teyvat record format: GOOD,
  plus everything else your account holds. Buttons and messages say
  Leyline JSON, and saved files are named `leyline-<date>.json`. A Leyline
  file is still a GOOD file (`"format": "GOOD"`), so Genshin Optimizer and
  any GOOD reader load it as before. Its keys are described for developers
  at https://teyvat.me/developers, and typed in `docs/leyline.d.ts` (was
  `docs/teyvatme-export.ts`; also served at
  https://teyvat.me/developers/leyline.d.ts).
- Every export and upload now says which Leyline it is: a new top-level
  `"leyline": 1`. GOOD readers skip it; the tracker reads files with or
  without it.

## 1.17.1

- Live sync: saved teams go to the tracker as they change. Choosing another
  team or changing who is in one shows on the tracker's Characters page
  without waiting for the next login. The team in use is read from the
  party the game sets up on the field, so co-op and a domain's trial
  characters change nothing. Needs the tracker of 2026-10-30 or later (an
  older one ignores them).

## 1.17.0

- New `gi_collections.quests`: every quest the game lists at login, finished
  or accepted (`{finished, active}`, quest ids). The tracker's Archive uses
  it for its World Quests list. Read at login only: the next login brings a
  quest finished in between.

## 1.16.0

- Live sync: the planner's routine goes to the tracker as it changes: the
  commissions, the expeditions, the realm currency, the Welkin Moon, the
  weekly boss discounts, the bosses' respawns and the rest of `gi_tasks`.
  Only the resin clock did; the rest waited for the next login. Needs the
  tracker of 2026-10-30 or later (an older one ignores them).
- Commissions are read as they are finished, not only from the login's
  list, with whether Katheryne's reward was taken (`gi_tasks.commissions.claimed`).
  The message is learned in game: once it has agreed with the commission
  list three times, which happens within a few scene loads.
- New `gi_tasks.resinSpent`: the Original Resin spent today, as the game
  counts it (shown on the tracker's planner).

## 1.15.5

- Live sync: friendship goes to the tracker as it changes. A domain, a
  commission or the Serenitea Pot raised a character's friendship in the
  game, but the tracker showed the new level only after the next login.
- Live sync: achievements go to the tracker as they are finished (and their
  reward taken), with the time they were finished, instead of at the next
  login. Updates the game bundles with other messages are read too.

## 1.15.4

- Windows: in the tray, the window no longer shows as a small title bar
  above the left end of the taskbar. A minimized window without a taskbar
  button is drawn there; it now waits off the screen.
- The window no longer opens tiny in the bottom-left corner after a
  session that ended minimized or in the tray (Quit from the tray menu,
  or a "Start minimized" run). The window's place and size were saved
  as the minimized window's; now the last place and size it had in sight
  are kept, and a tiny one saved by an earlier version is dropped, so
  that window opens at its default size once.
- Android: the resin alert follows its setting at once. Changing the level
  (or turning it on) only took effect at the next capture, so with the game
  closed a new level never rang and an alarm set at the old level still
  did. The Test notification now says when resin is full ("Full at …")
  instead of "Resin is full".

## 1.15.3

- Android: Stop capture always ends the VPN. Sometimes Android's VPN stayed
  connected after Stop (the key icon and "VPN activated" stayed) until it
  was disconnected by hand; the forwarder now closes the tunnel under any
  task that still holds it. A failed start also closes the tunnel it was
  given instead of leaving it open.
- Android: the "Capture stopped" alert comes only when a running capture
  ended, and it says "VPN disconnected" (it said "Another VPN took over"
  also when the VPN was disconnected in Android's settings).
- Android, self-update: Install now no longer loops between "Installing"
  and "Tap to install". Each Install now abandoned the previous install
  session, and Android's answer for that one (cancelled) undid the new
  one; answers for old sessions are now ignored, and while Android's
  Update screen waits, Install now opens that screen again instead of
  starting over. A background check no longer replaces an install under
  way.
- Android 12+, self-update: updates now install without asking, as meant
  (when you leave the app, capture off). The app lacked the permission
  Android needs for that, so every update asked with "Tap to install".
- Android, self-update: an update Play Protect (or another Android check)
  refuses says "Update failed · Android blocked it (Play Protect)" instead
  of a raw status, or of going back to Install now as if nothing happened.
- Android: the update notification and the resin alert no longer replace
  each other (they shared one notification number).

## 1.15.2

- Android, Export: each filter's minimums show right under it while its
  switch is on (they hid behind a tap on the row's name), and it reads "Not
  exported" when off. The numbers can be typed (tap one, type 80, Done),
  and − / + repeat while held.
- Android, Resin alert: the threshold can be typed or stepped by one beside
  the slider.
- The "Tracker uploads always include everything" note under Filters is
  gone. Desktop: an artifact's minimum rarity now goes 1–5 like the game's
  (it allowed 0–6).
- Language from the system: a Chinese script setting now wins over the
  region (Simplified Chinese set up for Hong Kong shows Simplified, not
  Traditional), and Cantonese shows Traditional Chinese instead of English.
- Android 10–12: a language picked in Settings now also reaches the capture
  notification, the resin alert and the Quick Settings tile (they stayed in
  the system's language; Android 13 and later were not affected).

## 1.15.1

- Desktop: the tray icon's tooltip also says how long ago the last capture
  was ("Last capture 12 min ago"), and keeps it current while Teyvat.me is
  in the tray. (Since 1.15.0 the tooltip follows the status in the tray too:
  before, a window hidden in the tray drew no frames, so it stayed on "Data
  decrypting" until the window was opened.)

## 1.15.0

- **Resin alert** (Settings, off by default): one notification when your
  Original Resin reaches the number you pick (160 to start with), with the
  time it will be full. It comes from the resin clock of your last capture,
  also with the game closed: a Windows notification on desktop (while
  Teyvat.me runs, in the tray too), an Android notification on the phone.
  A Test button shows one.
- Windows: in the tray, Teyvat.me no longer keeps one CPU core at 100 %.
  A hidden window never gets the redraws the window library waits for, so
  from the first update after hiding it polled without pause (about 3.5 %
  of a many-core CPU in Task Manager). In the tray it is now minimized and
  left off the taskbar and Alt+Tab, at about 1 % of one core.

## 1.14.1

- **Capture done**: once a login's data has all arrived and gone quiet, the
  status reads "Capture done" instead of staying on "Data decrypting" for the
  whole session. It waits like the login snapshot: 5 s of quiet, the world's
  exploration up to 12 s, about 20 s at most. A new login starts over.
- "Session key found" now reads **"Login detected"** (status line, toast,
  Android's activity list).

## 1.14.0

- **Your profile**: the in-game nickname, signature, name card, profile
  picture and birthday, from the profile the game loads at login (the
  profiles of players you look up are left out). In the log they show as
  lengths only.
- **Offerings** (Sacred Sakura, Tree of Dreams, …) are read again for
  accounts with an offering never levelled. The game leaves a level of 0
  out, and that one offering made the whole list unreadable. Reputation
  cities not started read as level 0 the same way.
- **Co-op**: in another player's world, its Statues of The Seven, map and
  exploration progress are no longer taken for yours.
- Desktop: the login snapshot waits up to 12 s for the world's exploration
  points, which the game sends 4–8 s after the bag. Most logins used to
  send it without them until the next world change. New data during the
  wait restarts its 5 s, for 20 s at most.
- **Weekly bosses**: the discounts left were read backwards. The game sends
  the discounts used, so a week with all three used showed 3 of 3 left, and
  a fresh week 0 of 3.
- Desktop: the language picker writes every language's name in its own
  script (Chinese, Japanese, Korean, Thai and Russian names were empty
  boxes outside their own language). Those fonts load only while the list
  is open.

New optional export fields: `gi_player.nickname`, `signature`, `nameCard`,
`profilePicture` and `birthday`; `gi_tasks.weeklyBosses.used` (with it,
`left` is the count left; exports without it held the count used there).

## 1.13.2

- **Genius Invokation TCG**: the card collection and decks are read again
  for accounts that hold a card 0 times (the game leaves its count out, and
  that one card used to make the whole collection unreadable).

## 1.13.1

- Windows: the window draws with Direct3D 12 instead of OpenGL. NVIDIA's
  OpenGL driver could close Teyvat.me with "Unable to recover from a kernel
  exception" while the game ran; Direct3D reports such trouble to the app
  instead. If Direct3D can't start, the window uses OpenGL as before.
- Android: the top bar's Discord button shows the Discord logo again.

## 1.12.0

- **New look, desktop and Android.** The home screen is one status strip
  (Capture · Open Genshin · Enter world, with History, Theme, Settings and
  Discord beside it), then your account and **Today** side by side (resin
  counting to full, realm currency, commissions, expeditions, weekly bosses,
  Spiral Abyss), then Export and Wish history, and teyvat.me at the foot.
  Narrow windows and phones show the same in one column.
- Android: a new setting, **Tile opens Genshin** (Settings, off by default).
  With it on, starting capture from the Quick Settings tile also opens the
  game.
- Live sync sends the resin clock (`gi_tasks.resin`, when the next resin
  comes) with each resin change, so the tracker's resin is exact between
  snapshots.

## 1.11.0

- **Serenitea Pot**: the furnishing sets you've learned and the companions'
  rewards you've collected.
- **Domains**: which domains you've cleared once (their first-clear reward
  taken).
- **NPCs met** and **Natlan's tribal chapters** (each chapter's state and
  when it started).

New optional export parts: `gi_collections.furnishingSets`,
`gi_collections.companionRewards`, `gi_collections.npcsMet`,
`gi_exploration.domainsCleared` and `gi_exploration.tribalChapters`.

- The game's own requests are no longer read as account data (one of them
  could replace the monsters you've seen with a shorter list).

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
