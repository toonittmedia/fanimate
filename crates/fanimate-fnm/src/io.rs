//! Reading and writing `.fnm` projects: one file (a ZIP archive of the
//! project folder), or a project folder.

use std::fs;
use std::path::{Path, PathBuf};

use std::sync::Arc;

use fanimate_core::{Camera, Color, Document, Frame, Guide, Layer, LayerKind, MediaFile};
use serde::{Deserialize, Serialize};

use crate::validate::{self, Report};
use crate::zip;
use crate::{FnmError, Result};

pub const FORMAT_NAME: &str = "fnm";
/// Version written by this build. Readers accept this version and older.
///
/// - 0: the Phase 1 engine spike.
/// - 1: shapes may carry a stroke width profile (`widths`), written by the
///   brush. Version 0 files need no conversion.
/// - 2: shapes may carry point keyframes (`path_keys`). Older files need
///   no conversion.
/// - 3: layers may carry a label colour (`label`). Older files need no
///   conversion; their layers are shown with a colour picked by id.
/// - 4: layers may carry a pivot, the centre of their rotation and scale
///   (`transform.pivot`), and shapes may say they are rectangles (`rect`).
///   Older files need no conversion: without a pivot a layer turns about its
///   origin, as it always did.
/// - 5: the project may carry guides (`guides` in `document.json`), layers
///   may be locked (`locked`) and shapes hidden (`visible: false`). Older
///   files need no conversion: they have no guides, and nothing locked or
///   hidden.
/// - 6: frames count from 1 and frame 0 is the rest pose, channels may loop
///   (`loop_mode`), layers inside groups may be reordered over time (`order`),
///   and a project may hold a preview picture (`thumbnail.png`). Older files
///   are converted on load: every frame moves up by one.
/// - 7: the manifest may list the features a reader needs (`requires`), and
///   layers may carry a record of what made their content (`provenance`).
///   Older files need no conversion: they require nothing and record no
///   provenance.
pub const FORMAT_VERSION: u32 = 7;

/// The feature a project needs when a layer carries a `provenance` record. A
/// reader that dropped the record would hide that a tool made the content.
pub const FEATURE_PROVENANCE: &str = "provenance@1";

/// The feature a project needs when it has a text layer: older readers have no
/// text layer kind to draw it with.
pub const FEATURE_TEXT: &str = "text@1";

/// The feature a project needs when a rig has master controllers: a reader that
/// ignored them would show the rig unposed.
pub const FEATURE_CONTROLLERS: &str = "controllers@1";

/// The features this build can read faithfully. A project whose manifest
/// requires anything else is refused, naming it (see [`required_features`]).
pub const SUPPORTED_FEATURES: &[&str] = &[FEATURE_PROVENANCE, FEATURE_TEXT, FEATURE_CONTROLLERS];

/// The longest feature name quoted back in an error, so a damaged manifest
/// cannot flood a message.
const MAX_FEATURE_NAME_SHOWN: usize = 80;

/// The features `doc` needs from a reader, written to the manifest. Empty for
/// a project that uses only what every version 7 reader understands.
pub fn required_features(doc: &Document) -> Vec<String> {
    let mut features = Vec::new();
    let (mut provenance, mut text, mut controllers) = (false, false, false);
    doc.walk(|l| {
        provenance |= l.provenance.is_some();
        text |= matches!(l.kind, LayerKind::Text(_));
        controllers |=
            matches!(&l.kind, LayerKind::Bone { skeleton, .. } if !skeleton.controllers.is_empty());
    });
    if provenance {
        features.push(FEATURE_PROVENANCE.to_owned());
    }
    if text {
        features.push(FEATURE_TEXT.to_owned());
    }
    if controllers {
        features.push(FEATURE_CONTROLLERS.to_owned());
    }
    features
}

/// The first version whose frames count from 1. Files before it start at
/// frame 0 and are shifted up by one on load, so they look the same.
pub(crate) const ONE_BASED_FRAMES: u32 = 6;

/// The optional preview picture at the root of a project.
pub const THUMBNAIL_NAME: &str = "thumbnail.png";
/// The largest preview picture that is saved or read. A bigger one is ignored.
pub const MAX_THUMBNAIL_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize)]
pub(crate) struct Manifest {
    format: String,
    version: u32,
    #[serde(default)]
    generator: String,
    /// Version 7: the features a reader needs to open the project faithfully.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    requires: Vec<String>,
}

/// `document.json`: everything except the layers themselves.
#[derive(Serialize, Deserialize)]
pub(crate) struct DocumentFile {
    name: String,
    width: f32,
    height: f32,
    fps: f32,
    start: Frame,
    end: Frame,
    background: Color,
    next_layer_id: u32,
    /// Version 5: guides placed from the rulers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    guides: Vec<Guide>,
    /// Version 5: the camera.
    #[serde(default, skip_serializing_if = "Camera::is_default")]
    camera: Camera,
    /// Version 6: the saved brush and shape styles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    styles: Vec<fanimate_core::StylePreset>,
    /// Top-level layer files, bottom to top, relative to the project folder.
    layers: Vec<String>,
}

pub(crate) fn layer_file(layer: &Layer) -> String {
    format!("layers/{:04}.json", layer.id.0)
}

/// Load a project and validate it. Problems are returned in the report but
/// not repaired; use [`load_and_repair`] to fix what can be fixed safely.
pub fn load(path: impl AsRef<Path>) -> Result<(Document, Report)> {
    let doc = read(path.as_ref())?;
    let report = validate::validate(&doc);
    Ok((doc, report))
}

/// Load a project, repairing anything the validator can fix safely.
pub fn load_and_repair(path: impl AsRef<Path>) -> Result<(Document, Report)> {
    let mut doc = read(path.as_ref())?;
    let report = validate::repair(&mut doc);
    Ok((doc, report))
}

/// Load a project from somewhere other than a folder on disk, such as a
/// ZIP archive or a browser upload: `files` reads a file by its path inside
/// the project folder (`manifest.json`, `layers/0001.json`, ...). `name` is
/// used in error messages. Validated, and repaired where that is safe.
pub fn load_from(name: &str, files: &dyn Fn(&str) -> std::io::Result<Vec<u8>>) -> Result<(Document, Report)> {
    let mut doc = read_from(Path::new(name), files)?;
    let report = validate::repair(&mut doc);
    Ok((doc, report))
}

fn read(path: &Path) -> Result<Document> {
    if path.is_dir() {
        if !path.join("manifest.json").is_file() {
            return Err(FnmError::NotFnm(path.to_path_buf()));
        }
        return read_from(path, &|rel| fs::read(path.join(rel)));
    }
    if !path.is_file() {
        return Err(FnmError::NotFnm(path.to_path_buf()));
    }
    let bytes = fs::read(path).map_err(|e| io_err(path, e))?;
    read_archive(path, &bytes)
}

/// Read a single-file project. The project folder may be the archive's
/// root, or one folder down (a project folder zipped by hand).
fn read_archive(path: &Path, bytes: &[u8]) -> Result<Document> {
    let not_fnm = || FnmError::NotFnm(path.to_path_buf());
    if !zip::is_zip(bytes) {
        return Err(not_fnm());
    }
    let archive = zip::Archive::read(bytes).map_err(|e| io_err(path, std::io::Error::other(e)))?;
    let prefix = archive_prefix(&archive).ok_or_else(not_fnm)?;
    read_from(
        path,
        &|rel| match archive.get_checked(&format!("{prefix}{rel}")) {
            Some(Ok(bytes)) => Ok(bytes),
            Some(Err(e)) => Err(std::io::Error::other(e)),
            None => Err(std::io::ErrorKind::NotFound.into()),
        },
    )
}

/// The folder inside an archive that holds the project: empty when the
/// project is at the root, `"name/"` when it is one folder down.
fn archive_prefix(archive: &zip::Archive) -> Option<String> {
    let manifest = archive
        .names()
        .filter(|n| n.rsplit('/').next() == Some("manifest.json") && n.matches('/').count() <= 1)
        .min_by_key(|n| n.len())?;
    Some(manifest.trim_end_matches("manifest.json").to_owned())
}

/// Load a project held in memory, such as one uploaded in the browser:
/// `name` is used in error messages. Validated, and repaired where that is
/// safe.
pub fn load_archive(name: &str, bytes: &[u8]) -> Result<(Document, Report)> {
    let mut doc = read_archive(Path::new(name), bytes)?;
    let report = validate::repair(&mut doc);
    Ok((doc, report))
}

