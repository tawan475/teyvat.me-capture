# teyvatme-game-data

Indexes what Teyvat.me needs to name things (characters, weapons, artifacts,
materials, talents) from the game data files. The build scripts read the
repository's pinned copy ([`game-data/`](../../game-data/README.md), through
`Pinned`) to build the blob the binaries embed, and the apps use it to look the
data up at run time. The `download` feature (off in the builds) keeps the old
Dimbreath download for the `cli` binary.

It is the `crates/game-data` member of Teyvat.me's Cargo workspace.

## Credits

Written by Erik Gilling ([konkers](https://github.com/konkers)) as
[konkers/anime-game-data](https://github.com/konkers/anime-game-data), MIT
licensed (Copyright (c) 2025 Erik Gilling; see [`LICENSE`](LICENSE) and the
repository's [`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md)). It was
vendored into irminsul at revision `dfc03cdb5e27807a876b8d4dff17b3f67a320f5c`
and changed there and here since; those changes are not upstream's.
