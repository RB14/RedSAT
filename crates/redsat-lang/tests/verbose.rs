//! The `--verbose` cost report: what each source line adds to the circuit.

use std::sync::{Arc, Mutex};

use redsat_lang::{CompilerOptions, Hooks, OptionLayer, SourceFile};
use redsat_sat::{Cadical, Solver};

/// The trace lines of running `program` with CaDiCaL.
fn trace_of(program: &str) -> Vec<String> {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&lines);
    let trace = move |line: &str| sink.lock().expect("not poisoned").push(line.to_string());
    let factory = || -> Box<dyn Solver> { Box::new(Cadical::new()) };
    let options =
        CompilerOptions::resolve(&OptionLayer::default(), &OptionLayer::default()).expect("valid");
    let source = SourceFile::new("costs.rsat", program);
    let mut out = Vec::new();
    let hooks = Hooks {
        trace: Some(&trace),
        ..Hooks::default()
    };
    let outcome = redsat_lang::run_with(&source, &options, &factory, &mut out, hooks);
    assert!(!outcome.has_errors(), "{}", outcome.render_diagnostics());
    lines.lock().expect("not poisoned").clone()
}

/// A row of the report, by column.
#[derive(Debug)]
struct Row {
    line: String,
    times: String,
    inputs: usize,
    clauses: usize,
    share: String,
    source: String,
}

fn number(text: &str) -> usize {
    text.trim().replace(',', "").parse().expect("a number")
}

/// The rows of the first cost report, and the circuit's (inputs, clauses).
fn report(trace: &[String]) -> (Vec<Row>, usize, usize) {
    let circuit = trace
        .iter()
        .find(|line| line.starts_with("  circuit "))
        .expect("a circuit line");
    let words: Vec<&str> = circuit.split_whitespace().collect();
    let before =
        |word: &str| words[words.iter().position(|w| w.starts_with(word)).expect(word) - 1];
    let (inputs, clauses) = (number(before("inputs")), number(before("clauses")));

    let start = trace
        .iter()
        .position(|line| line.starts_with("  cost "))
        .expect("a cost report");
    let rows = trace[start + 2..]
        .iter()
        .take_while(|line| line.starts_with("             ") && !line.contains("free: "))
        .map(|line| {
            // Fixed columns: line, unknowns, times, inputs, AND gates, clauses, share, source.
            let chars: Vec<char> = line.chars().collect();
            let column = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
            Row {
                line: column(13, 19).trim().to_string(),
                times: column(29, 37).trim().to_string(),
                inputs: number(&column(39, 48)),
                clauses: number(&column(62, 72)),
                share: column(74, 79).trim().to_string(),
                source: column(81, chars.len()),
            }
        })
        .collect();
    (rows, inputs, clauses)
}

#[test]
fn the_cost_report_charges_each_line_and_adds_up() {
    let trace = trace_of(
        "var x, y: int[0..=1000]
require x > -5
require x * y == 391
for i in 0..3 {
    require x != i
}
require divides(y, 391)
solve x, y
",
    );
    let (rows, inputs, clauses) = report(&trace);
    let row = |line: &str| {
        rows.iter()
            .find(|row| row.line == line)
            .unwrap_or_else(|| panic!("no row for line {line}: {rows:#?}"))
    };

    // Every input and clause is charged to exactly one line.
    assert_eq!(rows.iter().map(|row| row.inputs).sum::<usize>(), inputs);
    assert_eq!(rows.iter().map(|row| row.clauses).sum::<usize>(), clauses);

    assert_eq!(row("1").inputs, 20, "x and y: 10 bits each");
    assert_eq!(row("2").share, "free", "the range of x decides it");
    assert!(
        row("3").clauses > row("5").clauses,
        "the multiplier dominates"
    );
    assert_eq!(row("5").times, "×3", "one requirement per iteration");
    // `divides` computes `391 % y`: its "y is not 0" check is made inside the standard library.
    assert!(row("7").source.starts_with("require divides(y, 391)"));
    assert!(
        rows.iter().all(|row| !row.line.contains("prelude")),
        "standard library costs are charged to the calls: {rows:#?}"
    );
    assert!(trace.iter().any(|line| line.contains("free: ")));
}
