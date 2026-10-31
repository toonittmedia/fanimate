//! The public surface of the API, listed in `api-surface.txt`.
//!
//! Anything public in `src/lib.rs` (functions, types, fields, constants) is
//! copied from the source into a list and compared with the checked-in file,
//! so a change to what scripts can rely on is a visible diff in review, never
//! an accident. After a deliberate change, run the tests with
//! `UPDATE_API_SURFACE=1` to rewrite the file, and check the diff.

use std::path::Path;

/// Every public item's declaration in `src`, up to the unit tests: the lines
/// that start with `pub`, joined when a signature wraps over several.
fn surface(src: &str) -> String {
    let mut out = Vec::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.starts_with("#[cfg(test)]") {
            break;
        }
        if !line.starts_with("pub ") {
            continue;
        }
        // a declaration ends at `{`, `;` or `,` outside its parentheses: a
        // wrapped signature has commas after each parameter
        let mut item = line.to_owned();
        let open = |s: &str| s.matches('(').count() as i32 - s.matches(')').count() as i32;
        while open(&item) > 0 || !(item.ends_with('{') || item.ends_with(';') || item.ends_with(',')) {
            let Some(next) = lines.next() else { break };
            item.push(' ');
            item.push_str(next.trim());
        }
        out.push(item.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    out.join("\n") + "\n"
}

#[test]
fn the_public_surface_is_the_one_on_file() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let now = surface(&source);
    let file = root.join("api-surface.txt");
    if std::env::var_os("UPDATE_API_SURFACE").is_some() {
        std::fs::write(&file, &now).unwrap();
        return;
    }
    let on_file = std::fs::read_to_string(&file).unwrap_or_default();
    // normalise line endings: the file may be checked out with CRLF
    let on_file = on_file.replace("\r\n", "\n");
    assert!(
        on_file == now,
        "the API's public surface changed. If that is on purpose, rerun with UPDATE_API_SURFACE=1 and \
         commit the new api-surface.txt.\n--- on file\n{on_file}--- now\n{now}"
    );
}

#[test]
fn the_extractor_joins_wrapped_signatures_and_stops_at_the_tests() {
    let src = "pub const A: u8 = 1;\n    pub fn f(\n        a: u8,\n    ) -> u8 {\n  fn private() {}\n#[cfg(test)]\npub fn hidden() {}\n";
    assert_eq!(surface(src), "pub const A: u8 = 1;\npub fn f( a: u8, ) -> u8 {\n");
}
