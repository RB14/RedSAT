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
    with_large_stack(|| run_here(source, options, factory, out))
}

fn run_here(
    source: &SourceFile,
    options: &CompilerOptions,
    factory: &SolverFactory,
    out: &mut (dyn Write + Send),
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
    let mut interpreter = Interpreter::new(source, options, &types, factory, out);
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

type Checked = (Program, TypeInfo, Vec<Diagnostic>);

fn front_end(source: &SourceFile, options: &CompilerOptions) -> Result<Checked, Vec<Diagnostic>> {
    let program = parser::parse(source).map_err(|error| vec![error])?;
    let (types, diagnostics) = check::check(&program, options);
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(diagnostics);
    }
    Ok((program, types, diagnostics))
}
