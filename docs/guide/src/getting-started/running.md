# Running programs

RedSAT programs live in `.rsat` files. This page covers the commands you will use every day;
the [command-line reference](../reference/cli.md) lists every option.

## Run a program

Running is the default action, so there is no `run` subcommand:

```console
$ redsat examples/pythagorean.rsat
```

What the program prints goes to standard output. Errors, warnings and notes go to standard
error, after the program's output. `redsat` exits with status 0 when the program ran without
errors and 1 otherwise. A `solve` that finds no solution is not an error: the program simply
prints `no solution` (or runs its `else` block) and carries on.

## Check a program without running it

```console
$ redsat check examples/coins.rsat
examples/coins.rsat: ok
```

`check` parses and type-checks the program, and stops there. It is quick, so it suits editors
and CI. Some mistakes only show up when the program runs, such as an array index that is out of
range: `check` does not find those, but a run (and the [editor](editor.md)) does.

## See what the solver worked with: `--stats`

```console
$ redsat examples/factor.rsat --stats
4295229443 = 65537 × 65539
stats: solve at examples/factor.rsat:14
  x: [34, 130158467] · 27 bits
  y: [34, 130158467] · 27 bits
  circuit: 5948 AND gates · CNF: 5832 variables, 16880 clauses
  1 SAT call · 1 solution · 78.67ms (build 18.12ms, search 60.39ms)
```

For every `solve`, `--stats` shows each unknown's **inferred range** and how many bits it
needed, the size of the circuit and of the SAT problem, how many times the solver was called,
and where the time went: *build* (inferring ranges, building the circuit) versus *search* (the
SAT solver).

## Watch it happen: `--verbose`

`-v` / `--verbose` reports progress as it happens: which files were loaded, what each `solve`
is about to do, how large the search space is, and how long each step took.

```console
$ redsat examples/coins.rsat --verbose
[redsat] loaded 2 files (examples/coins.rsat, <prelude>) · parsed and checked in 1.48ms
[redsat] solve #1 at examples/coins.rsat:12
[redsat]   model      first, minimize · 2 requirements · 28 IR terms
[redsat]   unknowns   4 (4 integer, 0 boolean), 0 fixed by domain inference · domains in 42.48µs
[redsat]              quarters: [0, 11] · 4 bits
[redsat]              dimes: [0, 28] · 5 bits
[redsat]              nickels: [0, 57] · 6 bits
[redsat]              pennies: [0, 289] · 9 bits
[redsat]   space      24 decision bits → 2^24 candidate assignments
[redsat]   circuit    414 AND gates over 24 inputs → CNF 291 variables (267 auxiliary), 788 clauses · built in 4.33ms
[redsat]   search     with CaDiCaL…
289¢ = 11 quarters + 1 dimes + 0 nickels + 4 pennies (16 coins)
[redsat]   searched   6 SAT calls · 1 solution · 13.31ms (CNF grew to 460 variables, 1305 clauses)
[redsat] finished in 19.55ms: 43 statements executed, no calls inlined, 1 solve taking 17.78ms
```

Reach for it when a program is slow: it tells you whether the time goes into building the
problem or into solving it, and how big the problem is. Each `solve` also gets a `cost` table
that breaks the size down by source line, like a database's `EXPLAIN ANALYZE`, so you can see
which statements make the problem big. The
[command-line reference](../reference/cli.md#reading---verbose) explains every line.

## Step through solutions: `--interactive`

```console
$ redsat examples/pythagorean.rsat --interactive
18² + 24² = 30²
── 1 shown · [Enter] next · [a] all · [q] quit ──
```

After each solution, RedSAT waits: press **Enter** for the next one, **a** to show all the
remaining ones, or **q** to stop enumerating and let the program continue. This works for every
`solve`, even one that would normally stop after the first solution.

## Choose the SAT solver: `--solver`

RedSAT uses [CaDiCaL](https://github.com/arminbiere/cadical), a fast, award-winning SAT solver,
by default. `--solver builtin` switches to RedSAT's own solver, written in pure Rust. It gives
the same answers, more slowly, and is mostly of interest for learning and testing.

## Compiler options

Options such as `--strict` change what the compiler accepts. They can also live in a
`redsat.toml` file next to your programs: see [Configuration](../reference/configuration.md).
