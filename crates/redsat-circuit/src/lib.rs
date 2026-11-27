//! The circuit layer of RedSAT.
//!
//! Programs are bit-blasted into an [`Aig`] (And-Inverter Graph). [`int`] provides exact,
//! overflow-free integer arithmetic on top of it (unsigned and two's complement),
//! [`CnfEncoder`] turns the parts of the circuit that matter into CNF, and [`CircuitSolver`]
//! combines all of this with an incremental SAT solver.

pub mod aig;
pub mod circuit_solver;
pub mod encoder;
pub mod int;

pub use aig::{Aig, Edge, Node, NodeId, Simulation};
pub use circuit_solver::CircuitSolver;
pub use encoder::CnfEncoder;
pub use int::{BitOp, IntVec, Layout};