fn read_from(base: &Path, files: &dyn Fn(&str) -> std::io::Result<Vec<u8>>) -> Result<Document> {
    let json = |rel: &str| -> Result<Vec<u8>> { files(rel).map_err(|e| io_err(&base.join(rel), e)) };
    let manifest: Manifest = match files("manifest.json") {
        Ok(bytes) => parse_json(&base.join("manifest.json"), &bytes)?,
        Err(_) => return Err(FnmError::NotFnm(base.to_path_buf())),
    };
    if manifest.format != FORMAT_NAME {
        return Err(FnmError::NotFnm(base.to_path_buf()));
    }
    if manifest.version > FORMAT_VERSION {
        return Err(FnmError::UnsupportedVersion {
            found: manifest.version,
            supported: FORMAT_VERSION,
        });
    }
    if let Some(missing) = manifest
        .requires
        .iter()
        .find(|f| !SUPPORTED_FEATURES.contains(&f.as_str()))
    {
        return Err(FnmError::MissingFeature {
            feature: missing.chars().take(MAX_FEATURE_NAME_SHOWN).collect(),
        });
    }

    let file: DocumentFile = parse_json(&base.join("document.json"), &json("document.json")?)?;
    let layers = file
        .layers
        .iter()
        .map(|rel| parse_json::<Layer>(&base.join(rel), &json(rel)?))
        .collect::<Result<Vec<_>>>()?;

    let mut doc = Document {
        name: file.name,
        width: file.width,
        height: file.height,
        fps: file.fps,
        start: file.start,
        end: file.end,
        background: file.background,
        layers,
        next_layer_id: file.next_layer_id,
        guides: file.guides,
        camera: file.camera,
        styles: file.styles,
        action_edit: None,
        media: Default::default(),
    };
    // Before version 6 the first frame was 0. Shift everything up by one so the
    // project plays as it did and frame 0 is left free for the rest pose; no key
    // is invented there. Only this reader shifts: the journal and the writers
    // always write the current version, so nothing is shifted twice.
    if manifest.version < ONE_BASED_FRAMES {
        doc.shift_frames(1);
    }
    // bones get ids of their own (version 7); older files get theirs here, and
    // the same ones on every load
    doc.assign_ids();
    // pictures used by image layers; a missing one is reported by the
    // validator and drawn as a grey placeholder, never a failed load
    for name in media_names(&doc) {
        if let Ok(bytes) = files(&name) {
            doc.media.insert(name, Arc::new(MediaFile { bytes }));
        }
    }
    Ok(doc)
}

/// Whether `name` is a media file name Fanimate reads and writes: a plain
/// file name inside the project's `media` folder, so a damaged project can
/// never point outside its own folder.
pub fn is_media_name(name: &str) -> bool {
    name.strip_prefix("media/").is_some_and(|f| {
        !f.is_empty()
            && f.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && !f.starts_with('.')
    })
}

/// The media files image layers use, with safe names.
pub(crate) fn media_names(doc: &Document) -> Vec<String> {
    let mut names = Vec::new();
    doc.walk(|l| {
        let media = match &l.kind {
            LayerKind::Image(img) => Some(&img.media),
            LayerKind::Audio(a) => Some(&a.media),
            _ => None,
        };
        if let Some(m) = media
            && is_media_name(m)
            && !names.contains(m)
        {
            names.push(m.clone());
        }
    });
    names
}

/// Write the media files `doc` uses into project folder `dir`, skipping
/// any already there (their names follow their content).
pub(crate) fn write_media(doc: &Document, dir: &Path) -> Result<()> {
    let names = media_names(doc);
    if names.is_empty() {
        return Ok(());
    }
    let media_dir = dir.join("media");
    fs::create_dir_all(&media_dir).map_err(|e| io_err(&media_dir, e))?;
    for name in names {
        let Some(file) = doc.media.get(&name) else {
            continue;
        };
        let path = dir.join(&name);
        if !path.is_file() {
            let tmp = path.with_extension("tmp");
            fs::write(&tmp, &file.bytes).map_err(|e| io_err(&tmp, e))?;
            fs::rename(&tmp, &path).map_err(|e| io_err(&path, e))?;
        }
    }
    Ok(())
}

/// Save a project.
///
/// A new project, or one already in a single file, is written as a single
/// file. A project that is already a folder stays a folder (so a project in
/// Git keeps its small, per-layer diffs); use [`save_folder`] to write a
/// folder where there is none yet.
///
/// Either way a crash or full disk mid-save never leaves a half-written
/// project: the new version is written beside the old one, and swapped in
/// only once it is complete.
///
/// A preview picture already in the project is kept.
pub fn save(doc: &Document, path: impl AsRef<Path>) -> Result<()> {
    save_with_thumbnail(doc, path, None)
}

/// Like [`save`], with a preview picture (`thumbnail.png`) for the project:
/// a PNG of at most [`MAX_THUMBNAIL_BYTES`]. A picture that is not a PNG or is
/// too big is ignored, as is `None`; in both cases the project's existing
/// preview, if it has one, is kept (it may be out of date, but is never lost
/// to a save that did not make a new one).
pub fn save_with_thumbnail(
    doc: &Document,
    path: impl AsRef<Path>,
    thumbnail_png: Option<&[u8]>,
) -> Result<()> {
    let path = path.as_ref();
    let old;
    let thumbnail: Option<&[u8]> = match thumbnail_png.filter(|t| is_thumbnail(t)) {
        Some(t) => Some(t),
        None => {
            old = read_thumbnail(path);
            old.as_deref()
        }
    };
    if path.is_dir() {
        save_folder_with(doc, path, thumbnail)
    } else {
        save_file(doc, path, thumbnail)
    }
}

/// Whether `bytes` can be a project's preview: a PNG, and small.
fn is_thumbnail(bytes: &[u8]) -> bool {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    bytes.len() <= MAX_THUMBNAIL_BYTES && bytes.starts_with(PNG)
}

/// The preview picture of the project at `path` (a file or a folder), if it
/// has a sound one. Anything else, including a project that cannot be read,
/// is `None`: a preview is never worth an error.
pub fn read_thumbnail(path: impl AsRef<Path>) -> Option<Vec<u8>> {
    let path = path.as_ref();
    let bytes = if path.is_dir() {
        let file = path.join(THUMBNAIL_NAME);
        if fs::metadata(&file).ok()?.len() > MAX_THUMBNAIL_BYTES as u64 {
            return None;
        }
        fs::read(file).ok()?
    } else {
        let bytes = fs::read(path).ok()?;
        if !zip::is_zip(&bytes) {
            return None;
        }
        let archive = zip::Archive::read(&bytes).ok()?;
        let name = format!("{}{THUMBNAIL_NAME}", archive_prefix(&archive)?);
        // checked before inflating, so a hostile entry cannot use up memory
        // (a size of 0 would lift the inflater's limit, and is no picture)
        if !(1..=MAX_THUMBNAIL_BYTES).contains(&archive.size(&name)?) {
            return None;
        }
        archive.get_checked(&name)?.ok()?
    };
    is_thumbnail(&bytes).then_some(bytes)
}

/// Write `doc` as a single file at `path`. The finished archive is read
/// back and checked before it replaces anything.
fn save_file(doc: &Document, path: &Path, thumbnail: Option<&[u8]>) -> Result<()> {
    // an action being edited is put away first, so the timeline is saved
    let doc = &*doc.mainline();
    let files = project_files(doc, thumbnail)?;
    let entries: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let bytes = zip::write(&entries).map_err(|e| io_err(path, std::io::Error::other(e)))?;
    let written = read_archive(path, &bytes)?;
    if written.layers.len() != doc.layers.len() {
        return Err(io_err(
            path,
            std::io::Error::other("the saved copy did not read back; nothing was changed"),
        ));
    }

    let tmp = sibling(path, "saving");
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut file, &bytes)?;
        // on disk, not just in the system's cache, before it replaces the old one
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io_err(path, e));
    }
    Ok(())
}

/// The files of a project folder: name inside the project and contents.
fn project_files(doc: &Document, thumbnail: Option<&[u8]>) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = vec![
        (
            "manifest.json".to_owned(),
            json_bytes(Path::new("manifest.json"), &manifest(doc))?,
        ),
        (
            "document.json".to_owned(),
            json_bytes(Path::new("document.json"), &document_file(doc))?,
        ),
    ];
    for layer in &doc.layers {
        let name = layer_file(layer);
        let bytes = json_bytes(Path::new(&name), layer)?;
        files.push((name, bytes));
    }
    if let Some(png) = thumbnail {
        files.push((THUMBNAIL_NAME.to_owned(), png.to_vec()));
    }
    for name in media_names(doc) {
        if let Some(file) = doc.media.get(&name) {
            files.push((name, file.bytes.clone()));
        }
    }
    Ok(files)
}

/// Save a project folder.
///
/// The project is written to a temporary folder next to `path` and swapped
/// into place only once every file is written.
pub fn save_folder(doc: &Document, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    // the folder is replaced whole, so its preview is carried over
    let kept = read_thumbnail(path);
    save_folder_with(doc, path, kept.as_deref())
}

