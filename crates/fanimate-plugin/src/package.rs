//! `.fnplugin` files: a plugin's folder as one ZIP archive, to share and to
//! install.
//!
//! An archive comes from someone else, so unpacking it is careful: a file
//! name that is absolute, climbs out with `..`, holds a drive letter or a
//! backslash is refused, a name listed twice is refused, the file and entry
//! counts and the unpacked size are capped, every entry's checksum is
//! verified, and the plugin lands in a hidden folder that is moved into
//! place only when the whole archive was read and its manifest is valid. An
//! install never runs anything.

use std::path::{Component, Path, PathBuf};

use fanimate_fnm::zip;

use crate::{MANIFEST_FILE, Plugin, PluginError, io_error};

/// The extension of a packed plugin.
pub const PACKAGE_EXTENSION: &str = "fnplugin";

/// The most files a package may hold.
pub const MAX_PACKAGE_FILES: usize = 2000;

/// The most a package may hold unpacked, in bytes.
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;

/// What an install did.
#[derive(Clone, Debug)]
pub struct Installed {
    /// The plugin as it now is on disk.
    pub plugin: Plugin,
    /// The version it replaced, when a plugin of that name was installed.
    pub replaced: Option<String>,
    /// How many files were written.
    pub files: usize,
}

fn problem(text: impl Into<String>) -> PluginError {
    PluginError::Package(text.into())
}

/// The folder name for a plugin called `name`: lower case, letters, digits,
/// `-` and `_`, with runs of anything else turned into one `-`.
pub fn folder_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_alphanumeric() || c == '_' {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() { "plugin".to_owned() } else { out }
}

/// Whether `name` is a path that stays inside the folder it is unpacked into:
/// relative, with plain components only.
fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || name.len() > 240
        || name.contains('\\')
        || name.contains('\0')
        || name.contains("//")
    {
        return false;
    }
    Path::new(name).components().all(|c| match c {
        Component::Normal(part) => {
            let part = part.to_string_lossy();
            !part.contains(':')
                && !part.chars().any(char::is_control)
                && !part.ends_with('.')
                && !part.ends_with(' ')
        }
        _ => false,
    }) && !name.starts_with('/')
}

/// Pack the plugin in `dir` (its folder) into the file `out`: every file
/// below it except those whose names start with `.` and links, with
/// `plugin.json` first. The manifest is checked first, so a package is
/// always one that installs. Returns how many files went in.
pub fn pack(dir: &Path, out: &Path) -> Result<usize, PluginError> {
    let plugin = Plugin::load(dir)?;
    if plugin.manifest.entry.is_some() {
        plugin.entry_path()?;
    }
    let out_abs = std::path::absolute(out).map_err(io_error(out))?;
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect(dir, "", &out_abs, &mut files)?;
    files.sort_by(|a, b| (a.0 != MANIFEST_FILE, &a.0).cmp(&(b.0 != MANIFEST_FILE, &b.0)));
    if files.len() > MAX_PACKAGE_FILES {
        return Err(problem(format!(
            "the plugin has more than {MAX_PACKAGE_FILES} files; leave out what it does not need"
        )));
    }
    let mut data = Vec::with_capacity(files.len());
    let mut total = 0usize;
    for (name, path) in &files {
        let bytes = std::fs::read(path).map_err(io_error(path))?;
        total += bytes.len();
        if total > MAX_PACKAGE_BYTES {
            return Err(problem("the plugin is larger than 64 MB"));
        }
        data.push((name.as_str(), bytes));
    }
    let refs: Vec<(&str, &[u8])> = data.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let archive = zip::write(&refs).map_err(problem)?;
    // beside the target first, so a failed write leaves no half package
    let part = out.with_extension(format!("{PACKAGE_EXTENSION}.part"));
    std::fs::write(&part, &archive).map_err(io_error(&part))?;
    std::fs::rename(&part, out).map_err(io_error(out))?;
    Ok(files.len())
}

