//! The plugin host: finds plugins, runs them, and packs and installs them.
//!
//! A plugin is a folder with a `plugin.json` manifest
//! ([`fanimate_api::Manifest`]) and an `entry`, a program the host starts as
//! a **separate process**. The plugin speaks the scripting API's JSON-RPC
//! call table ([`fanimate_api::rpc`]) over its standard input and output,
//! one request per line, exactly as a script talks to `fanimate-cli script`,
//! and the host answers every call with only the permissions the manifest
//! declares. The host works on an [`Editor`] its caller hands in (the
//! headless CLI today, the app later), so a plugin changes a project only
//! through the API's transactions: one undo step per call, all or nothing.
//!
//! There is no JavaScript or WebAssembly engine inside Fanimate. A plugin is
//! run by the interpreter that its entry's extension names (Python for
//! `.py`, Node for `.js`, `.mjs` and `.ts`) or is a program of its own. An
//! embedded engine would add a large dependency and its own security
//! surface; a separate process costs a start-up and keeps every crash on the
//! plugin's side. The price is that the host is **not a sandbox**: the
//! plugin can do whatever its user can, apart from the project, which it
//! reaches only through the API with the permissions it declared. See
//! `docs/plugins.md`.
//!
//! - [`discover`] lists the plugins in some folders ([`folders`] gives the
//!   user's plugins folder and any others).
//! - [`run`] starts a plugin against an editor, with limits on time, line
//!   length, output and the number of calls, and returns a [`RunReport`].
//! - [`pack`] makes a `.fnplugin` file (a ZIP archive of the folder) and
//!   [`install`] unpacks one into the plugins folder, refusing anything that
//!   would land outside it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use fanimate_api::Manifest;
use fanimate_editor::Editor;

mod host;
mod package;

pub use host::{Ending, Handled, Limit, MAX_LINE, RunOptions, RunReport, run, run_with};
pub use package::{
    Installed, MAX_PACKAGE_BYTES, MAX_PACKAGE_FILES, PACKAGE_EXTENSION, folder_name, install, install_bytes,
    pack,
};

/// The manifest's file name inside a plugin's folder.
pub const MANIFEST_FILE: &str = "plugin.json";

/// How long a plugin may run when its manifest does not say.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// The longest a manifest's `timeout_seconds` can make a run; a larger
/// number is cut to this.
pub const MAX_TIMEOUT_SECONDS: u64 = 600;

/// The most plugins read from one folder, so a folder of thousands of
/// entries cannot hold up a start.
const MAX_PLUGINS_PER_FOLDER: usize = 500;

/// Why a plugin could not be found, read, started, packed or installed. A
/// plugin that starts and then fails is not an error: its [`RunReport`] says
/// how it ended.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("{}: {message}", path.display())]
    Manifest { path: PathBuf, message: String },
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the plugin \"{0}\" has no \"entry\" in its manifest, so there is nothing to run")]
    NoEntry(String),
    #[error("the plugin's entry {} was not found", .0.display())]
    EntryMissing(PathBuf),
    #[error("the plugin's entry {} leads outside the plugin's folder", .0.display())]
    EntryEscapes(PathBuf),
    #[error("the plugin \"{plugin}\" has no command \"{command}\"")]
    UnknownCommand { plugin: String, command: String },
    #[error("{0}")]
    NoInterpreter(String),
    #[error("the plugin could not be started: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("{0}")]
    Package(String),
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> PluginError + '_ {
    move |source| PluginError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A plugin found on disk: its manifest, its folder and how long it may run.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    pub manifest: Manifest,
    /// The folder that holds `plugin.json`; the plugin runs in it.
    pub dir: PathBuf,
    /// The manifest's `timeout_seconds` (at most [`MAX_TIMEOUT_SECONDS`]), or
    /// [`DEFAULT_TIMEOUT`].
    pub timeout: Duration,
    /// The commands the manifest declares (`commands`), each its own entry in
    /// Fanimate's Plugins menu; the plugin is told which one was chosen in
    /// `$FANIMATE_COMMAND`. Empty when it declares none.
    pub commands: Vec<PluginCommand>,
}

/// The most commands a plugin may declare.
pub const MAX_COMMANDS: usize = 20;

