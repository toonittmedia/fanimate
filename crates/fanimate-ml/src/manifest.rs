//! What a model says about itself: `model.json` inside a `.fnmodel` package.
//!
//! Everything the policy (D2-6) needs to judge a model is in it: the weights'
//! licence, a licence file, where the weights came from, and how the training
//! data was gathered. [`ModelManifest::check`] returns every problem at once,
//! so whoever packs a model sees all that is wrong, not the first thing.

use serde::{Deserialize, Serialize};

use crate::policy::{self, Consent};

/// The manifest format this reader knows.
pub const SCHEMA: u32 = 1;

/// The most a manifest may be, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// The most a model's weights may be, in bytes (1 GiB).
pub const MAX_WEIGHTS_BYTES: u64 = 1 << 30;

/// What a model is for. One model, one task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    LineCleanup,
    InbetweenRanking,
    AutoRigKeypoints,
    ColourAssist,
    LipSyncAlignment,
    SpeechToText,
    PoseFromPhoto,
}

impl Task {
    pub const ALL: [Task; 7] = [
        Self::LineCleanup,
        Self::InbetweenRanking,
        Self::AutoRigKeypoints,
        Self::ColourAssist,
        Self::LipSyncAlignment,
        Self::SpeechToText,
        Self::PoseFromPhoto,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::LineCleanup => "Line clean-up from raster",
            Self::InbetweenRanking => "Inbetween ranking",
            Self::AutoRigKeypoints => "Auto-rig keypoints",
            Self::ColourAssist => "Colour assist",
            Self::LipSyncAlignment => "Lip sync alignment",
            Self::SpeechToText => "Speech to text",
            Self::PoseFromPhoto => "Pose from a photo",
        }
    }
}

/// Where the training data came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// What the model was trained on, in words (a sentence or more).
    pub training_data: String,
    pub consent: Consent,
    /// Whether any of it was art gathered from the web without the artists' consent. Must be false.
    pub scraped_art: bool,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub schema: u32,
    /// Lower case letters, digits, `.`, `-` and `_`; starts with a letter or digit.
    pub id: String,
    pub name: String,
    pub version: String,
    pub task: Task,
    /// The runtime that runs the weights, such as `onnx`.
    pub runtime: String,
    /// The weights' file name inside the package.
    pub weights: String,
    pub size: u64,
    /// SHA-256 of the weights, lower case hex.
    pub sha256: String,
    /// The weights' licence, an SPDX expression.
    pub licence: String,
    /// The licence text's file name inside the package.
    pub licence_file: String,
    /// Where the weights came from: a URL or a description of who made them.
    pub source: String,
    pub provenance: Provenance,
}

/// A file name that stays inside the folder it is written to.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && !name.contains("..")
}

/// A model id that is safe as a folder name.
pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    id.len() <= 64
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

impl ModelManifest {
    /// Read a manifest and check its shape (not yet the policy).
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err("the manifest is larger than 64 KB".to_owned());
        }
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("the manifest is not valid: {e}"))?;
        if manifest.schema != SCHEMA {
            return Err(format!(
                "the manifest is schema {}; this Fanimate reads schema {SCHEMA}",
                manifest.schema
            ));
        }
        Ok(manifest)
    }

    /// Every way the manifest is wrong or breaks the policy; empty when it is sound.
    pub fn check(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !is_valid_id(&self.id) {
            problems
                .push("the id must be 1 to 64 lower case letters, digits, \".\", \"-\" or \"_\"".to_owned());
        }
        for (label, text, max) in [
            ("name", &self.name, 80),
            ("version", &self.version, 32),
            ("runtime", &self.runtime, 32),
            ("source", &self.source, 300),
        ] {
            if text.trim().is_empty() || text.chars().count() > max {
                problems.push(format!("\"{label}\" must be 1 to {max} characters"));
            }
        }
        if !is_plain_name(&self.weights) {
            problems.push("\"weights\" must be a plain file name".to_owned());
        }
        if !is_plain_name(&self.licence_file) || self.licence_file == self.weights {
            problems.push("\"licence_file\" must be a plain file name, not the weights".to_owned());
        }
        if self.size == 0 || self.size > MAX_WEIGHTS_BYTES {
            problems.push("\"size\" must be from 1 byte to 1 GiB".to_owned());
        }
        if self.sha256.len() != 64 || !self.sha256.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
            problems.push("\"sha256\" must be 64 lower case hex digits".to_owned());
        }
        if let Err(why) = policy::check_licence(&self.licence) {
            problems.push(format!("licence: {why}"));
        }
        let p = &self.provenance;
        if p.training_data.trim().len() < 20 {
            problems.push("provenance: say what the model was trained on (at least a sentence)".to_owned());
        }
        if p.scraped_art {
            problems
                .push("provenance: art scraped without its artists' consent is never accepted".to_owned());
        }
        if !policy::consent_ok(p.consent) {
            problems.push(
                "provenance: the training data must be commissioned, donated, public domain or under a permissive open licence, and say which"
                    .to_owned(),
            );
        }
        problems
    }
}

