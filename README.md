# RedSAT

**RedSAT** (**Red**uce → **SAT**) is a general-purpose programming language whose programs are
*solved*, not executed. You write ordinary-looking imperative code, but some variables are
**unknowns**. You state what must hold, and say which values you want. The compiler reduces the
program, in polynomial time, to a Boolean satisfiability (SAT) problem, runs a SAT solver on
it, and translates the answer back into your program's values.

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

## Try it

- **In your browser:** the [playground](playground/) runs the compiler and its SAT solvers as
  WebAssembly, with nothing to install. It is deployed to GitHub Pages by
  [`.github/workflows/pages.yml`](.github/workflows/pages.yml): the playground sits at the
  site root and the guide under `/guide/`.
- **Learn the language:** the [RedSAT Guide](docs/guide/src/introduction.md) is a tutorial,
  a cookbook of modelling patterns and a complete reference. Every code sample in it is tested.
- **Why it works the way it does:** the [design document](docs/design.md) covers the
  semantics, the encodings (with measurements) and the architecture.

## What you get

- **Exact integers.** There is no overflow. Bit widths come from the constraints
  (`x + y == 35` with `x, y >= 0` bounds both by 35); explicit bounds such as `int[0..=9]` or
  `uint<8>` take precedence. Unbounded unknowns are searched with growing widths and a warning,
  or rejected in strict mode.
- **Imperative code over unknowns.** `if`, `for`, bounded `while`, functions with early
  returns, arrays (even indexed by unknowns), strings, chars, enums and structs all work when
  the values are unknown.
- **Known and unknown parts side by side.** `?` is an anonymous unknown, so a Sudoku grid is
  simply `[[5, 3, ?, …], …]`.
- **Ways to ask:**
  - `solve x` finds one solution;
  - `solve all` finds every one (or `--interactive`, one at a time);
  - `solve unique` also proves there is no other;
  - `limit K` caps the count;
  - `minimize` and `maximize` optimize;
  - `else` runs when there is no solution.
- **"No solution" explained.** The compiler points at a minimal set of requirements that
  cannot hold together.
- **A standard library written in RedSAT.** `digits`, `sorted` (a sorting network),
  `lex_less`, `exactly`, `at_most`, `is_permutation`, string helpers and more. Your own
  functions and `import "file.rsat"` work the same way.
- **Two solvers.** CaDiCaL is the default. A pure-Rust CDCL solver (`--solver builtin`) needs
  no native code.
