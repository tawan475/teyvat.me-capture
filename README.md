<p align="center">
  <a href="https://github.com/tawan475/teyvat.me-capture/releases/latest">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://gi-cdn.475.dev/readme/banner-dark.webp">
      <img alt="Teyvat.me Capture: capture your Genshin account while you play. Windows, Linux, macOS and Android." src="https://gi-cdn.475.dev/readme/banner-light.webp" width="100%">
    </picture>
  </a>
</p>

# <img alt="" src="https://gi-cdn.475.dev/readme/icon.webp" width="36" align="top"> Teyvat.me Capture

<p align="center">
  <a href="https://github.com/tawan475/teyvat.me-capture/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/tawan475/teyvat.me-capture?label=release&color=6366f1"></a>
  <a href="https://github.com/tawan475/teyvat.me-capture/actions/workflows/rust.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/tawan475/teyvat.me-capture/rust.yml?branch=develop&label=CI"></a>
  <a href="LICENSE"><img alt="MIT licence" src="https://img.shields.io/github/license/tawan475/teyvat.me-capture?color=6366f1"></a>
  <img alt="Windows · Linux · macOS · Android" src="https://img.shields.io/badge/platforms-Windows%20%C2%B7%20Linux%20%C2%B7%20macOS%20%C2%B7%20Android-334155">
  <a href="https://teyvat.me"><img alt="Tracker: teyvat.me" src="https://img.shields.io/badge/tracker-teyvat.me-f59e0b"></a>
</p>

<p align="center">
  <a href="https://github.com/tawan475/teyvat.me-capture/releases/latest"><img alt="Download" src="https://gi-cdn.475.dev/readme/btn-download.webp" height="44"></a>&nbsp;
  <a href="https://teyvat.me"><img alt="teyvat.me" src="https://gi-cdn.475.dev/readme/btn-teyvatme.webp" height="44"></a>
</p>

> [!NOTE]
> **Unofficial fan project.** Teyvat.me is not affiliated with or endorsed by HoYoverse. Genshin Impact and the names from its world belong to HoYoverse, and none of its logos or art ship here.

