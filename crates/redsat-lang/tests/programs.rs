//! Golden tests: run every program under `tests/programs/` and `examples/`, and compare what it
//! prints (stdout, then rendered diagnostics) with the expected output.
//!
//! Expected outputs live next to test programs (`name.out`) and, for the examples, in
//! `tests/expected/examples/`. Run with `REDSAT_BLESS=1` to (re)write them after reviewing.

use std::fs;
use std::path::{Path, PathBuf};

use redsat_lang::config::{find_config, load_config};
use redsat_lang::{CompilerOptions, OptionLayer, SourceFile};
use redsat_sat::{Cadical, Solver};

fn run_program(path: &Path) -> String {
    let text = fs::read_to_string(path).expect("readable program");
    let name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    let source = SourceFile::new(name, text);
    let layer = find_config(path.parent().expect("a directory"))
        .map(|config| load_config(&config).expect("valid redsat.toml"))
        .unwrap_or_default();
    let options = CompilerOptions::resolve(&layer, &OptionLayer::default()).expect("valid options");
    let mut out = Vec::new();
    let factory = || Box::new(Cadical::new()) as Box<dyn Solver>;
    let outcome = redsat_lang::run(&source, &options, &factory, &mut out);
    let mut text = String::from_utf8(out).expect("UTF-8 output");
    for diagnostic in &outcome.diagnostics {
        text.push_str(&diagnostic.render(&source));
    }
    text
}

fn programs_in(directory: &Path, found: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)
        .expect("readable directory")
        .map(|e| e.expect("entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            programs_in(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rsat") {
            found.push(path);
        }
    }
}

#[test]
fn golden_programs() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut cases = Vec::new();

    let mut programs = Vec::new();
    programs_in(&crate_dir.join("tests/programs"), &mut programs);
    cases.extend(programs.into_iter().map(|program| {
        let expected = program.with_extension("out");
        (program, expected)
    }));

    let mut examples = Vec::new();
    programs_in(&crate_dir.join("../../examples"), &mut examples);
    cases.extend(examples.into_iter().map(|example| {
        let name = example
            .file_stem()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        (
            example,
            crate_dir.join(format!("tests/expected/examples/{name}.out")),
        )
    }));

    let bless = std::env::var_os("REDSAT_BLESS").is_some();
    let mut failures = Vec::new();
    for (program, expected_path) in &cases {
        let actual = run_program(program);
        if bless {
            fs::write(expected_path, &actual).expect("writable expectation");
            continue;
        }
        let expected = fs::read_to_string(expected_path).unwrap_or_default();
        if actual != expected {
            failures.push(format!(
                "── {} ──\n--- expected ({})\n{expected}--- actual\n{actual}",
                program.display(),
                expected_path.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} programs differ:\n\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
