//! `solve all` on several threads (cube-and-conquer, see `redsat_lang::Parallel`).

use redsat_lang::{CompilerOptions, Hooks, OptionLayer, Parallel, SourceFile};
use redsat_sat::{Cadical, Solver};

/// What `program` prints, enumerating on `workers` threads (1: the usual single solver).
fn output(program: &str, workers: usize) -> String {
    let factory = || -> Box<dyn Solver> { Box::new(Cadical::new()) };
    let parallel = Parallel {
        workers,
        solver: Box::new(|| -> Box<dyn Solver> { Box::new(Cadical::new()) }),
    };
    let hooks = Hooks {
        parallel: (workers > 1).then_some(&parallel),
        ..Hooks::default()
    };
    let options =
        CompilerOptions::resolve(&OptionLayer::default(), &OptionLayer::default()).expect("valid");
    let source = SourceFile::new("parallel.rsat", program);
    let mut out = Vec::new();
    let outcome = redsat_lang::run_with(&source, &options, &factory, &mut out, hooks);
    let mut text = String::from_utf8(out).expect("UTF-8");
    text.push_str(&outcome.render_diagnostics());
    text
}

fn sorted_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().collect();
    lines.sort_unstable();
    lines
}

const QUEENS: &str = "var queen: [int[0..=7]; 8]
require all_different(queen)
require all_different([queen[i] + i for i in 0..8])
require all_different([queen[i] - i for i in 0..8])
solve all queen
";

#[test]
fn parallel_enumeration_finds_the_same_solutions_in_a_stable_order() {
    let serial = output(QUEENS, 1);
    let parallel = output(QUEENS, 4);
    assert_eq!(serial.lines().count(), 92);
    assert_eq!(sorted_lines(&serial), sorted_lines(&parallel));
    for _ in 0..3 {
        assert_eq!(
            output(QUEENS, 4),
            parallel,
            "the order must not vary between runs"
        );
    }
}

#[test]
fn bodies_run_for_every_solution_and_their_errors_stop_the_run() {
    let counted = output(
        "var x, y: int[0..=20]\nrequire x + y == 20\nvar n = 0\nsolve all x, y { n = n + 1 }\nprint n\n",
        4,
    );
    assert_eq!(counted.trim(), "21");
    let failed = output(
        "var x: int[0..=5]\nvar names = [\"a\", \"b\"]\nsolve all x { print names[x] }\n",
        4,
    );
    assert!(
        failed.starts_with("a\nb\nerror: index 2 is out of range"),
        "{failed}"
    );
}

#[test]
fn limits_and_missing_solutions_behave_as_without_threads() {
    // A limit keeps the single solver (finishing cubes could cost far more than the solutions).
    let limited = "var x: int[0..=99]\nrequire x % 7 == 3\nsolve all x limit 3\n";
    assert_eq!(output(limited, 4), output(limited, 1));
    let none = "var x, y: int[0..=30]\nrequire x + y > 70\nsolve all x, y\n";
    assert_eq!(output(none, 4), output(none, 1));
}
