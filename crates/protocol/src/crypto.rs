// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475
// Portions copyright (c) 2024 IceDynamix and auto-artifactarium contributors
// See THIRD_PARTY_NOTICES.md.

use std::collections::HashMap;

use rand_mt::Mt64;
use tracing::{debug, error, info, instrument, trace, warn};

use crate::bytes_as_hex;
use crate::cs_rand::Random;

/// Length of a session XOR key in bytes: 512 big-endian 64-bit Mersenne Twister
/// draws.
const KEY_LEN: usize = 4096;

/// Smallest command the magic-byte checks can read. A real `GameCommand` is far
/// longer (12 bytes of fixed header), but two bytes is the hard floor below
/// which indexing `data[0]`/`data[len - 2]` would panic.
const MIN_COMMAND_LEN: usize = 2;

/// Number of candidate send times tried when recovering a session key, i.e. the
/// tolerated client/server clock skew is +/- `TIME_CANDIDATES / 2` ms.
const TIME_CANDIDATES: i64 = 3000;

/// Number of consecutive client seeds drawn from each candidate send time.
///
/// This was deliberately raised from 5 to 1000; because [`bruteforce`] searches
/// depth-major it only costs anything when the key is genuinely unrecoverable.
const SEED_DEPTH: i32 = 1000;

#[instrument(skip_all)]
pub fn decrypt_command(key: &[u8], encrypted: &mut [u8]) {
    if key.is_empty() {
        // `key[i % key.len()]` below would divide by zero. An empty key reaches
        // us when a caller supplies an empty initial key -- an empty base64
        // string decodes to an empty `Vec` without error -- so complain loudly
        // rather than taking down the whole capture task.
        error!("refusing to decrypt with an empty XOR key; leaving the command untouched");
        return;
    }

    trace!(data = bytes_as_hex(encrypted), "before decryption");

    for i in 0..encrypted.len() {
        encrypted[i] ^= key[i % key.len()];
    }

    trace!(data = bytes_as_hex(encrypted), "after decryption");
}

pub fn lookup_initial_key(initial_keys: &HashMap<u16, Vec<u8>>, bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < MIN_COMMAND_LEN {
        warn!(
            len = bytes.len(),
            "command too short to carry a key version"
        );
        return None;
    }

    let version = u16::from_be_bytes(bytes[..2].try_into().unwrap()) ^ 0x4567;

    // attempt to fetch from user provided initial keys, otherwise use our own baked-in ones
    let key = initial_keys.get(&version).cloned();
    match key {
        Some(key) => {
            info!(version, "found initial decryption key");
            Some(key)
        }
        None => {
            info!(version, "didn't find decryption key");
            None
        }
    }
}

/// MT19937-64's middle word, twist matrix and masks.
const MT_M: usize = 156;
const MT_MATRIX_A: u64 = 0xB502_6F5A_A966_19E9;
const MT_UPPER: u64 = 0xFFFF_FFFF_8000_0000;
const MT_LOWER: u64 = 0x7FFF_FFFF;

/// Output `index` (0 or 1) of `Mt64::new(seed)` for each of `seeds`,
/// computing only the state words that output reads.
///
/// A real generator initialises all 312 words of its state and twists all of
/// them before its first output, but output `i` only reads words `i`, `i + 1`
/// and `i + MT_M` of the initial state. Stopping there is ~4x less work, and
/// this runs once per candidate in every key search. Several seeds at once
/// run as independent chains the CPU can overlap.
fn mt64_early_outputs<const L: usize>(seeds: [u64; L], index: usize) -> [u64; L] {
    debug_assert!(index < 2);
    let mut word = seeds;
    let mut at_index = seeds;
    let mut after_index = seeds;
    for i in 1..=index + MT_M {
        for w in &mut word {
            *w = 6_364_136_223_846_793_005u64
                .wrapping_mul(*w ^ (*w >> 62))
                .wrapping_add(i as u64);
        }
        if i == index {
            at_index = word;
        } else if i == index + 1 {
            after_index = word;
        }
    }
    // `word` is now state word `index + MT_M`.
    std::array::from_fn(|lane| {
        let x = (at_index[lane] & MT_UPPER) | (after_index[lane] & MT_LOWER);
        let mut y = word[lane] ^ (x >> 1) ^ if x & 1 == 0 { 0 } else { MT_MATRIX_A };
        y ^= (y >> 29) & 0x5555_5555_5555_5555;
        y ^= (y << 17) & 0x71D6_7FFF_EDA6_0000;
        y ^= (y << 37) & 0xFFF7_EEE0_0000_0000;
        y ^ (y >> 43)
    })
}