/// A manifest that meets the policy, for the tests of this crate.
#[cfg(test)]
pub(crate) fn sound_manifest() -> ModelManifest {
    ModelManifest {
        schema: SCHEMA,
        id: "line-cleanup-1".into(),
        name: "Line clean-up".into(),
        version: "1.0.0".into(),
        task: Task::LineCleanup,
        runtime: "onnx".into(),
        weights: "model.onnx".into(),
        size: 1024,
        sha256: "0".repeat(64),
        licence: "Apache-2.0".into(),
        licence_file: "LICENSE.txt".into(),
        source: "https://example.org/line-cleanup".into(),
        provenance: Provenance {
            training_data: "Line drawings commissioned from six artists, each with a signed consent form"
                .into(),
            consent: Consent::Commissioned,
            scraped_art: false,
            notes: String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::sound_manifest as sound;
    use super::*;

    #[test]
    fn a_sound_manifest_has_no_problems_and_round_trips() {
        let m = sound();
        assert!(m.check().is_empty(), "{:?}", m.check());
        let bytes = serde_json::to_vec(&m).unwrap();
        assert_eq!(ModelManifest::parse(&bytes).unwrap(), m);
    }

    #[test]
    fn every_breach_of_the_policy_is_reported_together() {
        let mut m = sound();
        m.licence = "CC-BY-NC-4.0".into();
        m.provenance.scraped_art = true;
        m.provenance.consent = Consent::Unknown;
        m.provenance.training_data = "stuff".into();
        m.id = "Bad Id".into();
        m.weights = "../escape.onnx".into();
        m.sha256 = "XYZ".into();
        m.size = 0;
        let problems = m.check().join("\n");
        for needle in [
            "id must be",
            "plain file name",
            "size",
            "sha256",
            "lawyer",
            "scraped",
            "commissioned",
            "trained on",
        ] {
            assert!(problems.contains(needle), "missing {needle} in:\n{problems}");
        }
        assert!(m.check().len() >= 8);
    }

    #[test]
    fn parsing_refuses_junk_a_newer_schema_and_a_huge_file() {
        assert!(ModelManifest::parse(b"not json").is_err());
        assert!(ModelManifest::parse(b"{}").is_err());
        let mut m = sound();
        m.schema = 9;
        let bytes = serde_json::to_vec(&m).unwrap();
        assert!(ModelManifest::parse(&bytes).unwrap_err().contains("schema 9"));
        assert!(ModelManifest::parse(&vec![b' '; MAX_MANIFEST_BYTES + 1]).is_err());
    }

    #[test]
    fn names_and_ids_stay_inside_their_folder() {
        for bad in [
            "",
            ".hidden",
            "a/b",
            "a\\b",
            "a..b",
            "a b",
            "con:x",
            &"x".repeat(101),
        ] {
            assert!(!is_plain_name(bad), "{bad:?}");
        }
        assert!(is_plain_name("model.onnx") && is_plain_name("LICENSE-2.0.txt"));
        for bad in ["", "Upper", "-lead", ".lead", "a/b", &"a".repeat(65)] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
        assert!(is_valid_id("whisper-tiny.en_1"));
    }
}