/// One thing a plugin can be asked to do, as its manifest declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginCommand {
    /// What the plugin is told (`$FANIMATE_COMMAND`): 1 to 40 letters, digits,
    /// dashes and underscores, starting with a letter or digit.
    pub id: String,
    /// What the menu shows: 1 to 60 characters.
    pub title: String,
}

impl Plugin {
    /// Read the plugin at `path`: its folder, or its `plugin.json`.
    pub fn load(path: &Path) -> Result<Self, PluginError> {
        let (dir, file) = if path.is_dir() {
            (path.to_path_buf(), path.join(MANIFEST_FILE))
        } else {
            let dir = match path.parent() {
                Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
                _ => PathBuf::from("."),
            };
            (dir, path.to_path_buf())
        };
        let text = read_manifest(&file)?;
        Self::from_text(&text, dir).map_err(|message| PluginError::Manifest { path: file, message })
    }

    /// A plugin whose manifest is `text` and whose folder is `dir`. The error
    /// says which field is wrong.
    pub fn from_text(text: &str, dir: PathBuf) -> Result<Self, String> {
        let manifest = Manifest::parse(text)?;
        let timeout = timeout_of(text)?;
        let commands = commands_of(text)?;
        Ok(Self {
            manifest,
            dir,
            timeout,
            commands,
        })
    }

    /// The declared command with this id.
    pub fn command(&self, id: &str) -> Option<&PluginCommand> {
        self.commands.iter().find(|c| c.id == id)
    }

    /// How the entry is started, when the manifest names one.
    pub fn entry_kind(&self) -> Option<EntryKind> {
        self.manifest.entry.as_deref().map(EntryKind::of)
    }

    /// The entry's path, checked: it exists, is a file and, with any links
    /// followed, stays inside the plugin's folder.
    pub fn entry_path(&self) -> Result<PathBuf, PluginError> {
        let entry = self
            .manifest
            .entry
            .as_deref()
            .ok_or_else(|| PluginError::NoEntry(self.manifest.name.clone()))?;
        let path = self.dir.join(entry);
        if !path.is_file() {
            return Err(PluginError::EntryMissing(path));
        }
        let inside = match (path.canonicalize(), self.dir.canonicalize()) {
            (Ok(file), Ok(dir)) => file.starts_with(dir),
            _ => false,
        };
        if !inside {
            return Err(PluginError::EntryEscapes(path));
        }
        // the plugin runs with its folder as the working folder, so the
        // entry is named by a path that does not depend on ours
        std::path::absolute(&path).map_err(io_error(&path))
    }
}

/// Read a manifest file, never more than the manifest limit.
fn read_manifest(file: &Path) -> Result<String, PluginError> {
    use std::io::Read;
    let f = std::fs::File::open(file).map_err(io_error(file))?;
    let mut text = String::new();
    f.take(fanimate_api::MAX_MANIFEST_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(io_error(file))?;
    Ok(text)
}

/// The manifest's `timeout_seconds`: a whole number of seconds, at least 1,
/// cut to [`MAX_TIMEOUT_SECONDS`]. Left out, [`DEFAULT_TIMEOUT`].
fn timeout_of(text: &str) -> Result<Duration, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("the manifest is not valid JSON: {e}"))?;
    match value.get("timeout_seconds") {
        None | Some(serde_json::Value::Null) => Ok(DEFAULT_TIMEOUT),
        Some(v) => match v.as_u64() {
            Some(s) if s >= 1 => Ok(Duration::from_secs(s.min(MAX_TIMEOUT_SECONDS))),
            _ => Err(format!(
                "\"timeout_seconds\" must be a whole number of seconds from 1 to {MAX_TIMEOUT_SECONDS}"
            )),
        },
    }
}

