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
[redsat] loaded 2 files (examples/sudoku.rsat, <prelude>) · parsed and checked in 513.12µs
[redsat] solve #1 at examples/sudoku.rsat:27
[redsat]   model      unique · 78 requirements · 242 IR terms
[redsat]   unknowns   51 (51 integer, 0 boolean), 0 fixed by domain inference · domains in 34.40µs
[redsat]              ?8:12: [1, 9] · 4 bits
[redsat]              ?8:16: [1, 9] · 4 bits
[redsat]              …
[redsat]              … and 43 more
[redsat]   space      204 decision bits → 2^204 candidate assignments
[redsat]   circuit    6502 AND gates over 204 inputs → CNF 4011 variables (3807 auxiliary), 6057 clauses · built in 3.09ms
[redsat]   cost       by source line (shared circuitry counts at the first line needing it)
[redsat]                line  unknowns   times     inputs   AND gates     clauses  share  source
[redsat]                   8         6                 24          30          24     0%  [5, 3, ?, ?, 7, ?, ?, ?, ?],
[redsat]                   …
[redsat]                  16         6                 24          30          24     0%  [?, ?, ?, ?, 8, ?, ?, 7, 9],
[redsat]                  20                ×9          0       2,146       1,984    33%  require all_different(grid[i]) // row i
[redsat]                  21                ×9          0       2,210       2,101    35%  require all_different([grid[r][i] for r in 0..9]) // column…
[redsat]                  24                ×9          0       1,840       1,768    29%  require all_different([grid[top + r][left + c] for r in 0..…
[redsat]   search     with portfolio of CaDiCaL ×4…
[redsat]   searched   2 SAT calls · 1 solution · 21.26ms (CNF grew to 4011 variables, 6058 clauses)
[redsat] finished in 25.59ms: 94 statements executed, no calls inlined, 1 solve taking 24.54ms
```

| Line        | Says                                                                                       |
|-------------|--------------------------------------------------------------------------------------------|
| `loaded`    | which files make up the program (yours, the standard library `<prelude>`, and imports), and how long parsing and type checking took |
| `solve #N`  | a `solve` is starting, and where it is                                                     |
| `model`     | its mode (`first`, `all`, `unique`, with `limit` and `minimize`/`maximize` if given), how many requirements it sees, and how many terms the program's constraints add up to |
| `unknowns`  | how many unknowns it involves, how many domain inference already pinned to a single value, and how long inference took; then up to eight of them with their ranges and widths (unnamed `?` holes are shown by their line and column) |
| `space`     | the total number of bits the solver must decide, and so the number of candidate assignments. A brute-force search would try them all; the SAT solver typically explores a tiny fraction |
| `circuit`   | the size of the Boolean circuit and of the CNF formula built from it (the *auxiliary* variables stand for the circuit's gates), and how long building them took |
| `cost`      | where that size comes from, line by line: see [below](#where-the-cost-comes-from) |
| `search`    | which SAT solver is running                                                                |
| `searched`  | how many SAT calls and solutions there were, how long the search took, and how big the formula grew (enumeration and optimization add clauses as they go) |
| `deepening` | no solution was found within the current bounds of unbounded integers, so RedSAT retries with twice as many bits |
| `domains`   | `contradictory … no solution`: domain inference alone proved there is no solution, without calling the solver |
| `finished`  | the totals for the run: statements executed (counting each loop iteration and inlined call again), function calls inlined and how deeply they nested, and the time spent in `solve`s |

Program output appears between these lines as it is printed. See
[Performance tips](../under-the-hood/performance.md) for how to act on what `--verbose` shows.

### Where the cost comes from

Like a database's `EXPLAIN ANALYZE`, the `cost` table breaks the problem down by source line,
so you can see which statements make it big. Here are the 100-digit factoring example's
lines:

```console
$ redsat examples/factor100.rsat --verbose
…
[redsat]   circuit    291413 AND gates over 3758 inputs → CNF 280006 variables (279672 auxiliary), 824713 clauses · built in 357.16ms
[redsat]   cost       by source line (shared circuitry counts at the first line needing it)
[redsat]                line  unknowns   times     inputs   AND gates     clauses  share  source
[redsat]                  16         2                334         657         972     0%  var x, y
[redsat]                  19                            0     221,601     662,644    80%  require x * y == n
[redsat]                  22                            0           0           2     0%  require is_odd(x) and is_odd(y)
[redsat]                  26                            0           0           0   free  require x > 1 and x <= root and y > root
[redsat]                  29                            0           0           0   free  require x <= y
[redsat]                  36               ×10      3,424      61,766     161,095    20%  require x % m * (y % m) % m == n % m
[redsat]                  42                            0           0           0   free  require x >= 10 ^ 49
[redsat]              free: folded into the inferred ranges (it may narrow the unknowns), so it needs no circuit
```

| Column      | Says                                                                                       |
|-------------|--------------------------------------------------------------------------------------------|
| `unknowns`  | how many unknowns the line declares (each `?` hole is one)                                  |
| `times`     | how many requirements the line made, when more than one (a `require` in a loop makes one per iteration) |
| `inputs`    | the bits the solver must decide that the line introduced: an unknown's bits, or auxiliary ones, such as the quotient behind each `x % m` |
| `AND gates` | the circuit the line added                                                                 |
| `clauses`   | the CNF clauses the line added, and their `share` of the total                             |

Some rules for reading it:

- Circuitry that several lines share counts once, at the first line that needs it, in program
  order. A later line that only reuses it shows little or nothing.
- A requirement marked **free** costs nothing because domain inference absorbed it. That does
  not make it useless: it may have narrowed the unknowns, and so shrunk every other line. Above,
  `x >= 10 ^ 49` is free, yet it is the reason `y` needs 168 bits instead of 326.
- What happens inside a standard-library function is charged to the line that calls it. What
  happens inside your own functions is charged to their lines.
- Long programs list their 24 costliest lines, and sum up the rest in one row.

## The built-in solver

`--solver builtin` uses RedSAT's own SAT solver instead of CaDiCaL. It is a conflict-driven
clause-learning (CDCL) solver written in pure Rust, with the same interface, so every feature
works with it. It is much slower on hard problems, but needs no native code, so it runs
anywhere Rust does, including in a web browser.
