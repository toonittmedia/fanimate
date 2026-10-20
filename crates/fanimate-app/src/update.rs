//! Updates: find a newer release, download it, check its signature and
//! install it when the user agrees.
//!
//! - **Never in the way.** Checking runs in the background a few seconds
//!   after start, only when "Check for updates" is on (Preferences, or the
//!   first-run dialog). Without a network it fails silently; Fanimate works
//!   the same offline.
//! - **Signed.** Every update file is published with an Ed25519 signature
//!   (`<file>.sig`) made by the release workflow with a key only it holds.
//!   A download whose signature does not match [`PUBLIC_KEY`] is deleted
//!   and never run.
//! - **Asks first.** A new version shows in the status bar; nothing is
//!   downloaded or installed until the user says so. Installing asks to
//!   save changes, waits for the render queue, and restarts Fanimate.
//! - **All or nothing.** The Windows installer rolls back on failure; on
//!   macOS and in an AppImage the old copy is only replaced by a rename once
//!   the new one is complete, and put back if that fails.
//! - **Where it can.** Flatpak and distribution packages update through the
//!   system's software centre, and copies unpacked from an archive get a
//!   link to the download page instead.
//!
//! Downloads use the system's `curl` (part of Windows 10 and later, macOS
//! and nearly every Linux), so Fanimate carries no network library.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The release workflow's update-signing key (`scripts/sign-update.py`).
pub const PUBLIC_KEY: [u8; 32] = [
    0xe2, 0xc7, 0x03, 0xcb, 0xb8, 0x0e, 0x2a, 0xee, 0xb1, 0x16, 0x85, 0x1f, 0x6c, 0x47, 0x7f, 0x8c, 0x20,
    0x40, 0xf0, 0x7c, 0xc9, 0x43, 0x01, 0x9c, 0x61, 0xf9, 0xd9, 0xbf, 0xcc, 0xf2, 0xfe, 0x56,
];

/// The releases list. `FANIMATE_UPDATE_FEED` replaces it (any address curl
/// reads, `file://` included), for testing updates without publishing.
const FEED: &str = "https://api.github.com/repos/toonittmedia/fanimate/releases?per_page=20";
pub const DOWNLOAD_PAGE: &str = "https://github.com/toonittmedia/fanimate/releases";

/// A release version, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }

    /// Parse `1.2.3` or `v1.2.3`; anything after the patch number (a
    /// pre-release label) is ignored.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.trim().trim_start_matches('v').splitn(3, '.');
        let mut next = || -> Option<u32> {
            let p = parts.next()?;
            let digits: String = p.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        };
        Some(Version(next()?, next()?, next()?))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// How this copy of Fanimate was installed, which decides how it updates.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// By the Windows installer: run the new installer.
    WindowsInstaller,
    /// An app bundle on macOS: swap the bundle.
    MacApp(PathBuf),
    /// A Linux AppImage: swap the file.
    AppImage(PathBuf),
    /// Flatpak or a distribution package: the system updates it.
    Managed,
    /// Unpacked from an archive or built from source: the user downloads.
    Portable,
}

impl Install {
    pub fn detect() -> Self {
        if std::env::var_os("FLATPAK_ID").is_some() {
            return Self::Managed;
        }
        if cfg!(target_os = "linux")
            && let Some(p) = std::env::var_os("APPIMAGE")
        {
            return Self::AppImage(PathBuf::from(p));
        }
        let Ok(exe) = std::env::current_exe() else {
            return Self::Portable;
        };
        if cfg!(target_os = "macos")
            && let Some(bundle) = exe
                .ancestors()
                .find(|a| a.extension().is_some_and(|e| e == "app"))
        {
            return Self::MacApp(bundle.to_path_buf());
        }
        // the installer leaves its uninstaller beside the program
        if cfg!(windows) && exe.parent().is_some_and(|d| d.join("unins000.exe").is_file()) {
            return Self::WindowsInstaller;
        }
        // a distribution package (the .deb installs to /opt/fanimate)
        if cfg!(target_os = "linux") && (exe.starts_with("/usr") || exe.starts_with("/opt")) {
            return Self::Managed;
        }
        Self::Portable
    }

