//! Factors a number by building a multiplier circuit by hand and letting CaDiCaL run it
//! *backwards*: "which inputs make this multiplier output n?"
//!
//! ```text
//! cargo run --release -p redsat-circuit --example factor -- 4295229443
//! ```
//!
//! It prints every factorization `n = x × y` with `2 ≤ x ≤ y`. If there is none, the solver has
//! proved that `n` is prime. This is Phase 0's milestone: the whole SAT pipeline, minus the
//! language front end, which will generate exactly this kind of circuit from `.rsat` source.

use std::process::ExitCode;
use std::time::Instant;

use num_bigint::BigUint;
use redsat_circuit::{BitVec, CircuitSolver, bitvec};
use redsat_sat::{Cadical, ClauseSink, SolveResult};

fn main() -> ExitCode {
    let argument = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "4295229443".into());
    let Ok(n) = argument.parse::<BigUint>() else {
        eprintln!("usage: factor <positive integer>");
        return ExitCode::FAILURE;
    };
    if n < BigUint::from(4u8) {
        println!("{n} has no factorization with both factors at least 2");
        return ExitCode::SUCCESS;
    }

    // Bounds a compiler can infer from the constraints below, which shrink the search space:
    // - both factors are at least 2, so each is at most n / 2, one bit fewer than n;
    // - x <= y and x * y == n imply x * x <= n, so x <= √n needs only half of n's bits.
    let n_bits = usize::try_from(n.bits()).expect("number too large");
    let (x_width, y_width) = (n_bits.div_ceil(2), n_bits - 1);
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let x = BitVec::new_input(aig, x_width);
    let y = BitVec::new_input(aig, y_width);
    let product = bitvec::mul(aig, &x, &y);
    let one = BitVec::constant(&BigUint::from(1u8));
    let constraints = [
        bitvec::eq(aig, &product, &BitVec::constant(&n)), // x * y == n
        bitvec::lt(aig, &one, &x),                        // x > 1
        bitvec::le(aig, &x, &y),                          // x <= y (symmetry breaking)
    ];
    for constraint in constraints {
        cs.require(constraint);
    }

    println!(
        "n = {n} ({n_bits} bits) · x: {x_width} bits, y: {y_width} bits · AIG: {} AND gates · \
         CNF: {} vars, {} clauses",
        cs.aig().num_ands(),
        cs.solver().num_vars(),
        cs.solver().num_clauses(),
    );

    let projection: Vec<_> = x.bits().iter().chain(y.bits()).copied().collect();
    let mut found = 0;
    loop {
        let start = Instant::now();
        let result = cs.solve();
        let elapsed = start.elapsed();
        match result {
            SolveResult::Sat => {
                found += 1;
                println!(
                    "  {n} = {} × {}   [{elapsed:.2?}]",
                    cs.value_of(&x),
                    cs.value_of(&y)
                );
                cs.block(&projection);
            }
            SolveResult::Unsat if found == 0 => {
                println!("  {n} is prime: no factorization exists   [{elapsed:.2?}]");
                break;
            }
            SolveResult::Unsat => {
                println!("  that's all: {found} factorization(s)   [{elapsed:.2?}]");
                break;
            }
            SolveResult::Unknown => {
                println!("  the solver gave up   [{elapsed:.2?}]");
                break;
            }
        }
    }
    ExitCode::SUCCESS
}
