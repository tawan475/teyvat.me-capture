# teyvatme-protocol

Reads the game's network traffic: Ethernet/IP/UDP → KCP → the XOR-encrypted
game commands → protobuf. It recovers the session key from the login
handshake, decompresses Oodle-compressed messages (a clean-room decoder in
`src/oodle.rs`), and recognises the messages Teyvat.me needs by their shape
(inventory, characters, achievements, anecdotes, the planner's routine,
wish history keys).

It is the `crates/protocol` member of Teyvat.me's Cargo workspace and is not
published to crates.io. Only the library builds by default. A small CLI that
dumps captured avatar or item packets is behind the `cli` feature:

```sh
cargo run -p teyvatme-protocol --features cli -- avatars path/to/packet.bin
cargo run -p teyvatme-protocol --features cli -- items path/to/packet.bin
```

## Development

Run from the repository root (the toolchain and `rustfmt.toml` are the
workspace's):

```sh
cargo clippy -p teyvatme-protocol --all-targets --all-features -- -Dwarnings
cargo test -p teyvatme-protocol --all-features
RUSTDOCFLAGS=-Dwarnings cargo doc -p teyvatme-protocol --no-deps --all-features
```

`python check.py` runs these with the rest of the workspace's checks.

## Credits

This crate started as **auto-artifactarium** (MIT, Copyright (c) 2024
IceDynamix; see [`LICENSE`](LICENSE) and the repository's
[`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md)), by way of
[hashblen/auto-artifactarium](https://github.com/hashblen/auto-artifactarium)
and [konkers/auto-artifactarium](https://github.com/konkers/auto-artifactarium),
which build on [IceDynamix/reliquary](https://github.com/IceDynamix/reliquary).
Files that still hold their lines say so in their header. Related work:
[juliuskreutz/stardb-exporter](https://github.com/juliuskreutz/stardb-exporter),
[PJK136/stardb-exporter](https://github.com/PJK136/stardb-exporter) and
[IceDynamix/reliquary-archiver](https://github.com/IceDynamix/reliquary-archiver).