    /// The end of the release file name that updates this kind of copy.
    pub fn asset_suffix(&self) -> Option<String> {
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" if cfg!(target_os = "linux") => "x86_64",
            "x86_64" => "x64",
            other => other,
        };
        match self {
            Self::WindowsInstaller => Some(format!("-windows-{arch}-setup.exe")),
            Self::MacApp(_) => Some(format!("-macos-{arch}.app.tar.gz")),
            Self::AppImage(_) => Some(format!("-linux-{arch}.AppImage")),
            Self::Managed | Self::Portable => None,
        }
    }
}

/// A newer release.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// The release page, with its notes.
    pub page: String,
    /// The update file and its signature, when this copy can install it.
    pub file: Option<(String, String, String)>,
    /// The update file's size in bytes, to show the download's progress.
    pub size: Option<u64>,
    /// What changed, in Markdown (the release's description).
    pub notes: String,
}

/// The newest release in a GitHub releases list newer than `current`.
/// Pre-releases count while Fanimate is below 1.0, when every release is
/// one. `suffix` picks the file this copy installs.
pub fn newest(list: &Value, current: Version, suffix: Option<&str>) -> Option<Release> {
    let mut best: Option<Release> = None;
    for r in list.as_array()? {
        if r.get("draft").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let pre = r.get("prerelease").and_then(Value::as_bool) == Some(true);
        if pre && current.0 >= 1 {
            continue;
        }
        let Some(version) = r.get("tag_name").and_then(Value::as_str).and_then(Version::parse) else {
            continue;
        };
        if version <= current || best.as_ref().is_some_and(|b| b.version >= version) {
            continue;
        }
        let assets: Vec<(&str, &str, Option<u64>)> = r
            .get("assets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some((
                    a.get("name")?.as_str()?,
                    a.get("browser_download_url")?.as_str()?,
                    a.get("size").and_then(Value::as_u64),
                ))
            })
            .collect();
        let mut size = None;
        let file = suffix.and_then(|suffix| {
            let (name, url, bytes) = assets.iter().find(|(n, _, _)| n.ends_with(suffix))?;
            let sig_name = format!("{name}.sig");
            let (_, sig, _) = assets.iter().find(|(n, _, _)| *n == sig_name)?;
            size = *bytes;
            Some(((*name).to_owned(), (*url).to_owned(), (*sig).to_owned()))
        });
        let page = r
            .get("html_url")
            .and_then(Value::as_str)
            .unwrap_or(DOWNLOAD_PAGE)
            .to_owned();
        let notes = r
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        best = Some(Release {
            version,
            page,
            file,
            size,
            notes,
        });
    }
    best
}

/// Whether `signature` is the release key's signature of `bytes`.
pub fn verify(bytes: &[u8], signature: &[u8], key: &[u8; 32]) -> bool {
    let (Ok(key), Ok(sig)) = (
        ed25519_compact::PublicKey::from_slice(key),
        ed25519_compact::Signature::from_slice(signature),
    ) else {
        return false;
    };
    key.verify(bytes, &sig).is_ok()
}

/// Where the updater is.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum State {
    #[default]
    Idle,
    Checking {
        /// Asked for by the user, so "up to date" and errors are shown.
        manual: bool,
    },
    UpToDate,
    Available(Release),
    Downloading(Release),
    /// Downloaded and verified, waiting to install.
    Ready(Release, PathBuf),
    Failed(String),
}

/// The updater, shared with its background thread.
#[derive(Clone, Default)]
pub struct Updater {
    pub state: Arc<Mutex<State>>,
    /// The file being downloaded, which grows as curl writes it.
    part: Arc<Mutex<Option<PathBuf>>>,
}

