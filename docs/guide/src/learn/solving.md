# Requirements and solving

Two statements do the real work in every RedSAT program: `require` says what must be true, and
`solve` asks for answers.

## `require`

`require` takes a condition, a `bool`, that every answer must satisfy. Several requirements
must all hold together, so these two programs mean the same:

```rsat
var x, y: int[0..=9]
require x + y == 10
require x - y == 4
solve x, y
```

```text
x = 7, y = 3
```

```rsat
var x, y: int[0..=9]
require x + y == 10 and x - y == 4
solve x, y
```

```text
x = 7, y = 3
```

Splitting requirements over several lines reads better, and it also helps when there is no
solution: RedSAT [points at the requirements](no-solution.md) that contradict each other.

## `solve`: the first answer

`solve` followed by some variables finds one answer and prints those variables:

```rsat
var x, y: int[1..=9]
require x * y == 12
solve x, y
```

```text
x = 4, y = 3
```

Which answer comes first is up to the solver. If you need a particular one, say so with more
requirements, or ask for the [best one](optimization.md).

You can list any expressions, not only variables. RedSAT then looks for values of those
expressions:

```rsat
var x, y: int[1..=9]
require x * y == 12
solve all x + y
```

```text
x + y = 7
x + y = 8
```

A bare `solve`, with nothing after it, shows every variable whose value depends on unknowns:

```rsat
var a, b: bool
var both = a and b
require a or b
require not both
solve all
```

```text
a = true, b = false, both = false
a = false, b = true, both = false
```

## `solve all`: every answer

```rsat
var x, y: int[1..=9]
require x * y == 12
solve all x, y
```

```text
x = 4, y = 3
x = 6, y = 2
x = 2, y = 6
x = 3, y = 4
```

Add `limit K` to stop after `K` answers. It also works without `all`, to ask for the first `K`:

```rsat
var x, y: int[1..=9]
require x * y == 12
solve x, y limit 2
```

```text
x = 4, y = 3
x = 6, y = 2
```

## `solve unique`: exactly one answer

`solve unique` finds an answer and then *proves* that no other one exists. That takes a second
call to the solver, and it is great for puzzles, where a well-made puzzle has exactly one
solution. If there are more, the program stops with an error that shows two of them:

```rsat,error
var a, b: int[0..=3]
require a + b == 3
solve unique a, b
```

```text
error: `solve unique` found more than one solution
 --> main.rsat:3:1
  |
3 | solve unique a, b
  | ^^^^^^^^^^^^^^^^^
  = note: one solution: a = 0, b = 3
  = note: another one:  a = 1, b = 2
```

`limit` cannot be combined with `unique`.

## Bodies: doing something with each answer

A `solve` can have a body in `{ … }`. It runs once per answer, with every unknown set to its
value, so inside it everything is known and you can print, format and compute as you like:

```rsat
var x, y: int[1..=9]
require x * y == 12 and x <= y
solve all x, y {
    print "12 = {x} × {y}"
}
```

```text
12 = 2 × 6
12 = 3 × 4
```

A body can also update ordinary variables declared outside it. This counts the answers instead
of printing them:

```rsat
var count = 0
var x, y: int[1..=9]
require x * y == 12
solve all x, y {
    count = count + 1
}
print "12 is a product of two digits in {count} ways"
```

```text
12 is a product of two digits in 4 ways
```

A body cannot add requirements (`require`) or start another `solve`: by the time it runs, the
answer has already been found.

## `else`: when there is no answer

An `else` block after a `solve` runs when there is no answer at all:

```rsat
var x: int[0..=9]
require x * x == 50
solve x {
    print "{x} squared is 50"
} else {
    print "50 is not the square of a digit"
}
```

```text
50 is not the square of a digit
```

Without `else`, RedSAT prints `no solution` and explains why: see
[When there is no solution](no-solution.md).

## Projection: only the listed variables count

The variables listed after `solve` decide what counts as a *different* answer. All other unknowns
only have to *exist*.

Which numbers from 1 to 20 are the sum of two squares, `a² + b²`?

```rsat
var total: int[1..=20]
var a, b: int[0..=5]
require a * a + b * b == total
solve all total
```

```text
total = 18
total = 20
total = 16
total = 17
total = 9
total = 10
total = 13
total = 8
total = 4
total = 5
total = 1
total = 2
```

Many totals can be made in several ways (`5 = 1² + 2² = 2² + 1²`), but since only `total` is
listed, each total is reported once. `a` and `b` are *witnesses*: they only need to exist.

## Several `solve`s: constraints accumulate

A program can have many `solve` statements. Each one sees **every** `require` made before it,
not just those since the previous `solve`:

```rsat
var x: int[0..=9]
require x > 6
solve all x
require x % 2 == 0
solve all x
```

```text
x = 8
x = 9
x = 7
x = 8
```

This lets known code between `solve`s steer the search, for example to tighten a requirement
and solve again. The flip side: if you put two unrelated puzzles in one file, the second one
also has to satisfy the first one's requirements. Keep unrelated problems in separate files.

`solve` must be a top-level statement: it cannot appear inside a function, or inside an `if`
whose condition depends on unknowns.

## Stepping through answers interactively

Run a program with `redsat --interactive` and RedSAT pauses after each answer of every `solve`:
press Enter for the next one, `a` for all the rest, or `q` to move on. It is a convenient way to
explore a problem with many answers without editing the program.

## Summary

```text
solve [all | unique] <expressions> [limit K] [minimize E | maximize E] [{ body }] [else { body }]
```

| Form                   | Finds                                                  |
|------------------------|--------------------------------------------------------|
| `solve x, y`           | the first answer                                       |
| `solve x, y limit 3`   | the first three answers                                |
| `solve all x, y`       | every answer (`limit K` stops after `K`)               |
| `solve unique x, y`    | the answer, plus a proof that it is the only one       |
| `solve x minimize e`   | an answer with the smallest `e`: see [Optimization](optimization.md) |
| `solve`                | the first answer, showing every variable that depends on unknowns |
