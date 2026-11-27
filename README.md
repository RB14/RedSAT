# RedSAT

**RedSAT** (**Red**uce → **SAT**) is a general-purpose programming language whose programs are
*solved*, not executed. You declare variables, state the constraints a solution must satisfy, and
say which values you want. The compiler reduces the program, in polynomial time, to a Boolean
satisfiability problem, runs a state-of-the-art SAT solver on it, and translates the answer back
into your program's integers, booleans, strings and arrays.

```rsat
var n = 1000036000099          // = 1000003 × 1000033
var x, y                       // unknowns: types and bit widths are inferred

require x > 1 and y > 1
require x <= y
require x * y == n             // exact math: no overflow, ever

solve x, y {
    print "{n} = {x} × {y}"
}
```

The language is specified in [`docs/design.md`](docs/design.md).

## Status

| Phase | What                                                                                   | State     |
|-------|----------------------------------------------------------------------------------------|-----------|
| 0     | Boolean backbone: AIG circuits, CNF encoding, DIMACS, CaDiCaL backend, exact arithmetic | ✅ done   |
| 1     | Language front end: parser, `var`/`require`/`solve`, inferred integer widths, VS Code grammar | next |

The `.rsat` program above is the Phase 1 target. Today, the same problem can be built by hand
as a circuit and solved (see *Try it* below).

## Getting started

### Prerequisites

- **Rust** (stable, 1.85 or newer): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **A C++ compiler and libclang**, used to build the bundled CaDiCaL solver:
  ```bash
  sudo apt install build-essential libclang-dev
  ```

### Build and test

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
```

### Try it

```bash
# Factor a number by running a multiplier circuit backwards (or prove it prime).
cargo run --release -p redsat-circuit --example factor -- 4295229443

# Solve any DIMACS CNF file; exits with 10 (SAT) or 20 (UNSAT), like competition solvers.
cargo run --release -p redsat-cli -- dimacs path/to/problem.cnf
```

## Repository layout

```text
crates/
  redsat-sat/       variables, literals, CNF, DIMACS I/O, the Solver trait, CaDiCaL backend
  redsat-circuit/   And-Inverter Graphs, Tseitin/Plaisted–Greenbaum encoder, exact bit-vector
                    gadgets, CircuitSolver (+ examples/factor.rs)
  redsat-cli/       the `redsat` command-line tool
docs/
  design.md         language design, semantics, architecture and roadmap
```

Dependencies point strictly downwards: `cli → (lang) → circuit → sat`.
