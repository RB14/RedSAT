//! The Boolean layer of RedSAT.
//!
//! This crate knows nothing about the RedSAT language. It provides the vocabulary that every
//! higher layer compiles down to: propositional [`Var`]iables and [`Lit`]erals, [`Cnf`] formulas,
//! the DIMACS exchange format, and the [`Solver`] abstraction together with its backends.

pub mod cnf;
pub mod dimacs;
pub mod lit;
pub mod solver;

#[cfg(feature = "cadical")]
pub mod cadical;

#[cfg(feature = "cadical")]
pub use cadical::Cadical;
pub use cnf::{ClauseSink, Cnf};
pub use lit::{Lit, Var};
pub use solver::{SolveResult, Solver};