fn collect(
    dir: &Path,
    prefix: &str,
    skip: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), PluginError> {
    let entries = std::fs::read_dir(dir).map_err(io_error(dir))?;
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // links could lead anywhere, so they are not followed
        let Ok(kind) = entry.file_type() else { continue };
        let name = format!("{prefix}{file_name}");
        if kind.is_dir() {
            collect(&path, &format!("{name}/"), skip, files)?;
        } else if kind.is_file() && std::path::absolute(&path).ok().as_deref() != Some(skip) {
            if !safe_name(&name) {
                return Err(problem(format!("the file name \"{name}\" cannot be packed")));
            }
            files.push((name, path));
            if files.len() > MAX_PACKAGE_FILES {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Install the package file `file` into `plugins` (the plugins folder, made
/// if it does not exist).
pub fn install(file: &Path, plugins: &Path) -> Result<Installed, PluginError> {
    let len = std::fs::metadata(file).map_err(io_error(file))?.len();
    if len > MAX_PACKAGE_BYTES as u64 {
        return Err(problem("the package is larger than 64 MB"));
    }
    let bytes = std::fs::read(file).map_err(io_error(file))?;
    install_bytes(&bytes, plugins)
}

/// [`install`] for a package already in memory.
pub fn install_bytes(bytes: &[u8], plugins: &Path) -> Result<Installed, PluginError> {
    let archive = zip::Archive::read(bytes).map_err(|e| problem(format!("not a plugin package: {e}")))?;
    if let Some(name) = archive.duplicates().first() {
        return Err(problem(format!("the package lists \"{name}\" twice")));
    }
    let names: Vec<String> = archive
        .names()
        .filter(|n| !n.ends_with('/'))
        .map(str::to_owned)
        .collect();
    if names.len() > MAX_PACKAGE_FILES {
        return Err(problem(format!(
            "the package has more than {MAX_PACKAGE_FILES} files"
        )));
    }
    let mut total = 0usize;
    for name in &names {
        if !safe_name(name) {
            return Err(problem(format!(
                "the package holds a file named \"{name}\", which is refused"
            )));
        }
        total = total.saturating_add(archive.size(name).unwrap_or(0));
    }
    if total > MAX_PACKAGE_BYTES {
        return Err(problem("the package unpacks to more than 64 MB"));
    }
    if !names.iter().any(|n| n == MANIFEST_FILE) {
        return Err(problem("the package has no plugin.json at its top"));
    }

    std::fs::create_dir_all(plugins).map_err(io_error(plugins))?;
    let staging = plugins.join(format!(".installing-{}", std::process::id()));
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging).map_err(io_error(&staging))?;
    let result = unpack_and_swap(&archive, &names, &staging, plugins);
    // whatever happened, the staging folder does not stay
    std::fs::remove_dir_all(&staging).ok();
    result
}

fn unpack_and_swap(
    archive: &zip::Archive<'_>,
    names: &[String],
    staging: &Path,
    plugins: &Path,
) -> Result<Installed, PluginError> {
    for name in names {
        let data = archive
            .get_checked(name)
            .ok_or_else(|| problem(format!("\"{name}\" is missing from the package")))?
            .map_err(|e| problem(format!("\"{name}\": {e}")))?;
        let target = staging.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        std::fs::write(&target, data).map_err(io_error(&target))?;
    }
    let plugin = Plugin::load(staging)?;
    if plugin.manifest.entry.is_some() {
        plugin.entry_path()?;
    }
    let target = plugins.join(folder_name(&plugin.manifest.name));
    let previous = Plugin::load(&target).ok().map(|p| p.manifest.version);
    if target.exists() {
        let old = plugins.join(format!(".old-{}", std::process::id()));
        std::fs::remove_dir_all(&old).ok();
        std::fs::rename(&target, &old).map_err(io_error(&target))?;
        if let Err(e) = std::fs::rename(staging, &target) {
            // put the old one back: an install never loses what was there
            std::fs::rename(&old, &target).ok();
            return Err(io_error(&target)(e));
        }
        std::fs::remove_dir_all(&old).ok();
    } else {
        std::fs::rename(staging, &target).map_err(io_error(&target))?;
    }
    let plugin = Plugin::load(&target)?;
    Ok(Installed {
        plugin,
        replaced: previous,
        files: names.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-plugin-pkg-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(version: &str) -> String {
        format!(
            r#"{{"name": "Title Maker", "version": "{version}", "api": "0.1", "entry": "main.py", "permissions": ["read", "edit"]}}"#
        )
    }

    fn make_plugin(dir: &Path, version: &str) {
        std::fs::create_dir_all(dir.join("lib")).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), manifest(version)).unwrap();
        std::fs::write(dir.join("main.py"), "print('hi')\n").unwrap();
        std::fs::write(dir.join("lib").join("util.py"), "x = 1\n").unwrap();
        std::fs::write(dir.join(".secret"), "left out").unwrap();
    }

    #[test]
    fn folder_names_are_plain() {
        assert_eq!(folder_name("Title Maker"), "title-maker");
        assert_eq!(folder_name("  A.B  c_d "), "a-b-c_d");
        assert_eq!(folder_name("..."), "plugin");
        assert_eq!(folder_name("../../etc"), "etc");
    }

    #[test]
    fn unsafe_names_are_refused() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../../x",
            "a\\b",
            "C:/x",
            "a/C:x",
            "x.",
            "x /y",
            "a//b",
        ] {
            assert!(!safe_name(bad), "{bad}");
        }
        for good in ["plugin.json", "lib/util.py", "a b/c.txt"] {
            assert!(safe_name(good), "{good}");
        }
    }

    #[test]
    fn a_plugin_packs_and_installs_and_a_new_version_replaces_the_old() {
        let root = temp("round");
        let src = root.join("src");
        make_plugin(&src, "1.0.0");
        let file = root.join("title-maker.fnplugin");
        assert_eq!(pack(&src, &file).unwrap(), 3, "the hidden file is left out");
        let plugins = root.join("plugins");
        let first = install(&file, &plugins).unwrap();
        assert_eq!((first.replaced.clone(), first.files), (None, 3));
        assert_eq!(first.plugin.dir, plugins.join("title-maker"));
        assert!(plugins.join("title-maker").join("lib").join("util.py").is_file());
        assert!(!plugins.join("title-maker").join(".secret").exists());

        std::fs::write(src.join(MANIFEST_FILE), manifest("1.1.0")).unwrap();
        pack(&src, &file).unwrap();
        let second = install(&file, &plugins).unwrap();
        assert_eq!(second.replaced.as_deref(), Some("1.0.0"));
        assert_eq!(second.plugin.manifest.version, "1.1.0");
        // nothing is left beside the plugin
        let left: Vec<_> = std::fs::read_dir(&plugins).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_package_that_climbs_out_is_refused_and_nothing_is_written() {
        let root = temp("evil");
        let plugins = root.join("plugins");
        let m = manifest("1.0.0");
        for name in ["../escape.txt", "/abs.txt", "lib/../../x", "C:/x.txt"] {
            let zip =
                zip::write(&[(MANIFEST_FILE, m.as_bytes()), ("main.py", b"x"), (name, b"bad")]).unwrap();
            let e = install_bytes(&zip, &plugins).unwrap_err();
            assert!(e.to_string().contains("refused"), "{name}: {e}");
        }
        assert!(!root.join("escape.txt").exists());
        let left = std::fs::read_dir(&plugins).map_or(0, Iterator::count);
        assert_eq!(left, 0, "a refused package leaves nothing");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_damaged_or_incomplete_package_is_refused_without_touching_the_installed_plugin() {
        let root = temp("damaged");
        let src = root.join("src");
        make_plugin(&src, "1.0.0");
        let file = root.join("p.fnplugin");
        pack(&src, &file).unwrap();
        let plugins = root.join("plugins");
        install(&file, &plugins).unwrap();

        assert!(install_bytes(b"not a zip", &plugins).is_err());
        let m = manifest("2.0.0");
        // no manifest
        let zip = zip::write(&[("main.py", b"x")]).unwrap();
        assert!(
            install_bytes(&zip, &plugins)
                .unwrap_err()
                .to_string()
                .contains("plugin.json")
        );
        // a manifest whose entry is not in the package
        let zip = zip::write(&[(MANIFEST_FILE, m.as_bytes())]).unwrap();
        assert!(matches!(
            install_bytes(&zip, &plugins),
            Err(PluginError::EntryMissing(_))
        ));
        // a bad manifest
        let zip = zip::write(&[(MANIFEST_FILE, b"{}")]).unwrap();
        assert!(matches!(
            install_bytes(&zip, &plugins),
            Err(PluginError::Manifest { .. })
        ));
        // a name listed twice
        let zip = zip::write(&[
            (MANIFEST_FILE, m.as_bytes()),
            ("main.py", b"1"),
            ("main.py", b"2"),
        ])
        .unwrap();
        assert!(
            install_bytes(&zip, &plugins)
                .unwrap_err()
                .to_string()
                .contains("twice")
        );

        assert_eq!(
            Plugin::load(&plugins.join("title-maker"))
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        let left: Vec<_> = std::fs::read_dir(&plugins).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn packing_checks_the_manifest_and_the_entry_first() {
        let root = temp("packcheck");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join(MANIFEST_FILE), manifest("1.0.0")).unwrap();
        let out = root.join("p.fnplugin");
        assert!(matches!(pack(&src, &out), Err(PluginError::EntryMissing(_))));
        assert!(!out.exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
