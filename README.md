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
| 1     | Language core: `var`/`require`/`solve`, inferred integer widths, loops, symbolic `if`, `all`/`unique`/`minimize`, `redsat.toml` + strict mode | ✅ done |
| 2     | Language server: live diagnostics, autocomplete, hovers/inlay hints with inferred domains | ✅ done |
| 3     | Arrays, functions, comprehensions, `all_different`, bounded `while`                     | ✅ done   |
| 4     | Characters, strings, enums, structs, `/` `%` and bitwise operators on unknowns          | ✅ done   |
| 5     | Explanations of "no solution", `--interactive`, binary-search optimization             | ✅ done   |

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
redsat --interactive examples/n_queens.rsat # step through solutions: Enter / a(ll) / q(uit)
redsat --strict examples/pell.rsat          # enable every strict check
redsat dimacs problem.cnf                   # solve a raw DIMACS file (exit 10 = SAT, 20 = UNSAT)
```

Without installing, use `cargo run --release -p redsat-cli -- <args>`.

## Examples

| Program | Shows |
|---|---|
| [`factor.rsat`](examples/factor.rsat) | running a multiplier backwards; `else` when there is no solution |
| [`sudoku.rsat`](examples/sudoku.rsat) | 2-D arrays, `all_different`, `solve unique` (solved and proven unique in ~30 ms) |
| [`n_queens.rsat`](examples/n_queens.rsat) | `solve all` over an array (all 92 solutions), functions returning strings |
| [`send_more_money.rsat`](examples/send_more_money.rsat) | the classic cryptarithm |
| [`magic_square.rsat`](examples/magic_square.rsat) | comprehensions over rows, columns and diagonals |
| [`graph_coloring.rsat`](examples/graph_coloring.rsat) | enums and structs |
| [`knapsack.rsat`](examples/knapsack.rsat) | `maximize`, sums filtered by unknowns |
| [`scheduling.rsat`](examples/scheduling.rsat) | structs with strings, functions, `minimize` the makespan |
| [`palindromes.rsat`](examples/palindromes.rsat) | strings of unknown characters |
| [`bits.rsat`](examples/bits.rsat) | bitwise operators and shifts |
| [`digit_puzzle.rsat`](examples/digit_puzzle.rsat) | `while … bound`, `/` and `%` on unknowns |
| [`pythagorean.rsat`](examples/pythagorean.rsat) | `solve all`, updating variables in a solve body |
| [`chickens_and_rabbits.rsat`](examples/chickens_and_rabbits.rsat) | bounds inferred from `a + b == 35` |
| [`coins.rsat`](examples/coins.rsat) | `minimize` |
| [`knights_and_knaves.rsat`](examples/knights_and_knaves.rsat) | boolean logic with `iff` |
| [`bigger_of_two.rsat`](examples/bigger_of_two.rsat) | an `if` whose condition depends on unknowns |
| [`sums_of_two_squares.rsat`](examples/sums_of_two_squares.rsat) | projection: auxiliary unknowns that only need to exist |
| [`pell.rsat`](examples/pell.rsat) | unbounded integers and iterative deepening |

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

[`editors/vscode/`](editors/vscode/) is a VS Code extension. It provides:
- syntax highlighting for `.rsat` files and for ```` ```rsat ```` blocks in Markdown;
- snippets;
- a language-server client for `redsat lsp`, which gives live diagnostics, autocomplete,
  hovers and inlay hints with each unknown's inferred domain and bit width,
  go-to-definition and an outline.

The server is the `redsat` binary (`cargo install --path crates/redsat-cli`). See the
extension's README for building and installing it.

## Repository layout

```text
crates/
  redsat-sat/       variables, literals, CNF, DIMACS I/O, the Solver trait, CaDiCaL backend
  redsat-circuit/   And-Inverter Graphs, Tseitin/Plaisted–Greenbaum encoder, exact integer
                    gadgets (IntVec), CircuitSolver (+ examples/factor.rs)
  redsat-lang/      lexer, parser, type inference, options, interpreter, domain inference,
                    bit-blasting and solving (+ golden tests in tests/programs/)
  redsat-lsp/       the language server behind `redsat lsp`
  redsat-cli/       the `redsat` command-line tool (+ an end-to-end test of `redsat lsp`)
editors/vscode/     VS Code extension: grammar, Markdown injection, snippets, LSP client
examples/           example programs
docs/design.md      language design, semantics, architecture and roadmap
```

Dependencies point strictly downwards: `cli → lsp → lang → circuit → sat`.

## Testing

`cargo test --workspace` runs unit tests, exhaustive circuit tests, randomized encoder
equivalence tests, and golden tests. The golden tests run every program in
`crates/redsat-lang/tests/programs/` and `examples/` and compare their output with checked-in
`.out` files. After a reviewed change in behaviour, regenerate them with:

```bash
REDSAT_BLESS=1 cargo test -p redsat-lang --test programs
```
