//! `fanimate-cli plugin`: make a plugin, test it, pack it, install the
//! package into a folder and list it, all through the real program.

use std::path::Path;
use std::process::{Command, Output};

fn cli(args: &[&str], dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn python_exists() -> bool {
    ["python", "python3", "py"].iter().any(|p| {
        Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

#[test]
fn a_plugin_is_made_tested_packed_installed_and_listed() {
    let root = std::env::temp_dir().join(format!("fanimate-cli-plugin-{}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).unwrap();

    let out = cli(&["plugin", "new", "Title Maker"], &root);
    assert!(out.status.success(), "{}", text(&out));
    let dir = root.join("title-maker");
    assert!(dir.join("plugin.json").is_file() && dir.join("main.py").is_file());
    // a second make into the same folder is refused
    assert!(!cli(&["plugin", "new", "Title Maker"], &root).status.success());
    // a bad name is refused before anything is written
    let out = cli(&["plugin", "new", "bad/name"], &root);
    assert!(
        !out.status.success() && text(&out).contains("name"),
        "{}",
        text(&out)
    );
    assert!(!root.join("bad").exists());

    let out = cli(&["plugin", "check", "title-maker"], &root);
    assert!(out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("permissions: read, edit"), "{}", text(&out));

    if python_exists() {
        let out = cli(&["plugin", "test", "title-maker"], &root);
        let said = text(&out);
        assert!(out.status.success(), "{said}");
        assert!(said.contains("layers 0 -> 1"), "{said}");
        // test never saves
        assert!(
            !cli(&["plugin", "test", "title-maker", "--save", "x.fnm"], &root)
                .status
                .success()
        );
        // run saves when asked
        let out = cli(&["plugin", "run", "title-maker", "--save", "made.fnm"], &root);
        assert!(out.status.success(), "{}", text(&out));
        assert!(root.join("made.fnm").is_file());
    }

    let out = cli(&["plugin", "pack", "title-maker", "-o", "tm.fnplugin"], &root);
    assert!(out.status.success(), "{}", text(&out));
    let plugins = root.join("plugins");
    let plugins_arg = plugins.to_string_lossy().into_owned();
    let out = cli(
        &["plugin", "install", "tm.fnplugin", "--folder", &plugins_arg],
        &root,
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("installed Title Maker 0.1.0"),
        "{}",
        text(&out)
    );
    assert!(plugins.join("title-maker").join("main.py").is_file());
    let out = cli(
        &["plugin", "install", "tm.fnplugin", "--folder", &plugins_arg],
        &root,
    );
    assert!(text(&out).contains("updated Title Maker"), "{}", text(&out));

    let out = cli(&["plugin", "list", "--folder", &plugins_arg], &root);
    assert!(text(&out).contains("Title Maker 0.1.0"), "{}", text(&out));

    // a package that is not one is refused
    std::fs::write(root.join("junk.fnplugin"), b"not a zip").unwrap();
    let out = cli(
        &["plugin", "install", "junk.fnplugin", "--folder", &plugins_arg],
        &root,
    );
    assert!(!out.status.success());
    std::fs::remove_dir_all(&root).ok();
}
