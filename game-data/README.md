# game-data

The game data the builds embed: a pinned copy of the files
`crates/game-data` reads, so `cargo build` (desktop, and the Android native
library) needs no network and no token. The build scripts
(`apps/desktop/build.rs`, `apps/android/native/build.rs`) read it through
`teyvatme_game_data::Pinned` and rebuild the embedded database when it changes.

| File | What |
|---|---|
| `manifest.json` | `id`: the data set (40 hex digits; it becomes the database's `git_hash` and the export's `gi_player.gameData`), `client`: the game client build it was decoded from, and each pinned file's SHA-256 |
| `ExcelBinOutput/*.json.gz` | The tables the crate reads, plus the exploration tables (City, ReputationCity, OfferingOpenState, WorldArea), whole |
| `TextMap/TextMap_MediumEN.json.gz` | English text, cut down to the text hashes those tables mention |
| `BinOutput/Scene/Point/scene<id>_point.json.gz` | The point file of every scene WorldAreaConfigData names |

The paths and contents are Dimbreath's dump layout, which this used to download
from gitlab.com until 2026-10-27. They now come from a **client data set**: the
game's own data files, decoded from the installed game client on the
maintainer's machine and published as a release of the private
genshin-game-data repository (its README, "Game data from the client").

## After a game update

Once genshin-game-data has a client data release of the new version (the
maintainer decodes it and runs `pnpm client-data publish` there):

```sh
python game-data/pin.py --release latest          # needs `gh`, signed in with access to genshin-game-data
python game-data/pin.py --from <decoder output>   # or straight from the decoder's folder on that machine
```

Then build, check the new characters and items show up, and commit
`game-data/` to `develop`; the next release carries it. Same input, same
bytes: rerunning on the same data set changes nothing.
