// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

//! Downloads the game data the decoder embeds, as the desktop app's build.rs
//! does: `OUT_DIR/game_data.gz` for `include_bytes!`, and the Dimbreath dump
//! commit it was built from as `TEYVATME_GAME_DATA_SHA`.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::{env, fs, io};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

#[tokio::main(flavor = "current_thread")]
async fn main() -> io::Result<()> {
    // Refreshed when this script changes, OUT_DIR is wiped or
    // TEYVATME_REFRESH_GAME_DATA changes, like the desktop build.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=TEYVATME_REFRESH_GAME_DATA");

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let cache_path = Path::new(&out_dir).join("game_data.json");
    let gz_path = Path::new(&out_dir).join("game_data.gz");

    let mut db = teyvatme_game_data::AnimeGameData::new_with_cache(&cache_path);
    // A warm OUT_DIR builds offline.
    let needs_update = match db.needs_update().await {
        Ok(needs_update) => needs_update,
        Err(e) => {
            println!(
                "cargo:warning=unable to check for new game data ({e}); falling back to the \
                 cached copy"
            );
            false
        }
    };
    let mut updated = false;
    if needs_update {
        match db.update().await {
            Ok(()) => updated = true,
            Err(e) => println!("cargo:warning=unable to download game data ({e})"),
        }
    }
    assert!(
        db.has_data(),
        "no cached game data at {} and the download failed: the first build needs network \
         access to gitlab.com",
        cache_path.display()
    );
    if updated || gz_needs_write(&gz_path, &cache_path) {
        write_game_data_gz(&db, &gz_path)?;
    }

    let game_data_sha = match read_game_data_sha(&gz_path) {
        Ok(sha) => sha,
        Err(e) => {
            println!("cargo:warning=unable to read the game data's dump commit ({e})");
            String::new()
        }
    };
    println!("cargo:rustc-env=TEYVATME_GAME_DATA_SHA={game_data_sha}");
    Ok(())
}

/// True when `game_data.gz` is missing, older than the json cache, or not a
/// complete gzip stream.
fn gz_needs_write(gz_path: &Path, cache_path: &Path) -> bool {
    let Ok(gz_modified) = fs::metadata(gz_path).and_then(|m| m.modified()) else {
        return true;
    };
    if let Ok(json_modified) = fs::metadata(cache_path).and_then(|m| m.modified())
        && gz_modified < json_modified
    {
        return true;
    }
    let Ok(file) = File::open(gz_path) else {
        return true;
    };
    let mut decoder = GzDecoder::new(file);
    let mut buf = [0u8; 64 * 1024];
    loop {
        match decoder.read(&mut buf) {
            Ok(0) => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
    }
}

#[derive(serde::Deserialize)]
struct GameDataMeta {
    git_hash: String,
}

/// The dump commit recorded in `game_data.gz`, checked to look like one.
fn read_game_data_sha(gz_path: &Path) -> io::Result<String> {
    let reader = io::BufReader::new(GzDecoder::new(File::open(gz_path)?));
    let meta: GameDataMeta = serde_json::from_reader(reader).map_err(io::Error::other)?;
    let sha = meta.git_hash.trim();
    if !(7..=64).contains(&sha.len()) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::other(format!(
            "{sha:?} does not look like a commit hash"
        )));
    }
    Ok(sha.to_ascii_lowercase())
}

/// Written to a temporary file and renamed, so an interrupted build leaves
/// no truncated gz behind.
fn write_game_data_gz(db: &teyvatme_game_data::AnimeGameData, gz_path: &Path) -> io::Result<()> {
    let tmp_path = gz_path.with_extension("gz.tmp");
    let mut writer = GzEncoder::new(File::create(&tmp_path)?, Compression::best());
    db.save_to_writer(&mut writer)
        .map_err(|e| io::Error::other(format!("unable to serialize game data: {e}")))?;
    let file = writer.finish()?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp_path, gz_path)
}