/// The manifest's `commands`: a list of `{"id": "...", "title": "..."}`. Left out,
/// none. Ids are short names that are safe to pass in the environment and
/// are not repeated; titles are plain text for a menu.
fn commands_of(text: &str) -> Result<Vec<PluginCommand>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("the manifest is not valid JSON: {e}"))?;
    let list = match value.get("commands") {
        None | Some(serde_json::Value::Null) => return Ok(Vec::new()),
        Some(serde_json::Value::Array(list)) => list,
        Some(_) => return Err("\"commands\" must be a list of {\"id\", \"title\"}".to_owned()),
    };
    if list.len() > MAX_COMMANDS {
        return Err(format!("a plugin may declare at most {MAX_COMMANDS} commands"));
    }
    let mut out: Vec<PluginCommand> = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let field = |name: &str| item.get(name).and_then(serde_json::Value::as_str);
        let (Some(id), Some(title)) = (field("id"), field("title")) else {
            return Err(format!(
                "command {} needs a text \"id\" and a text \"title\"",
                i + 1
            ));
        };
        let id_ok = !id.is_empty()
            && id.len() <= 40
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric());
        if !id_ok {
            return Err(format!(
                "command \"{id}\": an id is 1 to 40 letters, digits, dashes and underscores, starting with a letter or digit"
            ));
        }
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 60 || title.chars().any(char::is_control) {
            return Err(format!(
                "command \"{id}\": a title is 1 to 60 characters of plain text"
            ));
        }
        if out.iter().any(|c| c.id == id) {
            return Err(format!("the command id \"{id}\" is used twice"));
        }
        out.push(PluginCommand {
            id: id.to_owned(),
            title: title.to_owned(),
        });
    }
    Ok(out)
}

/// What starts a plugin's entry, chosen by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// `.py`: run by Python 3.
    Python,
    /// `.js`, `.mjs`, `.cjs`: run by Node.
    JavaScript,
    /// `.ts`, `.mts`: run by Node 22.6 or later, which strips the types.
    TypeScript,
    /// Anything else: a program of its own.
    Program,
}

impl EntryKind {
    pub fn of(entry: &str) -> Self {
        let ext = Path::new(entry)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "py" => Self::Python,
            "js" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "mts" => Self::TypeScript,
            _ => Self::Program,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::JavaScript => "JavaScript (Node)",
            Self::TypeScript => "TypeScript (Node 22.6 or later)",
            Self::Program => "a program",
        }
    }
}

/// A program that runs plugin code, with the arguments that come before the
/// entry (`py -3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpreter {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl Interpreter {
    fn new(program: impl Into<OsString>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(OsString::from).collect(),
        }
    }

    /// Whether it runs at all: `--version` succeeds. (On Windows a `python3`
    /// that is only the Store's shortcut fails here.)
    fn works(&self) -> bool {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        host::no_window(&mut cmd);
        cmd.status().is_ok_and(|s| s.success())
    }
}

/// The Python that runs `.py` plugins: `$FANIMATE_PYTHON`, else the first of
/// `python`, `py -3` and `python3` (Windows) or `python3` and `python` that
/// runs. Looked for once.
pub fn find_python() -> Option<Interpreter> {
    static FOUND: OnceLock<Option<Interpreter>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let candidates: Vec<Interpreter> = if cfg!(windows) {
                vec![
                    Interpreter::new("python", &[]),
                    Interpreter::new("py", &["-3"]),
                    Interpreter::new("python3", &[]),
                ]
            } else {
                vec![Interpreter::new("python3", &[]), Interpreter::new("python", &[])]
            };
            from_env("FANIMATE_PYTHON")
                .into_iter()
                .chain(candidates)
                .find(Interpreter::works)
        })
        .clone()
}

/// The Node that runs `.js`, `.mjs` and `.ts` plugins: `$FANIMATE_NODE`, else
/// `node`. Looked for once.
pub fn find_node() -> Option<Interpreter> {
    static FOUND: OnceLock<Option<Interpreter>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            from_env("FANIMATE_NODE")
                .into_iter()
                .chain([Interpreter::new("node", &[])])
                .find(Interpreter::works)
        })
        .clone()
}

fn from_env(name: &str) -> Option<Interpreter> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(|program| Interpreter {
            program,
            args: Vec::new(),
        })
}

/// The program and arguments that start `entry`, an entry of kind `kind`,
/// with the interpreters given (none: not found on this computer).
pub fn command_line(
    kind: EntryKind,
    entry: &Path,
    python: Option<&Interpreter>,
    node: Option<&Interpreter>,
) -> Result<(OsString, Vec<OsString>), PluginError> {
    let with = |i: &Interpreter, flags: &[&str]| {
        let mut args = i.args.clone();
        args.extend(flags.iter().map(OsString::from));
        args.push(entry.as_os_str().to_owned());
        (i.program.clone(), args)
    };
    match kind {
        EntryKind::Python => python.map(|p| with(p, &[])).ok_or_else(|| {
            PluginError::NoInterpreter(
                "the plugin is written in Python, and no Python 3 was found: install it, or set FANIMATE_PYTHON to its path"
                    .to_owned(),
            )
        }),
        EntryKind::JavaScript => node.map(|n| with(n, &[])).ok_or_else(no_node),
        EntryKind::TypeScript => node
            .map(|n| {
                with(
                    n,
                    &["--experimental-strip-types", "--disable-warning=ExperimentalWarning"],
                )
            })
            .ok_or_else(no_node),
        EntryKind::Program => Ok((entry.as_os_str().to_owned(), Vec::new())),
    }
}