/// The first 64-bit word of `new_key_from_seed(seed)` for each of `seeds`,
/// without building generators.
fn first_key_words<const L: usize>(seeds: [u64; L]) -> [u64; L] {
    // `key_generator`: seeded with the first generator's first output, then
    // that generator's first output skipped.
    mt64_early_outputs(mt64_early_outputs(seeds, 0), 1)
}

fn first_key_word(seed: u64) -> u64 {
    first_key_words([seed])[0]
}

/// The Mersenne Twister that produces a session key's bytes, positioned at the
/// key's first 64-bit word.
fn key_generator(seed: u64) -> Mt64 {
    let mut first = Mt64::new(seed);
    let mut generator = Mt64::new(first.next_u64());

    let _ = generator.next_u64(); // skip first number

    generator
}

pub fn new_key_from_seed(seed: u64) -> Vec<u8> {
    let mut generator = key_generator(seed);

    // Fill in place: the previous version reserved 512 bytes and then pushed
    // 4096, reallocating three times per candidate key.
    let mut key = vec![0u8; KEY_LEN];
    // `as_chunks_mut` over `chunks_exact_mut(8)`: the chunk size is a constant,
    // so this yields `&mut [u8; 8]` and `copy_from_slice` becomes a fixed-size
    // copy with no length check. Clippy's `chunks_exact_to_as_chunks` asks for
    // it by name on newer toolchains.
    let (words, remainder) = key.as_chunks_mut::<8>();
    debug_assert!(remainder.is_empty(), "KEY_LEN must be a multiple of 8");
    for word in words {
        *word = generator.next_u64().to_be_bytes();
    }
    key
}

/// The four key bytes a candidate seed has to reproduce for a command to look
/// like plaintext.
///
/// A decrypted `GameCommand` starts with `45 67` and ends with `89 AB`, so those
/// four key bytes are fully determined by the ciphertext. Checking them without
/// building the whole 4096-byte key is what makes the bruteforce reject path
/// cheap.
struct MagicProbe {
    /// Expected `key[0]` and `key[1]`.
    prefix: [u8; 2],
    /// `(key byte index, expected value)` for the two trailing magic bytes,
    /// sorted by index so the generator only has to be walked forwards once.
    /// The indices wrap modulo [`KEY_LEN`], so the pair can arrive out of order.
    suffix: [(usize, u8); 2],
}

impl MagicProbe {
    fn new(data: &[u8]) -> Option<Self> {
        if data.len() < MIN_COMMAND_LEN {
            return None;
        }

        let last = data.len() - 1;
        let mut suffix = [
            ((last - 1) % KEY_LEN, data[last - 1] ^ 0x89),
            (last % KEY_LEN, data[last] ^ 0xAB),
        ];
        suffix.sort_by_key(|&(index, _)| index);

        Some(Self {
            prefix: [data[0] ^ 0x45, data[1] ^ 0x67],
            suffix,
        })
    }

    /// Whether `new_key_from_seed(seed)` would reproduce all four magic bytes.
    ///
    /// Both prefix bytes live in the generator's first word, so all but roughly
    /// one candidate in 2^16 is rejected after a single draw and without
    /// allocating a key at all.
    fn matches(&self, seed: u64) -> bool {
        self.prefix_matches(first_key_word(seed)) && self.matches_after_prefix(seed)
    }

    /// Whether the key's first word, from [`first_key_words`], carries the
    /// two leading magic bytes.
    fn prefix_matches(&self, first_word: u64) -> bool {
        let first = first_word.to_be_bytes();
        first[0] == self.prefix[0] && first[1] == self.prefix[1]
    }

    /// The trailing magic bytes, for a seed whose prefix already matched.
    fn matches_after_prefix(&self, seed: u64) -> bool {
        let mut generator = key_generator(seed);
        let mut word = generator.next_u64().to_be_bytes();

        // Byte index of `word`'s first byte.
        let mut base = 0usize;
        for &(index, expected) in &self.suffix {
            while base + 8 <= index {
                word = generator.next_u64().to_be_bytes();
                base += 8;
            }
            if word[index - base] != expected {
                return false;
            }
        }

        true
    }
}

/// Search a single candidate send time for the session key.
///
/// This is the retained-anchor fast path of `GameSniffer::recover_session_key`,
/// so it runs on every message the installed key fails to decrypt: a wrong
/// answer here installs a wrong session key in production, not just in a test.
/// [`bruteforce`] covers the same space plus a +/-1499 ms sweep, but searches it
/// depth-major across all candidate times at once and so cannot call this.
pub fn guess(seed: i64, server_seed: u64, depth: i32, data: &[u8]) -> Option<Vec<u8>> {
    let probe = MagicProbe::new(data)?;

    let mut generator = Random::seeded(seed as i32);
    for i in 0..depth {
        let combined_seed = generator.next_safe_uint64() ^ server_seed;
        if probe.matches(combined_seed) {
            trace!(
                time_seed = seed,
                depth = i,
                combined_seed,
                "session key seed recovered"
            );
            info!(depth = i, "session key recovered");
            return Some(new_key_from_seed(combined_seed));
        }
    }

    None
}

