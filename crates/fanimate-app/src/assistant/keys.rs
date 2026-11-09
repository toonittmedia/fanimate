//! API keys. They live in their own file in the user's data folder, never in
//! the settings that are saved with the window layout, in a project, in a log
//! or on a command line, and an environment variable (`ANTHROPIC_API_KEY` and
//! the like) always wins, so a studio can supply keys without Fanimate saving
//! any.
//!
//! The file is plain text readable by the user's account only (mode 0600 where
//! the system has modes; on Windows the folder is the user's own). It is the
//! same protection `gh`, `aws` and most command-line tools give a token. Using
//! the operating system's credential store instead is a planned change.

use std::path::PathBuf;

use serde_json::{Map, Value};

use super::ProviderKind;

const FILE: &str = "ai-keys.json";

/// Where a key came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    None,
    /// An environment variable.
    Variable,
    /// Saved by the user in Fanimate.
    Saved,
}

pub struct Keys {
    dir: Option<PathBuf>,
}

fn name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenRouter => "openrouter",
        ProviderKind::Custom => "custom",
    }
}

impl Keys {
    /// The user's data folder.
    pub fn new() -> Self {
        Self {
            dir: crate::data::dir(),
        }
    }

    #[cfg(test)]
    fn in_folder(dir: PathBuf) -> Self {
        Self { dir: Some(dir) }
    }

    fn read(&self) -> Map<String, Value> {
        let Some(dir) = &self.dir else {
            return Map::new();
        };
        std::fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn saved(&self, kind: ProviderKind) -> Option<String> {
        self.read()
            .get(name(kind))
            .and_then(Value::as_str)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
    }

    /// The key for a provider: the environment variable if set, else the saved one.
    pub fn get(&self, kind: ProviderKind) -> Option<String> {
        std::env::var(kind.key_variable())
            .ok()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| self.saved(kind))
    }

    pub fn source(&self, kind: ProviderKind) -> Source {
        if std::env::var(kind.key_variable()).is_ok_and(|k| !k.trim().is_empty()) {
            Source::Variable
        } else if self.saved(kind).is_some() {
            Source::Saved
        } else {
            Source::None
        }
    }

    /// Save a key, replacing any saved one.
    pub fn save(&self, kind: ProviderKind, key: &str) -> std::io::Result<()> {
        let key = key.trim();
        if key.is_empty() || key.contains(['\n', '\r']) {
            return Err(std::io::Error::other("that is not a key"));
        }
        let mut all = self.read();
        all.insert(name(kind).to_owned(), Value::String(key.to_owned()));
        self.write(&all)
    }

    /// Forget a saved key.
    pub fn forget(&self, kind: ProviderKind) -> std::io::Result<()> {
        let mut all = self.read();
        if all.remove(name(kind)).is_some() {
            self.write(&all)?;
        }
        Ok(())
    }

    fn write(&self, all: &Map<String, Value>) -> std::io::Result<()> {
        let dir = self
            .dir
            .as_ref()
            .ok_or_else(|| std::io::Error::other("there is no folder to keep the key in"))?;
        std::fs::create_dir_all(dir)?;
        let part = dir.join(format!("{FILE}.part"));
        std::fs::write(&part, Value::Object(all.clone()).to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(part, dir.join(FILE))
    }
}

impl Default for Keys {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fanimate-keys-{tag}-{}", std::process::id()))
    }

    #[test]
    fn a_saved_key_is_read_back_and_forgotten() {
        let dir = folder("roundtrip");
        let keys = Keys::in_folder(dir.clone());
        assert_eq!(keys.saved(ProviderKind::OpenRouter), None);
        keys.save(ProviderKind::OpenRouter, "  sk-or-test  ").unwrap();
        keys.save(ProviderKind::Custom, "other").unwrap();
        assert_eq!(
            keys.saved(ProviderKind::OpenRouter).as_deref(),
            Some("sk-or-test")
        );
        keys.forget(ProviderKind::OpenRouter).unwrap();
        assert_eq!(keys.saved(ProviderKind::OpenRouter), None);
        // forgetting one leaves the others
        assert_eq!(keys.saved(ProviderKind::Custom).as_deref(), Some("other"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn nonsense_is_refused_and_a_damaged_file_reads_as_no_keys() {
        let dir = folder("damaged");
        let keys = Keys::in_folder(dir.clone());
        assert!(keys.save(ProviderKind::Anthropic, "   ").is_err());
        assert!(keys.save(ProviderKind::Anthropic, "a\nb").is_err());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "{not json").unwrap();
        assert_eq!(keys.read().len(), 0);
        std::fs::write(dir.join(FILE), "[1,2]").unwrap();
        assert_eq!(keys.read().len(), 0);
        // saving over a damaged file repairs it
        keys.save(ProviderKind::Anthropic, "k").unwrap();
        assert_eq!(keys.read().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_folder_is_an_error_not_a_panic() {
        let keys = Keys { dir: None };
        assert!(keys.save(ProviderKind::Anthropic, "k").is_err());
        assert!(keys.forget(ProviderKind::Anthropic).is_ok());
        assert_eq!(keys.saved(ProviderKind::Anthropic), None);
    }
}
