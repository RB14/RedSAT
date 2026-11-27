//! End-to-end: arithmetic circuits solved by CaDiCaL.

use num_bigint::BigInt;
use redsat_circuit::{CircuitSolver, IntVec, Layout, int};
use redsat_sat::{Cadical, SolveResult};

/// Requires `x * y == n` with `2 <= x <= y`, over inputs wide enough for any factor.
fn factoring_problem(n: u64) -> (CircuitSolver<Cadical>, IntVec, IntVec) {
    let n = BigInt::from(n);
    let factor = Layout::for_range(&BigInt::from(0), &(&n / 2));
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let x = IntVec::new_input(aig, factor);
    let y = IntVec::new_input(aig, factor);
    let product = int::mul(aig, &x, &y);
    let constraints = [
        int::eq(aig, &product, &IntVec::constant(&n)),
        int::lt(aig, &IntVec::constant(&BigInt::from(1)), &x),
        int::le(aig, &x, &y),
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
    assert_eq!(cs.value_of(&x), BigInt::from(1009));
    assert_eq!(cs.value_of(&y), BigInt::from(1013));
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
        pairs.push((i64::try_from(&vx).unwrap(), i64::try_from(&vy).unwrap()));
        let projection: Vec<_> = x.bits().iter().chain(y.bits()).copied().collect();
        cs.block(&projection);
    }
    pairs.sort_unstable();
    let expected: Vec<(i64, i64)> = (2..=18)
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
    let small = |width| Layout {
        width,
        signed: false,
    };
    let x = IntVec::new_input(aig, small(3));
    let y = IntVec::new_input(aig, small(3));
    let z = IntVec::new_input(aig, small(4));
    let sum = int::add(aig, &x, &y);
    let sum_is_six = int::eq(aig, &sum, &IntVec::constant(&BigInt::from(6)));
    let z_above_x = int::lt(aig, &x, &z);
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

#[test]
fn solves_with_negative_numbers() {
    // a, b ∈ [-8, 7] with a · b == -12 and a < b: (-6, 2), (-4, 3), (-3, 4), (-2, 6).
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let nibble = Layout {
        width: 4,
        signed: true,
    };
    let a = IntVec::new_input(aig, nibble);
    let b = IntVec::new_input(aig, nibble);
    let product = int::mul(aig, &a, &b);
    let constraints = [
        int::eq(aig, &product, &IntVec::constant(&BigInt::from(-12))),
        int::lt(aig, &a, &b),
    ];
    for constraint in constraints {
        cs.require(constraint);
    }
    let mut pairs = Vec::new();
    while cs.solve() == SolveResult::Sat {
        pairs.push((
            i64::try_from(cs.value_of(&a)).unwrap(),
            i64::try_from(cs.value_of(&b)).unwrap(),
        ));
        let projection: Vec<_> = a.bits().iter().chain(b.bits()).copied().collect();
        cs.block(&projection);
    }
    pairs.sort_unstable();
    assert_eq!(pairs, [(-6, 2), (-4, 3), (-3, 4), (-2, 6)]);
}

#[test]
fn assumptions_hold_for_one_call_only() {
    // x ∈ [0, 15]; minimise x > 5 by temporarily asking for something smaller each time.
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let x = IntVec::new_input(
        aig,
        Layout {
            width: 4,
            signed: false,
        },
    );
    let above_five = int::lt(aig, &IntVec::constant(&BigInt::from(5)), &x);
    cs.require(above_five);

    assert_eq!(cs.solve(), SolveResult::Sat);
    let mut best = cs.value_of(&x);
    loop {
        let bound = IntVec::constant(&best);
        let better = int::lt(cs.aig_mut(), &x, &bound);
        match cs.solve_assuming(&[better]) {
            SolveResult::Sat => best = cs.value_of(&x),
            _ => break,
        }
    }
    assert_eq!(best, BigInt::from(6));
    assert_eq!(
        cs.solve(),
        SolveResult::Sat,
        "the failed assumption left no trace"
    );
}

#[test]
fn reports_which_assumptions_conflict() {
    // x ∈ [0, 15]; the assumptions x > 10, x < 5 and x != 7: the first two conflict.
    let mut cs = CircuitSolver::new(Cadical::new());
    let aig = cs.aig_mut();
    let x = IntVec::new_input(
        aig,
        Layout {
            width: 4,
            signed: false,
        },
    );
    let above = int::lt(aig, &IntVec::constant(&BigInt::from(10)), &x);
    let below = int::lt(aig, &x, &IntVec::constant(&BigInt::from(5)));
    let not_seven = !int::eq(aig, &x, &IntVec::constant(&BigInt::from(7)));
    assert_eq!(
        cs.solve_assuming(&[above, below, not_seven]),
        SolveResult::Unsat
    );
    assert!(cs.failed(above) && cs.failed(below));
    assert!(!cs.failed(not_seven));
}