/// How far along a download is, 0 to 1, from the bytes so far and the
/// update file's size. `None` when the size is not known.
pub fn fraction(done: u64, total: Option<u64>) -> Option<f32> {
    let total = total.filter(|t| *t > 0)?;
    Some((done as f64 / total as f64).clamp(0.0, 1.0) as f32)
}

impl Updater {
    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn set(&self, s: State) {
        if let Ok(mut st) = self.state.lock() {
            *st = s;
        }
    }

    /// Look for a newer release in the background.
    pub fn check(&self, manual: bool, install: &Install) {
        if matches!(self.state(), State::Checking { .. } | State::Downloading(_)) {
            return;
        }
        self.set(State::Checking { manual });
        let me = self.clone();
        let suffix = install.asset_suffix();
        std::thread::spawn(move || {
            let feed = std::env::var("FANIMATE_UPDATE_FEED").unwrap_or_else(|_| FEED.to_owned());
            let result = fetch(&feed, None, 30).and_then(|bytes| {
                let list: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "the release list could not be read")?;
                Ok(newest(&list, Version::current(), suffix.as_deref()))
            });
            me.set(match result {
                Ok(Some(r)) => State::Available(r),
                Ok(None) => State::UpToDate,
                Err(e) => State::Failed(format!("Could not check for updates: {e}")),
            });
        });
    }

    /// Download and verify a release's update file in the background.
    pub fn download(&self, release: Release, folder: PathBuf) {
        let Some((name, url, sig_url)) = release.file.clone() else {
            return;
        };
        self.set(State::Downloading(release.clone()));
        let part = folder.join(format!("{name}.part"));
        let _ = std::fs::remove_file(&part);
        if let Ok(mut shared) = self.part.lock() {
            *shared = Some(part);
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let path = folder.join(&name);
                let part = folder.join(format!("{name}.part"));
                fetch(&url, Some(&part), 1800)?;
                let sig = fetch(&sig_url, None, 60)?;
                let bytes = std::fs::read(&part).map_err(|e| e.to_string())?;
                if !verify(&bytes, &sig, &PUBLIC_KEY) {
                    let _ = std::fs::remove_file(&part);
                    return Err(
                        "the download is not signed by Fanimate's release key, so it was deleted".into(),
                    );
                }
                std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
                Ok(path)
            })();
            me.set(match result {
                Ok(path) => State::Ready(release, path),
                Err(e) => State::Failed(format!("Could not download the update: {e}")),
            });
        });
    }

    pub fn reset(&self) {
        self.set(State::Idle);
        if let Ok(mut part) = self.part.lock() {
            *part = None;
        }
    }

    /// How much of the update has been downloaded, while it downloads. The
    /// download runs in curl on its own thread; this only looks at the size
    /// of the file it is writing, so asking costs the interface nothing.
    pub fn progress(&self) -> Option<f32> {
        let State::Downloading(release) = self.state() else {
            return None;
        };
        let part = self.part.lock().ok()?.clone()?;
        let done = std::fs::metadata(part).ok()?.len();
        fraction(done, release.size)
    }
}

/// Fetch `url` with curl, into `out` or into memory.
fn fetch(url: &str, out: Option<&Path>, seconds: u32) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl");
    cmd.args(["--fail", "--silent", "--show-error", "--location", "--max-time"])
        .arg(seconds.to_string())
        .args(["--header", "Accept: application/vnd.github+json"])
        .arg("--user-agent")
        .arg(format!("Fanimate/{}", env!("CARGO_PKG_VERSION")));
    if let Some(out) = out {
        cmd.arg("--output").arg(out);
    }
    cmd.arg(url);
    no_window(&mut cmd);
    let output = cmd
        .output()
        .map_err(|_| "curl, which downloads updates, was not found".to_owned())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if err.is_empty() {
            "no connection".into()
        } else {
            err
        });
    }
    Ok(output.stdout)
}

