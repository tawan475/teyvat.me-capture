# Changelog

Each release's section is its GitHub release notes (`.github/workflows/release.yaml`).

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
