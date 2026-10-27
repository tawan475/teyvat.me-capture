// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::{env, fs, io};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use winresource::WindowsResource;

#[tokio::main]
async fn main() -> io::Result<()> {
    // The game data comes from the repository's pinned copy (`game-data/`, see
    // its README), so a build needs no network. Without these, cargo would re-run
    // this script on every source edit; with them the database is rebuilt when
    // the script, the icon or the pinned files change, or OUT_DIR is wiped.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").unwrap();
    let pin_dir = Path::new(&manifest_dir).join("../../game-data");
    println!("cargo:rerun-if-changed={}", pin_dir.display());
    let pinned = teyvatme_game_data::Pinned::open(&pin_dir).map_err(io::Error::other)?;

    // Build the database and save it where the source includes it.
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let cache_path = Path::new(&out_dir).join("game_data.json");
    let gz_path = Path::new(&out_dir).join("game_data.gz");

    let mut db = teyvatme_game_data::AnimeGameData::new_with_cache(&cache_path);
    let mut updated = false;
    if db
        .needs_update_from(&pinned)
        .await
        .map_err(io::Error::other)?
    {
        db.update_from(&pinned)
            .await
            .map_err(|e| io::Error::other(format!("unable to build the game data: {e:#}")))?;
        updated = true;
    }

    // The gz is what `monitor.rs` embeds with `include_bytes!`, so it must exist
    // whether or not this run refreshed the json cache.  Writing it only inside
    // the update branch used to leave a warm json cache next to a missing gz,
    // which failed every later build with an error pointing at monitor.rs.
    if updated || gz_needs_write(&gz_path, &cache_path) {
        write_game_data_gz(&db, &gz_path)?;
    }

    // Which game data set the binary carries (its 40-hex id; Dimbreath dump
    // commits before 2026-10-27), for the log and the export's
    // `gi_player.gameData`. Read back out of the gz rather than the json cache:
    // the gz is what `monitor.rs` embeds, and the cache is only written on a
    // best-effort basis. Cargo replays this directive from its saved build
    // script output on builds that do not re-run the script. Empty when it
    // cannot be read, which the binary treats as "unknown"; that is not worth
    // failing a build over.
    let game_data_sha = match read_game_data_sha(&gz_path) {
        Ok(sha) => sha,
        Err(e) => {
            println!("cargo:warning=unable to read the game data's data set id ({e})");
            String::new()
        }
    };
    println!("cargo:rustc-env=TEYVATME_GAME_DATA_SHA={game_data_sha}");

    // Add icon to windows binary.
    if env::var_os("CARGO_CFG_WINDOWS").is_some() {
        WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set_manifest(
                r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
        <requestedPrivileges>
            <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
        </requestedPrivileges>
    </security>
</trustInfo>
</assembly>
"#,
            )
            .compile()?;
    }

    // `cfg(unix)` in a build script describes the *host*, not the target, so it
    // silently drops the link directive when cross compiling a unix target from
    // a non-unix host (and vice versa).  CARGO_CFG_UNIX / CARGO_FEATURE_* are
    // set by cargo for the target being built, mirroring CARGO_CFG_WINDOWS above.
    if env::var_os("CARGO_CFG_UNIX").is_some()
        && env::var_os("CARGO_FEATURE_STATIC_LIBPCAP").is_some()
    {
        println!("cargo:rustc-link-lib=static=pcap");
    }
    Ok(())
}

/// True when `game_data.gz` is missing, unreadable, older than the
/// `game_data.json` cache it is generated from, or not a complete gzip stream
/// (e.g. truncated by an interrupted build from an earlier version of this
/// script, which wrote the file in place).
fn gz_needs_write(gz_path: &Path, cache_path: &Path) -> bool {
    let Ok(gz_modified) = fs::metadata(gz_path).and_then(|m| m.modified()) else {
        return true;
    };

    // No json cache to compare against means the gz is all we have to go on.
    if let Ok(json_modified) = fs::metadata(cache_path).and_then(|m| m.modified())
        && gz_modified < json_modified
    {
        return true;
    }

    !gz_is_complete(gz_path)
}

/// Decompresses `gz_path` and discards the result, just to confirm the stream
/// (including its trailer) is intact.
fn gz_is_complete(gz_path: &Path) -> bool {
    let Ok(file) = File::open(gz_path) else {
        return false;
    };
    let mut decoder = GzDecoder::new(file);
    let mut buf = [0u8; 64 * 1024];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

/// The part of the game data blob this script reads back: `teyvatme-game-data`
/// serializes the data set it was built from as `git_hash`, but has no public
/// accessor for it.
#[derive(serde::Deserialize)]
struct GameDataMeta {
    git_hash: String,
}

/// The data set id recorded in `game_data.gz`, checked to look like one.
fn read_game_data_sha(gz_path: &Path) -> io::Result<String> {
    let reader = io::BufReader::new(GzDecoder::new(File::open(gz_path)?));
    let meta: GameDataMeta = serde_json::from_reader(reader).map_err(io::Error::other)?;
    let sha = meta.git_hash.trim();
    let looks_like_a_commit =
        (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit());
    if !looks_like_a_commit {
        return Err(io::Error::other(format!(
            "{sha:?} does not look like a commit hash"
        )));
    }
    Ok(sha.to_ascii_lowercase())
}

/// Writes the compressed game data to a temporary file and renames it into
/// place, so an interrupted build cannot leave a truncated `game_data.gz`
/// behind.
fn write_game_data_gz(db: &teyvatme_game_data::AnimeGameData, gz_path: &Path) -> io::Result<()> {
    let tmp_path = gz_path.with_extension("gz.tmp");
    let file = File::create(&tmp_path)?;
    let mut writer = GzEncoder::new(file, Compression::best());
    db.save_to_writer(&mut writer)
        .map_err(|e| io::Error::other(format!("unable to serialize game data: {e}")))?;
    let file = writer.finish()?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp_path, gz_path)
}
