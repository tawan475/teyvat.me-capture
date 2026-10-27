// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// See THIRD_PARTY_NOTICES.md.

//! The game data files this repository carries: `game-data/` at its root, a
//! pinned, gzipped copy of the files this crate reads, taken from a client
//! data set (the game's own data files, decoded from the installed client;
//! see `game-data/README.md`). The build scripts read it, so a build needs no
//! network and no token.
//!
//! `game-data/manifest.json` names the data set (`id`, 40 hex digits, which
//! the database records as its `git_hash` and the export as
//! `gi_player.gameData`) and lists the files; each file is
//! `game-data/<path>.gz`, `<path>` as in Dimbreath's dump
//! (`ExcelBinOutput/AvatarExcelConfigData.json`).

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::GameDataSource;

pub const MANIFEST: &str = "manifest.json";

/// The fields of `manifest.json` this reads.
#[derive(Debug, Deserialize)]
struct Manifest {
    id: String,
}

/// A pinned copy of game data files in a folder (see the module docs).
#[derive(Debug)]
pub struct Pinned {
    dir: PathBuf,
    id: String,
}

impl Pinned {
    pub fn open<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        let path = dir.join(MANIFEST);
        let text = fs::read_to_string(&path)
            .with_context(|| format!("unable to read the pinned game data's {}", path.display()))?;
        let manifest: Manifest = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a game data manifest", path.display()))?;
        let id = manifest.id.trim().to_ascii_lowercase();
        if id.len() != 40 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!(
                "{}: the data set id {id:?} is not 40 hex digits",
                path.display()
            );
        }
        Ok(Self {
            dir: dir.to_owned(),
            id,
        })
    }

    /// The data set's id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The folder, for build scripts' `rerun-if-changed`.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

impl GameDataSource for Pinned {
    async fn get_latest_hash(&self) -> Result<String> {
        Ok(self.id.clone())
    }

    async fn get_json_file<T: DeserializeOwned>(&self, git_ref: &str, path: &str) -> Result<T> {
        if git_ref != self.id {
            bail!(
                "the pinned game data is data set {}, not {git_ref}",
                self.id
            );
        }
        let file = self.dir.join(format!("{path}.gz"));
        let reader = BufReader::new(GzDecoder::new(
            File::open(&file).with_context(|| format!("unable to open {}", file.display()))?,
        ));
        serde_json::from_reader(reader)
            .with_context(|| format!("unable to parse {}", file.display()))
    }
}