fn no_node() -> PluginError {
    PluginError::NoInterpreter(
        "the plugin is written in JavaScript or TypeScript, and Node was not found: install Node (22.6 or later for TypeScript), or set FANIMATE_NODE to its path"
            .to_owned(),
    )
}

/// The user's plugins folder: `$FANIMATE_PLUGIN_DIR` if set, else `plugins`
/// in Fanimate's folder for the user's data (the one that holds the
/// settings). `None` when the system names no such folder.
pub fn user_folder() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("FANIMATE_PLUGIN_DIR") {
        return Some(dir);
    }
    // the same folder the app keeps its settings in
    let data = if cfg!(windows) {
        env("APPDATA")?.join("Fanimate").join("data")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/Fanimate")
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".local/share")))?
            .join("fanimate")
    };
    Some(data.join("plugins"))
}

/// Where plugins are looked for: the user's plugins folder, then `extra`.
pub fn folders(extra: &[PathBuf]) -> Vec<PathBuf> {
    user_folder().into_iter().chain(extra.iter().cloned()).collect()
}

/// What [`discover`] found.
#[derive(Debug, Default)]
pub struct Discovery {
    /// Every plugin that could be read, by name.
    pub plugins: Vec<Plugin>,
    /// Folders that look like plugins and could not be read, with why.
    pub problems: Vec<(PathBuf, String)>,
}

/// Every plugin in `folders`: each sub-folder holding a `plugin.json`. A
/// folder that does not exist is skipped, a manifest that cannot be read is
/// a problem, not a failure, and when two plugins share a name the first
/// folder's wins. Folders whose names start with `.` are skipped (an
/// install in progress uses them).
pub fn discover(folders: &[PathBuf]) -> Discovery {
    let mut found = Discovery::default();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .filter(|p| p.join(MANIFEST_FILE).is_file())
            .collect();
        dirs.sort();
        if dirs.len() > MAX_PLUGINS_PER_FOLDER {
            found.problems.push((
                folder.clone(),
                format!("more than {MAX_PLUGINS_PER_FOLDER} plugins: only the first are read"),
            ));
            dirs.truncate(MAX_PLUGINS_PER_FOLDER);
        }
        for dir in dirs {
            match Plugin::load(&dir) {
                Ok(p) => {
                    let taken = found
                        .plugins
                        .iter()
                        .find(|q| q.manifest.name.eq_ignore_ascii_case(&p.manifest.name));
                    if let Some(first) = taken {
                        let message = format!(
                            "a plugin called \"{}\" is already in {}",
                            p.manifest.name,
                            first.dir.display()
                        );
                        found.problems.push((dir, message));
                    } else {
                        found.plugins.push(p);
                    }
                }
                Err(e) => found.problems.push((dir, e.to_string())),
            }
        }
    }
    found.plugins.sort_by_key(|p| p.manifest.name.to_lowercase());
    found
}

