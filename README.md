# Logbook

> **Logbook is a working name.** The final name and brand come later;
> `python tools/rename.py <name>` renames the project in one go.

Logbook reads Genshin Impact's network traffic while you play and keeps your
account (characters, weapons, artifacts, materials, achievements, wish
history, the planner's routine) up to date on the tracker, the website that
shares Logbook's brand. It exports the
[GOOD](https://frzyc.github.io/genshin-optimizer/#/doc) format other tools
read too.

It is packet capture, not screen reading: it must be running before the game
connects, so it can see the login handshake that carries the session key.

Logbook is the successor of [irminsul](https://github.com/tawan475/irminsul)
(itself a fork of [konkers/irminsul](https://github.com/konkers/irminsul)).
This repository started fresh on 2026-10-25 from irminsul's code at that
point; see [Credits](#credits).

## Layout

| Path | What it is |
|---|---|
| `apps/desktop` | The desktop app (Windows, Linux, macOS): capture, decoding, tracker sign-in and uploads, live sync, wish history. Package `logbook`, binary `logbook`. Still irminsul's egui UI; it will be redesigned. |
| `apps/android` | The Android app: a per-app VPN that forwards the game's traffic and keeps a copy of it (Kotlin + a Rust native library in `apps/android/native`). An early spike. |
| `crates/core` | `logbook-core`: everything platform-free that both apps share. That is the decode layer from captured frames to account data, the account model, the GOOD export, live sync's deltas and policy, and the capture recordings. |
| `crates/protocol` | `logbook-protocol`: KCP, the game's encryption and key recovery, Oodle decompression, and the message matchers. |
| `crates/game-data` | `logbook-game-data`: downloads and indexes Dimbreath's game data dump (used at build time and run time). |

## Building

The toolchain is pinned in `rust-toolchain.toml` (a nightly, because of
`rustfmt.toml`). The first build downloads the game data dump.

```sh
cargo run                         # the desktop app (Windows: pktmon backend)
cargo build --features pcap       # with the pcap backend (Npcap SDK on Windows)
python check.py                   # the full local gate: fmt, clippy, tests, builds
python -m unittest discover -p "test_*.py"   # the scripts' tests
```

The Android app's native library builds with
[cargo-ndk](https://github.com/bbqsrc/cargo-ndk) (`apps/android/native`:
`cargo ndk -t arm64-v8a -P 29 build --release`). The Gradle build in
`apps/android` runs that step itself.

## Contracts with the tracker and with irminsul installs

Some names keep irminsul's spelling on purpose:

- **The tracker's API.** That means the `irminsul` OAuth client and token
  audience, the `/api/oauth/irminsul` and `/api/irminsul/{whoami,import,changes}`
  routes, the GOOD `gi_*` keys, the GOOD `source` value `"Irminsul"`, and the
  upload's `irminsul_capture.json` file name.
- **What installed copies of irminsul already stored on users' machines.**
  That is the credential store entry `irminsul`, the app id `Irminsul` (where
  `app.ron` lives), the startup task `Irminsul`, the single-instance lock
  `irminsul_app_instance`, and the debug data folder `irminsul-data`.

## Still to do

- **Coming over from irminsul.**
  - **First-launch migration.** Logbook should take over irminsul's settings,
    data folder, sign-in and startup task under its own names. It should then
    stop using the stored irminsul names above.
  - **Bridge release.** The last irminsul release points its updater at this
    repository (irminsul's `src/update.rs`: `repo_owner`/`repo_name` and the
    asset names), so installed copies update to Logbook. Logbook starts at
    1.0.0 so that it ranks above every irminsul release (0.2.x).
  - **Release workflow.** It was left behind with irminsul's; it needs
    rewriting for the new asset names.
- **The new UI and branding.** That covers the desktop UI, the icon and art
  (the current ones are placeholders), the Android app's UI (design in
  progress), and the user docs.
- **The tracker.** It should know the app by its new name. The old irminsul
  client must keep working for copies that never update.

## Licence

MIT, see [`LICENSE`](LICENSE). Every source file starts with an SPDX header.
Files that hold code from irminsul, anime-game-data or auto-artifactarium name
the original copyright holder too. The full upstream notices are in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).

## Credits

Logbook stands on the work of:

- Erik Gilling ([konkers](https://github.com/konkers)), who wrote irminsul and
  anime-game-data;
- IceDynamix, hashblen, Julius Kreutz and the auto-artifactarium contributors,
  for the packet decoding Logbook's protocol crate grew from;
- PJK136, emmachase (pktmon), the Genshin Optimizer project and Inventory
  Kamera.

The full list is in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
