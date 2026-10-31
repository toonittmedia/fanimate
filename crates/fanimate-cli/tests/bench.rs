//! `fanimate-cli bench` runs end to end: it evaluates, tessellates, saves a
//! project to a temporary folder and loads it back. It once failed with
//! "The system cannot find the path specified" because it never made that
//! folder, which went unnoticed since projects became single files.

use std::process::Command;

#[test]
fn the_cpu_benchmark_runs_and_reports_save_and_load() {
    let out = Command::new(env!("CARGO_BIN_EXE_fanimate-cli"))
        .args(["bench", "--bones", "8", "--frames", "20"])
        .output()
        .expect("fanimate-cli runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let errors = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{errors}\n{text}");
    assert!(text.contains("save") && text.contains("load"), "{text}");
}