Reads your Genshin Impact account (characters, weapons, artifacts, materials,
achievements and wish history) from the game's own network traffic while you
play, and saves it as [GOOD](https://frzyc.github.io/genshin-optimizer/#/doc)
JSON for Genshin Optimizer and most other tools. No account or sign-in needed;
[teyvat.me](https://teyvat.me), the tracker website, is optional.

> [!IMPORTANT]
> Start it **before** the game connects: it is packet capture, not screen
> reading, and needs the login that carries the session key.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://gi-cdn.475.dev/readme/app-dark.webp">
    <img alt="Teyvat.me Capture after a capture: account counts, today's resin and tasks, export and wish history" src="https://gi-cdn.475.dev/readme/app-light.webp" width="100%">
  </picture>
</p>

## How it works

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://gi-cdn.475.dev/readme/steps-dark.webp">
  <img alt="1. Start the app before Genshin. 2. Enter the world: the status turns to Data decrypting. 3. Save GOOD JSON, copy the wish URL, or send it to teyvat.me." src="https://gi-cdn.475.dev/readme/steps-light.webp" width="100%">
</picture>

<details>
<summary><b>Step by step</b></summary>

1. Start Teyvat.me Capture. On Windows, accept the administrator prompt.
2. Start Genshin and enter the world. The status goes **Capture → Open
   Genshin → Enter world**, then says **Data decrypting**.
3. **Save GOOD JSON** (or **Copy**) and import it in Genshin Optimizer or any
   tool that reads GOOD. Turn on **Auto-export** to write a file after every
   capture.
4. For wish history, open **Wish → History** in game once per login. Then copy
   the wish URL for paimon.moe or StarDB, or save the history as UIGF.
5. Optional: connect [teyvat.me](https://teyvat.me) under **Destinations** to
   upload each capture and sync changes live.

If Genshin was already in the world when Teyvat.me started, it says
**Launch missed**: go back to the title screen and enter again, or restart the
game.

</details>

**Is it safe?** Teyvat.me only reads. It never sends anything to the game or
changes its files, and your data goes nowhere unless you choose a destination.
Like every tool of this kind, it is used at your own risk.

## What it does

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://gi-cdn.475.dev/readme/features-dark.webp">
  <img alt="Reads the game's traffic · GOOD JSON export · Wish history · Today at a glance · Live sync · Only reads" src="https://gi-cdn.475.dev/readme/features-light.webp" width="100%">
</picture>

## Download

From the [latest release](https://github.com/tawan475/teyvat.me-capture/releases/latest):

| Platform | File | Notes |
|---|---|---|
| Windows 10/11 | `teyvatme-windows.exe` | Uses Windows' built-in packet capture (pktmon). Asks for administrator rights. |
| Windows with Npcap | `teyvatme-windows-pcap.exe` | Only if you already use [Npcap](https://npcap.com) and prefer it. |
| Linux x86_64 | `teyvatme-linux-x86_64.tar.gz` | Needs packet-capture rights (root, or `cap_net_raw` on the binary). |
| macOS (Apple silicon) | `Teyvatme-macos-arm64.app.tar.gz` | Asks once for access to `/dev/bpf`. |
| Android 10+ | `teyvatme-android.apk` | A local VPN that sees only Genshin's traffic. Allow installing from your browser or file manager. |

`SHA256SUMS.txt` lists every file's checksum. The desktop app updates itself
from this page.

## For developers

<details>
<summary><b>Layout</b></summary>

| Path | What it is |
|---|---|
| `apps/desktop` | The desktop app (Windows, Linux, macOS): capture, decoding, export, tracker sign-in and uploads, live sync, wish history. Package `teyvatme`, binary `teyvatme`; egui UI with the OS's own window frame and dialogs. |
| `apps/android` | The Android app: a per-app VPN that forwards the game's traffic and decodes it on the phone (Kotlin + Compose, and a Rust native library in `apps/android/native`). |
| `crates/core` | `teyvatme-core`: everything platform-free that both apps share. That is the decode layer from captured frames to account data, the account model, the GOOD export, live sync's deltas and policy, and the capture recordings. |
| `crates/protocol` | `teyvatme-protocol`: KCP, the game's encryption and key recovery, Oodle decompression, and the message matchers. |
| `crates/game-data` | `teyvatme-game-data`: indexes the game data (characters, weapons, artifacts, materials, talents) from the pinned copy in [`game-data/`](game-data/README.md), at build time and run time. |

</details>

<details>
<summary><b>Building</b></summary>

The toolchain is pinned in `rust-toolchain.toml` (a nightly, because of
`rustfmt.toml`). The game data comes from the pinned copy in
[`game-data/`](game-data/README.md): builds need no network.

```sh
cargo run                         # the desktop app (Windows: pktmon backend)
cargo run -- --isolated           # a separate copy: own settings, sign-in and locks
cargo build --features pcap       # with the pcap backend (Npcap SDK on Windows)
python check.py                   # the full local gate: fmt, clippy, tests, builds
python -m unittest discover -p "test_*.py"   # the scripts' tests
```

The Android app's native library builds with
[cargo-ndk](https://github.com/bbqsrc/cargo-ndk) (`apps/android/native`:
`cargo ndk -t arm64-v8a -P 29 build --release`). The Gradle build in
`apps/android` runs that step itself.

The icons come from `apps/desktop/assets/make_icons.py` (Pillow).

</details>

<details>
<summary><b>Releases</b></summary>

Work happens on `develop`. `main` only receives `develop` through a pull
request with every CI check green, and releases are made from `main`:
**Actions → Create Release → Run workflow** on `main`. The version is
`package.version` in `apps/desktop/Cargo.toml` (bump it in the release PR, and
add its section to [`CHANGELOG.md`](CHANGELOG.md)). The workflow builds the
four desktop files and the signed APK (`.github/workflows/android.yml`) and
publishes them with checksums.

</details>

<details>
<summary><b>Export format</b></summary>

The file is GOOD JSON with a few keys of Teyvat.me's own beside it. What each
holds, which export carries which, and the rules they keep:
[docs/export-format.md](docs/export-format.md); every field typed in
[docs/teyvatme-export.ts](docs/teyvatme-export.ts).

</details>

<details>
<summary><b>Contracts with the tracker and with irminsul installs</b></summary>

Some names keep irminsul's spelling on purpose:

- **The tracker's API.** That means the `irminsul` OAuth client and token
  audience, the `/api/oauth/irminsul` and `/api/irminsul/{whoami,import,changes}`
  routes, the GOOD `gi_*` keys, the GOOD `source` value `"Irminsul"`, and the
  upload's `irminsul_capture.json` file name.

</details>

<details>
<summary><b>Coming over from irminsul</b></summary>

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

A saved tracker address of `https://genshin-tracker.475.dev/api` moves to
`https://teyvat.me/api`, and its sign-in moves with it. Logs, crash reports
and packet logs stay with irminsul: they describe its runs.

The bridge release itself (irminsul's updater pointed at this repository) is
still to come.

</details>

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
