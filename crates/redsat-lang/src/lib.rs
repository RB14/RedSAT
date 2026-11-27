//! The RedSAT language: from source text to solved answers.
//!
//! The pipeline:
//!
//! ```text
//! lexer → parser → check (names, types, strict options)
//!       → interp (run concrete code; record constraints as word-level `ir` terms)
//!       → at each `solve`: domain (bounds, exact sizes) → blast (circuit) → SAT → decode
//!       → the solve body runs once per solution
//! ```

pub mod ast;
mod blast;
pub mod check;
pub mod config;
pub mod diagnostic;
pub mod domain;
mod interp;
pub mod ir;
pub mod lexer;
pub mod load;
pub mod parser;
mod solve;
pub mod source;
pub mod syntax;

use std::io::Write;

pub use config::{CompilerOptions, ExplicitTypes, OptionLayer, UnboundedInts};
pub use diagnostic::{Diagnostic, Severity};
use redsat_sat::Solver;
pub use solve::SolveStats;
pub use source::{SourceFile, SourceMap, Span};

use crate::ast::Program;
use crate::check::TypeInfo;
use crate::interp::Interpreter;

/// Creates a fresh SAT solver. RedSAT calls it once per `solve` attempt.
pub type SolverFactory = dyn Fn() -> Box<dyn Solver> + Sync;

/// What to do after a solution in interactive mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Show the next solution, then ask again.
    Next,
    /// Show all remaining solutions without asking.
    All,
    /// Stop enumerating.
    Quit,
}

/// Asked after each solution in interactive mode, with the number of solutions shown so far.
pub type AskFn = dyn Fn(u64) -> Answer + Sync;

/// Told about the progress of a run, one line at a time, as with `redsat --verbose`.
pub type TraceFn = dyn Fn(&str) + Sync;

/// Optional callbacks for a run.
#[derive(Clone, Copy, Default)]
pub struct Hooks<'h> {
    /// Asked after each solution whether to continue (interactive mode).
    pub ask: Option<&'h AskFn>,
    /// Told about progress: files loaded, each solve's model, circuit and search, and a
    /// summary.
    pub trace: Option<&'h TraceFn>,
}

/// The result of checking or running a program.
#[derive(Debug)]
pub struct RunOutcome {
    /// Errors, warnings and notes, in the order they arose.
    pub diagnostics: Vec<Diagnostic>,
    /// One entry per executed `solve`.
    pub stats: Vec<SolveStats>,
    /// Every file of the program (main file, prelude, imports), to render diagnostics with.
    pub sources: SourceMap,
    /// How long each phase took.
    pub phases: Vec<(&'static str, std::time::Duration)>,
    /// What elaboration did.
    pub steps: ExecSteps,
}

/// Counters of the work the interpreter did while elaborating a program.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExecSteps {
    /// Statements executed (each loop iteration and inlined call counts its statements again).
    pub statements: u64,
    /// Function calls inlined.
    pub calls: u64,
    /// The deepest nesting of inlined calls.
    pub max_depth: usize,
}

impl RunOutcome {
    /// Whether the program failed.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// All diagnostics, rendered with source snippets.
    #[must_use]
    pub fn render_diagnostics(&self) -> String {
        self.diagnostics
            .iter()
            .map(|d| d.render(&self.sources))
            .collect()
    }
}

/// The stack size for compiling and running a program. Programs are elaborated by a
/// tree-walking interpreter, and inlined (possibly recursive) function calls nest deeply; the
/// memory is only reserved, not used, unless a program needs it.
#[cfg(not(target_arch = "wasm32"))]
const STACK_SIZE: usize = 1 << 30;

