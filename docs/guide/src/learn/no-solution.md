# When there is no solution

Sometimes the requirements contradict each other. Maybe the puzzle really has no answer, or
maybe the model has a bug. Either way, RedSAT tries to tell you *why*.

## Explanations

When a `solve` without an `else` block finds no solution, RedSAT prints `no solution` and then
explains which requirements cannot hold together:

```rsat
var x, y: int[0..=10]
var z: int[0..=5]
require x + y == 12
require z > 3
require x > 9
require y > 5
solve x, y
```

```text
no solution
note: no solution: these requirements cannot all hold together
 --> main.rsat:3:9
  |
3 | require x + y == 12
  |         ^^^^^^^^^^^
 ::: main.rsat:5:9
  |
5 | require x > 9
  |         ^^^^^
 ::: main.rsat:6:9
  |
6 | require y > 5
  |         ^^^^^
```

The explanation is **minimal**: every requirement it lists is needed for the contradiction, and
the ones that have nothing to do with it, like `z > 3`, are left out. Drop or relax any one of
the listed requirements and the contradiction goes away (though another one may remain).

Requirements created inside loops or functions are reported at the line that wrote them. Three
pigeons cannot sit in two holes without sharing:

```rsat
var hole: [int[0..=3]; 3]
require all_different(hole)
for i in 0..3 {
    require hole[i] < 2
}
solve hole
```

```text
no solution
note: no solution: these requirements cannot all hold together
 --> main.rsat:2:9
  |
2 | require all_different(hole)
  |         ^^^^^^^^^^^^^^^^^^^
 ::: main.rsat:4:13
  |
4 |     require hole[i] < 2
  |             ^^^^^^^^^^^
```

When a single requirement is impossible on its own, RedSAT says so:

```rsat
var x: int[0..=9]
require x * x == 50
solve x
```

```text
no solution
note: no solution: this requirement can never hold
 --> main.rsat:2:9
  |
2 | require x * x == 50
  |         ^^^^^^^^^^^
```

A requirement that is false before any solving starts gets a warning of its own:

```rsat
require 1 + 1 == 3
solve
```

```text
no solution
warning: this requirement is always false, so the problem has no solution
 --> main.rsat:1:9
  |
1 | require 1 + 1 == 3
  |         ^^^^^^^^^^
note: no solution: this requirement can never hold
 --> main.rsat:1:9
  |
1 | require 1 + 1 == 3
  |         ^^^^^^^^^^
```

### How explanations are found

Each source line that adds requirements is switched on by its own "selector". The solver is
asked to solve with all selectors on, and when it fails it reports which selectors it needed for
its proof. RedSAT then tries dropping them one by one, keeping only those the contradiction
cannot do without.

This means solving the problem a few more times. RedSAT skips the explanation when the original
proof took more than 10 seconds, and stops minimising after 5 seconds, so a hard problem never
gets much slower because of it.

Explanations use the ranges you *declared* for your unknowns, not the narrower ones RedSAT
inferred from the requirements, since those already contain the requirements' consequences.

## Handling "no solution" yourself: `else`

If "no solution" is an expected outcome, handle it with `else`. The explanation is then not
printed:

```rsat
var n = 97
var x, y: int[2..=n]
require x * y == n
solve x, y {
    print "{n} = {x} × {y}"
} else {
    print "{n} is prime"
}
```

```text
97 is prime
```

## Unbounded integers

RedSAT needs a finite range for every integer to build its circuit. Usually it infers one from
your requirements, but not always: nobody can, in general. (Deciding whether an arbitrary
polynomial equation has an integer solution is *undecidable*, as Matiyasevich proved in 1970.)

When an integer is left unbounded, RedSAT warns you and searches with growing bounds: first
`|x| < 2^32`, then `2^64`, and so on, doubling the number of bits up to `2^256`. Any solution it
finds is a genuine one, because the arithmetic is exact:

```rsat
var x, y: int
require y > 0
require x * x - 2 * y * y == 1
solve all x, y limit 2
```

```text
x = 3, y = 2
x = 17, y = 12
warning: could not infer bounds for `x`, `y`; searched |value| < 2^32
 --> main.rsat:1:5
  |
1 | var x, y: int
  |     ^
  = help: add bounds (e.g. `int[0..=1000]`) for a complete search
```

But "no solution" is then only "no solution *within the bounds searched*", and RedSAT says so.
There is no explanation in that case, because there is nothing to explain yet: a larger answer
might exist. (This program limits the search to 64 bits with the `max-int-bits` option.)

```rsat,max-int-bits=64
var x, y
require y > 0
require x * x == 2 * y * y
solve x, y
```

```text
no solution with |x|, |y| < 2^64
warning: could not infer bounds for `x`, `y`; searched |value| < 2^64
 --> main.rsat:1:5
  |
1 | var x, y
  |     ^
  = help: add bounds (e.g. `int[0..=1000]`) for a complete search
```

The cure is to give such unknowns bounds. If you would rather be forced to, the
`unbounded-ints = "error"` [compiler option](../reference/configuration.md) (part of strict
mode) turns the warning into an error:

```rsat,error,unbounded-ints=error
var x, y: int
require y > 0
require x * x - 2 * y * y == 1
solve all x, y limit 2
```

```text
error: cannot infer a finite domain for `x`
 --> main.rsat:1:5
  |
1 | var x, y: int
  |     ^ unbounded
  = note: required by the compiler option unbounded-ints = "error"
  = help: give it bounds, e.g. `var x: int[0..=1000]`, or a constraint that limits it
```

## Debugging a model with no solution

When the explanation points at requirements that *should* be compatible, the bug is usually in
one of them. Some techniques that help:

- **Solve with fewer requirements.** Comment out the requirements the explanation names, one at
  a time, and look at the answers you get: which one is wrong?
- **Check your ranges.** `int[0..9]` stops at 8. Hovering over an unknown in the
  [editor](../getting-started/editor.md), or running with `--stats`, shows the ranges RedSAT
  inferred.
- **Remember that constraints accumulate.** Every `solve` sees all earlier requirements,
  including those written for an earlier `solve` in the same file.
- **Test pieces on known values.** Functions work on known values too, so `print` a helper's
  result for a case you can check by hand.
