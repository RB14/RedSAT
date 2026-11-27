# FAQ

## Why SAT?

Because SAT solvers are some of the most heavily optimised search engines in existence. Decades
of research and yearly competitions have made conflict-driven clause learning solvers astonishingly
good at the problems people actually meet, from hardware verification to scheduling. SAT is
also a universal target: every problem in NP can be reduced to it in polynomial time.

RedSAT's job is to do that reduction for you, from a program that reads like ordinary code, and
to do it well: exact arithmetic, tight bit widths, compact encodings. The solver does the
searching.

## Is every program fast, then?

No. The problems RedSAT is good for are mostly *NP-hard*, and no known algorithm solves every
instance quickly. The reduction to SAT always takes polynomial time, but the solver may need
exponential time on hard instances. Factoring products of two large primes is an example that is
easy to write and very hard to solve.

In practice, many real problems solve in milliseconds, and the
[Performance tips](under-the-hood/performance.md) help with the rest.

## Can integers overflow?

Never. Integers are mathematical integers, and every intermediate result gets as many bits as
its value can need. A type like `uint<8>` limits what a *variable* may hold, not the arithmetic:
`a + b` may be 510 even if `a` and `b` are `uint<8>`. If you want arithmetic that wraps around
like a CPU's, write it: `(a + b) % 256`.

## What if RedSAT cannot bound an integer?

Then it warns you, and searches with growing bounds: first 32-bit magnitudes, then 64, and so on
up to 256. Any answer it finds is genuine, but "no solution" then only means "none within the
bounds searched". Give the unknown a range to make the search complete, or turn the warning into
an error with `unbounded-ints = "error"`. See [Unbounded integers](learn/no-solution.md#unbounded-integers).

## Does RedSAT have macros?

No, and none are planned, because functions already do that job. Every function call is
**inlined** at compile time: the function's body is run right where it is called, with the
arguments in place of the parameters. On known arguments that is compile-time computation, much
like a macro or C++'s `constexpr`, and loops in functions unroll into as many requirements as
you need. Functions are also *hygienic*: they only see their own parameters and variables, so
they cannot capture or clash with the caller's names.

And unlike macros, the same function works on unknowns too, where its body becomes part of the
circuit. See [Functions](learn/functions.md).

## Is the answer always right?

Every answer is checked against all requirements, using the language's own rules for known
values, before it is shown. If the solver ever produced a wrong answer (or RedSAT built a wrong
circuit), you would get an internal error instead of a wrong answer.

"No solution" is a proof, as long as every unknown had a finite range. With unbounded integers it
only covers the bounds that were searched, and RedSAT says so.

## Can I use floating-point numbers?

Not yet. RedSAT has exact integers, booleans, characters, strings, arrays, enums and structs.
Fixed-point arithmetic is easy to write with integers (count in cents, or in thousandths).

## How is RedSAT different from MiniZinc, Z3 or Prolog?

They are close relatives. MiniZinc is a modelling language for constraint solvers; Z3 is an SMT
solver with a rich API; Prolog searches by backtracking. RedSAT's angle is to look and feel like
an ordinary imperative language, with `var`, `if`, loops, functions, mutation and early
returns that all work on unknowns, and to compile everything down to plain SAT, with exact
integers whose sizes are inferred.

## Which SAT solver does RedSAT use?

[CaDiCaL](https://github.com/arminbiere/cadical) by default, and its own CDCL solver, written in
Rust, with `--solver builtin`. `redsat dimacs` runs either solver on a raw CNF file.

## Where do I report a bug or suggest a feature?

In the RedSAT repository. Please include the program and the output, ideally cut down to the
smallest program that still shows the problem.
