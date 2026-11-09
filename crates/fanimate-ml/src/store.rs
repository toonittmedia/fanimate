//! The models folder: install a verified package, list what is installed,
//! remove one, and load one for use.
//!
//! The store never trusts what it wrote: [`ModelStore::load`] checks the
//! signature and the weights' hash again each time, so a file changed on disk
//! after installing is refused, and it loads nothing at all while AI features
//! are off. Installing writes into a hidden staging folder and moves it into
//! place only when everything was written, so a failed install leaves what
//! was there untouched.

use std::path::{Path, PathBuf};

use crate::manifest::ModelManifest;
use crate::package::{MANIFEST_FILE, ModelError, SIGNATURE_FILE, sha256_hex, signed_by, verify};

/// An installed model.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub manifest: ModelManifest,
    pub dir: PathBuf,
}

/// A model read back for use: its manifest and checked weights.
#[derive(Debug)]
pub struct Loaded {
    pub manifest: ModelManifest,
    pub weights: Vec<u8>,
}

/// The user's models folder: `$FANIMATE_MODELS_DIR`, else `models` in the data
/// folder (`$FANIMATE_DATA_DIR`, or the system's place for Fanimate's data, the
/// one that holds the settings). `None` when the system names no such folder.
pub fn user_folder() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("FANIMATE_MODELS_DIR") {
        return Some(dir);
    }
    if let Some(dir) = env("FANIMATE_DATA_DIR") {
        return Some(dir.join("models"));
    }
    let data = if cfg!(windows) {
        env("APPDATA")?.join("Fanimate").join("data")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/Fanimate")
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".local/share")))?
            .join("fanimate")
    };
    Some(data.join("models"))
}

pub struct ModelStore {
    dir: PathBuf,
}

