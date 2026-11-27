# RedSAT

**RedSAT** (**Red**uce → **SAT**) is a general-purpose programming language whose programs are
*solved*, not executed. You declare variables, state the constraints a solution must satisfy, and
say which values you want. The compiler reduces the program, in polynomial time, to a Boolean
satisfiability problem, runs a state-of-the-art SAT solver on it, and translates the answer back
into your program's values.

```rsat
var n = 4295229443             // = 65537 × 65539
var x, y                       // unknowns: types and bit widths are inferred

require x > 1 and y > 1
require x <= y
require x * y == n             // exact math: no overflow, ever

solve x, y {
    print "{n} = {x} × {y}"
} else {
    print "{n} is prime"
}
```

```console
$ redsat examples/factor.rsat
4295229443 = 65537 × 65539
```

The language is specified in [`docs/design.md`](docs/design.md).

## Status

| Phase | What                                                                                   | State     |
|-------|----------------------------------------------------------------------------------------|-----------|
| 0     | Boolean backbone: AIG circuits, CNF encoding, DIMACS, CaDiCaL backend, exact arithmetic | ✅ done   |
| 1     | Language: `var`/`require`/`solve`, inferred integer widths, loops, symbolic `if`, `all`/`unique`/`minimize`, `redsat.toml` + strict mode, VS Code highlighting | ✅ done |
| 2     | Language server: live diagnostics, autocomplete, hovers with inferred domains           | next      |
| 3     | Arrays, functions, comprehensions, `all_different`, cardinality constraints             | planned   |

## Getting started

### Prerequisites

- **Rust** (stable, 1.85 or newer): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **A C++ compiler and libclang**, used to build the bundled CaDiCaL solver:
  ```bash
  sudo apt install build-essential libclang-dev
  ```

### Build, test, install

```bash
cargo build --workspace
cargo test --workspace
cargo install --path crates/redsat-cli     # puts `redsat` on your PATH
```

### Run programs

```bash
redsat examples/pythagorean.rsat            # run a program (the default action)
redsat examples/factor.rsat --stats         # …and show inferred domains, bit widths, CNF size
redsat check examples/coins.rsat            # parse and type-check only
redsat --strict examples/pell.rsat          # enable every strict check
redsat dimacs problem.cnf                   # solve a raw DIMACS file (exit 10 = SAT, 20 = UNSAT)
```

Without installing, use `cargo run --release -p redsat-cli -- <args>`.

## Examples

| Program                                                        | Shows                                                       |
|----------------------------------------------------------------|-------------------------------------------------------------|
| [`factor.rsat`](examples/factor.rsat)                          | running a multiplier backwards; `else` when there is no solution |
| [`pythagorean.rsat`](examples/pythagorean.rsat)                | `solve all`, ranged types, updating variables in a solve body |
| [`chickens_and_rabbits.rsat`](examples/chickens_and_rabbits.rsat) | bounds inferred from `a + b == 35`; `solve unique`        |
| [`coins.rsat`](examples/coins.rsat)                            | `minimize`                                                  |
| [`knights_and_knaves.rsat`](examples/knights_and_knaves.rsat)  | boolean logic with `iff`                                    |
| [`bigger_of_two.rsat`](examples/bigger_of_two.rsat)            | an `if` whose condition depends on unknowns                 |
| [`sums_of_two_squares.rsat`](examples/sums_of_two_squares.rsat) | projection: auxiliary unknowns that only need to exist     |
| [`pell.rsat`](examples/pell.rsat)                              | unbounded integers and iterative deepening                  |

## Compiler options

Like `tsconfig.json`, an optional `redsat.toml` in the program's directory (or any parent)
configures the compiler. Command-line flags override it.

```toml
[compiler]
strict = true                # explicit-types = "unknowns" + unbounded-ints = "error"
explicit-types = "unknowns"  # "off" | "unknowns" | "all"
unbounded-ints = "error"     # "deepen" | "error"
fallback-int-bits = 32
max-int-bits = 256
```

See [design §6.3](docs/design.md#63-compiler-options-redsattoml) for the details.

## Editor support

[`editors/vscode/`](editors/vscode/) is a VS Code extension that provides syntax highlighting for
`.rsat` files and for ```` ```rsat ```` code blocks in Markdown, plus snippets. See its README for
installation.

## Repository layout

```text
crates/
  redsat-sat/       variables, literals, CNF, DIMACS I/O, the Solver trait, CaDiCaL backend
  redsat-circuit/   And-Inverter Graphs, Tseitin/Plaisted–Greenbaum encoder, exact integer
                    gadgets (IntVec), CircuitSolver (+ examples/factor.rs)
  redsat-lang/      lexer, parser, type inference, options, interpreter, domain inference,
                    bit-blasting and solving (+ golden tests in tests/programs/)
  redsat-cli/       the `redsat` command-line tool
editors/vscode/     VS Code extension: grammar, Markdown injection, snippets
examples/           example programs
docs/design.md      language design, semantics, architecture and roadmap
```

Dependencies point strictly downwards: `cli → lang → circuit → sat`.

## Testing

`cargo test --workspace` runs unit tests, exhaustive circuit tests, randomized encoder
equivalence tests, and golden tests. The golden tests run every program in
`crates/redsat-lang/tests/programs/` and `examples/` and compare their output with checked-in
`.out` files. After a reviewed change in behaviour, regenerate them with:

```bash
REDSAT_BLESS=1 cargo test -p redsat-lang --test programs
```
