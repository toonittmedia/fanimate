//! What a client of the API is allowed to do, and the plugin manifest that
//! asks for it.
//!
//! A host that serves the API to code it did not write (a plugin, a script
//! someone shared) gives that code a [`Permissions`] set, and every call is
//! checked against it before it runs (`rpc::handle_line_as`). A plugin says
//! what it needs in its [`Manifest`]; the host grants exactly that. The host
//! the user starts by hand (`fanimate-cli script`) grants everything unless it
//! is told otherwise, because the user is the author.
//!
//! Three permissions cover the API today, from least to most:
//!
//! - `read`: queries (the layers, the text, the curves, the fonts).
//! - `edit`: every call that changes the document, and `undo` and `redo`.
//! - `save`: writing the project to disk (the host's `save`).
//!
//! `quit` (ending one's own session) needs nothing; `shutdown` (stopping the
//! host for everyone connected) needs `edit`. A new permission is added here
//! with the call that needs it, never by loosening an existing one.

use serde::Deserialize;

use crate::API_VERSION;

/// One thing a client may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Permission {
    Read,
    Edit,
    Save,
}

impl Permission {
    /// Every permission, least first.
    pub const ALL: [Permission; 3] = [Self::Read, Self::Edit, Self::Save];

    /// The name a manifest and the command line use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::Save => "save",
        }
    }

    /// The permission with this name (case does not matter), if there is one.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.as_str().eq_ignore_ascii_case(name.trim()))
    }

    /// What it lets a client do, for a message.
    pub fn about(self) -> &'static str {
        match self {
            Self::Read => "read the project",
            Self::Edit => "change the project",
            Self::Save => "write the project to disk",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permissions(u8);

impl Permissions {
    /// Nothing: every call is refused.
    pub const NONE: Self = Self(0);

    /// Everything.
    pub fn all() -> Self {
        Permission::ALL.into_iter().fold(Self::NONE, Self::with)
    }

    /// This set and `p`.
    #[must_use]
    pub fn with(self, p: Permission) -> Self {
        Self(self.0 | p.bit())
    }

    pub fn contains(self, p: Permission) -> bool {
        self.0 & p.bit() != 0
    }

    /// The names of the permissions in the set, least first.
    pub fn names(self) -> Vec<&'static str> {
        Permission::ALL
            .into_iter()
            .filter(|p| self.contains(*p))
            .map(Permission::as_str)
            .collect()
    }

    /// A list of permission names, as typed after `--permissions`:
    /// `"read,edit"`. A name that is not one is an error that lists the real ones.
    pub fn parse_list(list: &str) -> Result<Self, String> {
        let mut set = Self::NONE;
        for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            let p = Permission::parse(name).ok_or_else(|| unknown(name))?;
            set = set.with(p);
        }
        Ok(set)
    }
}

fn unknown(name: &str) -> String {
    let known: Vec<&str> = Permission::ALL.iter().map(|p| p.as_str()).collect();
    format!(
        "there is no permission \"{}\" (the permissions are {})",
        name.chars().take(40).collect::<String>(),
        known.join(", ")
    )
}

/// The most a manifest file may hold, in bytes.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// What a plugin says about itself: `plugin.json`. Fields a later Fanimate adds
/// are ignored by this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// The API version the plugin was written for, `major.minor` (`"0.1"`).
    pub api: String,
    pub description: String,
    /// The file or command that starts the plugin, relative to the manifest.
    pub entry: Option<String>,
    pub permissions: Permissions,
}

#[derive(Deserialize)]
struct Raw {
    name: Option<String>,
    version: Option<String>,
    api: Option<String>,
    description: Option<String>,
    entry: Option<String>,
    permissions: Option<Vec<String>>,
}

impl Manifest {
    /// Read and check a manifest. The error says which field is wrong and how.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_MANIFEST_BYTES {
            return Err("the manifest is larger than 64 KB".to_owned());
        }
        let raw: Raw =
            serde_json::from_str(text).map_err(|e| format!("the manifest is not valid JSON: {e}"))?;
        let name = raw.name.ok_or("the manifest needs a \"name\"")?.trim().to_owned();
        if name.is_empty() || name.chars().count() > 64 {
            return Err("\"name\" must be 1 to 64 characters".to_owned());
        }
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        {
            return Err("\"name\" may hold letters, digits, spaces, \"-\", \"_\" and \".\" only".to_owned());
        }
        let version = raw
            .version
            .ok_or("the manifest needs a \"version\"")?
            .trim()
            .to_owned();
        if version.is_empty()
            || version.len() > 32
            || !version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        {
            return Err(
                "\"version\" must be 1 to 32 characters of letters, digits, \".\", \"-\" and \"+\""
                    .to_owned(),
            );
        }
        let api = raw
            .api
            .ok_or("the manifest needs the \"api\" version it was written for")?;
        let api = api.trim().to_owned();
        check_api(&api)?;
        let description = raw.description.unwrap_or_default();
        if description.chars().count() > 500 {
            return Err("\"description\" is longer than 500 characters".to_owned());
        }
        let entry = match raw.entry {
            Some(e) => {
                check_entry(&e)?;
                Some(e)
            }
            None => None,
        };
        let listed = raw
            .permissions
            .ok_or("the manifest needs a \"permissions\" list (it may be empty)")?;
        if listed.len() > 16 {
            return Err("\"permissions\" has more than 16 entries".to_owned());
        }
        let mut permissions = Permissions::NONE;
        for name in &listed {
            permissions = permissions.with(Permission::parse(name).ok_or_else(|| unknown(name))?);
        }
        Ok(Self {
            name,
            version,
            api,
            description,
            entry,
            permissions,
        })
    }
}

