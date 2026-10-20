//! Embeds the release notes (`docs/releases/vX.Y.Z.md`) for What's New, and
//! on Windows the app icon and version details in fanimate.exe, so Explorer,
//! the Start menu, shortcuts and the taskbar show Fanimate's icon.

use std::path::Path;

/// Write `release_notes.rs`: every release's notes as (version, text), newest
/// first, so a release's notes ship in the app that carries them.
fn release_notes() {
    let dir = Path::new("../../docs/releases");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut found: Vec<((u32, u32, u32), String, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let version = stem.trim_start_matches('v');
        let parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
        if let [a, b, c] = parts[..] {
            println!("cargo:rerun-if-changed={}", path.display());
            found.push(((a, b, c), version.to_owned(), path.canonicalize().unwrap_or(path)));
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut out = String::from(
        "pub const NOTES: &[(&str, &str)] = &[
",
    );
    for (_, version, path) in &found {
        // `{:?}` escapes the backslashes of a Windows path
        out.push_str(&format!(
            "    ({version:?}, include_str!({:?})),
",
            path.display().to_string()
        ));
    }
    out.push_str(
        "];
",
    );
    let target = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("release_notes.rs");
    std::fs::write(target, out).expect("the release notes list can be written");
}

fn main() {
    release_notes();
    println!("cargo:rerun-if-changed=../../brand/app-icon/fanimate.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../brand/app-icon/fanimate.ico")
        .set("ProductName", "Fanimate")
        .set("FileDescription", "Fanimate")
        .set("CompanyName", "The Fanimate Contributors")
        .set("LegalCopyright", "The Fanimate Contributors. MIT or Apache 2.0.");
    if let Err(e) = res.compile() {
        // a build without the icon still works; say why it is missing
        println!("cargo:warning=could not embed the Windows icon: {e}");
    }
}
