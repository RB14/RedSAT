# Performance tips

RedSAT turns your program into a SAT problem in polynomial time, but *solving* that problem can
take exponential time: the problems RedSAT is good for are typically NP-hard. In practice, SAT
solvers are remarkably fast on most real problems, and a few modelling habits make a big
difference.

## Measure first

Run with `--verbose` (or `--stats`) and look at three numbers for the slow `solve`:

```console
[redsat]   space      54 decision bits → 2^54 candidate assignments
[redsat]   circuit    5948 AND gates over 54 inputs → CNF 5832 variables (5778 auxiliary), 16880 clauses · built in 20.79ms
[redsat]   searched   1 SAT call · 1 solution · 102.73ms (CNF grew to 5832 variables, 16880 clauses)
```

- **Build time versus search time.** If *built in* is large, the problem is big: look at loops
  and calls that generate many requirements. If *searched* is large, the problem is hard for the
  solver: read on.
- **Decision bits** (`space`) are the bits the solver has to choose. Fewer is better, and each
  bit you save halves the space a naive search would face.
- **Circuit size.** Hundreds of thousands of gates are fine. Tens of millions are not.
- **SAT calls.** `solve all` makes one call per answer, `unique` two, and an optimization one per
  bit of the objective's range, roughly.

## Give unknowns tight ranges

The single most effective habit. Every unknown's range decides its number of bits, and with them
the size of every circuit it flows into. `int[1..=9]` needs 4 bits; a plain `int` that RedSAT
cannot bound starts at 33 and grows from there.

- Declare the range when you know it: a digit is `int[0..=9]`, an index is `int[0..N]`.
- Let requirements bound the rest: `x > 1` and `x * y == n` already bound `x` and `y`.
- Check the inferred ranges with `--stats` or by hovering in the [editor](../getting-started/editor.md).
  An unknown with a much wider range than you expected is a hint.

## Prefer addition to multiplication

A sum of two `n`-bit numbers costs about `n` gates; a product of two unknowns costs about `n²`,
and multiplier circuits are notoriously hard for SAT solvers. Factoring a 64-bit number is easy
to state and very hard to solve.

- Multiplying by a known number is cheap: `25 * quarters` is just a few adders.
- A product of an unknown and a 0/1 decision is better written as a condition:
  `sum(w[i] for i in 0..n if take[i])` rather than `sum(w[i] * x[i] …)`.
- Exponents and products of unknowns (`x * x * x`) multiply circuits too.

## Break symmetries

If the answers of your problem come in groups of "the same answer in disguise" (interchangeable
colours, machines, teams, items), the solver may explore each disguise separately. One or two
[symmetry-breaking](../cookbook/symmetry.md) requirements can cut the work by orders of
magnitude, especially when proving that there is no solution or that an answer is optimal.

## Choose good unknowns

The way you represent an answer matters more than any trick. In N-Queens, one column number per
row makes "one queen per row" automatic, while a yes/no per square needs it as a constraint.
Use the [modelling questions](../cookbook/modelling.md) as a checklist.

## Mind what you ask for

- `solve` (the first answer) is the cheapest. `solve unique` proves something extra, which can
  cost more than finding the answer.
- `solve all` on a problem with millions of answers will take a while; count a sample with
  `limit`, or step through them with `--interactive`.
- Optimization repeats the search many times. Tighter bounds on the objective mean fewer probes.
- Keep `while … bound K` bounds as small as your problem allows: every iteration is a copy of
  the loop body in the circuit.

## Expect heavy tails

Solving times are sensitive to small changes. Two encodings of the same problem, or the same
encoding with the factors in a different order, can differ by orders of magnitude on one
instance and swap places on the next. When comparing models, try several instances before
drawing conclusions.
