// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Checking a downloaded update before it is installed.
//!
//! A release carries `SHA256SUMS.txt` (one line per file) and
//! `SHA256SUMS.txt.minisig`, a minisign signature over it made by the
//! release workflow as its last step. An update installs only when:
//!
//! 1. the signature over `SHA256SUMS.txt` is good for one of [`UPDATE_KEYS`];
//! 2. the signature's trusted comment names the release's version
//!    ([`trusted_comment`]), so a signed list of an older release cannot be
//!    served for a newer one;
//! 3. `SHA256SUMS.txt` has a line for the file;
//! 4. the downloaded file's SHA-256 is that line's.
//!
//! [`expected_hash`] does 1 to 3 and answers the hash 4 compares against;
//! the desktop and the Android app (through its native library) share it.
//! A release without a signature is refused: there is no unsigned path.
//!
//! By hand: `minisign -Vm SHA256SUMS.txt -P <key>`, then `sha256sum -c
//! SHA256SUMS.txt`.

use minisign_verify::{PublicKey, Signature};

/// The minisign public keys updates are signed with (base64, the second
/// line of a `minisign.pub`). A list, so a new key can ship a release ahead
/// of retiring the old one. The secret key lives only in the repository's
/// main-only `release` environment.
pub const UPDATE_KEYS: &[&str] = &[
    // Key A4685756A7B45C5C, made 2026-11-18 (the first: releases from 1.18.0).
    "RWRcXLSnVldopL8tuyDx/JECEMOE+3plsO4wUWOSdc1WVf3TwngzoX+I",
];

/// Why an update was refused. The apps show one message for all of them
/// (the user's choice); the log names the cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The release has no `SHA256SUMS.txt.minisig`, or it is not a minisign
    /// signature.
    NoSignature,
    /// The signature is not one of our keys' over this `SHA256SUMS.txt`.
    BadSignature,
    /// The signature is for another version's list.
    OtherVersion,
    /// `SHA256SUMS.txt` has no line for the file.
    NotListed,
    /// The file's SHA-256 is not the listed one.
    HashDiffers,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSignature => "the release is not signed",
            Self::BadSignature => "SHA256SUMS.txt did not pass the signature check",
            Self::OtherVersion => "the signature is for another version",
            Self::NotListed => "the file is not listed in SHA256SUMS.txt",
            Self::HashDiffers => "the download does not match its checksum",
        })
    }
}

impl std::error::Error for Refused {}

/// The trusted comment the release workflow signs `version`'s list with.
pub fn trusted_comment(version: &str) -> String {
    format!("teyvat.me-capture {}", version.trim_start_matches('v'))
}

/// The SHA-256 (lowercase hex) `sums` lists for `asset`, once `signature`
/// (the `.minisig` file's text; `None` when the release has none) proves
/// `sums` is `version`'s list, signed by one of `keys`.
pub fn expected_hash(
    sums: &[u8],
    signature: Option<&str>,
    version: &str,
    asset: &str,
    keys: &[&str],
) -> Result<String, Refused> {
    let signature = signature
        .and_then(|text| Signature::decode(text).ok())
        .ok_or(Refused::NoSignature)?;
    let signed = keys.iter().any(|key| {
        PublicKey::from_base64(key).is_ok_and(|key| key.verify(sums, &signature, false).is_ok())
    });
    if !signed {
        return Err(Refused::BadSignature);
    }
    // The trusted comment is covered by the signature's global part, which
    // `verify` checked.
    if signature.trusted_comment() != trusted_comment(version) {
        return Err(Refused::OtherVersion);
    }
    listed_hash(sums, asset).ok_or(Refused::NotListed)
}

/// `asset`'s hash in a `sha256sum` list (`<hex>  <name>`, the name with `*`
/// in front for binary mode).
fn listed_hash(sums: &[u8], asset: &str) -> Option<String> {
    let text = std::str::from_utf8(sums).ok()?;
    text.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        let hex = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        (name == asset && hex).then(|| hash.to_ascii_lowercase())
    })
}

