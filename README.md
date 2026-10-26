# Teyvat.me Capture

> **Unofficial fan project.** Teyvat.me is not affiliated with or endorsed by
> HoYoverse. Genshin Impact and the names from its world belong to HoYoverse,
> and none of its logos or art ship here.

Teyvat.me Capture reads your Genshin Impact account from the game's own
network traffic while you play: characters, weapons, artifacts, materials,
achievements and wish history. It saves it as
[GOOD](https://frzyc.github.io/genshin-optimizer/#/doc) JSON, the format
Genshin Optimizer and most other tools read. If you like, it also keeps your
account up to date on [teyvat.me](https://teyvat.me), the tracker website
that shares its name. No account or sign-in is needed to capture and export.

It is packet capture, not screen reading: start it **before** the game
connects, so it sees the login that carries the session key.

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

## Using it

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

**Is it safe?** Teyvat.me only reads. It never sends anything to the game or
changes its files, and your data goes nowhere unless you choose a destination.
Like every tool of this kind, it is used at your own risk.

## Layout

| Path | What it is |
|---|---|
| `apps/desktop` | The desktop app (Windows, Linux, macOS): capture, decoding, export, tracker sign-in and uploads, live sync, wish history. Package `teyvatme`, binary `teyvatme`; egui UI with the OS's own window frame and dialogs. |
| `apps/android` | The Android app: a per-app VPN that forwards the game's traffic and decodes it on the phone (Kotlin + Compose, and a Rust native library in `apps/android/native`). |
| `crates/core` | `teyvatme-core`: everything platform-free that both apps share. That is the decode layer from captured frames to account data, the account model, the GOOD export, live sync's deltas and policy, and the capture recordings. |
| `crates/protocol` | `teyvatme-protocol`: KCP, the game's encryption and key recovery, Oodle decompression, and the message matchers. |
| `crates/game-data` | `teyvatme-game-data`: downloads and indexes Dimbreath's game data dump (used at build time and run time). |

## Building

The toolchain is pinned in `rust-toolchain.toml` (a nightly, because of
`rustfmt.toml`). The first build downloads the game data dump.

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

## Releases

Work happens on `develop`. `main` only receives `develop` through a pull
request with every CI check green, and releases are made from `main`:
**Actions → Create Release → Run workflow** on `main`. The version is
`package.version` in `apps/desktop/Cargo.toml` (bump it in the release PR, and
add its section to [`CHANGELOG.md`](CHANGELOG.md)). The workflow builds the
four desktop files and the signed APK (`.github/workflows/android.yml`) and
publishes them with checksums.

## Export format

The file is GOOD JSON with a few keys of Teyvat.me's own beside it. What each
holds, which export carries which, and the rules they keep:
[docs/export-format.md](docs/export-format.md); every field typed in
[docs/teyvatme-export.ts](docs/teyvatme-export.ts).

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

A saved tracker address of `https://genshin-tracker.475.dev/api` moves to
`https://teyvat.me/api`, and its sign-in moves with it. Logs, crash reports
and packet logs stay with irminsul: they describe its runs.

The bridge release itself (irminsul's updater pointed at this repository) is
still to come.

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
