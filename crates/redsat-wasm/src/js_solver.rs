//! SAT solvers implemented in JavaScript, such as CaDiCaL compiled with Emscripten.
//!
//! The playground's Web Worker loads a solver module and hands the compiler a function that
//! creates instances. Each instance follows the IPASIR-like contract of
//! `playground/src/solvers/*.js`: `add(clauses)`, `solve(assumptions)`, `value(lit)`,
//! `failed(lit)` and `release()`, with DIMACS literals. Calls are synchronous.

use std::collections::{HashMap, HashSet};

use redsat_sat::{ClauseSink, Lit, SolveResult, Solver, Var};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// One instance of a JavaScript SAT solver.
    pub type JsSatSolver;

    /// Adds zero-terminated clauses.
    #[wasm_bindgen(method)]
    fn add(this: &JsSatSolver, clauses: &[i32]);

    /// Solves under assumptions: 10 = SAT, 20 = UNSAT, 0 = unknown.
    #[wasm_bindgen(method)]
    fn solve(this: &JsSatSolver, assumptions: &[i32]) -> i32;

    /// Whether the literal is true in the last model.
    #[wasm_bindgen(method)]
    fn value(this: &JsSatSolver, literal: i32) -> bool;

    /// Whether the assumption belongs to the last unsat core.
    #[wasm_bindgen(method)]
    fn failed(this: &JsSatSolver, literal: i32) -> bool;

    /// Frees the instance.
    #[wasm_bindgen(method)]
    fn release(this: &JsSatSolver);
}

/// Clause literals buffered before they are handed to JavaScript in one call.
const FLUSH_AT: usize = 1 << 16;

/// A [`Solver`] backed by a JavaScript solver instance.
pub(crate) struct JsSolver {
    inner: JsSatSolver,
    name: &'static str,
    num_vars: u32,
    num_clauses: usize,
    /// One more than the highest variable the solver has seen (in a clause or assumption).
    known_vars: u32,
    pending: Vec<i32>,
    has_model: bool,
    failed: HashSet<Lit>,
}

impl JsSolver {
    pub(crate) fn new(inner: JsSatSolver, name: &str) -> Self {
        Self {
            inner,
            name: intern(name),
            num_vars: 0,
            num_clauses: 0,
            known_vars: 0,
            pending: Vec::new(),
            has_model: false,
            failed: HashSet::new(),
        }
    }

    fn mention(&mut self, lits: &[Lit]) {
        if let Some(highest) = lits.iter().map(|lit| lit.var().index() + 1).max() {
            self.known_vars = self.known_vars.max(highest);
        }
    }

    fn flush(&mut self) {
        if !self.pending.is_empty() {
            self.inner.add(&self.pending);
            self.pending.clear();
        }
    }
}

impl Drop for JsSolver {
    fn drop(&mut self) {
        self.inner.release();
    }
}

impl ClauseSink for JsSolver {
    fn new_var(&mut self) -> Var {
        let var = Var::new(self.num_vars);
        self.num_vars += 1;
        var
    }

    fn add_clause(&mut self, clause: &[Lit]) {
        self.mention(clause);
        self.pending
            .extend(clause.iter().map(|lit| lit.to_dimacs()));
        self.pending.push(0);
        self.num_clauses += 1;
        self.has_model = false;
        if self.pending.len() >= FLUSH_AT {
            self.flush();
        }
    }

    fn num_vars(&self) -> u32 {
        self.num_vars
    }

    fn num_clauses(&self) -> usize {
        self.num_clauses
    }
}

impl Solver for JsSolver {
    fn name(&self) -> &'static str {
        self.name
    }

    fn solve_with(&mut self, assumptions: &[Lit]) -> SolveResult {
        self.flush();
        self.mention(assumptions);
        self.has_model = false;
        self.failed.clear();
        let dimacs: Vec<i32> = assumptions.iter().map(|lit| lit.to_dimacs()).collect();
        match self.inner.solve(&dimacs) {
            10 => {
                self.has_model = true;
                SolveResult::Sat
            }
            20 => {
                self.failed = assumptions
                    .iter()
                    .copied()
                    .filter(|lit| self.inner.failed(lit.to_dimacs()))
                    .collect();
                SolveResult::Unsat
            }
            _ => SolveResult::Unknown,
        }
    }

    fn value(&self, lit: Lit) -> bool {
        assert!(
            self.has_model,
            "no model available: the last solve did not return SAT"
        );
        // Ask for the variable, so that `value(!l) == !value(l)`. Variables the solver never
        // saw are unconstrained and read as false.
        let positive = lit.var().index() < self.known_vars
            && self.inner.value(Lit::new(lit.var(), false).to_dimacs());
        positive != lit.is_negated()
    }

    fn failed(&self, lit: Lit) -> bool {
        self.failed.contains(&lit)
    }
}

/// Solver names live as long as the program: intern each distinct one once.
fn intern(name: &str) -> &'static str {
    thread_local! {
        static NAMES: std::cell::RefCell<HashMap<String, &'static str>> = std::cell::RefCell::default();
    }
    NAMES.with(|names| {
        *names
            .borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| Box::leak(name.to_string().into_boxed_str()))
    })
}
