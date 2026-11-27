# Command line

```text
redsat <FILE> [OPTIONS]              run a program (the default action)
redsat check <FILE> [OPTIONS]        parse and type-check a program, without running it
redsat repl [OPTIONS]                build a program interactively
redsat dimacs <PATH> [--solver S]    solve a CNF formula in DIMACS format
redsat lsp                           run the language server, for editors
redsat help [COMMAND]                show help
```

`redsat --help` (or `-h`) and `redsat <command> --help` describe every option; `redsat --version`
(or `-V`) prints the version.

## Running a program

```console
$ redsat program.rsat [OPTIONS]
```

The program's output (`print` and solutions) goes to standard output. Diagnostics (errors,
warnings and notes) go to standard error, after the program's output, followed by the
statistics if `--stats` was given. The exit status is 0 when the program ran without errors,
and 1 otherwise. A `solve` without a solution is not an error.

| Option                       | Effect                                                                 |
|------------------------------|------------------------------------------------------------------------|
| `--stats`                    | After the run, print statistics for every `solve` to standard error (see below). |
| `-v`, `--verbose`            | Report progress to standard error as it happens (see below).           |
| `--interactive`              | Pause after each solution of every `solve`: Enter shows the next one, `a` (or `all`) all the remaining ones, and `q` (or `quit`) stops enumerating that `solve`. End of input counts as `q`. |
| `--solver <cadical\|builtin>` | The SAT solver: `cadical` (the default) or `builtin`, RedSAT's own CDCL solver written in Rust. Both give correct answers; `builtin` is slower. |
| `-j, --threads <N>` | How many threads may work on a hard SAT problem (default: the number of cores, at most 4). Differently configured solvers race and the first answer wins; easy problems are solved on one thread. With several threads, which of several valid solutions is found can vary from run to run: use `-j 1` for reproducible output. |

It also accepts the compiler options below.

## Compiler options

These override the settings of a [`redsat.toml`](configuration.md) file. They are accepted when
running a program, and by `check` and `repl`.

| Option                                 | Effect                                                         |
|----------------------------------------|----------------------------------------------------------------|
| `--strict`                             | Enable every strict check: `explicit-types = "unknowns"` and `unbounded-ints = "error"`, unless set otherwise. |
| `--explicit-types <off\|unknowns\|all>` | Which declarations need a type annotation.                     |
| `--unbounded-ints <deepen\|error>`      | What to do with integers whose range cannot be inferred.       |
| `--fallback-int-bits <BITS>`           | The first magnitude, in bits, tried for unbounded integers (default 32). |
| `--max-int-bits <BITS>`                | Stop widening unbounded integers beyond this many bits (default 256). |
| `--config <FILE>`                      | Use this configuration file instead of searching for `redsat.toml`. |

## `redsat check`

```console
$ redsat check program.rsat
program.rsat: ok
```

Parses and type-checks the program and the files it imports, applying the compiler options,
without running it. Prints `<file>: ok` and exits with 0 when there are no errors; otherwise
prints the errors and exits with 1. Errors that only arise while the program runs (an index out
of range, a loop bound that depends on unknowns, …) are not detected.

## `redsat repl`

Starts an interactive session; see [The REPL](../getting-started/repl.md). Accepts the compiler
options and `--solver`. It looks for a `redsat.toml` in the current directory and its parents.

## `redsat dimacs`

```console
$ cat problem.cnf
p cnf 3 2
1 -3 0
2 3 -1 0
$ redsat dimacs problem.cnf
s SATISFIABLE
v -1 -2 -3 0
```

Solves a raw CNF formula in DIMACS format, the standard input format of SAT solvers: a
`p cnf <variables> <clauses>` header, then one clause per line as a list of literals ending in
`0` (`-3` is "not variable 3"). It prints the answer in SAT Competition format: an `s` line with
`SATISFIABLE` or
`UNSATISFIABLE`, and for a satisfiable formula `v` lines with the value of every variable
(positive for true, negative for false), ending with `0`. Use `-` as the path to read standard
input. The exit status follows the SAT solver convention: **10** for satisfiable, **20** for
unsatisfiable. `--solver` picks the solver, as when running programs.

## `redsat lsp`