/// Run a program without a console window on Windows.
fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Install a verified update and start the new version. On success the
/// caller quits at once.
pub fn install(kind: &Install, file: &Path) -> Result<(), String> {
    match kind {
        Install::WindowsInstaller => {
            // the installer closes Fanimate, replaces it, rolls back on
            // failure, and starts the new version (its Run section)
            Command::new(file)
                .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
                .spawn()
                .map(drop)
                .map_err(|e| format!("the installer could not start: {e}"))
        }
        Install::MacApp(bundle) => {
            let staging = bundle.with_extension("app.new");
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
            let ok = Command::new("tar")
                .arg("-xzf")
                .arg(file)
                .arg("-C")
                .arg(&staging)
                .status()
                .is_ok_and(|s| s.success());
            let new = std::fs::read_dir(&staging).ok().and_then(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .find(|p| p.extension().is_some_and(|e| e == "app"))
            });
            let Some(new) = new.filter(|_| ok) else {
                let _ = std::fs::remove_dir_all(&staging);
                return Err("the update could not be unpacked".into());
            };
            swap(bundle, &new)?;
            let _ = std::fs::remove_dir_all(&staging);
            Command::new("open")
                .arg("-n")
                .arg(bundle)
                .spawn()
                .map(drop)
                .map_err(|e| e.to_string())
        }
        Install::AppImage(current) => {
            let new = current.with_extension("AppImage.new");
            std::fs::copy(file, &new).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            // one rename: the old file stays whole until the new one is
            // in place
            std::fs::rename(&new, current).map_err(|e| {
                let _ = std::fs::remove_file(&new);
                e.to_string()
            })?;
            Command::new(current).spawn().map(drop).map_err(|e| e.to_string())
        }
        Install::Managed | Install::Portable => Err("this copy of Fanimate cannot update itself".into()),
    }
}

/// Replace `old` with `new` by renames, putting `old` back if the second
/// rename fails.
fn swap(old: &Path, new: &Path) -> Result<(), String> {
    let aside = old.with_extension("app.old");
    let _ = std::fs::remove_dir_all(&aside);
    std::fs::rename(old, &aside).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(new, old) {
        let _ = std::fs::rename(&aside, old);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&aside);
    Ok(())
}

