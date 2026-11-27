//! Golden tests: run every program under `tests/programs/` and `examples/`, and compare what it
//! prints (stdout, then rendered diagnostics) with the expected output.
//!
//! Expected outputs live next to test programs (`name.out`) and, for the examples, in a
//! `// Output:` comment block at the bottom of each example, so readers see what it prints. Run
//! with `REDSAT_BLESS=1` to (re)write them after reviewing.

use std::fs;
use std::path::{Path, PathBuf};

use redsat_lang::config::{find_config, load_config};
use redsat_lang::{CompilerOptions, OptionLayer, SourceFile};
use redsat_sat::{Cadical, Cdcl, Solver};

fn run_program(path: &Path) -> String {
    run_program_with(path, false).0
}

/// Runs a program with CaDiCaL or the built-in solver; returns its output and whether it failed.
fn run_program_with(path: &Path, builtin: bool) -> (String, bool) {
    let text = fs::read_to_string(path).expect("readable program");
    let name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    let source = SourceFile::new(name, text).with_path(path);
    let layer = find_config(path.parent().expect("a directory"))
        .map(|config| load_config(&config).expect("valid redsat.toml"))
        .unwrap_or_default();
    let options = CompilerOptions::resolve(&layer, &OptionLayer::default()).expect("valid options");
    let mut out = Vec::new();
    let factory = move || -> Box<dyn Solver> {
        if builtin {
            Box::new(Cdcl::new())
        } else {
            Box::new(Cadical::new())
        }
    };
    let outcome = redsat_lang::run(&source, &options, &factory, &mut out);
    let mut text = String::from_utf8(out).expect("UTF-8 output");
    text.push_str(&outcome.render_diagnostics());
    (text, outcome.has_errors())
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

/// The line that starts the expected output at the bottom of an example.
const OUTPUT_MARKER: &str = "// Output:";

/// Where an example's `// Output:` block starts, if it has one.
fn output_block_start(text: &str) -> Option<usize> {
    text.rfind(&format!("\n{OUTPUT_MARKER}\n"))
        .map(|index| index + 1)
}

/// The output documented at the bottom of an example.
fn documented_output(text: &str) -> String {
    let Some(start) = output_block_start(text) else {
        return String::new();
    };
    text[start..]
        .lines()
        .skip(1)
        .map(|line| {
            line.strip_prefix("// ")
                .unwrap_or(line.trim_start_matches("//"))
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

/// `text` with its `// Output:` block replaced by (or extended with) `output`.
fn with_documented_output(text: &str, output: &str) -> String {
    let program = output_block_start(text).map_or(text, |start| &text[..start]);
    let mut result = format!("{}\n\n{OUTPUT_MARKER}\n", program.trim_end());
    for line in output.lines() {
        if line.is_empty() {
            result.push_str("//\n");
        } else {
            result.push_str(&format!("// {}\n", line.trim_end()));
        }
    }
    result
}

/// Outputs compared line by line, ignoring trailing whitespace (which editors strip).
fn normalized(text: &str) -> Vec<&str> {
    text.lines().map(str::trim_end).collect()
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

    let bless = std::env::var_os("REDSAT_BLESS").is_some();
    let mut failures = Vec::new();

    let mut examples = Vec::new();
    programs_in(&crate_dir.join("../../examples"), &mut examples);
    for example in &examples {
        let actual = match hint_for(example) {
            Some(hint) => run_hinted(example, hint),
            None => run_program(example),
        };
        let text = fs::read_to_string(example).expect("readable example");
        if bless {
            fs::write(example, with_documented_output(&text, &actual)).expect("writable example");
            continue;
        }
        let expected = documented_output(&text);
        if normalized(&actual) != normalized(&expected) {
            failures.push(format!(
                "── {} ──\n--- expected (its {OUTPUT_MARKER} block)\n{expected}--- actual\n{actual}",
                example.display()
            ));
        }
    }

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
        cases.len() + examples.len(),
        failures.join("\n")
    );
}

/// Programs whose output legitimately depends on which solutions a solver finds first: they
/// print the first (or first few) solutions of a problem with several.
const SOLVER_DEPENDENT: &[&str] = &[
    "not_unique",
    "functions_and_returns",
    "optimize_all_optima",
    "palindromes",
    "scheduling",
    "standard_library",
];

/// Programs too slow for the built-in solver to be part of the test suite (large multipliers).
const TOO_SLOW_FOR_BUILTIN: &[&str] = &["factor", "pell", "factor100"];

/// Examples that no solver finishes (genuinely hard problems), with a requirement that gives the
/// answer away. Tested with the hint added before their `solve`: their `// Output:` block shows
/// the answer, so this checks that the encoding accepts it.
const HINTS: &[(&str, &str)] = &[(
    "factor100",
    "require x == 32560992426697430739231873273019988725361522757257",
)];

fn hint_for(path: &Path) -> Option<&'static str> {
    let name = path.file_stem()?.to_string_lossy();
    HINTS
        .iter()
        .find(|(example, _)| *example == name)
        .map(|(_, hint)| *hint)
}

/// Runs `path` with `hint` added on the line before its last `solve`.
fn run_hinted(path: &Path, hint: &str) -> String {
    let text = fs::read_to_string(path).expect("readable program");
    let at = text.rfind("\nsolve ").expect("a solve statement") + 1;
    let hinted = format!("{}{hint}\n{}", &text[..at], &text[at..]);
    let directory = std::env::temp_dir().join(format!("redsat-hint-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("a temporary directory");
    let copy = directory.join(path.file_name().expect("a file"));
    fs::write(&copy, hinted).expect("writable copy");
    let output = run_program(&copy);
    fs::remove_dir_all(&directory).ok();
    output.replace(&copy.display().to_string(), &path.display().to_string())
}

/// Every program also runs on the built-in CDCL solver. Each decoded model is checked against
/// the program's constraints, so a wrong answer would surface as an internal error. Outputs must
/// match as sorted lines, unless they depend on the order in which solutions are found.
#[test]
fn programs_agree_on_the_builtin_solver() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut programs = Vec::new();
    programs_in(&crate_dir.join("tests/programs"), &mut programs);
    programs_in(&crate_dir.join("../../examples"), &mut programs);
    let mut failures = Vec::new();
    let mut differing = Vec::new();
    for program in &programs {
        let name = program
            .file_stem()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        if TOO_SLOW_FOR_BUILTIN.contains(&name.as_str()) {
            continue;
        }
        let (reference, reference_failed) = run_program_with(program, false);
        let (builtin, builtin_failed) = run_program_with(program, true);
        if builtin.contains("internal error") || reference_failed != builtin_failed {
            failures.push(format!("{name}: failed={builtin_failed}\n{builtin}"));
            continue;
        }
        let sorted = |text: &str| {
            let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
            lines.sort();
            lines
        };
        if sorted(&reference) != sorted(&builtin) && !SOLVER_DEPENDENT.contains(&name.as_str()) {
            differing.push(name);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(differing.is_empty(), "outputs differ for: {differing:?}");
}
