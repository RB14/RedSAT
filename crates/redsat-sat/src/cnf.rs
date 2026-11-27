//! In-memory CNF formulas and the [`ClauseSink`] abstraction.

use crate::{Lit, Var};

/// A destination for clauses: an in-memory [`Cnf`], a solver, and so on.
///
/// Encoders write into a sink instead of a concrete formula type. The same encoding code can
/// then build a formula to inspect or export, or stream clauses straight into a solver.
pub trait ClauseSink {
    /// Allocates a fresh variable that no clause has mentioned so far.
    fn new_var(&mut self) -> Var;

    /// Adds a clause, i.e. a disjunction of literals. Every variable in it must have been
    /// allocated by [`ClauseSink::new_var`]. The empty clause makes the formula unsatisfiable.
    fn add_clause(&mut self, clause: &[Lit]);

    /// The number of variables allocated so far.
    fn num_vars(&self) -> u32;

    /// The number of clauses added so far.
    fn num_clauses(&self) -> usize;
}

/// A formula in conjunctive normal form, held in memory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cnf {
    num_vars: u32,
    clauses: Vec<Vec<Lit>>,
}

impl Cnf {
    /// Creates an empty formula without variables.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty formula whose variables `0..num_vars` are already allocated.
    ///
    /// # Panics
    ///
    /// If `num_vars` exceeds the number of representable variables.
    #[must_use]
    pub fn with_vars(num_vars: u32) -> Self {
        assert!(num_vars <= Var::MAX_INDEX + 1, "too many variables");
        Self {
            num_vars,
            clauses: Vec::new(),
        }
    }

    /// The clauses of the formula, in insertion order.
    #[must_use]
    pub fn clauses(&self) -> &[Vec<Lit>] {
        &self.clauses
    }

    /// Whether every clause is satisfied by the total assignment `value`.
    pub fn is_satisfied_by(&self, value: impl Fn(Var) -> bool) -> bool {
        self.clauses.iter().all(|clause| {
            clause
                .iter()
                .any(|lit| value(lit.var()) != lit.is_negated())
        })
    }

    /// Copies this formula into `sink`, allocating a fresh sink variable for each variable of the
    /// formula. Returns the translation table: variable `i` of the formula became `table[i]`.
    pub fn load_into<S: ClauseSink + ?Sized>(&self, sink: &mut S) -> Vec<Var> {
        let table: Vec<Var> = (0..self.num_vars).map(|_| sink.new_var()).collect();
        let mut buffer = Vec::new();
        for clause in &self.clauses {
            buffer.clear();
            buffer.extend(
                clause
                    .iter()
                    .map(|lit| Lit::new(table[lit.var().index() as usize], lit.is_negated())),
            );
            sink.add_clause(&buffer);
        }
        table
    }
}

impl ClauseSink for Cnf {
    fn new_var(&mut self) -> Var {
        let var = Var::new(self.num_vars);
        self.num_vars += 1;
        var
    }

    fn add_clause(&mut self, clause: &[Lit]) {
        debug_assert!(
            clause.iter().all(|lit| lit.var().index() < self.num_vars),
            "clause mentions an unallocated variable"
        );
        self.clauses.push(clause.to_vec());
    }

    fn num_vars(&self) -> u32 {
        self.num_vars
    }

    fn num_clauses(&self) -> usize {
        self.clauses.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_clauses_under_an_assignment() {
        let mut cnf = Cnf::new();
        let (a, b) = (cnf.new_var(), cnf.new_var());
        cnf.add_clause(&[a.pos(), b.pos()]);
        cnf.add_clause(&[a.neg()]);
        assert!(cnf.is_satisfied_by(|var| var == b));
        assert!(!cnf.is_satisfied_by(|var| var == a));
        assert!(!cnf.is_satisfied_by(|_| false));
    }

    #[test]
    fn empty_clause_is_unsatisfiable() {
        let mut cnf = Cnf::new();
        cnf.add_clause(&[]);
        assert!(!cnf.is_satisfied_by(|_| true));
    }

    #[test]
    fn loading_renumbers_into_a_non_empty_sink() {
        let mut source = Cnf::new();
        let (a, b) = (source.new_var(), source.new_var());
        source.add_clause(&[a.pos(), b.neg()]);

        let mut target = Cnf::new();
        let existing = target.new_var();
        let table = source.load_into(&mut target);

        assert_eq!(table.len(), 2);
        assert!(!table.contains(&existing));
        assert_eq!(target.clauses(), &[vec![table[0].pos(), table[1].neg()]]);
        assert_eq!(target.num_vars(), 3);
    }
}
