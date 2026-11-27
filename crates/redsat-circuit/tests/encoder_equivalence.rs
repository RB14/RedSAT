//! The CNF encoding must agree with circuit simulation on every input assignment.

use redsat_circuit::{Aig, CnfEncoder, Edge};
use redsat_sat::{Cadical, Lit, SolveResult, Solver};

/// A small deterministic xorshift generator, so failures are reproducible from the seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn coin(&mut self) -> bool {
        self.next() & 1 == 1
    }
}

const INPUTS: usize = 5;

/// Builds a random circuit and returns it with all the edges it created.
fn random_circuit(seed: u64) -> (Aig, Vec<Edge>, Vec<Edge>) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut aig = Aig::new();
    let inputs: Vec<Edge> = (0..INPUTS).map(|_| aig.new_input()).collect();
    let mut pool = inputs.clone();
    pool.push(Edge::TRUE);
    for _ in 0..40 {
        let pick = |rng: &mut Rng| pool[rng.below(pool.len())].negate_if(rng.coin());
        let (a, b, c) = (pick(&mut rng), pick(&mut rng), pick(&mut rng));
        let gate = match rng.below(4) {
            0 => aig.and(a, b),
            1 => aig.or(a, b),
            2 => aig.xor(a, b),
            _ => aig.ite(a, b, c),
        };
        pool.push(gate);
    }
    (aig, inputs, pool)
}

/// Assumptions fixing every input to the bits of `assignment`, plus the expected simulation.
fn fix_inputs(
    aig: &Aig,
    inputs: &[Edge],
    encoder: &mut CnfEncoder,
    solver: &mut Cadical,
    assignment: u32,
) -> (Vec<Lit>, redsat_circuit::Simulation) {
    let values: Vec<bool> = (0..inputs.len())
        .map(|i| assignment >> i & 1 == 1)
        .collect();
    let assumptions = inputs
        .iter()
        .zip(&values)
        .map(|(&input, &value)| {
            let lit = encoder.literal(aig, input, solver);
            if value { lit } else { !lit }
        })
        .collect();
    (assumptions, aig.simulate(&values))
}

#[test]
fn required_edges_are_satisfiable_exactly_when_they_simulate_true() {
    for seed in 0..40 {
        let (aig, inputs, pool) = random_circuit(seed);
        for &root in pool.iter().rev().take(6) {
            let mut solver = Cadical::new();
            let mut encoder = CnfEncoder::new();
            encoder.require(&aig, root, &mut solver);
            for assignment in 0..1u32 << INPUTS {
                let (assumptions, sim) =
                    fix_inputs(&aig, &inputs, &mut encoder, &mut solver, assignment);
                let expected = if sim.value(root) {
                    SolveResult::Sat
                } else {
                    SolveResult::Unsat
                };
                assert_eq!(solver.solve_with(&assumptions), expected, "seed {seed}");
            }
        }
    }
}

#[test]
fn literals_are_equivalent_to_their_edges_even_after_one_sided_encoding() {
    for seed in 0..40 {
        let (mut aig, inputs, pool) = random_circuit(seed);
        let guard = aig.new_input();
        let roots: Vec<Edge> = pool.iter().rev().take(6).copied().collect();

        let mut solver = Cadical::new();
        let mut encoder = CnfEncoder::new();
        // First encode every root one-sidedly, behind a guard that we will assume false...
        for &root in &roots {
            let guarded = aig.implies(guard, root);
            encoder.require(&aig, guarded, &mut solver);
        }
        // ...then demand exact literals, which must add the missing directions. The guard is
        // the last input and the assignments below never set its bit, so it is always off.
        let inputs: Vec<Edge> = inputs.iter().copied().chain([guard]).collect();
        for &root in &roots {
            let lit = encoder.literal(&aig, root, &mut solver);
            for assignment in 0..1u32 << INPUTS {
                let (mut assumptions, sim) =
                    fix_inputs(&aig, &inputs, &mut encoder, &mut solver, assignment);
                for (polarity, holds) in [(lit, sim.value(root)), (!lit, !sim.value(root))] {
                    assumptions.push(polarity);
                    let expected = if holds {
                        SolveResult::Sat
                    } else {
                        SolveResult::Unsat
                    };
                    assert_eq!(solver.solve_with(&assumptions), expected, "seed {seed}");
                    assumptions.pop();
                }
            }
        }
    }
}
