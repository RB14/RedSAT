//! Solving circuit constraints with an incremental SAT solver.

use num_bigint::BigUint;
use redsat_sat::{SolveResult, Solver};

use crate::aig::{Aig, Edge, Simulation};
use crate::bitvec::BitVec;
use crate::encoder::CnfEncoder;

/// An [`Aig`] wired to an incremental [`Solver`]. Build a circuit, require some of its edges,
/// solve, and read the model back in terms of the circuit.
pub struct CircuitSolver<S> {
    aig: Aig,
    encoder: CnfEncoder,
    solver: S,
    model: Option<Simulation>,
}

impl<S: Solver> CircuitSolver<S> {
    /// Wraps `solver`, which should not have any clauses yet.
    pub fn new(solver: S) -> Self {
        Self {
            aig: Aig::new(),
            encoder: CnfEncoder::new(),
            solver,
            model: None,
        }
    }

    /// The circuit built so far.
    pub fn aig(&self) -> &Aig {
        &self.aig
    }

    /// The circuit, for building more of it.
    pub fn aig_mut(&mut self) -> &mut Aig {
        &mut self.aig
    }

    /// The underlying solver, e.g. for statistics.
    pub fn solver(&self) -> &S {
        &self.solver
    }

    /// Constrains `edge` to be true in every future solution.
    pub fn require(&mut self, edge: Edge) {
        self.encoder.require(&self.aig, edge, &mut self.solver);
    }

    /// Searches for an assignment of the circuit inputs that satisfies all requirements.
    pub fn solve(&mut self) -> SolveResult {
        let result = self.solver.solve();
        self.model = (result == SolveResult::Sat).then(|| self.read_model());
        result
    }

    /// The value of `edge` in the last model.
    ///
    /// # Panics
    ///
    /// If the last [`solve`](Self::solve) did not return SAT, or `edge` was created after it.
    pub fn value(&self, edge: Edge) -> bool {
        self.model().value(edge)
    }

    /// The value of `bits` in the last model. Panics like [`value`](Self::value).
    pub fn value_of(&self, bits: &BitVec) -> BigUint {
        let model = self.model();
        bits.evaluate(|edge| model.value(edge))
    }

    /// Excludes the last model's combination of values on `edges` from every future solution.
    ///
    /// This is a *projected* blocking clause. Everything outside `edges` stays free, so repeated
    /// solve-and-block enumerates each distinct combination of `edges` exactly once, however many
    /// ways the rest of the circuit can complete it.
    ///
    /// # Panics
    ///
    /// If the last [`solve`](Self::solve) did not return SAT.
    pub fn block(&mut self, edges: &[Edge]) {
        let model = self.model();
        let differs: Vec<Edge> = edges
            .iter()
            .map(|&edge| edge.negate_if(model.value(edge)))
            .collect();
        let any_differs = self.aig.or_all(differs);
        self.require(any_differs);
    }

    fn model(&self) -> &Simulation {
        self.model
            .as_ref()
            .expect("no model available: the last solve did not return SAT")
    }

    /// Reads the circuit inputs from the solver and re-evaluates the circuit. Internal gate
    /// variables are deliberately ignored: with polarity-reduced encodings their values need not
    /// match their gates.
    fn read_model(&self) -> Simulation {
        let inputs: Vec<bool> = self
            .aig
            .inputs()
            .iter()
            .map(|&node| {
                self.encoder
                    .var(node)
                    .is_some_and(|var| self.solver.value(var.pos()))
            })
            .collect();
        self.aig.simulate(&inputs)
    }
}
