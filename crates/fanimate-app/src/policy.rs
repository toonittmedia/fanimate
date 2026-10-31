//! Finding and reading the policy files (see `fanimate_core::policy`).
//!
//! Two files, both optional: `fanimate-policy.json` beside the program, which
//! an administrator sets for everyone who uses the computer, and `policy.json`
//! in the user's Fanimate folder. Where they disagree the stricter wins, and
//! a file that cannot be read counts as the strictest policy, never as none.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use fanimate_core::Policy;

/// Read a policy file's text. A field the file does not mention keeps its
/// default, and a name this version does not know is ignored, so a newer
/// policy file works on an older Fanimate. Text that is not a policy is an
/// error.
fn parse(text: &str) -> Result<Policy, String> {
    // an empty file is a policy that says nothing
    if text.trim().is_empty() {
        return Ok(Policy::default());
    }
    // serde would also read a JSON array as the fields in order, so that
    // `[true]` meant something; a policy file is an object or it is damaged
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("it is not a JSON object".into());
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

/// Read a policy file's text, or, if it is damaged, the locked policy and a
/// message saying why.
fn read(text: &str) -> (Policy, Option<String>) {
    match parse(text) {
        Ok(policy) => (policy, None),
        Err(e) => (
            Policy::LOCKED,
            Some(format!(
                "the policy file could not be read ({e}), so Fanimate stays locked down"
            )),
        ),
    }
}

/// Beside the program: for everyone on this computer.
const ADMIN_FILE: &str = "fanimate-policy.json";
/// In the user's Fanimate folder, beside the settings.
const USER_FILE: &str = "policy.json";

/// The policy in force, and a message for the user if a file was damaged.
pub fn load() -> (Policy, Option<String>) {
    let mut files = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        files.push(dir.join(ADMIN_FILE));
    }
    if let Some(dir) = eframe::storage_dir("Fanimate") {
        files.push(dir.join(USER_FILE));
    }
    load_from(&files)
}

/// The policy the given files add up to. A file that is not there says
/// nothing; one that is there and cannot be read or understood locks
/// everything, and the message names it.
pub fn load_from(files: &[PathBuf]) -> (Policy, Option<String>) {
    let mut policy = Policy::default();
    let mut notes = Vec::new();
    for file in files {
        match std::fs::read_to_string(file) {
            Ok(text) => {
                let (read, note) = read(&text);
                policy = policy.stricter(read);
                notes.extend(note.map(|n| named(file, &n)));
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => {
                policy = policy.stricter(Policy::LOCKED);
                notes.push(named(
                    file,
                    &format!("the policy file could not be read ({e}), so Fanimate stays locked down"),
                ));
            }
        }
    }
    (policy, (!notes.is_empty()).then(|| notes.join(" ")))
}

fn named(file: &Path, message: &str) -> String {
    let name = file
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    format!("{name}: {message}.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-policy-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn no_policy_text_is_no_restriction() {
        assert_eq!(parse("").unwrap(), Policy::default());
        assert_eq!(
            parse(
                "  
"
            )
            .unwrap(),
            Policy::default()
        );
        assert_eq!(parse("{}").unwrap(), Policy::default());
    }

    #[test]
    fn a_policy_names_only_what_it_changes() {
        let p = parse(r#"{ "ai": false }"#).unwrap();
        assert!(!p.ai_allowed() && p.network_allowed() && p.cloud_allowed());
        let p = parse(r#"{ "offline": true, "cloud": true }"#).unwrap();
        assert!(!p.network_allowed() && !p.cloud_allowed() && p.ai_allowed());
    }

    #[test]
    fn names_from_a_newer_fanimate_are_ignored() {
        let p = parse(r#"{ "ai": false, "marketplace": "closed", "future": [1, 2] }"#).unwrap();
        assert!(!p.ai_allowed());
    }

    #[test]
    fn text_that_is_not_a_policy_locks_everything_and_says_so() {
        for bad in [
            "{",
            "nonsense",
            r#"{ "offline": "yes" }"#,
            r#"{ "ai": 3 }"#,
            "[true]",
            "null",
        ] {
            let (p, note) = read(bad);
            assert_eq!(p, Policy::LOCKED, "{bad}");
            assert!(note.is_some_and(|n| n.contains("policy file")), "{bad}");
        }
        let (p, note) = read(r#"{ "ai": false }"#);
        assert!(note.is_none() && !p.ai_allowed());
    }

    #[test]
    fn no_files_means_no_restriction() {
        let d = dir("none");
        let (policy, note) = load_from(&[d.join(ADMIN_FILE), d.join(USER_FILE)]);
        assert_eq!(policy, Policy::default());
        assert!(note.is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_stricter_of_two_files_wins() {
        let d = dir("two");
        std::fs::write(d.join(ADMIN_FILE), r#"{ "ai": false }"#).unwrap();
        std::fs::write(d.join(USER_FILE), r#"{ "offline": true, "ai": true }"#).unwrap();
        let (policy, note) = load_from(&[d.join(ADMIN_FILE), d.join(USER_FILE)]);
        assert!(
            policy.offline && !policy.ai,
            "the user cannot lift the administrator's rule"
        );
        assert!(note.is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_damaged_file_locks_everything_and_is_named() {
        let d = dir("damaged");
        std::fs::write(d.join(USER_FILE), "{ not json").unwrap();
        let (policy, note) = load_from(&[d.join(ADMIN_FILE), d.join(USER_FILE)]);
        assert_eq!(policy, Policy::LOCKED);
        let note = note.unwrap();
        assert!(note.starts_with("policy.json: ") && note.ends_with('.'), "{note}");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn a_file_that_cannot_be_read_locks_everything() {
        // a folder where the file should be: it exists but reading it fails
        let d = dir("unreadable");
        std::fs::create_dir(d.join(ADMIN_FILE)).unwrap();
        let (policy, note) = load_from(&[d.join(ADMIN_FILE)]);
        assert_eq!(policy, Policy::LOCKED);
        assert!(note.unwrap().contains("could not be read"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
