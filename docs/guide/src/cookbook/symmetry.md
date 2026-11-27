# Symmetry breaking

Many problems have answers that are "the same in disguise": swap two interchangeable things and
you get another answer that is not really new. Colours in a map, identical machines, teams with
no names, the rotations of a magic square.

Symmetric answers cost you twice:

- **Enumeration repeats itself.** `solve all` lists every disguise separately.
- **Proofs repeat themselves.** When there is no answer, or when proving an answer optimal, the
  solver may have to rule out each disguise of every dead end separately.

*Symmetry breaking* adds requirements that keep (at least) one representative of every group of
symmetric answers, and rule out the rest.

## Ordering interchangeable unknowns

When two unknowns play interchangeable roles, require them to be in order. This is why the
factoring example says `require x <= y`: without it, every factorization would be found twice,
as `x × y` and `y × x`.

```rsat
var x, y: int[2..=50]
require x * y == 60
var count = 0
solve all x, y {
    count = count + 1
}
print "{count} ordered pairs"

require x <= y
count = 0
solve all x, y {
    count = count + 1
}
print "{count} pairs once we require x <= y"
```

```text
10 ordered pairs
5 pairs once we require x <= y
```

## Ordering interchangeable arrays: `lex_less`

When the interchangeable things are arrays, order them *lexicographically*, like words in a
dictionary. The standard library's `lex_less(xs, ys)` holds when `xs` comes before `ys`.

**The problem.** Find four 5-bit codewords that differ pairwise in at least 3 positions, so that
any single flipped bit can be detected and corrected. (Error-correcting codes like this protect
data on disks and in transmission.)

The four codewords are interchangeable: any reordering of an answer is another answer. Requiring
`code[0] < code[1] < code[2] < code[3]` in dictionary order keeps exactly one ordering of each
set:

```rsat
var N = 4
var code: [[int[0..=1]; 5]; N]

fn distance(a: [int], b: [int]) -> int {
    return count(a[i] != b[i] for i in 0..len(a))
}
for i in 0..N {
    for j in i + 1..N {
        require distance(code[i], code[j]) >= 3
    }
}
for i in 1..N {
    require lex_less(code[i - 1], code[i])      // symmetry breaking
}

var count = 0
solve all code {
    count = count + 1
}
print "{count} codes"
```

```text
120 codes
```

Without the `lex_less` loop, the same program finds 2880 answers, 4! = 24 times as many, and
takes much longer to list them. `lex_less_eq` is the variant that allows equal arrays.

## Fixing one choice

When values, rather than variables, are interchangeable (like colours), fix one choice: give the
first region the first colour. The [graph colouring](graph-colouring.md) recipe shows this,
cutting 18 colourings down to 6. To go further, require that the second region uses one of the
first two colours, and so on.

## Rotations and reflections

The 8 magic squares from [Grid puzzles](grid-puzzles.md#magic-squares) are the rotations and
reflections of a single square. A classic way to keep one of each: require that the top-left
corner is the smallest corner, and that the top-right corner is smaller than the bottom-left
one.

```rsat
var square: [[int[1..=9]; 3]; 3]
var total = 15

require all_different([square[r][c] for r in 0..3 for c in 0..3])
for i in 0..3 {
    require sum(square[i]) == total
    require sum(square[r][i] for r in 0..3) == total
}
require sum(square[i][i] for i in 0..3) == total
require sum(square[i][2 - i] for i in 0..3) == total

// Symmetry breaking: one square per group of rotations and reflections.
var corners = [square[0][0], square[0][2], square[2][0], square[2][2]]
require all(square[0][0] <= c for c in corners)
require square[0][2] < square[2][0]

solve all square {
    for row in square {
        print row
    }
}
```

```text
[2, 9, 4]
[7, 5, 3]
[6, 1, 8]
```

Rotating moves another corner to the top left, and reflecting along the main diagonal swaps the
top-right and bottom-left corners, so each of the 8 disguises fails one of the two rules except
one.

## A word of caution

Symmetry-breaking requirements must keep at least one answer from every group, or you lose
answers. Add them only for symmetries you are sure of, and test them on a small instance by
comparing counts with and without.
