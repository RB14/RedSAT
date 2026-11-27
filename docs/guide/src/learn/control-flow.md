# Control flow over unknowns

RedSAT has the control flow you know: `if`, `for` and `while`. On known values they behave
exactly as in any language. The interesting part is what happens when a condition depends on
unknowns, because then RedSAT cannot know which way the program goes. It handles *every* way,
and lets the solver decide.

## `if`

When the condition is known, `if` is ordinary: only the branch taken is run. When the condition
depends on unknowns, **both branches are compiled**, and every variable assigned in them gets
the value "this if the condition holds, otherwise that".

Two numbers between 0 and 100 add up to 50, and the bigger one is 42. Which are they?

```rsat
var a, b: int[0..=100]

var biggest = a
if b > a {
    biggest = b         // biggest becomes: b if b > a, otherwise a
}

require biggest == 42
require a + b == 50

solve all a, b
```

```text
a = 8, b = 42
a = 42, b = 8
```

This is ordinary imperative code, the kind you would write for known numbers, and it works on
unknowns unchanged. `else if` chains work too. Here is a tax table with three brackets; which
income pays exactly 9000 in tax?

```rsat
var income: int[0..=10000]
var tax = 0
if income < 1000 {
    tax = income
} else if income <= 5000 {
    tax = 2 * income
} else {
    tax = 2 * income + 500
}
require tax == 9000
solve all income {
    print "an income of {income} pays {tax}"
}
```

```text
an income of 4500 pays 9000
```

### `require` inside an `if`

A `require` inside a branch only applies when that branch is taken. Under a condition `c`,
`require p` means "`c` implies `p`":

```rsat
var big: bool
var x: int[0..=9]
if big {
    require x > 7
} else {
    require x < 2
}
solve all big, x
```

```text
big = false, x = 0
big = false, x = 1
big = true, x = 8
big = true, x = 9
```

### Printing needs a known path

Under a condition that depends on unknowns, RedSAT cannot know whether the program would get
there, so it cannot print:

```rsat,error
var x: int[0..=9]
if x > 5 {
    print "big"
}
```

```text
error: cannot print under a condition that depends on unknowns
 --> main.rsat:3:5
  |
3 |     print "big"
  |     ^^^^^^^^^^^
  = help: print inside a `solve` body instead
```

## `for`

`for` loops over a range, an array or a string:

```rsat
for i in 0..3 {
    print "i = {i}"
}
for word in ["red", "sat"] {
    print word
}
for c in "hey" {
    print c
}
```

```text
i = 0
i = 1
i = 2
red
sat
h
e
y
```

The range of a `for` must be known: RedSAT unrolls the loop, running the body once for each
value. The loop variable is known in each copy of the body, which makes `for` the main tool for
generating requirements. This checks every pair of queens on an `N`×`N` board, however big `N`
is:

```rsat
var N = 8
var queen: [int[0..N]; N]       // queen[r]: the column of the queen in row r
for r1 in 0..N {
    for r2 in r1 + 1..N {
        require abs(queen[r1] - queen[r2]) != r2 - r1
    }
}
```

(The whole program is in the [cookbook](../cookbook/grid-puzzles.md#n-queens).)

Looping over a string of unknown length runs the body for every position up to the string's
maximum length, each one guarded by "this position is within the string". So this just works:

```rsat
var word: string[1..=3]
var vowels = 0
for c in word {
    if c in "aeiou" {
        vowels = vowels + 1
    }
}
require all(c in 'a'..='e' for c in word)
require vowels == 2 and word[0] == 'b'
solve all word
```

```text
word = "baa"
word = "bae"
word = "bee"
word = "bea"
```

The loop variable cannot be assigned to.

## `while`

A `while` loop whose condition is known runs as usual. How many steps does 27 take to reach 1 in
the [Collatz sequence](https://en.wikipedia.org/wiki/Collatz_conjecture)?

```rsat
var n = 27
var steps = 0
while n != 1 {
    n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 }
    steps = steps + 1
}
print "27 reaches 1 after {steps} steps"
```

```text
27 reaches 1 after 111 steps
```

When the condition depends on unknowns, RedSAT cannot know how often the loop runs, so you must
give it a **bound**, the most iterations it may take:

```rsat
var x: int[0..=100]
var n = x
while n >= 10 bound 3 {
    n = n - 10
}
require n == 5
solve all x
```

```text
x = 5
x = 15
x = 35
x = 25
```

`while cond bound K` unrolls the loop `K` times, each iteration guarded by the condition, and
then **requires that the loop has finished**. That is why `x = 45` is not an answer: it would
need four iterations. Pick a bound that is large enough for every answer you want, but no
larger, since each iteration adds to the size of the problem.

Forgetting the bound is an error:

```rsat,error
var n: int[1..=9]
while n > 1 {
    n = n - 1
}
```

```text
error: this loop's condition depends on unknowns
 --> main.rsat:2:7
  |
2 | while n > 1 {
  |       ^^^^^
  = help: add a maximum number of iterations: `while … bound 100 { … }`
```

A known loop is allowed up to ten million iterations before RedSAT assumes it never ends.

## `and`, `or` and `if` guard what follows

As shown in [Expressions](expressions.md#guarded-evaluation), `a and b`, `a or b`,
`a implies b` and `if` expressions only evaluate their second part where it matters. Together
with symbolic `if`, this means that defensive code, such as checking an index before using it,
works on unknowns exactly as you would hope.