/// Whether a file whose SHA-256 is `actual` (hex, any case) is the one
/// [`expected_hash`] answered.
pub fn hash_matches(expected: &str, actual: &str) -> Result<(), Refused> {
    if expected.eq_ignore_ascii_case(actual.trim()) {
        Ok(())
    } else {
        Err(Refused::HashDiffers)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use minisign::KeyPair;

    use super::*;

    const ASSET: &str = "teyvatme-windows.exe";
    const HASH: &str = "2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae";

    fn sums() -> Vec<u8> {
        format!(
            "{HASH}  {ASSET}\n\
             fcde2b2edba56bf408601fb721fe9b5c338d10ee429ea04fae5511b68fbf8fb9 *teyvatme-android.apk\n"
        )
        .into_bytes()
    }

    /// A key pair made here, and `data` signed for `version` with it.
    fn signed(data: &[u8], version: &str) -> (String, String) {
        let KeyPair { pk, sk } = KeyPair::generate_unencrypted_keypair().unwrap();
        let comment = trusted_comment(version);
        let signature =
            minisign::sign(Some(&pk), &sk, Cursor::new(data), Some(&comment), None).unwrap();
        (pk.to_base64(), signature.to_string())
    }

    #[test]
    fn a_signed_list_gives_the_files_hash() {
        let (key, signature) = signed(&sums(), "1.18.1");
        let other = signed(b"x", "1.18.1").0;
        // Any of the keys compiled in will do (a new key ships a release
        // ahead of the old one's retirement).
        for keys in [vec![key.as_str()], vec![other.as_str(), key.as_str()]] {
            assert_eq!(
                expected_hash(&sums(), Some(&signature), "1.18.1", ASSET, &keys),
                Ok(HASH.to_string())
            );
        }
        // The tag's `v` is not part of the version.
        assert_eq!(
            expected_hash(&sums(), Some(&signature), "v1.18.1", ASSET, &[&key]),
            Ok(HASH.to_string())
        );
        // The binary-mode star is not part of the name.
        assert!(
            expected_hash(
                &sums(),
                Some(&signature),
                "1.18.1",
                "teyvatme-android.apk",
                &[&key]
            )
            .is_ok()
        );
    }

    #[test]
    fn a_release_without_a_signature_is_refused() {
        let (key, _) = signed(&sums(), "1.18.1");
        assert_eq!(
            expected_hash(&sums(), None, "1.18.1", ASSET, &[&key]),
            Err(Refused::NoSignature)
        );
        assert_eq!(
            expected_hash(
                &sums(),
                Some("<html>Not Found</html>"),
                "1.18.1",
                ASSET,
                &[&key]
            ),
            Err(Refused::NoSignature)
        );
    }

    #[test]
    fn a_changed_list_or_another_key_is_refused() {
        let (key, signature) = signed(&sums(), "1.18.1");
        // One hash swapped for another file's.
        let tampered = String::from_utf8(sums())
            .unwrap()
            .replace(&HASH[..8], "00000000");
        assert_eq!(
            expected_hash(
                tampered.as_bytes(),
                Some(&signature),
                "1.18.1",
                ASSET,
                &[&key]
            ),
            Err(Refused::BadSignature)
        );
        // Signed, but not by us.
        let (_, foreign) = signed(&sums(), "1.18.1");
        assert_eq!(
            expected_hash(&sums(), Some(&foreign), "1.18.1", ASSET, &[&key]),
            Err(Refused::BadSignature)
        );
        // No key compiled in: nothing verifies.
        assert_eq!(
            expected_hash(&sums(), Some(&signature), "1.18.1", ASSET, &[]),
            Err(Refused::BadSignature)
        );
    }

    #[test]
    fn an_older_releases_signed_list_is_refused_for_a_newer_one() {
        let (key, signature) = signed(&sums(), "1.18.0");
        assert_eq!(
            expected_hash(&sums(), Some(&signature), "1.18.1", ASSET, &[&key]),
            Err(Refused::OtherVersion)
        );
    }

    #[test]
    fn a_file_the_list_lacks_is_refused() {
        let (key, signature) = signed(&sums(), "1.18.1");
        assert_eq!(
            expected_hash(
                &sums(),
                Some(&signature),
                "1.18.1",
                "teyvatme-linux-x86_64.tar.gz",
                &[&key]
            ),
            Err(Refused::NotListed)
        );
    }

    #[test]
    fn the_download_must_have_the_listed_hash() {
        assert_eq!(hash_matches(HASH, &HASH.to_uppercase()), Ok(()));
        assert_eq!(
            hash_matches(
                HASH,
                "fcde2b2edba56bf408601fb721fe9b5c338d10ee429ea04fae5511b68fbf8fb9"
            ),
            Err(Refused::HashDiffers)
        );
    }

    #[test]
    fn every_compiled_key_parses() {
        for key in UPDATE_KEYS {
            assert!(
                PublicKey::from_base64(key).is_ok(),
                "{key} is not a minisign key"
            );
        }
    }
}