/// Runs `job` on a thread with a large stack and returns its result.
#[cfg(not(target_arch = "wasm32"))]
fn with_large_stack<T: Send>(job: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("redsat".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, job)
            .expect("cannot start the compiler thread");
        handle
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// WebAssembly has no threads; its stack size is set when linking instead.
#[cfg(target_arch = "wasm32")]
fn with_large_stack<T: Send>(job: impl FnOnce() -> T + Send) -> T {
    job()
}

/// Parses and type-checks `source` (and the files it imports) without running it.
#[must_use]
pub fn check(source: &SourceFile, options: &CompilerOptions) -> RunOutcome {
    with_large_stack(|| {
        let (sources, checked) = front_end(source, options);
        let diagnostics = match checked {
            Ok((_, _, warnings)) => warnings,
            Err(errors) => errors,
        };
        RunOutcome {
            diagnostics,
            stats: Vec::new(),
            sources,
            phases: Vec::new(),
            steps: ExecSteps::default(),
        }
    })
}

/// Compiles and runs `source`. Program output (`print`, solutions) goes to `out`; diagnostics
/// are returned.
pub fn run(
    source: &SourceFile,
    options: &CompilerOptions,
    factory: &SolverFactory,
    out: &mut (dyn Write + Send),
) -> RunOutcome {
    run_with(source, options, factory, out, Hooks::default())
}

/// Like [`run`], with [`Hooks`]: asking whether to continue after each solution (as with
/// `redsat --interactive`) and reporting progress (as with `redsat --verbose`).
pub fn run_with(
    source: &SourceFile,
    options: &CompilerOptions,
    factory: &SolverFactory,
    out: &mut (dyn Write + Send),
    hooks: Hooks<'_>,
) -> RunOutcome {
    with_large_stack(|| run_here(source, options, factory, out, hooks))
}

fn run_here(
    source: &SourceFile,
    options: &CompilerOptions,
    factory: &SolverFactory,
    out: &mut (dyn Write + Send),
    hooks: Hooks<'_>,
) -> RunOutcome {
    let trace = |line: String| {
        if let Some(trace) = hooks.trace {
            trace(&line);
        }
    };
    let started = web_time::Instant::now();
    let (sources, checked) = front_end(source, options);
    let mut phases = vec![("parse, load imports and type-check", started.elapsed())];
    let files: Vec<&str> = sources.files().iter().map(SourceFile::name).collect();
    trace(format!(
        "loaded {} file{} ({}) · parsed and checked in {:.2?}",
        files.len(),
        if files.len() == 1 { "" } else { "s" },
        files.join(", "),
        phases[0].1
    ));
    let (program, types, mut diagnostics) = match checked {
        Ok(checked) => checked,
        Err(diagnostics) => {
            return RunOutcome {
                diagnostics,
                stats: Vec::new(),
                sources,
                phases,
                steps: ExecSteps::default(),
            };
        }
    };
    let running = web_time::Instant::now();
    let mut interpreter = Interpreter::new(&sources, options, &types, factory, out, hooks);
    let result = interpreter.run(&program);
    phases.push(("elaborate and solve", running.elapsed()));
    let steps = interpreter.steps;
    let solving: std::time::Duration = interpreter.stats.iter().map(|s| s.elapsed).sum();
    let calls = match steps.calls {
        0 => "no calls inlined".to_string(),
        1 => "1 call inlined".to_string(),
        n => format!(
            "{n} calls inlined (nested at most {} deep)",
            steps.max_depth
        ),
    };
    trace(format!(
        "finished in {:.2?}: {} statement{} executed, {calls}, {} solve{} taking {:.2?}",
        started.elapsed(),
        steps.statements,
        if steps.statements == 1 { "" } else { "s" },
        interpreter.stats.len(),
        if interpreter.stats.len() == 1 {
            ""
        } else {
            "s"
        },
        solving
    ));
    diagnostics.append(&mut interpreter.warnings);
    if let Err(error) = result {
        diagnostics.push(error);
    }
    let stats = std::mem::take(&mut interpreter.stats);
    drop(interpreter);
    RunOutcome {
        diagnostics,
        stats,
        sources,
        phases,
        steps,
    }
}

/// The inferred domain of the unknowns created by one declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferredDomain {
    /// The declared name's location.
    pub span: Span,
    /// The declared name.
    pub name: String,
    /// The domain, e.g. `[2, 500018000049]`, or `bool` (the hull over all its unknowns, for an
    /// array or struct).
    pub domain: String,
    /// The bit width of one unknown (the widest, for an array or struct).
    pub width: usize,
    /// How many unknowns the declaration created (more than one for arrays and structs).
    pub unknowns: usize,
}

/// What editor tooling needs to know about a program.
#[derive(Debug)]
pub struct Analysis {
    /// Errors and warnings from parsing, type checking and elaboration.
    pub diagnostics: Vec<Diagnostic>,
    /// Types and symbols, when the program parsed.
    pub types: Option<check::TypeInfo>,
    /// The inferred domain of each unknown declaration, when elaboration got that far.
    pub domains: Vec<InferredDomain>,
    /// Every file of the program.
    pub sources: SourceMap,
}

/// Analyses `source` for editor tooling: parses, type-checks, and elaborates the program as a run
/// would, but stops each `solve` after domain inference instead of calling the SAT solver.
/// Elaboration gives up silently after `budget`.
#[must_use]
pub fn analyze(
    source: &SourceFile,
    options: &CompilerOptions,
    budget: std::time::Duration,
) -> Analysis {
    with_large_stack(|| {
        let (sources, loaded) = load::load(source);
        let program = match loaded {
            Ok(program) => program,
            Err(diagnostics) => {
                return Analysis {
                    diagnostics,
                    types: None,
                    domains: Vec::new(),
                    sources,
                };
            }
        };
        let (types, mut diagnostics) = check::check(&program, options);
        if diagnostics.iter().any(Diagnostic::is_error) {
            return Analysis {
                diagnostics,
                types: Some(types),
                domains: Vec::new(),
                sources,
            };
        }
        let factory = || -> Box<dyn Solver> { unreachable!("analysis never solves") };
        let mut sink = std::io::sink();
        let mut interpreter = Interpreter::new(
            &sources,
            options,
            &types,
            &factory,
            &mut sink,
            Hooks::default(),
        );
        interpreter.analysis = Some(interp::AnalysisState::new(budget));
        let result = interpreter.run(&program);
        diagnostics.append(&mut interpreter.warnings);
        match result {
            Err(error) if error.message == interp::ANALYSIS_TIMEOUT => {}
            Err(error) => diagnostics.push(error),
            Ok(()) => {}
        }
        let domains = interpreter
            .analysis
            .take()
            .map(|state| state.finish(&sources))
            .unwrap_or_default();
        drop(interpreter);
        Analysis {
            diagnostics,
            types: Some(types),
            domains,
            sources,
        }
    })
}

type Checked = (Program, TypeInfo, Vec<Diagnostic>);

fn front_end(
    source: &SourceFile,
    options: &CompilerOptions,
) -> (SourceMap, Result<Checked, Vec<Diagnostic>>) {
    let (sources, loaded) = load::load(source);
    let program = match loaded {
        Ok(program) => program,
        Err(errors) => return (sources, Err(errors)),
    };
    let (types, diagnostics) = check::check(&program, options);
    if diagnostics.iter().any(Diagnostic::is_error) {
        return (sources, Err(diagnostics));
    }
    (sources, Ok((program, types, diagnostics)))
}