Runs the language server over standard input and output, as editors expect. See
[Editor support](../getting-started/editor.md).

## Reading `--stats`

```console
$ redsat examples/factor.rsat --stats
4295229443 = 65537 × 65539
stats: solve at examples/factor.rsat:14
  x: [34, 130158467] · 27 bits
  y: [34, 130158467] · 27 bits
  circuit: 5948 AND gates · CNF: 5832 variables, 16880 clauses
  1 SAT call · 1 solution · 78.67ms (build 18.12ms, search 60.39ms)
```

For each `solve`, in order:

- **the unknowns** the `solve` depends on, each with the range RedSAT inferred for it and the
  number of bits that range needs;
- the **circuit** size in AND gates, and the size of the **CNF** formula given to the solver;
- the number of **SAT calls** (one per answer, plus one to prove there are no more, plus the
  probes of an optimization), the number of **solutions** reported, and the time taken, split
  into *build* (ranges, circuit, CNF) and *search* (SAT solving and decoding).

When unbounded integers were involved, a line says which magnitude was searched.

## Reading `--verbose`

`--verbose` prints the same kind of information, and more, *while* the program runs, each line
starting with `[redsat]`. Here is a Sudoku:

```console
$ redsat examples/sudoku.rsat --verbose
[redsat] loaded 2 files (examples/sudoku.rsat, <prelude>) · parsed and checked in 578.61µs
[redsat] solve #1 at examples/sudoku.rsat:27
[redsat]   model      unique · 78 requirements · 242 IR terms
[redsat]   unknowns   51 (51 integer, 0 boolean), 0 fixed by domain inference · domains in 28.54µs
[redsat]              ?8:12: [1, 9] · 4 bits
[redsat]              ?8:16: [1, 9] · 4 bits
[redsat]              …
[redsat]              … and 43 more
[redsat]   space      204 decision bits → 2^204 candidate assignments
[redsat]   circuit    6553 AND gates over 204 inputs → CNF 4062 variables (3858 auxiliary), 6210 clauses · built in 2.17ms
[redsat]   search     with CaDiCaL…
[redsat]   searched   2 SAT calls · 1 solution · 11.25ms (CNF grew to 4062 variables, 6211 clauses)
[redsat] finished in 14.34ms: 94 statements executed, no calls inlined, 1 solve taking 13.50ms
```

| Line        | Says                                                                                       |
|-------------|--------------------------------------------------------------------------------------------|
| `loaded`    | which files make up the program (yours, the standard library `<prelude>`, and imports), and how long parsing and type checking took |
| `solve #N`  | a `solve` is starting, and where it is                                                     |
| `model`     | its mode (`first`, `all`, `unique`, with `limit` and `minimize`/`maximize` if given), how many requirements it sees, and how many terms the program's constraints add up to |
| `unknowns`  | how many unknowns it involves, how many domain inference already pinned to a single value, and how long inference took; then up to eight of them with their ranges and widths (unnamed `?` holes are shown by their line and column) |
| `space`     | the total number of bits the solver must decide, and so the number of candidate assignments. A brute-force search would try them all; the SAT solver typically explores a tiny fraction |
| `circuit`   | the size of the Boolean circuit and of the CNF formula built from it (the *auxiliary* variables stand for the circuit's gates), and how long building them took |
| `search`    | which SAT solver is running                                                                |
| `searched`  | how many SAT calls and solutions there were, how long the search took, and how big the formula grew (enumeration and optimization add clauses as they go) |
| `deepening` | no solution was found within the current bounds of unbounded integers, so RedSAT retries with twice as many bits |
| `domains`   | `contradictory … no solution`: domain inference alone proved there is no solution, without calling the solver |
| `finished`  | the totals for the run: statements executed (counting each loop iteration and inlined call again), function calls inlined and how deeply they nested, and the time spent in `solve`s |

Program output appears between these lines as it is printed. See
[Performance tips](../under-the-hood/performance.md) for how to act on what `--verbose` shows.

## The built-in solver

`--solver builtin` uses RedSAT's own SAT solver instead of CaDiCaL. It is a conflict-driven
clause-learning (CDCL) solver written in pure Rust, with the same interface, so every feature
works with it. It is much slower on hard problems, but needs no native code, so it runs
anywhere Rust does, including in a web browser.