- **Tooling:**
  - a REPL;
  - `--verbose` progress (decision bits, search space, CNF size, timings);
  - a VS Code extension with a language server (diagnostics, completion, hovers showing each
    unknown's inferred domain);
  - the web playground.

## Install

You need **Rust** (stable, 1.85 or newer) and, to build the bundled CaDiCaL solver, **a C++
compiler and libclang**:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
sudo apt install build-essential libclang-dev

cargo install --path crates/redsat-cli     # puts `redsat` on your PATH
```

## Use

```bash
redsat examples/sudoku.rsat                 # run a program (the default action)
redsat -v examples/factor.rsat              # report each phase: unknowns, search space, CNF, timings
redsat --stats examples/coins.rsat          # per-solve domains, bit widths and CNF size
redsat --interactive examples/n_queens.rsat # step through solutions: Enter / a(ll) / q(uit)
redsat --solver builtin examples/bits.rsat  # use the pure-Rust solver
redsat --strict examples/pell.rsat          # turn on every strict check
redsat check examples/coins.rsat            # parse and type-check only
redsat repl                                 # build a program one statement at a time
redsat dimacs problem.cnf                   # solve a raw DIMACS file (exit 10 = SAT, 20 = UNSAT)
```

Without installing, use `cargo run --release -p redsat-cli -- <args>`. The
[command-line reference](docs/guide/src/reference/cli.md) lists every option.

An optional `redsat.toml` in the program's directory (or any parent) configures the compiler,
like `tsconfig.json`. Command-line flags override it:

```toml
[compiler]
strict = true                # explicit-types = "unknowns" + unbounded-ints = "error"
explicit-types = "unknowns"  # "off" | "unknowns" | "all"
unbounded-ints = "error"     # "deepen" | "error"
fallback-int-bits = 32
max-int-bits = 256
```

## Examples

Each example explains its problem in plain words at the top and shows its output at the bottom.

| Program | Shows |
|---|---|
| [`chickens_and_rabbits.rsat`](examples/chickens_and_rabbits.rsat) | bounds inferred from `a + b == 35`; `solve unique` |
| [`bigger_of_two.rsat`](examples/bigger_of_two.rsat) | an `if` whose condition depends on unknowns |
| [`digit_puzzle.rsat`](examples/digit_puzzle.rsat) | `while … bound`, `/` and `%` on unknowns |
| [`send_more_money.rsat`](examples/send_more_money.rsat) | the classic cryptarithm, `all_different` |
| [`sudoku.rsat`](examples/sudoku.rsat) | `?` holes in a grid, `solve unique` (solved and proven unique in ~20 ms) |
| [`n_queens.rsat`](examples/n_queens.rsat) | `solve all` over an array (all 92 solutions), functions returning strings |
| [`magic_square.rsat`](examples/magic_square.rsat) | comprehensions over rows, columns and diagonals |
| [`knights_and_knaves.rsat`](examples/knights_and_knaves.rsat) | boolean logic with `iff` |
| [`coins.rsat`](examples/coins.rsat) | `minimize` |
| [`graph_coloring.rsat`](examples/graph_coloring.rsat) | enums and structs |
| [`scheduling.rsat`](examples/scheduling.rsat) | structs with strings, functions, `minimize` the makespan |
| [`knapsack.rsat`](examples/knapsack.rsat) | `maximize`, sums filtered by unknowns |
| [`pythagorean.rsat`](examples/pythagorean.rsat) | `solve all`, updating variables in a solve body |
| [`sums_of_two_squares.rsat`](examples/sums_of_two_squares.rsat) | projection: auxiliary unknowns that only need to exist |
| [`factor.rsat`](examples/factor.rsat) | running a multiplier backwards; `else` when there is no solution |
| [`pell.rsat`](examples/pell.rsat) | unbounded integers and iterative deepening |
| [`palindromes.rsat`](examples/palindromes.rsat) | strings of unknown characters |
| [`bits.rsat`](examples/bits.rsat) | bitwise operators, the standard library's `bit` and `popcount` |

## Editor support

[`editors/vscode/`](editors/vscode/) is a VS Code extension. It highlights `.rsat` files and
```` ```rsat ```` blocks in Markdown, has snippets, and runs the language server (`redsat lsp`)
for live diagnostics, completion, hovers and inlay hints with each unknown's inferred domain,
go-to-definition and an outline. See its README for building and installing it.

## Repository layout

```text
crates/
  redsat-sat/       literals, CNF, DIMACS, the incremental Solver trait, CaDiCaL and the
                    built-in CDCL solver
  redsat-circuit/   And-Inverter Graphs, the Tseitin/Plaisted–Greenbaum encoder, exact
                    integer gadgets
  redsat-lang/      the language: parser, type checker, standard library (prelude.rsat),
                    interpreter, domain inference, bit-blasting, solving
  redsat-lsp/       the language server behind `redsat lsp`
  redsat-cli/       the `redsat` command-line tool and REPL
  redsat-wasm/      the compiler as WebAssembly, for the playground
playground/         the web playground (CodeMirror, Web Workers, Emscripten-built solvers)
editors/vscode/     the VS Code extension
docs/guide/         the RedSAT Guide (mdBook)
docs/design.md      the language design, semantics, encodings and architecture
examples/           example programs
bench/              benchmark programs (bench/run.sh)
```

Dependencies point strictly downwards: `cli → lsp → lang → circuit → sat`, with `wasm → lang`.

## Testing

```bash
cargo test --workspace                       # everything below
REDSAT_BLESS=1 cargo test -p redsat-lang --test programs   # re-record outputs after a reviewed change
cd playground && npm test                    # the playground's checks
```

The test suite includes:
- unit tests, plus exhaustive tests of the arithmetic circuits;
- randomized checks that the CNF encoding agrees with the circuits;
- golden tests that run every program under `crates/redsat-lang/tests/programs/` and
  `examples/`, the latter against the `// Output:` block at the bottom of each example;
- the same programs on the built-in solver;
- every code sample in the guide;
- checks that the editor grammars, the guide and the compiler agree on the language's
  vocabulary;
- an end-to-end test of the language server.