fn io(path: &Path, e: std::io::Error) -> ModelError {
    ModelError::Io(format!("{}: {e}", path.display()))
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Verify `package` against `trusted` keys and install it, replacing a
    /// model with the same id.
    pub fn install(&self, package: &[u8], trusted: &[[u8; 32]]) -> Result<Installed, ModelError> {
        let v = verify(package, trusted)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| io(&self.dir, e))?;
        let staging = self.dir.join(format!(".installing-{}", std::process::id()));
        std::fs::remove_dir_all(&staging).ok();
        std::fs::create_dir_all(&staging).map_err(|e| io(&staging, e))?;
        let result = (|| {
            for (name, bytes) in [
                (MANIFEST_FILE, v.manifest_bytes.as_slice()),
                (SIGNATURE_FILE, v.signature.as_slice()),
                (v.manifest.weights.as_str(), v.weights.as_slice()),
                (v.manifest.licence_file.as_str(), v.licence.as_slice()),
            ] {
                let path = staging.join(name);
                std::fs::write(&path, bytes).map_err(|e| io(&path, e))?;
            }
            let target = self.dir.join(&v.manifest.id);
            if target.exists() {
                let old = self.dir.join(format!(".old-{}", std::process::id()));
                std::fs::remove_dir_all(&old).ok();
                std::fs::rename(&target, &old).map_err(|e| io(&target, e))?;
                if let Err(e) = std::fs::rename(&staging, &target) {
                    // put the old one back: an install never loses what was there
                    std::fs::rename(&old, &target).ok();
                    return Err(io(&target, e));
                }
                std::fs::remove_dir_all(&old).ok();
            } else {
                std::fs::rename(&staging, &target).map_err(|e| io(&target, e))?;
            }
            Ok(Installed {
                manifest: v.manifest.clone(),
                dir: target,
            })
        })();
        std::fs::remove_dir_all(&staging).ok();
        result
    }

    /// The installed models, by id. A folder that is not a readable model is skipped.
    pub fn list(&self) -> Vec<Installed> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut found: Vec<Installed> = entries
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| {
                let dir = e.path();
                let bytes = std::fs::read(dir.join(MANIFEST_FILE)).ok()?;
                let manifest = ModelManifest::parse(&bytes).ok()?;
                (dir.file_name()
                    .is_some_and(|n| n.to_string_lossy() == manifest.id))
                .then_some(Installed { manifest, dir })
            })
            .collect();
        found.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        found
    }

    pub fn remove(&self, id: &str) -> Result<(), ModelError> {
        if !crate::manifest::is_valid_id(id) {
            return Err(ModelError::NotFound(id.to_owned()));
        }
        let dir = self.dir.join(id);
        if !dir.join(MANIFEST_FILE).is_file() {
            return Err(ModelError::NotFound(id.to_owned()));
        }
        std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))
    }

    /// Read model `id` for use. Nothing loads while `ai_allowed` is false
    /// (the user's "turn off all AI features", or a policy file), and a model
    /// is only returned after its manifest passes the policy, its signature
    /// is from a trusted key and its weights are the size and hash it says.
    pub fn load(&self, id: &str, trusted: &[[u8; 32]], ai_allowed: bool) -> Result<Loaded, ModelError> {
        if !ai_allowed {
            return Err(ModelError::AiOff);
        }
        if !crate::manifest::is_valid_id(id) {
            return Err(ModelError::NotFound(id.to_owned()));
        }
        let dir = self.dir.join(id);
        let read =
            |name: &str| std::fs::read(dir.join(name)).map_err(|_| ModelError::NotFound(id.to_owned()));
        let manifest_bytes = read(MANIFEST_FILE)?;
        let manifest = ModelManifest::parse(&manifest_bytes).map_err(ModelError::Damaged)?;
        if manifest.id != id {
            return Err(ModelError::Damaged(
                "the folder and the manifest name different models".to_owned(),
            ));
        }
        let problems = manifest.check();
        if !problems.is_empty() {
            return Err(ModelError::Policy(problems));
        }
        let signature = read(SIGNATURE_FILE).map_err(|_| ModelError::Signature)?;
        if !signed_by(&manifest_bytes, &signature, trusted) {
            return Err(ModelError::Signature);
        }
        let path = dir.join(&manifest.weights);
        let size = std::fs::metadata(&path).map_err(|e| io(&path, e))?.len();
        if size != manifest.size {
            return Err(ModelError::Weights(format!(
                "{size} bytes, not {}",
                manifest.size
            )));
        }
        let weights = std::fs::read(&path).map_err(|e| io(&path, e))?;
        let found = sha256_hex(&weights);
        if found != manifest.sha256 {
            return Err(ModelError::Weights(format!(
                "SHA-256 is {found}, not {}",
                manifest.sha256
            )));
        }
        Ok(Loaded { manifest, weights })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::tests::{SEED, WEIGHTS, package};
    use crate::package::{pack, public_key};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-ml-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn a_package_installs_lists_loads_and_removes() {
        let dir = temp("round");
        let store = ModelStore::new(&dir);
        let trusted = [public_key(&SEED)];
        assert!(store.list().is_empty());
        let installed = store.install(&package(), &trusted).unwrap();
        assert_eq!(installed.manifest.id, "line-cleanup-1");
        assert_eq!(store.list().len(), 1);
        let loaded = store.load("line-cleanup-1", &trusted, true).unwrap();
        assert_eq!(loaded.weights, WEIGHTS);
        // installing again replaces it and leaves nothing behind
        store.install(&package(), &trusted).unwrap();
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(names.len(), 1, "{names:?}");
        store.remove("line-cleanup-1").unwrap();
        assert!(store.list().is_empty());
        assert_eq!(
            store.remove("line-cleanup-1").unwrap_err(),
            ModelError::NotFound("line-cleanup-1".into())
        );
        assert!(store.remove("../escape").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nothing_loads_while_ai_is_off_and_a_refused_package_installs_nothing() {
        let dir = temp("off");
        let store = ModelStore::new(&dir);
        let trusted = [public_key(&SEED)];
        store.install(&package(), &trusted).unwrap();
        assert_eq!(
            store.load("line-cleanup-1", &trusted, false).unwrap_err(),
            ModelError::AiOff
        );
        // a package from an untrusted key, and one that breaks the policy, leave the store as it was
        let other = pack(crate::manifest::sound_manifest(), WEIGHTS, b"licence", &[3; 32]).unwrap();
        assert_eq!(
            store.install(&other, &trusted).unwrap_err(),
            ModelError::Signature
        );
        assert_eq!(store.list().len(), 1);
        assert!(
            store.load("line-cleanup-1", &trusted, true).is_ok(),
            "the first model is untouched"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn weights_or_manifest_changed_on_disk_after_installing_are_refused_at_load() {
        let dir = temp("tamper");
        let store = ModelStore::new(&dir);
        let trusted = [public_key(&SEED)];
        store.install(&package(), &trusted).unwrap();
        let weights = dir.join("line-cleanup-1").join("model.onnx");
        // same size, one bit different
        let mut bytes = std::fs::read(&weights).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&weights, &bytes).unwrap();
        assert!(matches!(
            store.load("line-cleanup-1", &trusted, true),
            Err(ModelError::Weights(_))
        ));
        // truncated
        std::fs::write(&weights, b"short").unwrap();
        assert!(matches!(
            store.load("line-cleanup-1", &trusted, true),
            Err(ModelError::Weights(_))
        ));
        // a manifest rewritten to say another licence
        store.install(&package(), &trusted).unwrap();
        let manifest = dir.join("line-cleanup-1").join("model.json");
        let text = std::fs::read_to_string(&manifest)
            .unwrap()
            .replace("Apache-2.0", "MIT");
        std::fs::write(&manifest, text).unwrap();
        assert_eq!(
            store.load("line-cleanup-1", &trusted, true).unwrap_err(),
            ModelError::Signature
        );
        // a signature file removed
        store.install(&package(), &trusted).unwrap();
        std::fs::remove_file(dir.join("line-cleanup-1").join("model.sig")).unwrap();
        assert_eq!(
            store.load("line-cleanup-1", &trusted, true).unwrap_err(),
            ModelError::Signature
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn junk_in_the_folder_is_skipped_and_odd_ids_are_not_found() {
        let dir = temp("junk");
        std::fs::create_dir_all(dir.join("not-a-model")).unwrap();
        std::fs::write(dir.join("stray.txt"), "x").unwrap();
        std::fs::create_dir_all(dir.join("bad")).unwrap();
        std::fs::write(dir.join("bad").join("model.json"), "{ nope").unwrap();
        let store = ModelStore::new(&dir);
        assert!(store.list().is_empty());
        for id in ["", "../x", "A", "no-such-model"] {
            assert!(
                matches!(store.load(id, &[], true), Err(ModelError::NotFound(_))),
                "{id}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
