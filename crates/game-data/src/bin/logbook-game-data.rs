// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2025 Erik Gilling and irminsul contributors
// See THIRD_PARTY_NOTICES.md.

use logbook_game_data::AnimeGameData;

#[tokio::main]
async fn main() {
    let mut data = AnimeGameData::new();

    data.update().await.unwrap();
    println!("{:#?}", data);
}