/// The plugin's `api` must be this Fanimate's major and minor version: the
/// API is unstable until 1.0, so a minor change may break a plugin.
fn check_api(api: &str) -> Result<(), String> {
    let ours: Vec<&str> = API_VERSION.split('.').take(2).collect();
    let theirs: Vec<&str> = api.split('.').collect();
    let numeric = !theirs.is_empty()
        && theirs.len() <= 2
        && theirs
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 6 && p.chars().all(|c| c.is_ascii_digit()));
    if !numeric {
        return Err(format!("\"api\" must look like \"{}\"", ours.join(".")));
    }
    if theirs.iter().zip(&ours).any(|(a, b)| a != b) || theirs.len() != 2 {
        return Err(format!(
            "the plugin was written for API {api}, and this Fanimate has {}",
            ours.join(".")
        ));
    }
    Ok(())
}

/// An entry stays inside the plugin's folder: relative, no `..`, no drive.
fn check_entry(entry: &str) -> Result<(), String> {
    let bad = entry.is_empty()
        || entry.len() > 260
        || entry.starts_with(['/', '\\'])
        || entry.contains(':')
        || entry.chars().any(char::is_control)
        || entry.split(['/', '\\']).any(|part| part == "..");
    if bad {
        return Err(
            "\"entry\" must be a path inside the plugin's folder (no \"..\", no drive, not absolute)"
                .to_owned(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"name": "Title Maker", "version": "1.2.0", "api": "0.1",
        "description": "Makes titles", "entry": "main.py", "permissions": ["read", "edit"]}"#;

    #[test]
    fn a_good_manifest_is_read() {
        let m = Manifest::parse(GOOD).unwrap();
        assert_eq!(
            (m.name.as_str(), m.version.as_str(), m.api.as_str()),
            ("Title Maker", "1.2.0", "0.1")
        );
        assert_eq!(m.entry.as_deref(), Some("main.py"));
        assert_eq!(m.permissions.names(), ["read", "edit"]);
        assert!(!m.permissions.contains(Permission::Save));
        // fields a later Fanimate adds are ignored, and an empty list is fine
        let later = r#"{"name":"x","version":"1","api":"0.1","permissions":[],"contributes":{"panels":[]}}"#;
        assert_eq!(Manifest::parse(later).unwrap().permissions, Permissions::NONE);
    }

    #[test]
    fn a_wrong_manifest_says_which_field_and_why() {
        let with = |field: &str, to: &str| {
            let mut v: serde_json::Value = serde_json::from_str(GOOD).unwrap();
            if to == "<remove>" {
                v.as_object_mut().unwrap().remove(field);
            } else {
                v[field] = serde_json::from_str(to).unwrap();
            }
            Manifest::parse(&v.to_string()).unwrap_err()
        };
        assert!(with("name", "<remove>").contains("\"name\""));
        assert!(with("name", "\"\"").contains("\"name\""));
        assert!(with("name", "\"a/b\"").contains("letters"));
        assert!(with("name", &format!("\"{}\"", "n".repeat(65))).contains("64"));
        assert!(with("version", "\"1 2\"").contains("\"version\""));
        assert!(with("api", "\"0.2\"").contains("API 0.2"));
        assert!(with("api", "\"1.0\"").contains("API 1.0"));
        assert!(with("api", "\"x\"").contains("\"api\""));
        assert!(with("api", "\"0\"").contains("API 0"));
        assert!(with("api", "\"0.1.0\"").contains("\"api\""));
        assert!(with("permissions", "<remove>").contains("\"permissions\""));
        assert!(with("permissions", "[\"read\", \"root\"]").contains("no permission \"root\""));
        assert!(with("permissions", "[\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\",\"read\"]").contains("16"));
        for bad in [
            "../x.py",
            "/etc/x",
            "C:\\x.exe",
            "a/../../b",
            "a\\..\\b",
            "",
            "x\u{0}y",
        ] {
            assert!(
                with("entry", &serde_json::to_string(bad).unwrap()).contains("\"entry\""),
                "{bad:?}"
            );
        }
        assert!(
            Manifest::parse("not json")
                .unwrap_err()
                .contains("not valid JSON")
        );
        assert!(Manifest::parse("[]").is_err());
        assert!(Manifest::parse("null").is_err());
        assert!(
            Manifest::parse(&"x".repeat(MAX_MANIFEST_BYTES + 1))
                .unwrap_err()
                .contains("64 KB")
        );
    }

    #[test]
    fn permission_lists_parse_and_name_a_wrong_one() {
        assert_eq!(
            Permissions::parse_list("read, EDIT").unwrap().names(),
            ["read", "edit"]
        );
        assert_eq!(Permissions::parse_list("").unwrap(), Permissions::NONE);
        assert_eq!(
            Permissions::parse_list("read,edit,save").unwrap(),
            Permissions::all()
        );
        let e = Permissions::parse_list("read,delete").unwrap_err();
        assert!(e.contains("\"delete\"") && e.contains("read, edit, save"), "{e}");
        // a hostile name is cut short in the message
        assert!(Permissions::parse_list(&"z".repeat(10_000)).unwrap_err().len() < 200);
    }
}
