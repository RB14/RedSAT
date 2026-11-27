//! [`Solver`] backend for [CaDiCaL](https://github.com/arminbiere/cadical), through the
//! RustSAT bindings, which build the vendored C++ sources.

use std::collections::HashSet;

use rustsat::solvers::{Interrupt, InterruptSolver, Solve, SolveIncremental, SolverResult};
use rustsat::types::{Lit as RsLit, TernaryVal};
use rustsat_cadical::{CaDiCaL, Config};

use crate::{ClauseSink, Lit, SolveResult, Solver, Var};

/// The CaDiCaL SAT solver: incremental, and a frequent SAT Competition winner.
pub struct Cadical {
    inner: CaDiCaL<'static, 'static>,
    num_vars: u32,
    num_clauses: usize,
    /// One past the highest variable index CaDiCaL has seen in a clause or an assumption.
    /// CaDiCaL is never asked about variables beyond this; they are unconstrained.
    known_vars: u32,
    buffer: Vec<RsLit>,
    has_model: bool,
    failed: HashSet<Lit>,
}

impl Cadical {
    /// Creates a solver with an empty formula.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: CaDiCaL::default(),
            num_vars: 0,
            num_clauses: 0,
            known_vars: 0,
            buffer: Vec::new(),
            has_model: false,
            failed: HashSet::new(),
        }
    }

    /// A solver that searches differently from [`Cadical::new`], for portfolios. Member 0 is the
    /// default configuration; the others vary CaDiCaL's preset (aimed at satisfiable or
    /// unsatisfiable instances), the random seed, the initial phase and variable shuffling, the
    /// usual ways to diversify a portfolio.
    #[must_use]
    pub fn diversified(member: u32) -> Self {
        let mut solver = Self::new();
        if member == 0 {
            return solver;
        }
        let config = match member % 3 {
            0 => Config::Default,
            1 => Config::Sat,
            _ => Config::Unsat,
        };
        let inner = &mut solver.inner;
        let _ = inner.set_configuration(config);
        let seed = i32::try_from(member).unwrap_or(i32::MAX);
        let _ = inner.set_option("seed", seed);
        let _ = inner.set_option("phase", i32::from(member % 2 == 0));
        if member >= 3 {
            let _ = inner.set_option("shuffle", 1);
            let _ = inner.set_option("shufflerandom", 1);
        }
        solver
    }

    fn mention(&mut self, lits: &[Lit]) {
        if let Some(highest) = lits.iter().map(|lit| lit.var().index() + 1).max() {
            self.known_vars = self.known_vars.max(highest);
        }
    }
}

impl Default for Cadical {
    fn default() -> Self {
        Self::new()
    }
}

fn to_rustsat(lit: Lit) -> RsLit {
    RsLit::new(lit.var().index(), lit.is_negated())
}

fn from_rustsat(lit: RsLit) -> Lit {
    Lit::new(Var::new(lit.var().idx32()), lit.is_neg())
}

impl ClauseSink for Cadical {
    fn new_var(&mut self) -> Var {
        let var = Var::new(self.num_vars);
        self.num_vars += 1;
        var
    }

    fn add_clause(&mut self, clause: &[Lit]) {
        self.mention(clause);
        self.buffer.clear();
        self.buffer.extend(clause.iter().copied().map(to_rustsat));
        self.inner
            .add_clause_ref(&self.buffer)
            .expect("CaDiCaL failed to add a clause");
        self.num_clauses += 1;
        self.has_model = false;
    }

    fn num_vars(&self) -> u32 {
        self.num_vars
    }

    fn num_clauses(&self) -> usize {
        self.num_clauses
    }
}

