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

- ▶️ **[Open the playground](https://rb14.github.io/redsat/).** Edit and run programs in your
  browser; nothing to install. The compiler and the SAT solvers (CaDiCaL, MiniSat and RedSAT's
  own) run on your device as WebAssembly. Every example has its own address, e.g.
  [Sudoku](https://rb14.github.io/redsat/#/examples/sudoku) or
  [SEND + MORE = MONEY](https://rb14.github.io/redsat/#/examples/send-more-money).
- 📖 **[Read the RedSAT Guide](https://rb14.github.io/redsat/guide/).** It has a tutorial, a
  cookbook of modelling patterns and a complete reference. Every code sample in it is tested
  against the compiler. The [documentation index](#documentation) below links each chapter's
  source in this repository.
- 🧠 **[Read the design document](docs/design.md)** for why the language works the way it does:
  the semantics, the encodings (with measurements) and the architecture.

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
- **Two solvers, and multithreading.** CaDiCaL is the default. A pure-Rust CDCL solver
  (`--solver builtin`) needs no native code. On multi-core machines, differently configured
  solvers race on hard problems (`--threads`): factoring a 36-bit semiprime takes 0.2 s instead
  of 7.8 s.
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
redsat -j 8 bench/factor36.rsat              # let up to 8 solvers race on hard SAT calls (default: up to 4)
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
| [`factor100.rsat`](examples/factor100.rsat) | a genuinely hard instance (a 100-digit RSA-style number): run it with `-v` to see the encoding, not to wait for an answer |
| [`pell.rsat`](examples/pell.rsat) | unbounded integers and iterative deepening |
| [`palindromes.rsat`](examples/palindromes.rsat) | strings of unknown characters |
| [`bits.rsat`](examples/bits.rsat) | bitwise operators, the standard library's `bit` and `popcount` |

## Documentation

The [RedSAT Guide](https://rb14.github.io/redsat/guide/) is the place to learn the language.
Its chapters are Markdown files in [`docs/guide/src/`](docs/guide/src/), so you can also read
them right here:

| Part | Chapters |
|---|---|
| **Getting started** | [Introduction](docs/guide/src/introduction.md) · [Installation](docs/guide/src/getting-started/installation.md) · [Your first program](docs/guide/src/getting-started/first-program.md) · [Running programs](docs/guide/src/getting-started/running.md) · [The REPL](docs/guide/src/getting-started/repl.md) · [Editor support](docs/guide/src/getting-started/editor.md) |
| **Learn RedSAT** | [Knowns and unknowns](docs/guide/src/learn/knowns-and-unknowns.md) · [Requirements and solving](docs/guide/src/learn/solving.md) · [Types](docs/guide/src/learn/types.md) · [Expressions and operators](docs/guide/src/learn/expressions.md) · [Control flow over unknowns](docs/guide/src/learn/control-flow.md) · [Functions](docs/guide/src/learn/functions.md) · [Imports](docs/guide/src/learn/imports.md) · [Optimization](docs/guide/src/learn/optimization.md) · [When there is no solution](docs/guide/src/learn/no-solution.md) |
| **Cookbook** | [How to model a problem](docs/guide/src/cookbook/modelling.md) · [Logic puzzles](docs/guide/src/cookbook/logic-puzzles.md) · [Grid puzzles](docs/guide/src/cookbook/grid-puzzles.md) · [Number puzzles](docs/guide/src/cookbook/number-puzzles.md) · [Graph colouring](docs/guide/src/cookbook/graph-colouring.md) · [Knapsack and optimization](docs/guide/src/cookbook/knapsack.md) · [Scheduling](docs/guide/src/cookbook/scheduling.md) · [Counting constraints](docs/guide/src/cookbook/counting.md) · [Symmetry breaking](docs/guide/src/cookbook/symmetry.md) |
| **Reference** | [Lexical structure](docs/guide/src/reference/lexical.md) · [Statements](docs/guide/src/reference/statements.md) · [Types](docs/guide/src/reference/types.md) · [Expressions and operators](docs/guide/src/reference/expressions.md) · [Built-in functions](docs/guide/src/reference/builtins.md) · [Standard library](docs/guide/src/reference/standard-library.md) · [Command line](docs/guide/src/reference/cli.md) · [Configuration](docs/guide/src/reference/configuration.md) · [Diagnostics](docs/guide/src/reference/diagnostics.md) |
| **Under the hood** | [How RedSAT works](docs/guide/src/under-the-hood/pipeline.md) · [Performance tips](docs/guide/src/under-the-hood/performance.md) · [FAQ](docs/guide/src/faq.md) · [Keeping the docs up to date](docs/guide/src/maintaining.md) |

More documentation:
- [`docs/design.md`](docs/design.md): the design, semantics, encodings and architecture.
- [`playground/README.md`](playground/README.md): how the web playground is built and deployed.
- [`editors/vscode/README.md`](editors/vscode/README.md): the VS Code extension.

To read the guide offline: `cargo install mdbook && mdbook serve docs/guide`.

## Editor support

[`editors/vscode/`](editors/vscode/) is a VS Code extension. It highlights `.rsat` files and
```` ```rsat ```` blocks in Markdown, has snippets, and runs the language server (`redsat lsp`).
The server gives you:
- live diagnostics;
- completion of names, struct fields and enum variants;
- standard-library and built-in functions with parameter placeholders and docs;
- signature help while you type arguments;
- hovers and inlay hints with each unknown's inferred domain;
- go-to-definition and an outline.

To install it, download the `.vsix` from the [latest release](https://github.com/RB14/RedSAT/releases)
and run `code --install-extension redsat-*.vsix`, with `redsat` on your PATH. See its README to
build it yourself.

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

`scripts/ci-local.sh` runs exactly what CI runs (formatting, lints, tests, the WebAssembly build
and the playground's tests) in about 15 seconds once built. Enable the pre-push hook once per
clone, and pushes that would fail CI are stopped before they leave your machine:

```bash
git config core.hooksPath .githooks   # runs scripts/ci-local.sh before every push
```

The hook checks the working tree, so commit (or stash) your changes before pushing.

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

## License

RedSAT is dual-licensed under either of

- the [MIT License](LICENSE-MIT), or
- the [Apache License, Version 2.0](LICENSE-APACHE),

at your option. The web playground also ships builds of [CaDiCaL](https://github.com/arminbiere/cadical)
and [MiniSat](https://github.com/niklasso/minisat), both under the MIT License (their licences are
published next to the modules).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
RedSAT by you, as defined in the Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
