//! Solving circuit constraints with an incremental SAT solver.

use num_bigint::BigInt;
use redsat_sat::{SolveResult, Solver};

use crate::aig::{Aig, Edge, Simulation};
use crate::encoder::CnfEncoder;
use crate::int::IntVec;

/// An [`Aig`] wired to an incremental [`Solver`]. Build a circuit, require some of its edges,
/// solve, and read the model back in terms of the circuit.
pub struct CircuitSolver<S> {
    aig: Aig,
    encoder: CnfEncoder,
    solver: S,
    model: Option<Simulation>,
    /// The assumptions of the last solve, with their literals.
    assumed: Vec<(Edge, redsat_sat::Lit)>,
}

impl<S: Solver> CircuitSolver<S> {
    /// Wraps `solver`, which should not have any clauses yet.
    pub fn new(solver: S) -> Self {
        Self {
            aig: Aig::new(),
            encoder: CnfEncoder::new(),
            solver,
            model: None,
            assumed: Vec::new(),
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
        self.solve_assuming(&[])
    }

    /// Like [`solve`](Self::solve), but additionally requires `assumptions` for this call only.
    /// For example, an optimizer can ask "is there a solution better than the current one?"
    /// without making the question permanent.
    pub fn solve_assuming(&mut self, assumptions: &[Edge]) -> SolveResult {
        self.assumed = assumptions
            .iter()
            .map(|&edge| {
                (
                    edge,
                    self.encoder.literal(&self.aig, edge, &mut self.solver),
                )
            })
            .collect();
        let lits: Vec<_> = self.assumed.iter().map(|&(_, lit)| lit).collect();
        let result = self.solver.solve_with(&lits);
        self.model = (result == SolveResult::Sat).then(|| self.read_model());
        result
    }

    /// Whether the assumption `edge` was needed for the last UNSAT answer (it belongs to the
    /// unsat core). Edges that were not assumed in the last solve report false.
    pub fn failed(&self, edge: Edge) -> bool {
        self.assumed
            .iter()
            .any(|&(assumed, lit)| assumed == edge && self.solver.failed(lit))
    }

    /// The value of `edge` in the last model.
    ///
    /// # Panics
    ///
    /// If the last [`solve`](Self::solve) did not return SAT, or `edge` was created after it.
    pub fn value(&self, edge: Edge) -> bool {
        self.model().value(edge)
    }

    /// The value of `int` in the last model. Panics like [`value`](Self::value).
    pub fn value_of(&self, int: &IntVec) -> BigInt {
        let model = self.model();
        int.evaluate(|edge| model.value(edge))
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
