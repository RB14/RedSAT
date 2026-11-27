//! The solver abstraction that RedSAT compiles against.

use std::fmt;

use crate::{ClauseSink, Lit};

/// The outcome of a satisfiability query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveResult {
    /// A model was found; read it with [`Solver::value`].
    Sat,
    /// No model exists under the given assumptions; see [`Solver::failed`].
    Unsat,
    /// The solver stopped without an answer, e.g. because it was interrupted.
    Unknown,
}

impl fmt::Display for SolveResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Sat => "SATISFIABLE",
            Self::Unsat => "UNSATISFIABLE",
            Self::Unknown => "UNKNOWN",
        })
    }
}

/// An incremental SAT solver, modelled after the standard IPASIR interface.
///
/// Clauses accumulate across calls, while assumptions apply to one call only. This combination
/// is what lets RedSAT:
/// - enumerate solutions: block the current one, then solve again;
/// - optimize: tighten a bound on the objective, then solve again;
/// - explain unsatisfiability: guard each constraint with a selector literal, assume all the
///   selectors, and ask which ones [`failed`](Solver::failed).
///
/// Backend failures such as running out of memory are unrecoverable and cause a panic.
pub trait Solver: ClauseSink {
    /// A short, human-readable name of the backend, e.g. `"CaDiCaL"`.
    fn name(&self) -> &'static str;

    /// Solves the formula under `assumptions`, unit literals that hold for this call only.
    fn solve_with(&mut self, assumptions: &[Lit]) -> SolveResult;

    /// Solves the formula without assumptions.
    fn solve(&mut self) -> SolveResult {
        self.solve_with(&[])
    }

    /// The value of `lit` in the model found by the last [`solve_with`](Solver::solve_with).
    /// Variables the solver did not need to assign are reported as false.
    ///
    /// # Panics
    ///
    /// If the last call did not return [`SolveResult::Sat`], or clauses were added since.
    fn value(&self, lit: Lit) -> bool;

    /// Whether the assumption `lit` was needed to derive the last [`SolveResult::Unsat`], i.e.
    /// whether it belongs to the reported unsat core.
    fn failed(&self, lit: Lit) -> bool;

    /// A handle that stops a [`solve_with`](Solver::solve_with) running on another thread; that
    /// call then returns [`SolveResult::Unknown`] and the solver stays usable. A stop that
    /// arrives before the call has started may be ignored. `None` if the backend cannot be
    /// stopped.
    fn interrupter(&mut self) -> Option<Interrupter> {
        None
    }
}

/// Stops a running solve from another thread (see [`Solver::interrupter`]).
pub type Interrupter = Box<dyn Fn() + Send + Sync>;

impl<S: Solver + ?Sized> Solver for Box<S> {
    fn name(&self) -> &'static str {
        (**self).name()
    }

    fn solve_with(&mut self, assumptions: &[Lit]) -> SolveResult {
        (**self).solve_with(assumptions)
    }

    fn value(&self, lit: Lit) -> bool {
        (**self).value(lit)
    }

    fn failed(&self, lit: Lit) -> bool {
        (**self).failed(lit)
    }

    fn interrupter(&mut self) -> Option<Interrupter> {
        (**self).interrupter()
    }
}
