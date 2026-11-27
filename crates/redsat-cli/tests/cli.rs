//! End-to-end tests of the `redsat` command line.

use std::path::PathBuf;
use std::process::Command;

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

#[test]
fn running_a_file_prints_its_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_redsat"))
        .arg(example("chickens_and_rabbits.rsat"))
        .output()
        .expect("redsat runs");
    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
    assert!(
        output.stderr.is_empty(),
        "no diagnostics: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn verbose_reports_each_phase() {
    let output = Command::new(env!("CARGO_BIN_EXE_redsat"))
        .args(["--verbose", "--solver", "builtin", "--threads", "1"])
        .arg(example("sudoku.rsat"))
        .output()
        .expect("redsat runs");
    assert!(output.status.success());
    let report = String::from_utf8(output.stderr).expect("UTF-8");
    for expected in [
        "[redsat] loaded 2 files",
        "[redsat] solve #1 at",
        "  model      unique",
        "  unknowns   51 (51 integer, 0 boolean)",
        "  space      204 decision bits → 2^204 candidate assignments",
        "  circuit    ",
        "  search     with built-in CDCL…",
        "  searched   2 SAT calls · 1 solution",
        "[redsat] finished in ",
    ] {
        assert!(
            report.contains(expected),
            "missing {expected:?} in:\n{report}"
        );
    }
}
