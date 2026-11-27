//! The circuit layer of RedSAT.
//!
//! Programs are bit-blasted into an [`Aig`] (And-Inverter Graph). [`bitvec`] provides exact,
//! overflow-free arithmetic gadgets on top of it, [`CnfEncoder`] turns the parts of the circuit
//! that matter into CNF, and [`CircuitSolver`] combines all of this with an incremental SAT
//! solver.

pub mod aig;
pub mod bitvec;
pub mod circuit_solver;
pub mod encoder;

pub use aig::{Aig, Edge, Node, NodeId, Simulation};
pub use bitvec::BitVec;
pub use circuit_solver::CircuitSolver;
pub use encoder::CnfEncoder;
