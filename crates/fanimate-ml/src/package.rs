//! `.fnmodel` packages: a model, its licence and its manifest in one ZIP
//! archive, signed.
//!
//! ```text
//! model.json      the manifest (see manifest.rs)
//! model.sig       Ed25519 signature of model.json's bytes, 64 bytes
//! <weights>       the weights, named by the manifest
//! <licence file>  the licence text, named by the manifest
//! ```
//!
//! The signature covers the manifest, and the manifest holds the weights'
//! SHA-256 and size, so one signature vouches for every byte. Verifying
//! checks, in order: the archive reads and holds no name twice, the
//! manifest parses and passes the policy, the signature is from a trusted
//! key, the weights are the size and hash it says, and the licence file is
//! there. Any failure refuses the package; nothing is written or run.

use ed25519_compact::{KeyPair, PublicKey, Seed, Signature};
use fanimate_fnm::zip;
use sha2::{Digest, Sha256};

use crate::manifest::{MAX_MANIFEST_BYTES, ModelManifest, is_plain_name};

pub const MANIFEST_FILE: &str = "model.json";
pub const SIGNATURE_FILE: &str = "model.sig";

/// The largest package read, in bytes.
pub const MAX_PACKAGE_BYTES: usize = (1 << 30) + (8 << 20);

/// Why a package or an installed model was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("{0}")]
    Damaged(String),
    #[error("the model breaks the policy: {}", .0.join("; "))]
    Policy(Vec<String>),
    #[error("the signature is missing, damaged or not from a key Fanimate trusts")]
    Signature,
    #[error("the weights are not what the manifest says ({0})")]
    Weights(String),
    #[error("AI features are turned off, so no model is loaded")]
    AiOff,
    #[error("there is no model \"{0}\"")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
    #[error("no model runtime is installed, so {0} cannot run")]
    NoRuntime(String),
}

/// A package that passed every check.
#[derive(Debug)]
pub struct Verified {
    pub manifest: ModelManifest,
    /// The manifest's exact bytes (what was signed).
    pub manifest_bytes: Vec<u8>,
    pub signature: Vec<u8>,
    pub weights: Vec<u8>,
    pub licence: Vec<u8>,
}

/// The SHA-256 of `bytes` as lower case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether `signature` is a trusted key's signature of `bytes`.
pub fn signed_by(bytes: &[u8], signature: &[u8], trusted: &[[u8; 32]]) -> bool {
    let Ok(sig) = Signature::from_slice(signature) else {
        return false;
    };
    trusted
        .iter()
        .filter_map(|k| PublicKey::from_slice(k).ok())
        .any(|k| k.verify(bytes, &sig).is_ok())
}

/// Check a package from top to bottom (see the module docs).
pub fn verify(package: &[u8], trusted: &[[u8; 32]]) -> Result<Verified, ModelError> {
    if package.len() > MAX_PACKAGE_BYTES {
        return Err(ModelError::Damaged("the package is larger than 1 GiB".to_owned()));
    }
    let archive =
        zip::Archive::read(package).map_err(|e| ModelError::Damaged(format!("not a model package: {e}")))?;
    if let Some(name) = archive.duplicates().first() {
        return Err(ModelError::Damaged(format!("the package lists \"{name}\" twice")));
    }
    for name in archive.names() {
        if !name.ends_with('/') && !is_plain_name(name) {
            return Err(ModelError::Damaged(format!(
                "the package holds a file named \"{name}\", which is refused"
            )));
        }
    }
    let read = |name: &str| -> Result<Vec<u8>, ModelError> {
        archive
            .get_checked(name)
            .ok_or_else(|| ModelError::Damaged(format!("the package has no {name}")))?
            .map_err(|e| ModelError::Damaged(format!("{name}: {e}")))
    };
    let manifest_bytes = read(MANIFEST_FILE)?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ModelError::Damaged(
            "the manifest is larger than 64 KB".to_owned(),
        ));
    }
    let manifest = ModelManifest::parse(&manifest_bytes).map_err(ModelError::Damaged)?;
    let problems = manifest.check();
    if !problems.is_empty() {
        return Err(ModelError::Policy(problems));
    }
    let signature = read(SIGNATURE_FILE).map_err(|_| ModelError::Signature)?;
    if !signed_by(&manifest_bytes, &signature, trusted) {
        return Err(ModelError::Signature);
    }
    // the size is known before the weights are unpacked, so a lying archive cannot ask for more
    match archive.size(&manifest.weights) {
        Some(n) if n as u64 == manifest.size => {}
        Some(n) => return Err(ModelError::Weights(format!("{n} bytes, not {}", manifest.size))),
        None => {
            return Err(ModelError::Damaged(format!(
                "the package has no {}",
                manifest.weights
            )));
        }
    }
    let weights = read(&manifest.weights)?;
    let found = sha256_hex(&weights);
    if found != manifest.sha256 {
        return Err(ModelError::Weights(format!(
            "SHA-256 is {found}, not {}",
            manifest.sha256
        )));
    }
    let licence = read(&manifest.licence_file)?;
    if licence.iter().all(u8::is_ascii_whitespace) {
        return Err(ModelError::Damaged("the licence file is empty".to_owned()));
    }
    Ok(Verified {
        manifest,
        manifest_bytes,
        signature,
        weights,
        licence,
    })
}

