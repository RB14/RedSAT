//! End-to-end: arithmetic circuits solved by CaDiCaL.

use num_bigint::BigUint;
use redsat_circuit::{BitVec, CircuitSolver, bitvec};
use redsat_sat::{Cadical, SolveResult};

/// Requires `x * y == n` with `2 <= x <= y`, over inputs wide enough for any factor.
fn factoring_problem(n: u64) -> (CircuitSolver<Cadical>, BitVec, BitVec) {
    let n = BigUint::from(n);
    let width = usize::try_from(n.bits()).unwrap() - 1; // both factors are at least 2
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let x = BitVec::new_input(aig, width);
    let y = BitVec::new_input(aig, width);
    let one = BitVec::constant(&BigUint::from(1u8));
    let product = bitvec::mul(aig, &x, &y);
    let constraints = [
        bitvec::eq(aig, &product, &BitVec::constant(&n)),
        bitvec::lt(aig, &one, &x),
        bitvec::le(aig, &x, &y),
    ];
    for constraint in constraints {
        cs.require(constraint);
    }
    (cs, x, y)
}

#[test]
fn factors_a_semiprime() {
    let (mut cs, x, y) = factoring_problem(1009 * 1013);
    assert_eq!(cs.solve(), SolveResult::Sat);
    assert_eq!(cs.value_of(&x), BigUint::from(1009u32));
    assert_eq!(cs.value_of(&y), BigUint::from(1013u32));
}

#[test]
fn proves_a_prime_has_no_factors() {
    let (mut cs, _, _) = factoring_problem(65537);
    assert_eq!(cs.solve(), SolveResult::Unsat);
}

#[test]
fn enumerates_every_factorization_exactly_once() {
    // 360 = 2^3 · 3^2 · 5 has 24 divisors, so 11 factor pairs with 2 <= x <= y (excluding 1 · 360).
    let (mut cs, x, y) = factoring_problem(360);
    let mut pairs = Vec::new();
    while cs.solve() == SolveResult::Sat {
        let (vx, vy) = (cs.value_of(&x), cs.value_of(&y));
        pairs.push((u64::try_from(&vx).unwrap(), u64::try_from(&vy).unwrap()));
        let projection: Vec<_> = x.bits().iter().chain(y.bits()).copied().collect();
        cs.block(&projection);
    }
    pairs.sort_unstable();
    let expected: Vec<(u64, u64)> = (2..=18)
        .filter(|d| 360 % d == 0)
        .map(|d| (d, 360 / d))
        .filter(|(a, b)| a <= b)
        .collect();
    assert_eq!(pairs, expected);
}

#[test]
fn projection_ignores_auxiliary_unknowns() {
    // x + y == 6 over 3-bit x and y, plus a 4-bit auxiliary z that only needs to exist (z > x).
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let x = BitVec::new_input(aig, 3);
    let y = BitVec::new_input(aig, 3);
    let z = BitVec::new_input(aig, 4);
    let sum = bitvec::add(aig, &x, &y);
    let sum_is_six = bitvec::eq(aig, &sum, &BitVec::constant(&BigUint::from(6u8)));
    let z_above_x = bitvec::lt(aig, &x, &z);
    cs.require(sum_is_six);
    cs.require(z_above_x);

    let projection: Vec<_> = x.bits().iter().chain(y.bits()).copied().collect();
    let mut count = 0;
    while cs.solve() == SolveResult::Sat {
        count += 1;
        cs.block(&projection);
    }
    assert_eq!(
        count, 7,
        "(0,6), (1,5), …, (6,0), each once despite many valid z"
    );
}
