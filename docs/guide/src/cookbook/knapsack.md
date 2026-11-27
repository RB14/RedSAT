# Knapsack and other optimization problems

**The problem.** You are packing for a hike. Your backpack can carry at most 200 units of
weight, and each item you could take has a weight and a value (how much you want it). Which
items should you pack to get the most value without going over the limit?

The same pattern shows up everywhere: choosing projects within a budget, loading cargo, picking
ads for a page, allocating servers.

**The model.** One yes/no unknown per item: is it packed? The total weight of the packed items
is at most the capacity, and we maximize the total value of the packed items:

```rsat
var names    = ["map", "compass", "water", "sandwich", "glucose", "tin", "banana", "apple"]
var weight   = [9, 13, 153, 50, 15, 68, 27, 39]
var value    = [150, 35, 200, 160, 60, 45, 60, 40]
var capacity = 200
var n = len(names)
var take: [bool; n]

require sum(weight[i] for i in 0..n if take[i]) <= capacity

solve take maximize sum(value[i] for i in 0..n if take[i]) {
    print "take: {[names[i] for i in 0..n if take[i]]}"
    print "weight {sum(weight[i] for i in 0..n if take[i])} of {capacity}, value {sum(value[i] for i in 0..n if take[i])}"
}
```

```text
take: ["map", "compass", "sandwich", "glucose", "tin", "banana"]
weight 182 of 200, value 510
```

The filter `if take[i]` depends on unknowns, and that is fine inside `sum`: each term becomes
"`weight[i]` if item `i` is taken, otherwise 0". Inside the `solve` body, `take` is known, so the
same comprehensions there are ordinary computations, and the array comprehension
`[names[i] for … if take[i]]` is allowed.

## Adding side conditions

Real selection problems come with strings attached, and each one is a single requirement. Say
the water is not optional, a compass is useless without the map, and the tin and the banana
don't fit in the side pocket together:

```rsat
var names    = ["map", "compass", "water", "sandwich", "glucose", "tin", "banana", "apple"]
var weight   = [9, 13, 153, 50, 15, 68, 27, 39]
var value    = [150, 35, 200, 160, 60, 45, 60, 40]
var capacity = 200
var n = len(names)
var take: [bool; n]

require sum(weight[i] for i in 0..n if take[i]) <= capacity
require take[2]                          // the water is not optional
require take[1] implies take[0]          // a compass is useless without the map
require not (take[5] and take[6])        // not both the tin and the banana

solve take maximize sum(value[i] for i in 0..n if take[i]) {
    print "take: {[names[i] for i in 0..n if take[i]]}"
    print "weight {sum(weight[i] for i in 0..n if take[i])} of {capacity}, value {sum(value[i] for i in 0..n if take[i])}"
}
```

```text
take: ["map", "compass", "water", "glucose"]
weight 190 of 200, value 445
```

A few more conditions you will often need, all one-liners:

| Condition                                  | Requirement                                     |
|--------------------------------------------|-------------------------------------------------|
| take at most 3 items                       | `require at_most(3, take)`                      |
| take exactly one of items 4, 5 and 6       | `require exactly(1, [take[4], take[5], take[6]])` |
| item 3 needs item 7                        | `require take[3] implies take[7]`               |
| items 1 and 2 go together or not at all    | `require take[1] iff take[2]`                   |

## Making change

The [Optimization](../learn/optimization.md) chapter's coin example is the same idea with
numbers instead of yes/no choices: how many of each coin, minimizing the total count.