/// Make a signed package: for the release process and for tests. `seed` is
/// the 32 bytes the signing key is made from; the key itself is never kept
/// here. The manifest's `size` and `sha256` are set from `weights`, so they
/// cannot be wrong.
pub fn pack(
    mut manifest: ModelManifest,
    weights: &[u8],
    licence: &[u8],
    seed: &[u8; 32],
) -> Result<Vec<u8>, ModelError> {
    manifest.size = weights.len() as u64;
    manifest.sha256 = sha256_hex(weights);
    let problems = manifest.check();
    if !problems.is_empty() {
        return Err(ModelError::Policy(problems));
    }
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| ModelError::Damaged(e.to_string()))?;
    let key = KeyPair::from_seed(Seed::new(*seed));
    let signature = key.sk.sign(&manifest_bytes, None);
    zip::write(&[
        (MANIFEST_FILE, &manifest_bytes),
        (SIGNATURE_FILE, signature.as_ref()),
        (&manifest.weights, weights),
        (&manifest.licence_file, licence),
    ])
    .map_err(ModelError::Damaged)
}

/// The 32-byte seed in `text`, standard base64 (the form of the
/// `UPDATE_SIGNING_KEY` secret that signs updates).
pub fn seed_from_base64(text: &str) -> Option<[u8; 32]> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::with_capacity(33);
    for c in text.trim().trim_start_matches('﻿').trim().bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(v);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out.try_into().ok()
}