/// Run `plugin` against `editor` with the default limits. See [`run`].
pub fn run_default(editor: &mut Editor, plugin: &Plugin) -> Result<RunReport, PluginError> {
    run(editor, plugin, &RunOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-plugin-lib-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_plugin(dir: &Path, manifest: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    }

    const GOOD: &str = r#"{"name": "Title Maker", "version": "1.0.0", "api": "0.1", "entry": "main.py", "permissions": ["read", "edit"]}"#;

    #[test]
    fn the_entry_kind_follows_the_extension() {
        assert_eq!(EntryKind::of("main.py"), EntryKind::Python);
        assert_eq!(EntryKind::of("tools/MAIN.PY"), EntryKind::Python);
        assert_eq!(EntryKind::of("main.js"), EntryKind::JavaScript);
        assert_eq!(EntryKind::of("main.mjs"), EntryKind::JavaScript);
        assert_eq!(EntryKind::of("main.cjs"), EntryKind::JavaScript);
        assert_eq!(EntryKind::of("main.ts"), EntryKind::TypeScript);
        assert_eq!(EntryKind::of("main.mts"), EntryKind::TypeScript);
        assert_eq!(EntryKind::of("bin/tool.exe"), EntryKind::Program);
        assert_eq!(EntryKind::of("tool"), EntryKind::Program);
    }

    #[test]
    fn each_kind_is_started_by_its_interpreter_or_says_what_is_missing() {
        let py = Interpreter::new("py", &["-3"]);
        let node = Interpreter::new("node", &[]);
        let entry = Path::new("/p/main");
        let os = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        let (prog, args) = command_line(EntryKind::Python, entry, Some(&py), Some(&node)).unwrap();
        assert_eq!((prog, args), ("py".into(), os(&["-3", "/p/main"])));
        let (prog, args) = command_line(EntryKind::JavaScript, entry, None, Some(&node)).unwrap();
        assert_eq!((prog, args), ("node".into(), os(&["/p/main"])));
        let (_, args) = command_line(EntryKind::TypeScript, entry, None, Some(&node)).unwrap();
        assert_eq!(args[0], "--experimental-strip-types");
        assert_eq!(args.last().unwrap(), "/p/main");
        let (prog, args) = command_line(EntryKind::Program, entry, None, None).unwrap();
        assert_eq!((prog, args.len()), ("/p/main".into(), 0));
        let e = command_line(EntryKind::Python, entry, None, Some(&node)).unwrap_err();
        assert!(e.to_string().contains("FANIMATE_PYTHON"), "{e}");
        let e = command_line(EntryKind::TypeScript, entry, Some(&py), None).unwrap_err();
        assert!(e.to_string().contains("22.6"), "{e}");
    }

    #[test]
    fn a_timeout_is_read_capped_and_checked() {
        let with = |t: &str| {
            let text = GOOD.replace(
                "\"permissions\"",
                &format!("\"timeout_seconds\": {t}, \"permissions\""),
            );
            Plugin::from_text(&text, PathBuf::from("."))
        };
        assert_eq!(
            Plugin::from_text(GOOD, PathBuf::from(".")).unwrap().timeout,
            DEFAULT_TIMEOUT
        );
        assert_eq!(with("5").unwrap().timeout, Duration::from_secs(5));
        assert_eq!(
            with("100000").unwrap().timeout,
            Duration::from_secs(MAX_TIMEOUT_SECONDS)
        );
        for bad in ["0", "-3", "1.5", "\"60\"", "true"] {
            assert!(with(bad).unwrap_err().contains("timeout_seconds"), "{bad}");
        }
    }

    #[test]
    fn a_plugin_loads_from_its_folder_or_its_manifest_and_a_bad_one_names_the_file() {
        let dir = temp("load");
        write_plugin(&dir.join("t"), GOOD);
        let a = Plugin::load(&dir.join("t")).unwrap();
        let b = Plugin::load(&dir.join("t").join(MANIFEST_FILE)).unwrap();
        assert_eq!(a.manifest, b.manifest);
        assert_eq!(a.entry_kind(), Some(EntryKind::Python));
        // the entry does not exist yet
        assert!(matches!(a.entry_path(), Err(PluginError::EntryMissing(_))));
        std::fs::write(dir.join("t").join("main.py"), "").unwrap();
        assert!(a.entry_path().unwrap().is_absolute());

        write_plugin(
            &dir.join("bad"),
            r#"{"name": "x", "version": "1", "api": "9.9", "permissions": []}"#,
        );
        let e = Plugin::load(&dir.join("bad")).unwrap_err();
        assert!(matches!(e, PluginError::Manifest { .. }));
        let text = e.to_string();
        assert!(text.contains("plugin.json") && text.contains("API 9.9"), "{text}");
        assert!(matches!(
            Plugin::load(&dir.join("missing")),
            Err(PluginError::Io { .. })
        ));
        let none = Plugin::from_text(
            r#"{"name": "x", "version": "1", "api": "0.1", "permissions": []}"#,
            dir.clone(),
        )
        .unwrap();
        assert!(matches!(none.entry_path(), Err(PluginError::NoEntry(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn discovery_reads_every_plugin_reports_bad_ones_and_keeps_the_first_of_a_name() {
        let first = temp("disc-a");
        let second = temp("disc-b");
        write_plugin(&first.join("b"), &GOOD.replace("Title Maker", "Beta"));
        write_plugin(&first.join("a"), &GOOD.replace("Title Maker", "alpha"));
        write_plugin(&first.join("broken"), "{ not json");
        write_plugin(&first.join(".installing"), GOOD);
        std::fs::create_dir_all(first.join("not-a-plugin")).unwrap();
        write_plugin(&second.join("again"), &GOOD.replace("Title Maker", "BETA"));
        write_plugin(&second.join("c"), GOOD);
        let found = discover(&[first.clone(), first.join("no-such-folder"), second.clone()]);
        let names: Vec<&str> = found.plugins.iter().map(|p| p.manifest.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Beta", "Title Maker"]);
        assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
        assert!(
            found
                .problems
                .iter()
                .any(|(p, m)| p.ends_with("broken") && m.contains("JSON"))
        );
        assert!(
            found
                .problems
                .iter()
                .any(|(p, m)| p.ends_with("again") && m.contains("already"))
        );
        std::fs::remove_dir_all(&first).ok();
        std::fs::remove_dir_all(&second).ok();
    }

    #[test]
    fn the_user_folder_is_named_plugins() {
        // FANIMATE_PLUGIN_DIR is not set by the test runner; either way the
        // folder is somewhere definite
        if std::env::var_os("FANIMATE_PLUGIN_DIR").is_none()
            && let Some(dir) = user_folder()
        {
            assert!(dir.ends_with("plugins"), "{}", dir.display());
        }
        assert_eq!(folders(&[PathBuf::from("x")]).last(), Some(&PathBuf::from("x")));
    }

    fn with_commands(commands: &str) -> Result<Plugin, String> {
        let text = format!(
            r#"{{"name": "T", "version": "1.0", "api": "0.1", "entry": "main.py", "permissions": [], "commands": {commands}}}"#
        );
        Plugin::from_text(&text, PathBuf::from("."))
    }

    #[test]
    fn declared_commands_are_read_with_their_ids_and_titles() {
        let none = Plugin::from_text(
            r#"{"name": "T", "version": "1.0", "api": "0.1", "entry": "main.py", "permissions": []}"#,
            PathBuf::from("."),
        )
        .unwrap();
        assert!(none.commands.is_empty());
        let p = with_commands(
            r#"[{"id": "wobble", "title": " Wobble the layer "}, {"id": "n-2_x", "title": "Number two"}]"#,
        )
        .unwrap();
        assert_eq!(p.commands.len(), 2);
        assert_eq!(p.command("wobble").unwrap().title, "Wobble the layer");
        assert!(p.command("missing").is_none());
        assert_eq!(with_commands("null").unwrap().commands.len(), 0);
    }

    #[test]
    fn bad_commands_are_refused_with_a_message_that_names_the_problem() {
        let long_id = "x".repeat(41);
        let long_title = "y".repeat(61);
        let many: Vec<String> = (0..=MAX_COMMANDS)
            .map(|i| format!(r#"{{"id": "c{i}", "title": "C"}}"#))
            .collect();
        for (bad, expect) in [
            ("{}", "must be a list"),
            ("[1]", "needs a text"),
            (r#"[{"id": "a"}]"#, "needs a text"),
            (r#"[{"id": "", "title": "T"}]"#, "an id is 1 to 40"),
            (r#"[{"id": "a b", "title": "T"}]"#, "an id is 1 to 40"),
            (r#"[{"id": "-a", "title": "T"}]"#, "an id is 1 to 40"),
            (r#"[{"id": "a=b", "title": "T"}]"#, "an id is 1 to 40"),
            (
                &format!(r#"[{{"id": "{long_id}", "title": "T"}}]"#),
                "an id is 1 to 40",
            ),
            (r#"[{"id": "a", "title": "  "}]"#, "a title is"),
            (
                &format!(r#"[{{"id": "a", "title": "{long_title}"}}]"#),
                "a title is",
            ),
            (r#"[{"id": "a", "title": "x\u0007y"}]"#, "a title is"),
            (
                r#"[{"id": "a", "title": "A"}, {"id": "a", "title": "B"}]"#,
                "used twice",
            ),
            (&format!("[{}]", many.join(",")), "at most"),
        ] {
            let e = with_commands(bad).unwrap_err();
            assert!(e.contains(expect), "{bad}: {e}");
        }
    }
}