/// The `i`-th candidate send time: 0, 0, +1, -1, +2, -2, ... ms around
/// `sent_time`.
///
/// `sent_time` comes off the wire, so the offset is applied with a wrapping add
/// instead of panicking in debug builds; the result is truncated to `i32` for
/// seeding regardless.
fn candidate_time(sent_time: u64, i: i64) -> i64 {
    let offset = if i % 2 == 0 { i / 2 } else { -(i - 1) / 2 };
    (sent_time as i64).wrapping_add(offset)
}

pub fn bruteforce(sent_time: u64, server_seed: u64, data: Vec<u8>) -> Option<(u64, Vec<u8>)> {
    debug!(
        sent_time,
        len = data.len(),
        "running session key bruteforce"
    );

    let Some(probe) = MagicProbe::new(&data) else {
        warn!(
            len = data.len(),
            "command too short to bruteforce a session key"
        );
        return None;
    };

    // One `Random` per candidate send time, so the search below can run
    // depth-major: every candidate time is tried at depth 0 before any is tried
    // at depth 1. The overwhelmingly common answer -- depth 0, a small clock
    // skew -- is then found within `TIME_CANDIDATES` probes instead of up to
    // `TIME_CANDIDATES * SEED_DEPTH` of them, so recovery cost stops scaling
    // with skew. A `Random` is 240 bytes, so holding 3000 of them is ~720 KiB.
    let mut generators: Vec<(i64, Random)> = (0..TIME_CANDIDATES)
        .map(|i| {
            let time = candidate_time(sent_time, i);
            (time, Random::seeded(time as i32))
        })
        .collect();

    for depth in 0..SEED_DEPTH {
        for (time, generator) in generators.iter_mut() {
            let combined_seed = generator.next_safe_uint64() ^ server_seed;
            if probe.matches(combined_seed) {
                trace!(
                    time_seed = *time,
                    depth, combined_seed, "session key seed recovered"
                );
                info!(
                    depth,
                    offset = *time - sent_time as i64,
                    "session key recovered"
                );
                return Some((*time as u64, new_key_from_seed(combined_seed)));
            }
        }
    }

    warn!(
        len = data.len(),
        "unable to find the session encryption key seed"
    );
    None
}

/// One pass of a late search: the time seeds of `window`, each at the draw
/// depths `depths`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatePass {
    pub window: LateWindow,
    pub depths: std::ops::Range<u32>,
}

/// Candidate time seeds per unit of work handed to a search thread.
const LATE_CHUNK_MS: u64 = 1_024;

/// Candidates whose first key word is computed together. Independent
/// Mersenne Twister chains in step keep the CPU busy where one chain waits on
/// each multiplication in turn.
const LANES: usize = 4;

/// The send times a late search tries as the first login's time seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LateWindow {
    pub from_ms: u64,
    pub to_ms: u64,
    /// Search from `to_ms` back: with no idea when the game started, the
    /// login is as likely anywhere, and a recent one is cheaper to find.
    /// Otherwise from `from_ms` on, because a window that opens at the game's
    /// start has its first login near there.
    pub newest_first: bool,
}

impl LateWindow {
    fn chunks(&self) -> u64 {
        if self.to_ms < self.from_ms {
            return 0;
        }
        (self.to_ms - self.from_ms) / LATE_CHUNK_MS + 1
    }

    /// The time seeds of chunk `index`, in search order.
    fn chunk(&self, index: u64) -> Box<dyn Iterator<Item = u64>> {
        let offset = index * LATE_CHUNK_MS;
        if self.newest_first {
            let hi = self.to_ms.saturating_sub(offset);
            let lo = hi.saturating_sub(LATE_CHUNK_MS - 1).max(self.from_ms);
            Box::new((lo..=hi).rev())
        } else {
            let lo = self.from_ms.saturating_add(offset);
            let hi = lo.saturating_add(LATE_CHUNK_MS - 1).min(self.to_ms);
            Box::new(lo..=hi)
        }
    }
}