impl Solver for Cadical {
    fn name(&self) -> &'static str {
        "CaDiCaL"
    }

    fn solve_with(&mut self, assumptions: &[Lit]) -> SolveResult {
        self.has_model = false;
        self.failed.clear();
        self.mention(assumptions);
        let assumptions: Vec<RsLit> = assumptions.iter().copied().map(to_rustsat).collect();
        match self
            .inner
            .solve_assumps(&assumptions)
            .expect("CaDiCaL failed while solving")
        {
            SolverResult::Sat => {
                self.has_model = true;
                SolveResult::Sat
            }
            SolverResult::Unsat => {
                // RustSAT reports the core as a clause over *negated* failed assumptions.
                let core = self
                    .inner
                    .core()
                    .expect("CaDiCaL failed to report its core");
                self.failed = core.into_iter().map(|lit| !from_rustsat(lit)).collect();
                SolveResult::Unsat
            }
            SolverResult::Interrupted => SolveResult::Unknown,
        }
    }

    fn value(&self, lit: Lit) -> bool {
        assert!(
            self.has_model,
            "no model available: the last solve did not return SAT"
        );
        // Unconstrained variables default to false. For a negative literal that reads as true,
        // which keeps `value(!l) == !value(l)`.
        let unconstrained = lit.is_negated();
        if lit.var().index() >= self.known_vars {
            return unconstrained;
        }
        match self
            .inner
            .lit_val(to_rustsat(lit))
            .expect("CaDiCaL failed to report a value")
        {
            TernaryVal::True => true,
            TernaryVal::False => false,
            TernaryVal::DontCare => unconstrained,
        }
    }

    fn failed(&self, lit: Lit) -> bool {
        self.failed.contains(&lit)
    }

    fn interrupter(&mut self) -> Option<crate::Interrupter> {
        let interrupter = self.inner.interrupter();
        Some(Box::new(move || interrupter.interrupt()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cnf;

    #[test]
    fn solves_and_reports_a_model() {
        let mut solver = Cadical::new();
        let (a, b) = (solver.new_var(), solver.new_var());
        solver.add_clause(&[a.pos(), b.pos()]);
        solver.add_clause(&[a.neg()]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        assert!(!solver.value(a.pos()));
        assert!(solver.value(b.pos()));
        assert!(!solver.value(b.neg()));
    }

    #[test]
    fn unconstrained_variables_read_as_false() {
        let mut solver = Cadical::new();
        let (a, unused) = (solver.new_var(), solver.new_var());
        solver.add_clause(&[a.pos()]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        assert!(!solver.value(unused.pos()));
        assert!(solver.value(unused.neg()));
    }

    #[test]
    fn detects_unsatisfiability_including_the_empty_clause() {
        let mut solver = Cadical::new();
        let a = solver.new_var();
        solver.add_clause(&[a.pos()]);
        solver.add_clause(&[a.neg()]);
        assert_eq!(solver.solve(), SolveResult::Unsat);

        let mut solver = Cadical::new();
        solver.add_clause(&[]);
        assert_eq!(solver.solve(), SolveResult::Unsat);
    }

    #[test]
    fn assumptions_are_temporary_and_failed_ones_are_reported() {
        let mut solver = Cadical::new();
        let (a, b, c) = (solver.new_var(), solver.new_var(), solver.new_var());
        solver.add_clause(&[a.neg(), b.neg()]); // not both a and b

        assert_eq!(
            solver.solve_with(&[a.pos(), b.pos(), c.pos()]),
            SolveResult::Unsat
        );
        assert!(solver.failed(a.pos()) || solver.failed(b.pos()));
        assert!(!solver.failed(c.pos()), "c is irrelevant to the conflict");

        assert_eq!(solver.solve_with(&[a.pos()]), SolveResult::Sat);
        assert!(!solver.value(b.pos()));
        assert_eq!(solver.solve(), SolveResult::Sat);
    }

    #[test]
    fn supports_incremental_clause_addition() {
        let mut solver = Cadical::new();
        let a = solver.new_var();
        assert_eq!(solver.solve(), SolveResult::Sat);
        solver.add_clause(&[a.pos()]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        assert!(solver.value(a.pos()));
        solver.add_clause(&[a.neg()]);
        assert_eq!(solver.solve(), SolveResult::Unsat);
    }

    #[test]
    #[should_panic(expected = "no model available")]
    fn reading_a_value_without_a_model_panics() {
        let mut solver = Cadical::new();
        let a = solver.new_var();
        solver.add_clause(&[a.pos()]);
        let _ = solver.value(a.pos());
    }

    #[test]
    fn loads_a_cnf() {
        // Pigeonhole: 3 pigeons, 2 holes. p(i, h) = pigeon i sits in hole h.
        let mut cnf = Cnf::new();
        let p: Vec<Vec<Var>> = (0..3)
            .map(|_| (0..2).map(|_| cnf.new_var()).collect())
            .collect();
        for pigeon in &p {
            cnf.add_clause(&[pigeon[0].pos(), pigeon[1].pos()]);
        }
        for hole in 0..2 {
            for i in 0..3 {
                for j in i + 1..3 {
                    cnf.add_clause(&[p[i][hole].neg(), p[j][hole].neg()]);
                }
            }
        }
        let mut solver = Cadical::new();
        cnf.load_into(&mut solver);
        assert_eq!(solver.num_vars(), 6);
        assert_eq!(solver.num_clauses(), cnf.num_clauses());
        assert_eq!(solver.solve(), SolveResult::Unsat);
    }
}
