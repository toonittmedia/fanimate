//! `fanimate-cli models` through the real program: pack a model, check the
//! policy refuses what it must, and verify, install, list and remove.

use std::path::Path;
use std::process::{Command, Output};

// 32 bytes of 0x07, base64: a signing seed for the test, never a real one
const SEED_B64: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(args)
        .env("FANIMATE_TEST_SEED", SEED_B64)
        .output()
        .unwrap()
}

fn said(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

fn manifest(licence: &str, scraped: bool, consent: &str) -> String {
    format!(
        r#"{{"id": "caption-voice", "name": "Caption voice", "version": "1.0", "task": "speech_to_text",
        "runtime": "onnx", "weights": "model.onnx", "licence": "{licence}", "licence_file": "LICENSE.txt",
        "source": "https://example.org/caption-voice",
        "provenance": {{"training_data": "Recordings donated by forty speakers with written consent", "consent": "{consent}", "scraped_art": {scraped}}}}}"#
    )
}

#[test]
fn a_model_is_packed_verified_installed_listed_and_removed_and_the_policy_refuses_the_rest() {
    let dir = std::env::temp_dir().join(format!("fanimate-cli-models-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let weights = write(&dir, "model.onnx", "tiny pretend weights");
    let licence = write(&dir, "LICENSE.txt", "Apache License 2.0 text");
    let good = write(&dir, "good.json", &manifest("Apache-2.0", false, "donated"));
    let out = dir.join("voice.fnmodel").to_string_lossy().into_owned();

    let packed = cli(&[
        "models",
        "pack",
        "--manifest",
        &good,
        "--weights",
        &weights,
        "--licence",
        &licence,
        "-o",
        &out,
        "--seed-env",
        "FANIMATE_TEST_SEED",
    ]);
    assert!(packed.status.success(), "{}", said(&packed));
    // the output names the public key that signed it: trust that, and only that
    let text = said(&packed);
    let public = text.rsplit("key ").next().unwrap().trim().to_owned();
    assert_eq!(public.len(), 64, "{text}");

    let ok = cli(&["models", "verify", &out, "--trust", &public]);
    assert!(
        ok.status.success() && said(&ok).contains("verified"),
        "{}",
        said(&ok)
    );
    // another key, or no key, is refused
    let wrong = cli(&["models", "verify", &out, "--trust", &"ab".repeat(32)]);
    assert!(
        !wrong.status.success() && said(&wrong).contains("signature"),
        "{}",
        said(&wrong)
    );
    assert!(
        !cli(&["models", "verify", &out]).status.success(),
        "no --trust, no accepting"
    );

    let folder = dir.join("models").to_string_lossy().into_owned();
    let installed = cli(&["models", "install", &out, "--trust", &public, "--folder", &folder]);
    assert!(installed.status.success(), "{}", said(&installed));
    let listed = cli(&["models", "list", "--folder", &folder]);
    assert!(
        said(&listed).contains("Caption voice 1.0") && said(&listed).contains("donated"),
        "{}",
        said(&listed)
    );
    let gone = cli(&["models", "remove", "caption-voice", "--folder", &folder]);
    assert!(gone.status.success(), "{}", said(&gone));
    assert!(said(&cli(&["models", "list", "--folder", &folder])).contains("no models installed"));

    // the policy stops a model before a package exists
    for (name, licence_text, scraped, consent, why) in [
        ("nc", "CC-BY-NC-4.0", false, "donated", "lawyer"),
        ("gpl", "GPL-3.0-only", false, "donated", "permissive"),
        ("scraped", "MIT", true, "donated", "scraped"),
        ("unknown", "MIT", false, "unknown", "commissioned"),
    ] {
        let bad = write(
            &dir,
            &format!("{name}.json"),
            &manifest(licence_text, scraped, consent),
        );
        let o = dir.join(format!("{name}.fnmodel")).to_string_lossy().into_owned();
        let refused = cli(&[
            "models",
            "pack",
            "--manifest",
            &bad,
            "--weights",
            &weights,
            "--licence",
            &licence,
            "-o",
            &o,
            "--seed-env",
            "FANIMATE_TEST_SEED",
        ]);
        assert!(!refused.status.success(), "{name} was packed");
        assert!(said(&refused).contains(why), "{name}: {}", said(&refused));
        assert!(!Path::new(&o).exists(), "{name}: nothing was written");
    }
    // a signing seed that is not there is refused, not guessed
    let no_seed = cli(&[
        "models",
        "pack",
        "--manifest",
        &good,
        "--weights",
        &weights,
        "--licence",
        &licence,
        "-o",
        &out,
        "--seed-env",
        "NO_SUCH_VARIABLE_FOR_FANIMATE",
    ]);
    assert!(!no_seed.status.success());
    std::fs::remove_dir_all(&dir).ok();
}