/// Settings for updates, remembered between sessions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Check for a new version a few seconds after starting.
    pub check: bool,
    /// A version the user chose to skip.
    pub skipped: Option<String>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check: true,
            skipped: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, pre: bool, assets: &[&str]) -> Value {
        serde_json::json!({
            "tag_name": tag, "prerelease": pre, "draft": false,
            "html_url": format!("https://example.com/{tag}"),
            "body": format!("Notes for {tag}"),
            "assets": assets.iter().map(|n| serde_json::json!({"name": n, "size": 1000, "browser_download_url": format!("https://dl/{n}")})).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(Version::parse("v0.1.2"), Some(Version(0, 1, 2)));
        assert_eq!(Version::parse("1.10.0-beta.1"), Some(Version(1, 10, 0)));
        assert_eq!(Version::parse("x"), None);
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert_eq!(Version(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn the_newest_signed_file_for_this_copy_is_chosen() {
        let suffix = "-windows-x64-setup.exe";
        let list = serde_json::json!([
            release("v0.2.0", true, &["Fanimate-0.2.0-windows-x64-setup.exe", "Fanimate-0.2.0-windows-x64-setup.exe.sig"]),
            release("v0.3.0", true, &["Fanimate-0.3.0-windows-x64-setup.exe"]),
            release("v0.1.0", true, &[]),
            {"tag_name": "v9.0.0", "draft": true},
        ]);
        let r = newest(&list, Version(0, 1, 0), Some(suffix)).unwrap();
        // 0.3.0 is newest but unsigned: offered, with no file to install
        assert_eq!(r.version, Version(0, 3, 0));
        assert!(r.file.is_none());
        let signed = serde_json::json!([list[0].clone()]);
        let r = newest(&signed, Version(0, 1, 0), Some(suffix)).unwrap();
        let (name, url, sig) = r.file.unwrap();
        assert_eq!(name, "Fanimate-0.2.0-windows-x64-setup.exe");
        assert!(url.ends_with(&name) && sig.ends_with(".sig"));
        // the size of the update file, not its signature's, and the notes
        assert_eq!(r.size, Some(1000));
        assert_eq!(r.notes, "Notes for v0.2.0");
        assert_eq!(r.page, "https://example.com/v0.2.0");
        // nothing newer
        assert!(newest(&list, Version(0, 3, 0), Some(suffix)).is_none());
        // after 1.0, pre-releases are not offered
        let list = serde_json::json!([release("v1.1.0", true, &[]), release("v1.0.1", false, &[])]);
        assert_eq!(
            newest(&list, Version(1, 0, 0), None).unwrap().version,
            Version(1, 0, 1)
        );
        assert!(
            newest(
                &serde_json::json!({"message": "rate limited"}),
                Version(0, 1, 0),
                None
            )
            .is_none()
        );
    }

    #[test]
    fn download_progress_is_a_fraction_of_the_file_size() {
        assert_eq!(fraction(0, Some(1000)), Some(0.0));
        assert_eq!(fraction(250, Some(1000)), Some(0.25));
        // curl may have written a little past what the feed said
        assert_eq!(fraction(1200, Some(1000)), Some(1.0));
        assert_eq!(fraction(10, None), None);
        assert_eq!(fraction(10, Some(0)), None);
    }

    #[test]
    fn a_download_reports_its_progress_from_the_file_it_writes() {
        let dir = std::env::temp_dir().join(format!("fanimate-update-progress-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let part = dir.join("update.part");
        std::fs::write(&part, vec![0u8; 400]).unwrap();
        let updater = Updater::default();
        let release = Release {
            version: Version(0, 3, 0),
            page: String::new(),
            file: None,
            size: Some(1000),
            notes: String::new(),
        };
        // nothing downloading: no progress
        assert_eq!(updater.progress(), None);
        updater.set(State::Downloading(release));
        *updater.part.lock().unwrap() = Some(part);
        assert_eq!(updater.progress(), Some(0.4));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_the_release_key_signature_passes() {
        let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([7; 32]));
        let data = b"installer bytes";
        let sig = pair.sk.sign(data, None);
        let key: [u8; 32] = pair.pk.as_ref().try_into().unwrap();
        assert!(verify(data, sig.as_ref(), &key));
        assert!(!verify(b"tampered", sig.as_ref(), &key));
        assert!(!verify(data, sig.as_ref(), &PUBLIC_KEY));
        assert!(!verify(data, b"short", &key));
    }

    #[test]
    fn each_kind_of_copy_names_its_update_file() {
        assert!(
            Install::WindowsInstaller
                .asset_suffix()
                .unwrap()
                .ends_with("-setup.exe")
        );
        assert!(
            Install::MacApp(PathBuf::from("/A.app"))
                .asset_suffix()
                .unwrap()
                .ends_with(".app.tar.gz")
        );
        assert!(
            Install::AppImage(PathBuf::new())
                .asset_suffix()
                .unwrap()
                .ends_with(".AppImage")
        );
        assert_eq!(Install::Portable.asset_suffix(), None);
        assert!(install(&Install::Portable, Path::new("x")).is_err());
    }

    #[test]
    fn a_failed_swap_puts_the_old_copy_back() {
        let dir = std::env::temp_dir().join(format!("fanimate-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let old = dir.join("Fanimate.app");
        std::fs::create_dir_all(old.join("Contents")).unwrap();
        // the new copy is missing: the rename fails and the old one returns
        assert!(swap(&old, &dir.join("missing.app")).is_err());
        assert!(old.join("Contents").is_dir());
        let new = dir.join("New.app");
        std::fs::create_dir_all(new.join("Fresh")).unwrap();
        swap(&old, &new).unwrap();
        assert!(old.join("Fresh").is_dir() && !dir.join("Fanimate.app.old").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
