# Teyvat.me

> **Unofficial fan project.** Teyvat.me is not affiliated with or endorsed by
> HoYoverse. Genshin Impact and the names from its world belong to HoYoverse,
> and none of its logos or art ship here.

Teyvat.me reads Genshin Impact's network traffic while you play and keeps your
account (characters, weapons, artifacts, materials, achievements, wish
history, the planner's routine) up to date on the tracker, the website that
shares Teyvat.me's brand. It exports the
[GOOD](https://frzyc.github.io/genshin-optimizer/#/doc) format other tools
read too.

It is packet capture, not screen reading: it must be running before the game
connects, so it can see the login handshake that carries the session key.

Teyvat.me is the successor of [irminsul](https://github.com/tawan475/irminsul)
(itself a fork of [konkers/irminsul](https://github.com/konkers/irminsul)).
This repository started fresh on 2026-10-25 from irminsul's code at that
point; see [Credits](#credits).

## Layout

| Path | What it is |
|---|---|
| `apps/desktop` | The desktop app (Windows, Linux, macOS): capture, decoding, tracker sign-in and uploads, live sync, wish history. Package `teyvatme`, binary `teyvatme`. Still irminsul's egui UI; it will be redesigned. |
| `apps/android` | The Android app: a per-app VPN that forwards the game's traffic and keeps a copy of it (Kotlin + a Rust native library in `apps/android/native`). An early spike. |
| `crates/core` | `teyvatme-core`: everything platform-free that both apps share. That is the decode layer from captured frames to account data, the account model, the GOOD export, live sync's deltas and policy, and the capture recordings. |
| `crates/protocol` | `teyvatme-protocol`: KCP, the game's encryption and key recovery, Oodle decompression, and the message matchers. |
| `crates/game-data` | `teyvatme-game-data`: downloads and indexes Dimbreath's game data dump (used at build time and run time). |

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

## Coming over from irminsul

The bridge release of irminsul installs Teyvat.me over irminsul's exe. On its
first launch Teyvat.me takes over what irminsul kept on the machine, under its
own names:

| What | irminsul | Teyvat.me | How |
|---|---|---|---|
| Settings (`app.ron`) | app id `Irminsul` | app id `Teyvat.me` | copied, irminsul's kept (`migrate.rs`) |
| Tracker sign-in | credential `irminsul` | credential `teyvatme` | moved on first read (`signin.rs` `MovingStore`) |
| Start on startup | task `Irminsul` | task `Teyvat.me` | moved, keeping "start minimized" (`autostart.rs`) |
| Single instance | `irminsul_app_instance` | `teyvatme_app_instance` | both held, so the two never capture at once |
| Debug data folder | `irminsul-data` | `teyvatme-data` | not moved (debug builds only) |

Logs, crash reports and packet logs stay with irminsul: they describe its runs.

## Still to do

- **Coming over from irminsul.**
  - **Bridge release.** The last irminsul release points its updater at this
    repository (irminsul's `src/update.rs`: `repo_owner`/`repo_name` and the
    asset names), so installed copies update to Teyvat.me. Teyvat.me starts at
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

Teyvat.me stands on the work of:

- Erik Gilling ([konkers](https://github.com/konkers)), who wrote irminsul and
  anime-game-data;
- IceDynamix, hashblen, Julius Kreutz and the auto-artifactarium contributors,
  for the packet decoding Teyvat.me's protocol crate grew from;
- PJK136, emmachase (pktmon), the Genshin Optimizer project and Inventory
  Kamera.

The full list is in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
