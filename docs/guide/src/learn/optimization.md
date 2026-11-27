# Optimization

Often any answer will not do: you want the *best* one. Add `minimize` or `maximize` to a `solve`,
followed by the expression to optimize.

How do you make 289 cents with as few coins as possible?

```rsat
var amount = 289
var quarters, dimes, nickels, pennies

require quarters >= 0 and dimes >= 0 and nickels >= 0 and pennies >= 0
require 25 * quarters + 10 * dimes + 5 * nickels + pennies == amount

var coins = quarters + dimes + nickels + pennies
solve quarters, dimes, nickels, pennies minimize coins {
    print "{coins} coins: {quarters} quarters, {dimes} dimes, {nickels} nickels, {pennies} pennies"
}
```

```text
16 coins: 11 quarters, 1 dimes, 0 nickels, 4 pennies
```

The objective can be any integer expression: a variable, a sum, a product, the `max` of several
values (as in [scheduling](../cookbook/scheduling.md), where the latest finishing time is
minimized), or a `sum` filtered by unknowns (as in the [knapsack](../cookbook/knapsack.md)).

## Every optimal answer

There may be several answers that are equally good. `solve all … minimize` (or `maximize`)
lists **all** of them. Which rectangles with a perimeter of 18 have the largest area?

```rsat
var w, h: int[1..=20]
require 2 * (w + h) == 18
solve all w, h maximize w * h {
    print "{w} × {h}: area {w * h}"
}
```

```text
4 × 5: area 20
5 × 4: area 20
```

`limit K` caps the number of optimal answers listed, as usual.

## Several goals at once

To optimize one thing first and use another to break ties, weight them so that the first always
dominates. If `x` matters more than `y` and `y` never exceeds 10, minimize `100 * x + y`:

```rsat
var x, y: int[0..=10]
require x + y >= 7
solve x, y minimize 100 * x + y
```

```text
x = 0, y = 7
```

## How it works

RedSAT finds the optimum by **binary search** over the objective's value. It first finds any
answer, then repeatedly asks the solver "is there an answer whose objective is at most `m`?",
halving the gap between the best answer so far and the best value still possible. The range it
searches comes from the same [domain inference](../under-the-hood/pipeline.md) that sizes every
unknown.

So the number of SAT calls grows with the number of *bits* of the objective, not its size: an
objective that can range over a trillion values needs only about 40 calls. Each probe is a
temporary assumption, so failed probes leave nothing behind.

Run with `--verbose` to see how many SAT calls an optimization took.

Two things follow from this:

- **Tight bounds help.** The smaller the objective's possible range, the fewer probes it needs,
  and the smaller the circuit behind it.
- **A known objective does nothing.** `solve x minimize 5` simply finds the first answer.