/// Whether `data` decrypted with `key` is a whole run of game commands: each
/// one's magic bytes where its own lengths put them, the last ending exactly
/// at the end.
///
/// The magic-byte probe checks four bytes, enough for the few million
/// candidates of one [`bruteforce`], but a late search tries billions, where
/// four bytes alone would let a wrong key through now and then. The lengths
/// have to add up as well, which no wrong key does by chance.
fn decrypts_to_commands(key: &[u8], data: &[u8]) -> bool {
    let mut plain = data.to_vec();
    decrypt_command(key, &mut plain);
    let mut offset = 0usize;
    while offset < plain.len() {
        let rest = &plain[offset..];
        if rest.len() < 12 || rest[0] != 0x45 || rest[1] != 0x67 {
            return false;
        }
        let header_len = u16::from_be_bytes([rest[4], rest[5]]) as usize;
        let data_len = u32::from_be_bytes([rest[6], rest[7], rest[8], rest[9]]) as usize;
        let Some(end) = (10 + header_len).checked_add(data_len) else {
            return false;
        };
        if end + 2 > rest.len() || rest[end] != 0x89 || rest[end + 1] != 0xAB {
            return false;
        }
        offset += end + 2;
    }
    true
}

/// Find the session key of a login whose generator was seeded before the
/// capture saw anything: every time seed and draw depth of each pass in turn,
/// against each server seed, on `threads` threads.
///
/// [`bruteforce`] looks around the login's own send time, which is right for
/// the first login of a game process and wrong for every later one -- after
/// the title screen the game draws from the generator its first login seeded,
/// possibly hours earlier. When the capture started after that first login,
/// that seed was never seen, so it has to be searched for. Returns the time
/// seed (to keep for the next login) and the key.
pub fn late_search(
    passes: &[LatePass],
    server_seeds: &[u64],
    data: &[u8],
    threads: usize,
) -> Option<(u64, Vec<u8>)> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    let probe = MagicProbe::new(data)?;
    let found: Mutex<Option<(u64, Vec<u8>)>> = Mutex::new(None);
    let done = AtomicBool::new(false);

    // Whether `combined`, whose first key word already matched, is this
    // message's key; recorded when it is.
    let accept = |time: u64, combined: u64| {
        if !probe.matches_after_prefix(combined) {
            return false;
        }
        let key = new_key_from_seed(combined);
        if !decrypts_to_commands(&key, data) {
            return false;
        }
        trace!(time_seed = time, combined, "late search seed");
        done.store(true, Ordering::Relaxed);
        *found.lock().unwrap() = Some((time, key));
        true
    };

    for pass in passes {
        let chunks = pass.window.chunks();
        let next = AtomicU64::new(0);
        std::thread::scope(|scope| {
            for _ in 0..threads.max(1) {
                scope.spawn(|| {
                    // `(depth, combined seed)` of the candidates waiting for a
                    // full set of lanes. Never carried over to the next time
                    // seed, so a match is credited to its own.
                    let mut lanes = [(0u32, 0u64); LANES];
                    let mut filled = 0;
                    while !done.load(Ordering::Relaxed) {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= chunks {
                            return;
                        }
                        for time in pass.window.chunk(index) {
                            let mut generator = Random::seeded(time as i32);
                            for _ in 0..pass.depths.start {
                                generator.next_safe_uint64();
                            }
                            for depth in pass.depths.clone() {
                                let client_seed = generator.next_safe_uint64();
                                for &server_seed in server_seeds {
                                    lanes[filled] = (depth, client_seed ^ server_seed);
                                    filled += 1;
                                    if filled < LANES {
                                        continue;
                                    }
                                    filled = 0;
                                    let words = first_key_words(lanes.map(|(_, seed)| seed));
                                    for (&(depth, combined), word) in lanes.iter().zip(words) {
                                        if probe.prefix_matches(word) && accept(time, combined) {
                                            info!(
                                                depth,
                                                "session key recovered by the late search"
                                            );
                                            return;
                                        }
                                    }
                                }
                            }
                            for &(depth, combined) in &lanes[..filled] {
                                if probe.prefix_matches(first_key_word(combined))
                                    && accept(time, combined)
                                {
                                    info!(depth, "session key recovered by the late search");
                                    return;
                                }
                            }
                            filled = 0;
                        }
                    }
                });
            }
        });
        if done.load(Ordering::Relaxed) {
            break;
        }
    }
    found.into_inner().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first six `NextDouble()` draws of .NET Framework 4.8's
    /// `System.Random`, as raw IEEE-754 bit patterns, captured on Windows
    /// PowerShell 5.1 with:
    ///
    /// ```powershell
    /// $r = New-Object System.Random(<seed>)
    /// 1..6 | % { '0x{0:x16}' -f [BitConverter]::DoubleToInt64Bits($r.NextDouble()) }
    /// ```
    ///
    /// [`Random`] is a hand port of that generator and key recovery is worthless
    /// if it drifts, so this is the crate's reference oracle. Seeds 0 and
    /// `i32::MAX` diverge only at the third draw, which is exactly the kind of
    /// seeding bug a refactor would introduce.
    const DOTNET_NEXT_DOUBLE_BITS: &[(i32, [u64; 6])] = &[
        (
            0,
            [
                0x3fe73d6286ae7ac5,
                0x3fea278783344f0f,
                0x3fe893a451b12749,
                0x3fe1dc74dbe3b8ea,
                0x3fca5f4b5d34be97,
                0x3fe1e2625d63c4c5,
            ],
        ),
        (
            1,
            [
                0x3fcfd45f463fa8bf,
                0x3fbc59b7a038b36f,
                0x3fdde380c33bc702,
                0x3fe8b0fb20b161f6,
                0x3fe50a65102a14ca,
                0x3fdbb2b5cbb7656c,
            ],
        ),
        (
            12345,
            [
                0x3fb11653be222ca7,
                0x3fb1f5f93c23ebf2,
                0x3fe8cae040b195c1,
                0x3fe05b40bd60b681,
                0x3fe9850aeb730a16,
                0x3fea794f3cb4f29e,
            ],
        ),
        // `(i32)1_756_400_000_000`, i.e. a realistic `PacketHead::sent_ms`
        // truncated the way `Random::seeded` truncates it.
        (
            -241624064,
            [
                0x3fd1bbfb0d2377f6,
                0x3fe4e2e5dce9c5cc,
                0x3fe6dc71396db8e2,
                0x3fec1aec10f835d8,
                0x3fdf1e05bc3e3c0b,
                0x3fc0aae4a92155c9,
            ],
        ),
        (
            i32::MAX,
            [
                0x3fe73d6286ae7ac5,
                0x3fea278783344f0f,
                0x3fe893a453312749,
                0x3fe1dc74dbe3b8ea,
                0x3fca5f4b5d34be97,
                0x3fe1e2625ce3c4c5,
            ],
        ),
        (
            i32::MIN,
            [
                0x3fe73d6286ae7ac5,
                0x3fea278783344f0f,
                0x3fe893a453312749,
                0x3fe1dc74dbe3b8ea,
                0x3fca5f4b5d34be97,
                0x3fe1e2625ce3c4c5,
            ],
        ),
        (
            161803398,
            [
                0x3fe86ca5aaf0d94b,
                0x3fc705a3c02e0b48,
                0x3fe93b4972327693,
                0x3f786e1a2030dc34,
                0x3fa8e5951831cb2a,
                0x3fc7a9e90e2f53d2,
            ],
        ),
    ];

    /// `(ulong)(new Random(seed).NextDouble() * (double)ulong.MaxValue)` from
    /// the same .NET Framework 4.8 runtime, i.e. the value key recovery
    /// actually consumes.
    const DOTNET_NEXT_SAFE_UINT64: &[(i32, [u64; 4])] = &[
        (
            0,
            [
                13396823736352909312,
                15076991733327230976,
                14167517994065479680,
                10296256650306605056,
            ],
        ),
        (
            12345,
            [
                1231263624214652672,
                1294214504634905088,
                14291894165341865984,
                9428855276824692736,
            ],
        ),
        (
            -241624064,
            [
                5111563812551514112,
                12040143699782295552,
                13178528441429331968,
                16201524320674955264,
            ],
        ),
    ];

    #[test]
    fn cs_random_is_bit_exact_with_dotnet_system_random() {
        for (seed, expected) in DOTNET_NEXT_DOUBLE_BITS {
            let mut random = Random::seeded(*seed);
            for (draw, want) in expected.iter().enumerate() {
                let got = random.next_double().to_bits();
                assert_eq!(
                    got, *want,
                    "seed {seed}, draw {draw}: {got:#018x} != {want:#018x}"
                );
            }
        }
    }

    #[test]
    fn next_safe_uint64_matches_dotnet() {
        for (seed, expected) in DOTNET_NEXT_SAFE_UINT64 {
            let mut random = Random::seeded(*seed);
            for (draw, want) in expected.iter().enumerate() {
                assert_eq!(random.next_safe_uint64(), *want, "seed {seed}, draw {draw}");
            }
        }
    }

    #[test]
    fn cs_random_folds_negative_seeds_onto_their_magnitude() {
        // .NET seeds from `Math.Abs(seed)`, with `i32::MIN` special-cased to
        // `i32::MAX` because its magnitude is not representable.
        for (a, b) in [(-1, 1), (i32::MIN, i32::MAX), (-12345, 12345)] {
            let mut left = Random::seeded(a);
            let mut right = Random::seeded(b);
            for draw in 0..8 {
                assert_eq!(
                    left.next_safe_uint64(),
                    right.next_safe_uint64(),
                    "seeds {a}/{b}, draw {draw}"
                );
            }
        }
    }

    #[test]
    fn key_from_seed_is_4096_big_endian_bytes() {
        for seed in [0u64, 1, 0xdead_beef_cafe_f00d, u64::MAX] {
            let key = new_key_from_seed(seed);
            assert_eq!(key.len(), KEY_LEN);

            // Straightforward reference construction the packed version must
            // stay byte-identical to.
            let mut first = Mt64::new(seed);
            let mut generator = Mt64::new(first.next_u64());
            let _ = generator.next_u64();
            let mut want = Vec::new();
            for _ in 0..KEY_LEN / 8 {
                want.extend_from_slice(&generator.next_u64().to_be_bytes());
            }

            assert_eq!(key, want, "seed {seed}");
        }
    }

    /// A ciphertext whose magic bytes decrypt correctly under `key`.
    fn craft_command(key: &[u8], len: usize) -> Vec<u8> {
        assert!(len >= 4, "prefix and suffix must not overlap");
        let mut data = vec![0u8; len];
        data[0] = key[0] ^ 0x45;
        data[1] = key[1] ^ 0x67;
        data[len - 2] = key[(len - 2) % KEY_LEN] ^ 0x89;
        data[len - 1] = key[(len - 1) % KEY_LEN] ^ 0xAB;
        data
    }

    /// The `server_seed` that makes `combined` the `draw`-th client seed drawn
    /// from time seed `time`.
    fn plant_seed(time: i64, draw: usize, combined: u64) -> u64 {
        let mut generator = Random::seeded(time as i32);
        let mut client_seed = 0;
        for _ in 0..=draw {
            client_seed = generator.next_safe_uint64();
        }
        client_seed ^ combined
    }

    #[test]
    fn magic_probe_agrees_with_the_full_key() {
        let combined = 0x5ca1_ab1e_0000_beef;
        let key = new_key_from_seed(combined);

        // 4097 and 8193 put the trailing magic bytes at key indices 4095 and 0,
        // i.e. the suffix pair wraps around the end of the key and arrives out
        // of order.
        for len in [4usize, 5, 7, 8, 9, 100, 4095, 4096, 4097, 4098, 8193] {
            let data = craft_command(&key, len);
            let probe = MagicProbe::new(&data).expect("long enough");
            assert!(probe.matches(combined), "len {len} should match");
            assert!(!probe.matches(combined ^ 1), "len {len} should not match");
        }
    }

    #[test]
    fn bruteforce_recovers_a_key_at_zero_skew_and_zero_depth() {
        let combined = 0x0123_4567_89ab_cdef;
        let key = new_key_from_seed(combined);
        let sent_time = 1_756_400_000_000u64;
        let server_seed = plant_seed(sent_time as i64, 0, combined);
        let data = craft_command(&key, 1400);

        let (time, found) = bruteforce(sent_time, server_seed, data).expect("key is recoverable");
        assert_eq!(time, sent_time);
        assert_eq!(found, key);
    }

    #[test]
    fn bruteforce_recovers_a_key_at_positive_skew_and_nonzero_depth() {
        // +7 ms of clock skew, and the 4th client seed drawn from that time.
        // Depth-major iteration must still reach it.
        let combined = 0xfeed_face_dead_beef;
        let key = new_key_from_seed(combined);
        let sent_time = 1_756_400_000_000u64;
        let server_seed = plant_seed(sent_time as i64 + 7, 3, combined);
        let data = craft_command(&key, 231);

        let (time, found) = bruteforce(sent_time, server_seed, data).expect("key is recoverable");
        assert_eq!(time, sent_time + 7);
        assert_eq!(found, key);
    }

    #[test]
    fn bruteforce_searches_the_full_negative_skew_range() {
        // -1499 ms is the very last offset the schedule emits; this pins both
        // the range and the alternating +/- order.
        let combined = 0x1122_3344_5566_7788;
        let key = new_key_from_seed(combined);
        let sent_time = 1_756_400_000_000u64;
        let server_seed = plant_seed(sent_time as i64 - 1499, 0, combined);
        let data = craft_command(&key, 64);

        let (time, found) = bruteforce(sent_time, server_seed, data).expect("key is recoverable");
        assert_eq!(time, sent_time - 1499);
        assert_eq!(found, key);
    }

    #[test]
    fn bruteforce_handles_a_command_longer_than_the_key() {
        let combined = 0x00ff_00ff_00ff_00ff;
        let key = new_key_from_seed(combined);
        let sent_time = 42u64;
        let server_seed = plant_seed(sent_time as i64, 0, combined);
        let data = craft_command(&key, 4097);

        let (_, found) = bruteforce(sent_time, server_seed, data).expect("key is recoverable");
        assert_eq!(found, key);
    }

    #[test]
    fn guess_respects_its_depth_bound() {
        let combined = 0xabad_1dea_0000_0001;
        let key = new_key_from_seed(combined);
        let time = 1234i64;
        let server_seed = plant_seed(time, 5, combined);
        let data = craft_command(&key, 100);

        // The planted seed is the 6th draw, so depth 6 reaches it and 5 does not.
        assert_eq!(guess(time, server_seed, 6, &data), Some(key));
        assert_eq!(guess(time, server_seed, 5, &data), None);
    }

    #[test]
    fn the_early_mt_outputs_match_the_full_generator() {
        let mut seeds = Mt64::new(0x7e57);
        for seed in [0, 1, u64::MAX, 0x8000_0000_0000_0000]
            .into_iter()
            .chain((0..2_000).map(|_| seeds.next_u64()))
        {
            let mut full = Mt64::new(seed);
            let first = full.next_u64();
            let second = full.next_u64();
            assert_eq!(mt64_early_outputs([seed], 0), [first], "seed {seed}");
            assert_eq!(
                mt64_early_outputs([seed, !seed], 1)[0],
                second,
                "seed {seed}"
            );
            assert_eq!(
                first_key_word(seed).to_be_bytes(),
                new_key_from_seed(seed)[..8],
                "seed {seed}"
            );
        }
    }

    /// A real one-command message under `key`: magic, lengths, a header and
    /// payload, tail.
    fn framed_command(key: &[u8], payload_len: usize) -> Vec<u8> {
        let mut data = vec![0x45, 0x67, 0x12, 0x34, 0, 3];
        data.extend_from_slice(&(payload_len as u32).to_be_bytes());
        data.extend_from_slice(&[0x08, 0x01, 0x10]);
        data.extend((0..payload_len).map(|i| i as u8));
        data.extend_from_slice(&[0x89, 0xAB]);
        decrypt_command(key, &mut data);
        data
    }

    #[test]
    fn only_a_key_that_frames_the_whole_message_passes() {
        let key = new_key_from_seed(99);
        let data = framed_command(&key, 40);
        assert!(decrypts_to_commands(&key, &data));
        assert!(!decrypts_to_commands(&new_key_from_seed(98), &data));

        // Four magic bytes right but lengths that do not add up: what a wrong
        // key that slipped past the probe looks like.
        let crafted = craft_command(&key, 64);
        assert!(MagicProbe::new(&crafted).unwrap().matches(99));
        assert!(!decrypts_to_commands(&key, &crafted));

        // Two commands back to back are one message.
        let mut plain = framed_command(&key, 5);
        decrypt_command(&key, &mut plain);
        let mut two = plain.clone();
        two.extend_from_slice(&plain);
        decrypt_command(&key, &mut two);
        assert!(decrypts_to_commands(&key, &two));
    }

    fn pass(
        from_ms: u64,
        to_ms: u64,
        newest_first: bool,
        depths: std::ops::Range<u32>,
    ) -> LatePass {
        LatePass {
            window: LateWindow {
                from_ms,
                to_ms,
                newest_first,
            },
            depths,
        }
    }

    #[test]
    fn the_late_search_finds_a_login_seeded_long_before() {
        // The game's first login 20 minutes after it started, the capture
        // only seeing its third login after that (draw 3), an hour later.
        let started = 1_760_000_000_000u64;
        let first_login = started + 20 * 60_000 + 417;
        let sent = first_login + 3_600_000;
        let combined = 0x0bad_c0de_0bad_c0de;
        let key = new_key_from_seed(combined);
        let server_seed = plant_seed(first_login as i64, 3, combined);
        let data = framed_command(&key, 300);

        // Not what the search around the login's own time can reach.
        assert!(bruteforce(sent, server_seed, data.clone()).is_none());

        let from_start = pass(started, sent, false, 0..8);
        let (time, found) =
            late_search(&[from_start], &[1, server_seed], &data, 4).expect("recoverable");
        assert_eq!((time, &found), (first_login, &key));

        let back_from_now = pass(sent - 2 * 3_600_000, sent, true, 0..8);
        assert_eq!(
            late_search(&[back_from_now], &[server_seed], &data, 4),
            Some((first_login, key))
        );
    }

    #[test]
    fn the_late_search_reaches_its_deeper_band_and_stops_at_the_window() {
        let login = 5_000_000u64;
        let combined = 0x1357_9bdf_2468_ace0;
        let key = new_key_from_seed(combined);
        let server_seed = plant_seed(login as i64, 20, combined);
        let data = framed_command(&key, 12);
        let around = |from_ms, to_ms| {
            [
                pass(from_ms, to_ms, false, 0..8),
                pass(from_ms, to_ms, false, 8..32),
            ]
        };

        assert_eq!(
            late_search(
                &around(login - 3_000, login + 3_000),
                &[server_seed],
                &data,
                2
            ),
            Some((login, key))
        );
        assert_eq!(
            late_search(&around(login + 1, login + 3_000), &[server_seed], &data, 2),
            None
        );
        // Not in a pass whose depths stop short of it.
        assert_eq!(
            late_search(
                &[pass(login, login, false, 0..20)],
                &[server_seed],
                &data,
                2
            ),
            None
        );
        // One thread does the same work, and every lane count: one server
        // seed or three.
        assert!(late_search(&around(login, login), &[server_seed], &data, 1).is_some());
        assert!(late_search(&around(login, login), &[7, 8, server_seed], &data, 1).is_some());
    }

    #[test]
    fn late_window_chunks_cover_it_once() {
        for newest_first in [false, true] {
            let window = LateWindow {
                from_ms: 10,
                to_ms: 10 + 3 * LATE_CHUNK_MS + 5,
                newest_first,
            };
            let mut times: Vec<u64> = (0..window.chunks()).flat_map(|i| window.chunk(i)).collect();
            if newest_first {
                assert_eq!(times.first(), Some(&window.to_ms));
            } else {
                assert_eq!(times.first(), Some(&window.from_ms));
            }
            times.sort_unstable();
            times.dedup();
            assert_eq!(times, (window.from_ms..=window.to_ms).collect::<Vec<_>>());
        }
    }

    #[test]
    fn candidate_times_alternate_around_the_send_time() {
        let base = 1_000_000u64;
        let times: Vec<i64> = (0..8).map(|i| candidate_time(base, i)).collect();
        assert_eq!(
            times,
            vec![
                1_000_000, 1_000_000, 1_000_001, 999_999, 1_000_002, 999_998, 1_000_003, 999_997
            ]
        );

        // The schedule spans exactly +/- 1499 ms.
        let extremes: Vec<i64> = (0..TIME_CANDIDATES)
            .map(|i| candidate_time(base, i) - base as i64)
            .collect();
        assert_eq!(extremes.iter().copied().max(), Some(1499));
        assert_eq!(extremes.iter().copied().min(), Some(-1499));
    }

    #[test]
    fn candidate_time_does_not_overflow_on_an_extreme_send_time() {
        // `sent_ms` comes straight off the wire, so the offset must wrap rather
        // than panic in a debug build.
        for sent_time in [0u64, 1, u64::MAX, i64::MAX as u64, i64::MAX as u64 + 1] {
            for i in 0..TIME_CANDIDATES {
                let _ = candidate_time(sent_time, i);
            }
        }
    }

    #[test]
    fn short_commands_do_not_panic() {
        let keys = HashMap::new();
        for len in 0..MIN_COMMAND_LEN {
            let data = vec![0u8; len];
            assert!(lookup_initial_key(&keys, &data).is_none(), "len {len}");
            assert!(guess(0, 0, 4, &data).is_none(), "len {len}");
            assert!(bruteforce(0, 0, data).is_none(), "len {len}");
        }
    }

    #[test]
    fn decrypt_with_an_empty_key_is_a_no_op() {
        let mut data = vec![1u8, 2, 3, 4];
        decrypt_command(&[], &mut data);
        assert_eq!(data, [1, 2, 3, 4]);
    }

    #[test]
    fn decrypt_command_round_trips() {
        let key = new_key_from_seed(7);
        let plain = b"\x45\x67hello world\x89\xab".to_vec();
        let mut data = plain.clone();

        decrypt_command(&key, &mut data);
        assert_ne!(data, plain);
        decrypt_command(&key, &mut data);
        assert_eq!(data, plain);
    }

    #[test]
    fn lookup_initial_key_xors_the_version_field() {
        let mut keys = HashMap::new();
        keys.insert(0x1234u16, vec![9u8; 4]);

        let bytes = (0x1234u16 ^ 0x4567).to_be_bytes();
        assert_eq!(lookup_initial_key(&keys, &bytes), Some(vec![9u8; 4]));
        assert_eq!(lookup_initial_key(&keys, &[0, 0]), None);
    }
}