fn save_folder_with(doc: &Document, path: &Path, thumbnail: Option<&[u8]>) -> Result<()> {
    // an action being edited is put away first, so the timeline is saved
    let doc = &*doc.mainline();
    let tmp = sibling(path, "saving");
    let old = sibling(path, "old");
    let _ = fs::remove_dir_all(&tmp);

    write_all(doc, &tmp, thumbnail)?;

    let had_old = path.exists();
    if had_old {
        let _ = fs::remove_dir_all(&old);
        fs::rename(path, &old).map_err(|e| io_err(path, e))?;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        // put the previous version back before reporting the failure
        if had_old {
            let _ = fs::rename(&old, path);
        }
        return Err(io_err(path, e));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

fn write_all(doc: &Document, dir: &Path, thumbnail: Option<&[u8]>) -> Result<()> {
    let layers_dir = dir.join("layers");
    fs::create_dir_all(&layers_dir).map_err(|e| io_err(&layers_dir, e))?;

    write_json(&dir.join("manifest.json"), &manifest(doc))?;
    write_json(&dir.join("document.json"), &document_file(doc))?;
    for layer in &doc.layers {
        write_json(&dir.join(layer_file(layer)), layer)?;
    }
    if let Some(png) = thumbnail {
        let file = dir.join(THUMBNAIL_NAME);
        fs::write(&file, png).map_err(|e| io_err(&file, e))?;
    }
    write_media(doc, dir)
}

/// The format version a project folder says it was written with, or `None`
/// if its manifest cannot be read.
pub(crate) fn folder_version(dir: &Path) -> Option<u32> {
    let bytes = fs::read(dir.join("manifest.json")).ok()?;
    serde_json::from_slice::<Manifest>(&bytes).ok().map(|m| m.version)
}

pub(crate) fn manifest(doc: &Document) -> Manifest {
    Manifest {
        format: FORMAT_NAME.into(),
        version: FORMAT_VERSION,
        generator: concat!("fanimate ", env!("CARGO_PKG_VERSION")).into(),
        requires: required_features(doc),
    }
}

pub(crate) fn document_file(doc: &Document) -> DocumentFile {
    DocumentFile {
        name: doc.name.clone(),
        width: doc.width,
        height: doc.height,
        fps: doc.fps,
        start: doc.start,
        end: doc.end,
        background: doc.background,
        next_layer_id: doc.next_layer_id,
        guides: doc.guides.clone(),
        camera: doc.camera.clone(),
        styles: doc.styles.clone(),
        layers: doc.layers.iter().map(layer_file).collect(),
    }
}

/// Write a JSON file so that it is never seen half-written: write a
/// temporary file beside it, then rename it over the old one.
pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    write_json(&tmp, value)?;
    fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

fn sibling(path: &Path, tag: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project.fnm".into());
    path.with_file_name(format!(".{name}.{tag}"))
}

fn parse_json<T: for<'de> Deserialize<'de>>(path: &Path, bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|source| FnmError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn json_bytes<T: Serialize>(path: &Path, value: &T) -> Result<Vec<u8>> {
    let mut text = serde_json::to_string_pretty(value).map_err(|source| FnmError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    fs::write(path, json_bytes(path, value)?).map_err(|e| io_err(path, e))
}

pub(crate) fn io_err(path: &Path, source: std::io::Error) -> FnmError {
    FnmError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::eval::evaluate;
    use fanimate_core::samples;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fanimate-fnm-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_project_loads_from_files_in_memory() {
        let dir = temp_dir("from-memory");
        let path = dir.join("hello.fnm");
        let doc = samples::hello();
        save_folder(&doc, &path).unwrap();
        let read = |rel: &str| fs::read(path.join(rel));
        let (loaded, report) = load_from("hello.fnm", &read).unwrap();
        assert_eq!(loaded, doc);
        assert!(report.is_clean(), "{report}");
        // no manifest: not a project; a missing layer: an error naming it
        let none = |_: &str| -> std::io::Result<Vec<u8>> { Err(std::io::ErrorKind::NotFound.into()) };
        assert!(matches!(load_from("x", &none), Err(FnmError::NotFnm(_))));
        let partial = |rel: &str| {
            if rel.starts_with("layers/") {
                Err(std::io::ErrorKind::NotFound.into())
            } else {
                fs::read(path.join(rel))
            }
        };
        assert!(
            load_from("x", &partial)
                .unwrap_err()
                .to_string()
                .contains("layers")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn round_trip_preserves_document() {
        let dir = temp_dir("round-trip");
        let path = dir.join("bench.fnm");
        let doc = samples::benchmark(40);
        save(&doc, &path).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert_eq!(loaded, doc);
        assert!(report.is_clean(), "{report}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// Every file in a project folder, by path relative to it.
    fn project_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    let rel = path.strip_prefix(root).unwrap().to_path_buf();
                    out.push((rel, fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    /// The engine's headless promise (WS1): a scene built and animated in
    /// code, saved and reopened with no UI, evaluates identically on every
    /// frame, and saving it again writes the same bytes.
    #[test]
    fn reopened_project_is_deterministic() {
        let dir = temp_dir("deterministic");
        let doc = samples::benchmark(40);
        let (first, second) = (dir.join("a.fnm"), dir.join("b.fnm"));
        save(&doc, &first).unwrap();
        let (loaded, _) = load(&first).unwrap();
        save(&loaded, &second).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
        let (folder_a, folder_b) = (dir.join("a-folder.fnm"), dir.join("b-folder.fnm"));
        save_folder(&doc, &folder_a).unwrap();
        save_folder(&loaded, &folder_b).unwrap();
        assert_eq!(project_files(&folder_a), project_files(&folder_b));

        let mut half_frames = doc.start * 2..=doc.end * 2;
        assert!(half_frames.all(|f| {
            let frame = f as f32 / 2.0;
            let (a, b) = (evaluate(&doc, frame), evaluate(&loaded, frame));
            a.len() == b.len()
                && a.iter().zip(&b).all(|(a, b)| {
                    a.layer == b.layer
                        && a.shape == b.shape
                        && a.transform == b.transform
                        && a.opacity == b.opacity
                })
        }));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_replaces_existing_project() {
        let dir = temp_dir("replace");
        let path = dir.join("p.fnm");
        save(&samples::benchmark(40), &path).unwrap();
        let hello = samples::hello();
        save(&hello, &path).unwrap();
        assert_eq!(load(&path).unwrap().0, hello);
        // no temporary folders left behind
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_folders_that_are_not_projects() {
        let dir = temp_dir("not-fnm");
        assert!(matches!(load(&dir), Err(FnmError::NotFnm(_))));
        fs::remove_dir_all(dir).unwrap();
    }

    /// Files from every released format version must keep opening. Each
    /// fixture was written by the release that used that version.
    #[test]
    fn opens_version_0_projects() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello-v0.fnm");
        let (doc, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        // the file starts at frame 0 and ends at 47; frames now count from 1
        assert_eq!((doc.start, doc.end), (1, 48));
        let LayerKind::Bone { skeleton, .. } = &doc.layers[0].kind else {
            unreachable!()
        };
        let frames: Vec<_> = skeleton.bones[0].angle.keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [1, 13, 25, 37, 48]);
    }

    #[test]
    fn brush_strokes_round_trip() {
        let dir = temp_dir("brush");
        let path = dir.join("p.fnm");
        let mut doc = samples::hello();
        let samples: Vec<_> = (0..40)
            .map(|i| {
                fanimate_core::BrushSample::new(
                    fanimate_core::Vec2::new(i as f32 * 5.0, (i as f32).sin() * 20.0),
                    0.7,
                )
            })
            .collect();
        let opts = fanimate_core::BrushOptions {
            color: fanimate_core::Color::BLACK,
            size: 8.0,
            taper: 15.0,
            tolerance: 0.5,
        };
        let stroke = fanimate_core::brush::build_stroke(&samples, &opts).unwrap();
        assert!(stroke.widths.is_some());
        let layer = doc.new_layer("Ink", fanimate_core::LayerKind::vector(vec![stroke]));
        doc.layers.push(layer);
        save_folder(&doc, &path).unwrap();
        let manifest = fs::read_to_string(path.join("manifest.json")).unwrap();
        assert!(manifest.contains(&format!("\"version\": {FORMAT_VERSION}")));
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn point_keys_round_trip() {
        use fanimate_core::{Interpolation, LayerKind, Vec2};
        let dir = temp_dir("point-keys");
        let path = dir.join("p.fnm");
        let mut doc = samples::hello();
        let mut shape = fanimate_core::Shape::new(
            "Blob",
            fanimate_core::Path::circle(Vec2::new(100.0, 100.0), 40.0),
            fanimate_core::Style::fill(fanimate_core::Color::BLACK),
        );
        let mut squashed = shape.path.clone();
        squashed.move_points(&[1, 3].into_iter().collect(), Vec2::new(0.0, -20.0));
        shape.key_path(0, shape.path.clone(), Interpolation::Smooth);
        shape.key_path(12, squashed, Interpolation::Linear);
        let layer = doc.new_layer("Blob", LayerKind::vector(vec![shape]));
        doc.layers.push(layer);
        save(&doc, &path).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn labels_round_trip() {
        let dir = temp_dir("labels");
        let path = dir.join("p.fnm");
        let mut doc = samples::hello();
        doc.layers[0].label = Some(fanimate_core::Label::Pink);
        save(&doc, &path).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn image_layers_and_their_pictures_round_trip() {
        let dir = temp_dir("media");
        let path = dir.join("p.fnm");
        let mut doc = samples::hello();
        let bytes = b"not really a png".to_vec();
        let name = MediaFile::name_for(&bytes, "png");
        doc.media.insert(name.clone(), Arc::new(MediaFile { bytes }));
        let layer = doc.new_layer(
            "pic",
            LayerKind::Image(fanimate_core::Image::new(name.clone(), 20.0, 10.0)),
        );
        doc.layers.push(layer);
        // a picture no layer uses is not saved
        doc.media
            .insert("media/unused.png".into(), Arc::new(MediaFile { bytes: vec![1] }));
        let folder = dir.join("folder.fnm");
        save_folder(&doc, &folder).unwrap();
        assert!(folder.join(&name).is_file());
        assert!(!folder.join("media/unused.png").exists());
        // the single file holds the same pictures
        save(&doc, &path).unwrap();
        for from in [&folder, &path] {
            let (loaded, report) = load(from).unwrap();
            assert!(report.is_clean(), "{report}");
            assert_eq!(loaded.layers, doc.layers);
            assert_eq!(loaded.media.len(), 1);
            assert_eq!(loaded.media[&name], doc.media[&name]);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn switch_and_audio_layers_round_trip() {
        use fanimate_core::{Interpolation, Key};
        let dir = temp_dir("switch-audio");
        let path = dir.join("p.fnm");
        let mut doc = samples::hello();
        let bytes = b"RIFF not really a wav".to_vec();
        let name = MediaFile::name_for(&bytes, "wav");
        doc.media.insert(name.clone(), Arc::new(MediaFile { bytes }));
        let mut audio = fanimate_core::Audio::new(name.clone());
        audio.start = -3;
        audio.volume = 0.5;
        let sound = doc.new_layer("line", LayerKind::Audio(audio));
        let mouths = ["rest", "AI"].map(|n| doc.new_layer(n, LayerKind::vector(vec![])));
        let switch = doc.new_layer(
            "mouth",
            LayerKind::Switch {
                children: mouths.to_vec(),
                switch: vec![Key {
                    frame: 4,
                    value: "AI".into(),
                    interp: Interpolation::Step,
                }],
            },
        );
        doc.layers.extend([sound, switch]);
        save(&doc, &path).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded.layers, doc.layers);
        assert_eq!(loaded.media[&name], doc.media[&name]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn media_names_stay_inside_the_project() {
        assert!(is_media_name("media/0123abcd-10.png"));
        for bad in [
            "../x.png",
            "media/../x.png",
            "media/a/b.png",
            "/etc/passwd",
            "media/",
            "media/.hidden",
        ] {
            assert!(!is_media_name(bad), "{bad}");
        }
    }

    #[test]
    fn opens_version_1_projects() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/strokes-v1.fnm");
        let (doc, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        let mut shapes = 0;
        doc.walk(|l| {
            if let fanimate_core::LayerKind::Vector { shapes: s } = &l.kind {
                shapes += s.iter().filter(|s| s.widths.is_some()).count();
            }
        });
        assert!(shapes > 0);
    }

    #[test]
    fn rejects_newer_format_versions() {
        let dir = temp_dir("newer");
        let path = dir.join("p.fnm");
        save_folder(&samples::hello(), &path).unwrap();
        fs::write(path.join("manifest.json"), r#"{"format":"fnm","version":99}"#).unwrap();
        assert!(matches!(
            load(&path),
            Err(FnmError::UnsupportedVersion { found: 99, .. })
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    /// Entries of a saved project, to build variants of it by hand.
    fn entries_of(path: &Path) -> Vec<(String, Vec<u8>)> {
        let bytes = fs::read(path).unwrap();
        let archive = zip::Archive::read(&bytes).unwrap();
        archive
            .names()
            .map(|n| (n.to_owned(), archive.get(n).unwrap().unwrap()))
            .collect()
    }

    fn pack(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
        let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        zip::write(&refs).unwrap()
    }

    #[test]
    fn a_new_project_is_one_file_and_reads_back() {
        let dir = temp_dir("one-file");
        let path = dir.join("hello.fnm");
        let doc = samples::hello();
        save(&doc, &path).unwrap();
        assert!(path.is_file());
        assert!(zip::is_zip(&fs::read(&path).unwrap()));
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, doc);
        // the same bytes open from memory, as in the browser
        let (again, _) = load_archive("hello.fnm", &fs::read(&path).unwrap()).unwrap();
        assert_eq!(again, doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saving_replaces_a_file_and_leaves_nothing_behind() {
        let dir = temp_dir("file-replace");
        let path = dir.join("p.fnm");
        save(&samples::benchmark(40), &path).unwrap();
        let hello = samples::hello();
        save(&hello, &path).unwrap();
        assert_eq!(load(&path).unwrap().0, hello);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_failed_save_leaves_the_old_file_untouched() {
        let dir = temp_dir("file-fail");
        let path = dir.join("p.fnm");
        let hello = samples::hello();
        save(&hello, &path).unwrap();
        let before = fs::read(&path).unwrap();
        // something is in the way of the temporary file
        fs::create_dir(sibling(&path, "saving")).unwrap();
        assert!(save(&samples::benchmark(40), &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(load(&path).unwrap().0, hello);
        // and a folder that does not exist is an error, not a panic
        assert!(save(&hello, dir.join("missing").join("p.fnm")).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_existing_folder_project_stays_a_folder() {
        let dir = temp_dir("stay-folder");
        let path = dir.join("p.fnm");
        save_folder(&samples::benchmark(40), &path).unwrap();
        let hello = samples::hello();
        save(&hello, &path).unwrap();
        assert!(path.is_dir());
        assert_eq!(load(&path).unwrap().0, hello);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_project_folder_zipped_by_hand_opens() {
        let dir = temp_dir("zipped-folder");
        let path = dir.join("p.fnm");
        let doc = samples::hello();
        save(&doc, &path).unwrap();
        let nested: Vec<_> = entries_of(&path)
            .into_iter()
            .map(|(n, b)| (format!("my project/{n}"), b))
            .collect();
        fs::write(&path, pack(&nested)).unwrap();
        assert_eq!(load(&path).unwrap().0, doc);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn single_files_from_newer_versions_and_other_files_are_refused() {
        let dir = temp_dir("file-refused");
        let path = dir.join("p.fnm");
        save(&samples::hello(), &path).unwrap();
        let newer: Vec<_> = entries_of(&path)
            .into_iter()
            .map(|(n, b)| {
                if n == "manifest.json" {
                    (n, br#"{"format":"fnm","version":99}"#.to_vec())
                } else {
                    (n, b)
                }
            })
            .collect();
        fs::write(&path, pack(&newer)).unwrap();
        assert!(matches!(
            load(&path),
            Err(FnmError::UnsupportedVersion { found: 99, .. })
        ));
        // a zip that is not a project, a file that is not a zip, and nothing at all
        fs::write(&path, zip::write(&[("readme.txt", b"hi")]).unwrap()).unwrap();
        assert!(matches!(load(&path), Err(FnmError::NotFnm(_))));
        fs::write(&path, b"just text").unwrap();
        assert!(matches!(load(&path), Err(FnmError::NotFnm(_))));
        assert!(matches!(load(dir.join("none.fnm")), Err(FnmError::NotFnm(_))));
        fs::remove_dir_all(dir).unwrap();
    }

    /// Cut short or changed anywhere, a single-file project is an error or
    /// opens with its problems reported, and never panics.
    #[test]
    fn damaged_single_files_never_panic() {
        let good = {
            let dir = temp_dir("file-damaged");
            let path = dir.join("p.fnm");
            save(&samples::hello(), &path).unwrap();
            let bytes = fs::read(&path).unwrap();
            fs::remove_dir_all(dir).unwrap();
            bytes
        };
        for cut in (0..good.len()).step_by(7) {
            let _ = load_archive("damaged.fnm", &good[..cut]);
        }
        let mut bytes = good.clone();
        for i in (0..bytes.len()).step_by(5) {
            bytes[i] ^= 0xff;
            let _ = load_archive("damaged.fnm", &bytes);
            bytes[i] ^= 0xff;
        }
    }

    /// A changed byte inside an entry is caught by its checksum.
    #[test]
    fn a_changed_byte_in_a_saved_file_is_reported() {
        let dir = temp_dir("file-checksum");
        let path = dir.join("p.fnm");
        save(&samples::hello(), &path).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        // inside the first entry's data, after its header and name
        let first = 30 + "manifest.json".len() + 4;
        bytes[first] ^= 1;
        assert!(load_archive("p.fnm", &bytes).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pivots_and_rectangles_round_trip_in_a_file_and_a_folder() {
        use fanimate_core::{Color, Shape, Style, Vec2};
        let dir = temp_dir("pivot-rect");
        let mut doc = samples::hello();
        let rect = Shape::rectangle(
            "Box",
            Vec2::new(5.0, 6.0),
            Vec2::new(80.0, 40.0),
            9.0,
            Style::fill(Color::BLACK),
        );
        let mut layer = doc.new_layer("box", LayerKind::vector(vec![rect]));
        layer.transform.pivot = Some(Vec2::new(45.0, 26.0));
        layer.transform.rotation.base = 0.5;
        doc.layers.push(layer);
        let (file, folder) = (dir.join("p.fnm"), dir.join("folder.fnm"));
        save(&doc, &file).unwrap();
        save_folder(&doc, &folder).unwrap();
        for from in [&file, &folder] {
            let (loaded, report) = load(from).unwrap();
            assert!(report.is_clean(), "{report}");
            assert_eq!(loaded, doc);
            let last = loaded.layers.last().unwrap();
            assert_eq!(last.transform.pivot, Some(Vec2::new(45.0, 26.0)));
            let LayerKind::Vector { shapes } = &last.kind else {
                unreachable!()
            };
            assert_eq!(shapes[0].parametric_rect().unwrap().radius, 9.0);
        }
        // a project without either writes neither
        let plain = dir.join("plain.fnm");
        save_folder(&samples::hello(), &plain).unwrap();
        for entry in fs::read_dir(plain.join("layers")).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap();
            assert!(
                !text.contains("\"pivot\":") && !text.contains("\"rect\":"),
                "{text}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// Version 5 adds guides, locked layers and hidden shapes; they come back
    /// as they were saved, as a file and as a folder.
    #[test]
    fn guides_locked_layers_and_hidden_shapes_round_trip() {
        use fanimate_core::{Color, Guide, Path, Shape, Style, Vec2};
        let dir = temp_dir("v5");
        let mut doc = samples::hello();
        doc.guides = vec![
            Guide {
                vertical: true,
                position: 320.5,
                locked: false,
            },
            Guide {
                vertical: false,
                position: 90.0,
                locked: true,
            },
        ];
        let mut hidden = Shape::new("hidden", Path::circle(Vec2::ZERO, 5.0), Style::fill(Color::BLACK));
        hidden.visible = false;
        // a camera that moves, and a colour that does
        doc.camera.zoom =
            fanimate_core::Channel::new(1.0).with_key(12, 2.0, fanimate_core::Interpolation::Linear);
        doc.camera.position.base = Vec2::new(30.0, -5.0);
        hidden.fill_keys = vec![fanimate_core::Key {
            frame: 4,
            value: Color::WHITE,
            interp: fanimate_core::Interpolation::Step,
        }];
        // a rectangle with one corner rounded
        let mut rounded = Shape::rectangle(
            "card",
            Vec2::new(5.0, 5.0),
            Vec2::new(40.0, 30.0),
            0.0,
            Style::fill(Color::WHITE),
        );
        assert!(rounded.set_rect_corner_radius(1, 6.0));
        // with only part of its stroke drawn, which grows over the animation
        rounded.style = rounded.style.with_stroke(Color::BLACK, 2.0);
        rounded.trim = Vec2::new(0.1, 0.8);
        rounded.trim_keys = vec![
            fanimate_core::Key {
                frame: 0,
                value: Vec2::new(0.1, 0.2),
                interp: fanimate_core::Interpolation::Smooth,
            },
            fanimate_core::Key {
                frame: 12,
                value: Vec2::new(0.1, 0.8),
                interp: fanimate_core::Interpolation::Smooth,
            },
        ];
        let mut layer = doc.new_layer("art", LayerKind::vector(vec![hidden, rounded]));
        layer.locked = true;
        doc.layers.push(layer);
        for name in ["file.fnm", "folder.fnm"] {
            let path = dir.join(name);
            if name == "file.fnm" {
                save(&doc, &path).unwrap();
            } else {
                save_folder(&doc, &path).unwrap();
            }
            let (loaded, report) = load(&path).unwrap();
            assert!(report.is_clean(), "{report}");
            assert_eq!(loaded, doc, "{name}");
        }
        let (loaded, _) = load(dir.join("file.fnm")).unwrap();
        assert_eq!(loaded.guides.len(), 2);
        assert!(loaded.layers.last().unwrap().locked);
        assert_eq!(loaded.camera, doc.camera);
        assert!(loaded.camera.is_animated());
        let LayerKind::Vector { shapes } = &loaded.layers.last().unwrap().kind else {
            unreachable!()
        };
        assert_eq!(
            shapes[1].parametric_rect().map(|r| r.corner_radii()),
            Some([0.0, 6.0, 0.0, 0.0]),
            "the corners kept their own radii"
        );
        // the manifest says version 5, so an older reader refuses what it
        // would misread: it would show the hidden shape and lose the guides
        let manifest = fs::read_to_string(dir.join("folder.fnm").join("manifest.json")).unwrap();
        assert!(
            manifest.contains(&format!("\"version\": {FORMAT_VERSION}")),
            "{manifest}"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    /// A project without any of the version 5 fields writes none of them, so
    /// its files are the same as before, and a version 4 project opens as it was.
    #[test]
    fn a_version_4_project_opens_unchanged_and_writes_no_new_fields() {
        let dir = temp_dir("v4");
        let path = dir.join("old.fnm");
        let doc = samples::hello();
        save_folder(&doc, &path).unwrap();
        let written = fs::read_to_string(path.join("document.json")).unwrap();
        assert!(
            !written.contains("guides") && !written.contains("camera"),
            "{written}"
        );
        let layer_text = fs::read_to_string(path.join("layers").join("0002.json"))
            .or_else(|_| fs::read_to_string(path.join("layers").join("0001.json")))
            .unwrap();
        assert!(!layer_text.contains("locked") && !layer_text.contains("\"visible\":false"));
        fs::write(path.join("manifest.json"), r#"{"format":"fnm","version":4}"#).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        // everything else is as it was; only the frames moved up by one
        let mut expected = doc.clone();
        expected.shift_frames(1);
        assert_eq!(loaded, expected);
        assert!(loaded.guides.is_empty() && loaded.layers.iter().all(|l| !l.locked));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A project saved before pivots has layers that turn about their origin
    /// and no rectangles, and opens exactly as it did.
    #[test]
    fn a_version_3_project_opens_unchanged() {
        let dir = temp_dir("v3");
        let path = dir.join("old.fnm");
        let doc = samples::hello();
        save_folder(&doc, &path).unwrap();
        fs::write(path.join("manifest.json"), r#"{"format":"fnm","version":3}"#).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        let mut expected = doc.clone();
        expected.shift_frames(1);
        assert_eq!(loaded, expected);
        assert!(loaded.layers.iter().all(|l| l.transform.pivot.is_none()));
        fs::remove_dir_all(dir).unwrap();
    }

    /// The files of a project as version 5 wrote them: frames from 0, a
    /// range of 0 to 119, and an opacity with keys on frames 0 and 24.
    fn version_5_files() -> Vec<(String, Vec<u8>)> {
        let manifest = r#"{"format":"fnm","version":5,"generator":"fanimate 0.2.3"}"#;
        let document = r##"{
            "name": "Old", "width": 640.0, "height": 360.0, "fps": 24.0,
            "start": 0, "end": 119, "background": "#1C0B3D", "next_layer_id": 2,
            "layers": ["layers/0001.json"]
        }"##;
        let layer = r#"{
            "id": 1, "name": "fade",
            "opacity": {"base": 1.0, "keys": [{"frame": 0, "value": 0.0}, {"frame": 24, "value": 1.0}]},
            "transform": {"position": {"base": [0.0, 0.0]}, "rotation": {"base": 0.0}, "scale": {"base": [1.0, 1.0]}},
            "type": "vector", "shapes": []
        }"#;
        [
            ("manifest.json", manifest),
            ("document.json", document),
            ("layers/0001.json", layer),
        ]
        .map(|(n, t)| (n.to_owned(), t.as_bytes().to_vec()))
        .to_vec()
    }

    fn opacity_frames(doc: &Document) -> Vec<Frame> {
        doc.layers[0].opacity.keys.iter().map(|k| k.frame).collect()
    }

    /// Version 6 counts frames from 1. An older project is shifted up by one
    /// when it is read, as a folder, as a single file and from memory, and
    /// no key is made on frame 0.
    #[test]
    fn a_version_5_project_is_shifted_to_count_from_one() {
        let dir = temp_dir("v5-shift");
        let files = version_5_files();
        let folder = dir.join("old.fnm");
        for (name, bytes) in &files {
            let file = folder.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
        let single = dir.join("single.fnm");
        fs::write(&single, pack(&files)).unwrap();

        for path in [&folder, &single] {
            let (doc, report) = load(path).unwrap();
            assert!(report.is_clean(), "{report}");
            assert_eq!((doc.start, doc.end), (1, 120));
            assert_eq!(opacity_frames(&doc), [1, 25]);
        }
        let (doc, _) = load_archive("single.fnm", &fs::read(&single).unwrap()).unwrap();
        assert_eq!((doc.start, doc.end), (1, 120));
        assert_eq!(opacity_frames(&doc), [1, 25]);

        // saved again it is a version 6 project, and reading that does not shift it again
        let saved = dir.join("new.fnm");
        save(&doc, &saved).unwrap();
        let (again, report) = load(&saved).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(again, doc);
        assert_eq!(opacity_frames(&again), [1, 25]);
        let manifest = entries_of(&saved).into_iter().find(|(n, _)| n == "manifest.json");
        let text = String::from_utf8(manifest.unwrap().1).unwrap();
        assert!(text.contains(&format!("\"version\": {FORMAT_VERSION}")), "{text}");
        fs::remove_dir_all(dir).unwrap();
    }

    /// A project written by this version, whatever it starts at, is never shifted.
    #[test]
    fn a_current_project_is_not_shifted() {
        let dir = temp_dir("no-shift");
        let mut doc = samples::hello();
        doc.start = 0;
        doc.end = 10;
        for (name, folder) in [("file.fnm", false), ("folder.fnm", true)] {
            let path = dir.join(name);
            if folder {
                save_folder(&doc, &path).unwrap();
            } else {
                save(&doc, &path).unwrap();
            }
            let (loaded, _) = load(&path).unwrap();
            assert_eq!((loaded.start, loaded.end), (0, 10), "{name}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// Version 6: a channel may loop and a layer may have an order. Both
    /// come back as saved, and a project without them writes neither field.
    #[test]
    fn loops_and_layer_order_round_trip_in_a_file_and_a_folder() {
        use fanimate_core::{Channel, Interpolation, LoopMode};
        let dir = temp_dir("loop-order");
        let mut doc = samples::hello();
        let mut walk = Channel::new(0.0)
            .with_key(1, 0.0, Interpolation::Linear)
            .with_key(13, 10.0, Interpolation::Linear);
        walk.loop_mode = LoopMode::Relative;
        doc.layers[0].transform.rotation = walk;
        let mut front = doc.new_layer("front", LayerKind::vector(vec![]));
        front.order =
            Channel::new(0.0)
                .with_key(1, 1.0, Interpolation::Step)
                .with_key(13, 0.0, Interpolation::Step);
        let back = doc.new_layer("back", LayerKind::vector(vec![]));
        let group = doc.new_layer(
            "pair",
            LayerKind::Group {
                children: vec![back, front],
            },
        );
        doc.layers.push(group);
        for (name, folder) in [("file.fnm", false), ("folder.fnm", true)] {
            let path = dir.join(name);
            if folder {
                save_folder(&doc, &path).unwrap();
            } else {
                save(&doc, &path).unwrap();
            }
            let (loaded, report) = load(&path).unwrap();
            assert!(report.is_clean(), "{name}: {report}");
            assert_eq!(loaded, doc, "{name}");
            assert_eq!(loaded.layers[0].transform.rotation.loop_mode, LoopMode::Relative);
            let LayerKind::Group { children } = &loaded.layers.last().unwrap().kind else {
                unreachable!()
            };
            assert_eq!(children[1].order.keys.len(), 2);
        }
        // an ordinary project has neither field in its files
        let plain = dir.join("plain.fnm");
        save_folder(&samples::hello(), &plain).unwrap();
        for entry in fs::read_dir(plain.join("layers")).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap();
            assert!(
                !text.contains("loop_mode") && !text.contains("\"order\""),
                "{text}"
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hidden_edges_and_saved_styles_round_trip_in_a_file_and_a_folder() {
        use fanimate_core::{Color, StylePreset};
        let dir = temp_dir("edges-styles");
        let mut doc = samples::hello();
        // hide the first drawn segment of the first shape that has one
        let mut hid = false;
        doc.walk_mut(|layer| {
            if let LayerKind::Vector { shapes } = &mut layer.kind {
                for shape in shapes.iter_mut() {
                    if !hid && shape.set_edge_hidden(1, true) {
                        hid = true;
                    }
                }
            }
        });
        assert!(hid, "the sample has a shape with a segment");
        doc.styles = vec![
            StylePreset {
                name: "Ink".into(),
                line: Color::BLACK,
                width: 4.0,
                fill: None,
                outline: true,
                smoothing: 0.25,
                taper: 12.0,
                pressure: false,
            },
            StylePreset {
                name: "Paint".into(),
                line: Color::BLACK,
                width: 9.0,
                fill: Some(Color::WHITE),
                outline: false,
                smoothing: 0.75,
                taper: 0.0,
                pressure: true,
            },
        ];
        for (name, folder) in [("file.fnm", false), ("folder.fnm", true)] {
            let path = dir.join(name);
            if folder {
                save_folder(&doc, &path).unwrap();
            } else {
                save(&doc, &path).unwrap();
            }
            let (loaded, report) = load(&path).unwrap();
            assert!(report.is_clean(), "{name}: {report}");
            assert_eq!(loaded, doc, "{name}");
            assert_eq!(loaded.styles.len(), 2);
        }
        // an ordinary project has neither in its files
        let plain = dir.join("plain.fnm");
        save_folder(&samples::hello(), &plain).unwrap();
        let text = fs::read_to_string(plain.join("document.json")).unwrap();
        assert!(!text.contains("styles"), "{text}");
        for entry in fs::read_dir(plain.join("layers")).unwrap().flatten() {
            let text = fs::read_to_string(entry.path()).unwrap();
            assert!(!text.contains("hidden_edge_flags"), "{text}");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// Stands for a preview picture: the signature is what the reader checks.
    fn preview(body: &[u8]) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(body);
        png
    }

    #[test]
    fn a_preview_picture_round_trips_in_a_file_and_a_folder() {
        let dir = temp_dir("thumbnail");
        let doc = samples::hello();
        let picture = preview(b"first preview picture");
        let (file, folder) = (dir.join("p.fnm"), dir.join("folder.fnm"));
        assert_eq!(read_thumbnail(&file), None, "nothing there yet");
        save_with_thumbnail(&doc, &file, Some(&picture)).unwrap();
        save_folder(&doc, &folder).unwrap();
        save_with_thumbnail(&doc, &folder, Some(&picture)).unwrap();
        for path in [&file, &folder] {
            assert_eq!(read_thumbnail(path), Some(picture.clone()));
            // the extra entry does not get in the way of opening
            let (loaded, report) = load(path).unwrap();
            assert!(report.is_clean(), "{report}");
            assert_eq!(loaded, doc);
        }
        assert!(folder.join(THUMBNAIL_NAME).is_file());
        assert!(entries_of(&file).iter().any(|(n, _)| n == THUMBNAIL_NAME));

        // saving the same project twice gives the same bytes
        let again = dir.join("again.fnm");
        save_with_thumbnail(&doc, &again, Some(&picture)).unwrap();
        assert_eq!(fs::read(&file).unwrap(), fs::read(&again).unwrap());

        // a plain save keeps the preview; a new one replaces it
        for path in [&file, &folder] {
            save(&doc, path).unwrap();
            assert_eq!(read_thumbnail(path), Some(picture.clone()));
            let newer = preview(b"a newer one");
            save_with_thumbnail(&doc, path, Some(&newer)).unwrap();
            assert_eq!(read_thumbnail(path), Some(newer));
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_preview_that_is_not_a_small_png_is_ignored() {
        let dir = temp_dir("thumbnail-bad");
        let doc = samples::hello();
        let kept = preview(b"the good one");
        let big = preview(&vec![0; MAX_THUMBNAIL_BYTES]);
        for (name, folder) in [("file.fnm", false), ("folder.fnm", true)] {
            let path = dir.join(name);
            if folder {
                save_folder(&doc, &path).unwrap();
            } else {
                save(&doc, &path).unwrap();
            }
            // with no preview yet, a bad one leaves the project without
            for bad in [&b"just text"[..], &big[..], &b""[..]] {
                save_with_thumbnail(&doc, &path, Some(bad)).unwrap();
                assert_eq!(read_thumbnail(&path), None, "{name}");
                assert_eq!(load(&path).unwrap().0, doc);
            }
            // with one, a bad one does not replace it
            save_with_thumbnail(&doc, &path, Some(&kept)).unwrap();
            save_with_thumbnail(&doc, &path, Some(&big)).unwrap();
            save_with_thumbnail(&doc, &path, Some(b"not a png")).unwrap();
            assert_eq!(read_thumbnail(&path), Some(kept.clone()), "{name}");
        }
        // the largest allowed preview is kept
        let largest = preview(&vec![0; MAX_THUMBNAIL_BYTES - 8]);
        let path = dir.join("largest.fnm");
        save_with_thumbnail(&doc, &path, Some(&largest)).unwrap();
        assert_eq!(read_thumbnail(&path), Some(largest));
        fs::remove_dir_all(dir).unwrap();
    }

    /// A preview read from a file somebody else made: too big, not a PNG,
    /// damaged or one folder down. Never a panic, and never in the way of the
    /// project opening.
    #[test]
    fn previews_in_files_from_elsewhere_are_checked() {
        let dir = temp_dir("thumbnail-read");
        let doc = samples::hello();
        let path = dir.join("p.fnm");
        save(&doc, &path).unwrap();
        let entries = entries_of(&path);
        let with = |extra: Vec<u8>| {
            let mut all = entries.clone();
            all.push((THUMBNAIL_NAME.to_owned(), extra));
            pack(&all)
        };

        let odd = dir.join("odd.fnm");
        for (what, bytes) in [
            ("too big", preview(&vec![0; MAX_THUMBNAIL_BYTES + 1])),
            ("not a png", b"GIF89a....".to_vec()),
            ("empty", Vec::new()),
        ] {
            fs::write(&odd, with(bytes)).unwrap();
            assert_eq!(read_thumbnail(&odd), None, "{what}");
            assert_eq!(load(&odd).unwrap().0, doc, "{what}");
        }

        // one folder down, as a project folder zipped by hand
        // bytes that do not repeat, so the archive stores them as they are
        // and the test can find and change one
        let good = preview(&(0u8..40).collect::<Vec<_>>());
        let nested: Vec<_> = entries
            .iter()
            .cloned()
            .chain([(THUMBNAIL_NAME.to_owned(), good.clone())])
            .map(|(n, b)| (format!("my project/{n}"), b))
            .collect();
        fs::write(&odd, pack(&nested)).unwrap();
        assert_eq!(read_thumbnail(&odd), Some(good.clone()));

        // a changed byte fails its checksum: no preview, but the project opens
        let mut bytes = with(good.clone());
        let at = bytes
            .windows(good.len())
            .position(|w| w == good.as_slice())
            .unwrap();
        bytes[at + good.len() - 1] ^= 1;
        fs::write(&odd, &bytes).unwrap();
        assert_eq!(read_thumbnail(&odd), None);
        assert_eq!(load(&odd).unwrap().0, doc);

        // nothing, a file that is not a project, and a folder without one
        assert_eq!(read_thumbnail(dir.join("none.fnm")), None);
        fs::write(&odd, b"just text").unwrap();
        assert_eq!(read_thumbnail(&odd), None);
        assert_eq!(read_thumbnail(&dir), None);
        for cut in (0..bytes.len()).step_by(11) {
            fs::write(&odd, &bytes[..cut]).unwrap();
            let _ = read_thumbnail(&odd);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    /// A save that fails leaves the old file, and its preview, as they were.
    #[test]
    fn a_failed_save_with_a_preview_leaves_the_old_file_untouched() {
        let dir = temp_dir("thumbnail-fail");
        let path = dir.join("p.fnm");
        let hello = samples::hello();
        let picture = preview(b"old preview");
        save_with_thumbnail(&hello, &path, Some(&picture)).unwrap();
        let before = fs::read(&path).unwrap();
        fs::create_dir(sibling(&path, "saving")).unwrap();
        assert!(save_with_thumbnail(&samples::benchmark(40), &path, Some(&preview(b"new"))).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(read_thumbnail(&path), Some(picture));
        assert_eq!(load(&path).unwrap().0, hello);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A project that uses nothing version 7 added writes no `requires`, so its
    /// manifest is what it was, and a version 6 project opens unchanged.
    #[test]
    fn a_version_6_project_opens_unchanged_and_requires_nothing() {
        let dir = temp_dir("v6");
        let path = dir.join("old.fnm");
        let doc = samples::hello();
        save_folder(&doc, &path).unwrap();
        let manifest = fs::read_to_string(path.join("manifest.json")).unwrap();
        assert!(!manifest.contains("requires"), "{manifest}");
        let layers = fs::read_to_string(path.join("layers").join("0002.json"))
            .or_else(|_| fs::read_to_string(path.join("layers").join("0001.json")))
            .unwrap();
        assert!(!layers.contains("provenance"), "{layers}");
        fs::write(path.join("manifest.json"), r#"{"format":"fnm","version":6}"#).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, doc, "frames are already counted from 1 in version 6");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn provenance_round_trips_and_the_manifest_requires_it() {
        let dir = temp_dir("provenance");
        let mut doc = samples::hello();
        let mut record = fanimate_core::Provenance::new("line-clean-up");
        record.model = "clean-lines".into();
        record.version = "1.2".into();
        record.date = "2026-10-30".into();
        doc.layers[0].provenance = Some(record.clone());
        assert_eq!(required_features(&doc), [FEATURE_PROVENANCE]);

        for name in ["folder.fnm", "file.fnm"] {
            let path = dir.join(name);
            if name == "folder.fnm" {
                save_folder(&doc, &path).unwrap();
                let manifest = fs::read_to_string(path.join("manifest.json")).unwrap();
                assert!(manifest.contains("provenance@1"), "{manifest}");
            } else {
                save(&doc, &path).unwrap();
            }
            let (loaded, report) = load(&path).unwrap();
            assert!(report.is_clean(), "{name}: {report}");
            assert_eq!(loaded, doc, "{name}");
            assert_eq!(loaded.layers[0].provenance.as_ref(), Some(&record));
            assert!(loaded.layers[1..].iter().all(|l| l.provenance.is_none()));
        }
        // provenance on a layer inside a group still counts
        let mut grouped = samples::hello();
        let child = grouped.new_layer("child", LayerKind::vector(vec![]));
        let mut group = grouped.new_layer(
            "group",
            LayerKind::Group {
                children: vec![child],
            },
        );
        if let LayerKind::Group { children } = &mut group.kind {
            children[0].provenance = Some(fanimate_core::Provenance::new("auto-rig"));
        }
        grouped.layers.push(group);
        assert_eq!(required_features(&grouped), [FEATURE_PROVENANCE]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A project that needs something this build lacks is refused with its
    /// name, not opened with that part missing; every way in checks it.
    #[test]
    fn a_project_that_needs_an_unknown_feature_is_refused_naming_it() {
        let dir = temp_dir("requires");
        let path = dir.join("p.fnm");
        save_folder(&samples::hello(), &path).unwrap();
        let manifest = r#"{"format":"fnm","version":7,"requires":["provenance@1","text@1","raster@1"]}"#;
        fs::write(path.join("manifest.json"), manifest).unwrap();
        match load(&path) {
            Err(FnmError::MissingFeature { feature }) => assert_eq!(feature, "raster@1"),
            other => panic!("expected a missing feature, got {other:?}"),
        }
        let read = |rel: &str| fs::read(path.join(rel));
        assert!(matches!(
            load_from("p.fnm", &read),
            Err(FnmError::MissingFeature { .. })
        ));
        let message = load(&path).unwrap_err().to_string();
        assert!(
            message.contains("raster@1") && message.contains("newer Fanimate"),
            "{message}"
        );

        // features this build knows are fine, and an empty list needs nothing
        for ok in [r#"["provenance@1"]"#, "[]"] {
            fs::write(
                path.join("manifest.json"),
                format!(r#"{{"format":"fnm","version":7,"requires":{ok}}}"#),
            )
            .unwrap();
            assert!(load(&path).is_ok(), "{ok}");
        }
        // a damaged manifest cannot flood the message
        let long = "x".repeat(10_000);
        fs::write(
            path.join("manifest.json"),
            format!(r#"{{"format":"fnm","version":7,"requires":["{long}"]}}"#),
        )
        .unwrap();
        let message = load(&path).unwrap_err().to_string();
        assert!(message.len() < 300, "{}", message.len());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_damaged_provenance_record_is_reported_and_repaired() {
        let dir = temp_dir("provenance-damaged");
        let path = dir.join("p.fnm");
        let mut doc = samples::hello();
        doc.layers[0].provenance = Some(fanimate_core::Provenance {
            tool: String::new(),
            model: "m".repeat(fanimate_core::MAX_PROVENANCE_TEXT + 10),
            version: String::new(),
            date: "soon".into(),
        });
        save_folder(&doc, &path).unwrap();
        // reported, not repaired
        let (loaded, report) = load(&path).unwrap();
        assert!(
            report
                .issues
                .iter()
                .any(|i| !i.repaired && i.message.contains("record of what made")),
            "{report}"
        );
        assert!(!loaded.layers[0].provenance.as_ref().unwrap().is_sound());
        // repaired: still recorded, now usable
        let (repaired, report) = load_and_repair(&path).unwrap();
        assert!(report.repaired_count() >= 1, "{report}");
        let record = repaired.layers[0].provenance.as_ref().unwrap();
        assert!(record.is_sound());
        assert_eq!(record.tool, "unknown");
        assert!(validate::validate(&repaired).is_clean());
        fs::remove_dir_all(dir).unwrap();
    }

    /// A project written before bones had ids opens with ids given in list
    /// order, the same ones every time, and they match what a new project
    /// gives its bones.
    #[test]
    fn bones_in_older_files_get_ids_on_load_and_keep_them() {
        let dir = temp_dir("bone-ids");
        let path = dir.join("old.fnm");
        let doc = samples::benchmark(6);
        save_folder(&doc, &path).unwrap();
        let mut stripped = 0;
        for entry in fs::read_dir(path.join("layers")).unwrap() {
            let file = entry.unwrap().path();
            let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
            let Some(skeleton) = json.get_mut("skeleton") else {
                continue;
            };
            skeleton.as_object_mut().unwrap().remove("next_bone_id");
            for bone in skeleton["bones"].as_array_mut().unwrap() {
                bone.as_object_mut().unwrap().remove("id");
                stripped += 1;
            }
            fs::write(&file, serde_json::to_vec(&json).unwrap()).unwrap();
        }
        assert!(stripped >= 6, "the benchmark has a bone layer");
        fs::write(path.join("manifest.json"), r#"{"format":"fnm","version":6}"#).unwrap();

        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "giving ids to old files is silent: {report}");
        assert_eq!(loaded, doc, "ids come out as a new project's would");
        let (again, _) = load(&path).unwrap();
        assert_eq!(again, loaded, "the same ids on every load");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bones_that_share_an_id_are_reported_and_given_new_ones() {
        let mut doc = samples::benchmark(4);
        doc.walk_mut(|l| {
            if let LayerKind::Bone { skeleton, .. } = &mut l.kind {
                skeleton.bones[2].id = skeleton.bones[1].id;
            }
        });
        let report = validate::validate(&doc);
        assert!(
            report
                .issues
                .iter()
                .any(|i| !i.repaired && i.message.contains("share an id")),
            "{report}"
        );
        let report = validate::repair(&mut doc);
        assert!(report.repaired_count() >= 1, "{report}");
        assert!(validate::validate(&doc).is_clean());
        doc.walk(|l| {
            if let LayerKind::Bone { skeleton, .. } = &l.kind {
                assert!(skeleton.ids_are_sound());
            }
        });
    }

    #[test]
    fn eased_and_curved_keys_round_trip_and_a_bad_curve_is_repaired() {
        use fanimate_core::{Bezier, Easing, Interpolation};
        let dir = temp_dir("easing");
        let path = dir.join("e.fnm");
        let mut doc = samples::hello();
        let ch = &mut doc.layers[0].opacity;
        ch.set_key(1, 1.0, Interpolation::Ease(Easing::Bounce));
        ch.set_key(10, 0.5, Interpolation::Bezier(Bezier([250, 100, 750, 1400])));
        ch.set_key(20, 1.0, Interpolation::Smooth);
        save(&doc, &path).unwrap();
        let (loaded, report) = load(&path).unwrap();
        assert!(report.is_clean(), "{report}");
        assert_eq!(loaded, doc);
        let text = serde_json::to_string(&loaded.layers[0].opacity).unwrap();
        assert!(
            text.contains(r#""interp":{"ease":"bounce"}"#)
                && text.contains(r#"{"bezier":[250,100,750,1400]}"#),
            "{text}"
        );

        // a curve whose control points are out of range is reported, and repaired
        let mut bad = doc.clone();
        bad.layers[0]
            .opacity
            .set_key(10, 0.5, Interpolation::Bezier(Bezier([-50, 100, 5000, 9999])));
        let report = validate::validate(&bad);
        assert!(
            report
                .issues
                .iter()
                .any(|i| !i.repaired && i.message.contains("curve outside the allowed range")),
            "{report}"
        );
        let report = validate::repair(&mut bad);
        assert!(report.repaired_count() >= 1, "{report}");
        assert!(validate::validate(&bad).is_clean());
        fs::remove_dir_all(dir).unwrap();
    }

    fn doc_with_text() -> Document {
        use fanimate_core::{Animator, Channel, Interpolation, Text, TextUnit};
        let mut doc = samples::hello();
        let mut text = Text::new("Hello, world");
        text.look_mut().size = 90.0;
        text.look_mut().weight = 700.0;
        text.look_mut().fill = Color::rgba(1.0, 0.8, 0.2, 1.0);
        let mut rise = Animator::new("Rise", TextUnit::Word);
        rise.position.base = fanimate_core::Vec2::new(0.0, 40.0);
        rise.opacity.base = 0.0;
        rise.selector.end.set_key(1, 0.0, Interpolation::Linear);
        rise.selector.end.set_key(24, 100.0, Interpolation::Smooth);
        rise.fill = Some(Channel::new(Color::WHITE));
        text.animators.push(rise);
        let layer = doc.new_layer("Title", LayerKind::Text(text));
        doc.layers.push(layer);
        doc
    }

    #[test]
    fn a_text_layer_round_trips_and_the_manifest_requires_text() {
        let dir = temp_dir("text");
        let doc = doc_with_text();
        assert_eq!(required_features(&doc), [FEATURE_TEXT]);
        for name in ["folder.fnm", "file.fnm"] {
            let path = dir.join(name);
            if name == "folder.fnm" {
                save_folder(&doc, &path).unwrap();
                let manifest = fs::read_to_string(path.join("manifest.json")).unwrap();
                assert!(manifest.contains("text@1"), "{manifest}");
                let layer = fs::read_to_string(path.join("layers").join("0007.json"))
                    .or_else(|_| {
                        let last = doc.layers.last().unwrap().id.0;
                        fs::read_to_string(path.join("layers").join(format!("{last:04}.json")))
                    })
                    .unwrap();
                assert!(
                    layer.contains("\"type\": \"text\"") && layer.contains("Hello, world"),
                    "{layer}"
                );
            } else {
                save(&doc, &path).unwrap();
            }
            let (loaded, report) = load(&path).unwrap();
            assert!(report.is_clean(), "{name}: {report}");
            assert_eq!(loaded, doc, "{name}");
            // the loaded text draws the same
            let a = evaluate(&doc, 12.0);
            let b = evaluate(&loaded, 12.0);
            assert_eq!(a.len(), b.len());
            assert!(
                a.iter()
                    .zip(&b)
                    .all(|(x, y)| x.transform == y.transform && x.opacity == y.opacity)
            );
        }
        // both features together
        let mut both = doc.clone();
        both.layers[0].provenance = Some(fanimate_core::Provenance::new("title-maker"));
        assert_eq!(required_features(&both), [FEATURE_PROVENANCE, FEATURE_TEXT]);
        // text inside a group still counts
        let mut grouped = samples::hello();
        let inner = grouped.new_layer("t", LayerKind::Text(fanimate_core::Text::new("x")));
        let group = grouped.new_layer(
            "g",
            LayerKind::Group {
                children: vec![inner],
            },
        );
        grouped.layers.push(group);
        assert_eq!(required_features(&grouped), [FEATURE_TEXT]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn text_with_settings_out_of_range_is_reported_and_repaired_and_a_missing_font_is_noticed() {
        use fanimate_core::{Animator, TextUnit};
        let mut doc = doc_with_text();
        fn wild(doc: &mut Document) -> &mut fanimate_core::Text {
            match &mut doc.layers.last_mut().unwrap().kind {
                LayerKind::Text(t) => t,
                _ => unreachable!(),
            }
        }
        {
            let t = wild(&mut doc);
            t.look_mut().size = f32::NAN;
            t.look_mut().weight = -5.0;
            t.look_mut().width = Some(-1.0);
            t.animators[0].position.base = fanimate_core::Vec2::new(f32::NAN, 0.0);
            t.animators[0].selector.end.keys.push(fanimate_core::Key {
                frame: -4,
                value: 1.0,
                interp: fanimate_core::Interpolation::Linear,
            });
            for _ in 0..70 {
                t.animators.push(Animator::new("many", TextUnit::Line));
            }
        }
        let report = validate::validate(&doc);
        let text = report.to_string();
        for expected in [
            "outside what is allowed",
            "animators; only the first 64",
            "not a finite number",
            "negative frames",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        let report = validate::repair(&mut doc);
        assert!(report.repaired_count() >= 4, "{report}");
        assert!(
            validate::validate(&doc).is_clean(),
            "{}",
            validate::validate(&doc)
        );
        assert!(wild(&mut doc).look().is_sound());
        assert_eq!(wild(&mut doc).animators.len(), 64);
        // the repaired text still draws
        assert!(!evaluate(&doc, 1.0).is_empty());

        // a family Fanimate does not have is a warning that cannot be repaired
        let mut odd = doc_with_text();
        wild(&mut odd).look_mut().family = "Papyrus".into();
        let report = validate::repair(&mut odd);
        assert!(
            report
                .issues
                .iter()
                .any(|i| !i.repaired && i.message.contains("Papyrus") && i.message.contains("Inter")),
            "{report}"
        );
        assert_eq!(
            wild(&mut odd).look().family,
            "Papyrus",
            "the name is kept for a machine that has the font"
        );
    }

    fn rig_with_controllers() -> Document {
        use fanimate_core::actions::{Controller, ControllerAxis, ControllerKind, ControllerLink, RigAction};
        use fanimate_core::{Channel, Interpolation, Skeleton, Vec2};
        let mut doc = samples::hello();
        let mut skeleton = Skeleton::default();
        let jaw = skeleton.add("jaw", None, Vec2::ZERO, 20.0, 0.0);
        let mut open = RigAction::new("open", None, 10);
        open.bones.push(fanimate_core::actions::BoneKeys {
            bone: jaw,
            angle: Channel::new(0.0)
                .with_key(0, 0.0, Interpolation::Linear)
                .with_key(10, 0.8, Interpolation::Linear),
        });
        skeleton.actions.push(open);
        let mut pad = Controller::new("Mouth", ControllerKind::Pad);
        pad.x = Channel::new(0.3)
            .with_key(1, 0.0, Interpolation::Linear)
            .with_key(20, 1.0, Interpolation::Smooth);
        pad.links.push(ControllerLink {
            action: 0,
            axis: ControllerAxis::X,
            from: 0.2,
            to: 0.8,
        });
        skeleton.controllers.push(pad);
        let layer = doc.new_layer(
            "Rig",
            LayerKind::Bone {
                skeleton,
                children: vec![],
            },
        );
        doc.layers.push(layer);
        doc
    }

    #[test]
    fn a_rig_with_controllers_round_trips_and_the_manifest_requires_controllers() {
        let dir = temp_dir("controllers");
        let doc = rig_with_controllers();
        assert_eq!(required_features(&doc), [FEATURE_CONTROLLERS]);
        for name in ["folder.fnm", "file.fnm"] {
            let path = dir.join(name);
            if name == "folder.fnm" {
                save_folder(&doc, &path).unwrap();
                let manifest = fs::read_to_string(path.join("manifest.json")).unwrap();
                assert!(manifest.contains("controllers@1"), "{manifest}");
            } else {
                save(&doc, &path).unwrap();
            }
            let (loaded, report) = load(&path).unwrap();
            assert!(report.is_clean(), "{name}: {report}");
            assert_eq!(loaded, doc, "{name}");
        }
        // a rig without them needs nothing
        let mut plain = doc.clone();
        if let LayerKind::Bone { skeleton, .. } = &mut plain.layers.last_mut().unwrap().kind {
            skeleton.controllers.clear();
        }
        assert!(required_features(&plain).is_empty());
        // this build reads the feature it writes
        assert!(SUPPORTED_FEATURES.contains(&FEATURE_CONTROLLERS));
        fs::remove_dir_all(dir).unwrap();
    }
}
