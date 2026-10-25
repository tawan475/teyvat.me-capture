# logbook-game-data

Downloads Dimbreath's game data dump and indexes what Logbook needs to name
things (characters, weapons, artifacts, materials, talents). The desktop app's
`build.rs` uses it to build the blob the binary embeds, and the app uses it to
look the data up at run time.

It is the `crates/game-data` member of Logbook's Cargo workspace.

## Credits

Written by Erik Gilling ([konkers](https://github.com/konkers)) as
[konkers/anime-game-data](https://github.com/konkers/anime-game-data), MIT
licensed (Copyright (c) 2025 Erik Gilling; see [`LICENSE`](LICENSE) and the
repository's [`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md)). It was
vendored into irminsul at revision `dfc03cdb5e27807a876b8d4dff17b3f67a320f5c`
and changed there and here since; those changes are not upstream's.
