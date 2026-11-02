# pktmon (vendored)

[emmachase/pktmon](https://github.com/emmachase/pktmon) 0.6.2, MIT (see
`LICENSE`), copied from crates.io so Teyvat.me can choose the capture path.
Changes from upstream:

- `Capture::with_path(CapturePath)` picks the real-time stream or the ETW
  ("legacy") path instead of always preferring the stream. The stream drops
  packets over 9000 bytes, which on a PC that merges incoming UDP datagrams
  (UDP receive offload, Windows 11 24H2) loses a third of the game's traffic.
- The ETW path's shutdown hook no longer unloads the pktmon driver: that
  stopped capture for every other program using pktmon.
- The ETW path passes bare IP packets on (`PacketPayload::IP`) instead of
  dropping every packet that isn't an Ethernet frame: some PCs report game
  packets at some layers only that way.
- `etw_session_losses()` reads the ETW session's lost event and buffer counts.
- `Capture::data_sources()` lists the data sources the real-time API
  enumerates, so the log can show which adapters and components pktmon could
  capture on. The crate's unused `list_data_sources` read the API's buffer
  through a `&PacketMonitorDataSourceList` whose pointer array is declared
  one entry long (undefined behaviour; release builds crashed in ntdll), so
  it reads the buffer field by field with bounds checks now.
- `install_shutdown_hook` keeps the latest backend's hook, not the first.
- `extern "stdcall"` is `extern "system"` (the same on 64-bit Windows;
  Rust is phasing the first out).

`Cargo.toml` is written for this workspace (no examples or dev-dependencies).