/// A 32-byte key as 64 lower case hex digits.
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The key in 64 hex digits, if it is one.
pub fn key_from_hex(text: &str) -> Option<[u8; 32]> {
    let t = text.trim();
    if t.len() != 64 || !t.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&t[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// The public half of the key `seed` makes.
pub fn public_key(seed: &[u8; 32]) -> [u8; 32] {
    *KeyPair::from_seed(Seed::new(*seed)).pk
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::manifest::sound_manifest;

    pub(crate) const SEED: [u8; 32] = [7; 32];
    pub(crate) const WEIGHTS: &[u8] = b"pretend these are the weights of a very small model";

    pub(crate) fn package() -> Vec<u8> {
        pack(sound_manifest(), WEIGHTS, b"Apache License 2.0 ...", &SEED).unwrap()
    }

    #[test]
    fn a_sound_package_verifies_and_gives_back_what_was_packed() {
        let v = verify(&package(), &[public_key(&SEED)]).unwrap();
        assert_eq!(v.weights, WEIGHTS);
        assert_eq!(v.manifest.size, WEIGHTS.len() as u64);
        assert_eq!(v.manifest.sha256, sha256_hex(WEIGHTS));
        assert_eq!(v.manifest.id, "line-cleanup-1");
    }

    #[test]
    fn keys_and_seeds_are_read_from_their_text_forms() {
        let key = public_key(&SEED);
        assert_eq!(key_from_hex(&hex(&key)), Some(key));
        assert_eq!(key_from_hex(&format!(" {} ", hex(&key))), Some(key));
        for bad in [
            "",
            "zz",
            &"g".repeat(64),
            &"a".repeat(63),
            "é".repeat(32).as_str(),
        ] {
            assert_eq!(key_from_hex(bad), None, "{bad:?}");
        }
        // 32 bytes of 0x07 in base64, as the signing secret is written
        assert_eq!(
            seed_from_base64("BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc="),
            Some(SEED)
        );
        assert_eq!(
            seed_from_base64(
                "﻿BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=
"
            ),
            Some(SEED)
        );
        for bad in [
            "",
            "not base64!",
            "AAAA",
            "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
        ] {
            assert_eq!(seed_from_base64(bad), None, "{bad}");
        }
    }

    #[test]
    fn sha256_matches_the_standard_test_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_package_signed_by_a_key_nobody_trusts_is_refused() {
        let other = public_key(&[9; 32]);
        assert_eq!(verify(&package(), &[other]).unwrap_err(), ModelError::Signature);
        assert_eq!(verify(&package(), &[]).unwrap_err(), ModelError::Signature);
    }

    #[test]
    fn changed_weights_a_changed_manifest_or_a_stripped_signature_are_refused() {
        let trusted = [public_key(&SEED)];
        // weights swapped after signing
        let good = verify(&package(), &trusted).unwrap();
        let evil = zip::write(&[
            (MANIFEST_FILE, &good.manifest_bytes),
            (SIGNATURE_FILE, &good.signature),
            (
                "model.onnx",
                b"pretend these are the weights of a very small model!!",
            ),
            ("LICENSE.txt", &good.licence),
        ])
        .unwrap();
        assert!(matches!(verify(&evil, &trusted), Err(ModelError::Weights(_))));
        // same length, different bytes: the hash catches it
        let mut swapped = WEIGHTS.to_vec();
        swapped[0] ^= 1;
        let evil = zip::write(&[
            (MANIFEST_FILE, &good.manifest_bytes),
            (SIGNATURE_FILE, &good.signature),
            ("model.onnx", &swapped),
            ("LICENSE.txt", &good.licence),
        ])
        .unwrap();
        let why = verify(&evil, &trusted).unwrap_err().to_string();
        assert!(why.contains("SHA-256"), "{why}");
        // manifest edited (a friendlier licence written in): the signature no longer matches
        let text = String::from_utf8(good.manifest_bytes.clone())
            .unwrap()
            .replace("Apache-2.0", "MIT");
        let evil = zip::write(&[
            (MANIFEST_FILE, text.as_bytes()),
            (SIGNATURE_FILE, &good.signature),
            ("model.onnx", WEIGHTS),
            ("LICENSE.txt", &good.licence),
        ])
        .unwrap();
        assert_eq!(verify(&evil, &trusted).unwrap_err(), ModelError::Signature);
        // no signature at all
        let bare = zip::write(&[
            (MANIFEST_FILE, &good.manifest_bytes),
            ("model.onnx", WEIGHTS),
            ("LICENSE.txt", &good.licence),
        ])
        .unwrap();
        assert_eq!(verify(&bare, &trusted).unwrap_err(), ModelError::Signature);
    }

    #[test]
    fn a_package_that_breaks_the_policy_cannot_be_packed_and_one_signed_anyway_is_still_refused() {
        let mut m = sound_manifest();
        m.licence = "CC-BY-NC-4.0".into();
        assert!(matches!(
            pack(m.clone(), WEIGHTS, b"licence", &SEED),
            Err(ModelError::Policy(_))
        ));
        // someone who signs a bad manifest by hand still cannot get it past verification
        m.size = WEIGHTS.len() as u64;
        m.sha256 = sha256_hex(WEIGHTS);
        let manifest = serde_json::to_vec(&m).unwrap();
        let sig = KeyPair::from_seed(Seed::new(SEED)).sk.sign(&manifest, None);
        let zipped = zip::write(&[
            (MANIFEST_FILE, &manifest),
            (SIGNATURE_FILE, sig.as_ref()),
            ("model.onnx", WEIGHTS),
            ("LICENSE.txt", b"licence"),
        ])
        .unwrap();
        let why = verify(&zipped, &[public_key(&SEED)]).unwrap_err().to_string();
        assert!(why.contains("lawyer"), "{why}");
    }

    #[test]
    fn damaged_hostile_and_incomplete_packages_are_refused_without_a_panic() {
        let trusted = [public_key(&SEED)];
        for bytes in [&b""[..], b"not a zip", &package()[..40], &vec![0u8; 500]] {
            assert!(verify(bytes, &trusted).is_err());
        }
        // a name that climbs out, a name listed twice, a missing licence, an empty licence
        let good = verify(&package(), &trusted).unwrap();
        let with = |extra: (&str, &[u8])| {
            zip::write(&[
                (MANIFEST_FILE, &good.manifest_bytes),
                (SIGNATURE_FILE, &good.signature),
                ("model.onnx", WEIGHTS),
                ("LICENSE.txt", &good.licence),
                extra,
            ])
            .unwrap()
        };
        assert!(
            verify(&with(("../x", b"1")), &trusted)
                .unwrap_err()
                .to_string()
                .contains("refused")
        );
        assert!(
            verify(&with(("model.onnx", b"2")), &trusted)
                .unwrap_err()
                .to_string()
                .contains("twice")
        );
        let no_licence = zip::write(&[
            (MANIFEST_FILE, &good.manifest_bytes),
            (SIGNATURE_FILE, &good.signature),
            ("model.onnx", WEIGHTS),
        ])
        .unwrap();
        assert!(
            verify(&no_licence, &trusted)
                .unwrap_err()
                .to_string()
                .contains("LICENSE.txt")
        );
        let blank = zip::write(&[
            (MANIFEST_FILE, &good.manifest_bytes),
            (SIGNATURE_FILE, &good.signature),
            ("model.onnx", WEIGHTS),
            ("LICENSE.txt", b"   \n"),
        ])
        .unwrap();
        assert!(
            verify(&blank, &trusted)
                .unwrap_err()
                .to_string()
                .contains("empty")
        );
    }
}
