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
pub mod parser;
mod solve;
pub mod source;
pub mod syntax;

use std::io::Write;

pub use config::{CompilerOptions, ExplicitTypes, OptionLayer, UnboundedInts};
pub use diagnostic::{Diagnostic, Severity};
use redsat_sat::Solver;
pub use solve::SolveStats;
pub use source::{SourceFile, Span};

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

/// The result of running a program.
#[derive(Debug, Default)]
pub struct RunOutcome {
    /// Errors and warnings, in the order they arose.
    pub diagnostics: Vec<Diagnostic>,
    /// One entry per executed `solve`.
    pub stats: Vec<SolveStats>,
}

impl RunOutcome {
    /// Whether the program failed.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// The stack size for compiling and running a program. Programs are elaborated by a
/// tree-walking interpreter, and inlined (possibly recursive) function calls nest deeply; the
/// memory is only reserved, not used, unless a program needs it.
const STACK_SIZE: usize = 1 << 30;

/// Runs `job` on a thread with a large stack and returns its result.
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

/// Parses and type-checks `source` without running it.
#[must_use]
pub fn check(source: &SourceFile, options: &CompilerOptions) -> Vec<Diagnostic> {
    with_large_stack(|| match front_end(source, options) {
        Ok((_, _, warnings)) => warnings,
        Err(errors) => errors,
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
    run_with(source, options, factory, out, None)
}

/// Like [`run`], but every `solve` asks `ask` after each solution whether to continue, as with
/// `redsat --interactive`.
pub fn run_with(
    source: &SourceFile,
    options: &CompilerOptions,
    factory: &SolverFactory,
    out: &mut (dyn Write + Send),
    ask: Option<&AskFn>,
) -> RunOutcome {
    with_large_stack(|| run_here(source, options, factory, out, ask))
}

fn run_here(
    source: &SourceFile,
    options: &CompilerOptions,
    factory: &SolverFactory,
    out: &mut (dyn Write + Send),
    ask: Option<&AskFn>,
) -> RunOutcome {
    let (program, types, mut diagnostics) = match front_end(source, options) {
        Ok(checked) => checked,
        Err(diagnostics) => {
            return RunOutcome {
                diagnostics,
                stats: Vec::new(),
            };
        }
    };
    let mut interpreter = Interpreter::new(source, options, &types, factory, out, ask);
    let result = interpreter.run(&program);
    diagnostics.append(&mut interpreter.warnings);
    if let Err(error) = result {
        diagnostics.push(error);
    }
    RunOutcome {
        diagnostics,
        stats: interpreter.stats,
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
#[derive(Debug, Default)]
pub struct Analysis {
    /// Errors and warnings from parsing, type checking and elaboration.
    pub diagnostics: Vec<Diagnostic>,
    /// Types and symbols, when the program parsed.
    pub types: Option<check::TypeInfo>,
    /// The inferred domain of each unknown declaration, when elaboration got that far.
    pub domains: Vec<InferredDomain>,
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
        let program = match parser::parse(source) {
            Ok(program) => program,
            Err(error) => {
                return Analysis {
                    diagnostics: vec![error],
                    ..Analysis::default()
                };
            }
        };
        let (types, mut diagnostics) = check::check(&program, options);
        if diagnostics.iter().any(Diagnostic::is_error) {
            return Analysis {
                diagnostics,
                types: Some(types),
                domains: Vec::new(),
            };
        }
        let factory = || -> Box<dyn Solver> { unreachable!("analysis never solves") };
        let mut sink = std::io::sink();
        let mut interpreter = Interpreter::new(source, options, &types, &factory, &mut sink, None);
        interpreter.analysis = Some(interp::AnalysisState::new(
            std::time::Instant::now() + budget,
        ));
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
            .map(|state| state.finish(source))
            .unwrap_or_default();
        drop(interpreter);
        Analysis {
            diagnostics,
            types: Some(types),
            domains,
        }
    })
}

type Checked = (Program, TypeInfo, Vec<Diagnostic>);

fn front_end(source: &SourceFile, options: &CompilerOptions) -> Result<Checked, Vec<Diagnostic>> {
    let program = parser::parse(source).map_err(|error| vec![error])?;
    let (types, diagnostics) = check::check(&program, options);
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(diagnostics);
    }
    Ok((program, types, diagnostics))
}
