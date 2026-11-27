# Knowns and unknowns

Every value in a RedSAT program is either **known** or **unknown**, and that one idea explains
most of the language.

## Known values: ordinary computation

A variable with a value is known, and so is anything computed only from known values. RedSAT
simply computes these, as any language would:

```rsat
var width = 7
var height = 6
var area = width * height
print "area = {area}"
print 2 ^ 100
```

```text
area = 42
1267650600228229401496703205376
```

`print` shows its arguments, separated by spaces. Notice the last line: integers in RedSAT are
**exact**. There is no 64-bit limit and no overflow, ever.

## Unknowns: what the solver finds

A `var` without a value is an **unknown**. Its value is what `solve` will look for:

```rsat
var x: int[0..=10]          // an unknown integer from 0 to 10
var y = 2 * x + 1           // depends on x, so y is not known either
require y == 15
solve x, y
```

```text
x = 7, y = 15
```

`y` has a value, `2 * x + 1`, but that value depends on the unknown `x`. RedSAT does not need to
know `x` to work with `y`: it builds a little circuit that computes `2 * x + 1` from the bits of
`x`, and the SAT solver figures out the rest.

Whether a variable is known, unknown, or *derived* from unknowns like `y` follows from how you
declare and use it, not from a keyword:

| Declaration           | What it is                                                  |
|-----------------------|-------------------------------------------------------------|
| `var x`               | a fresh unknown; its type is inferred from how it is used   |
| `var x: int[0..=9]`   | a fresh unknown with an explicit type                       |
| `var x = expression`  | known if the expression is, otherwise derived from unknowns |

## You can only print what you know

Outside a `solve`, an unknown has no value yet, so it cannot be printed:

```rsat,error
var x: int[0..=9]
require x > 4
print "x is {x}"
```

```text
error: this value depends on unknowns, so it cannot be shown here
 --> main.rsat:3:14
  |
3 | print "x is {x}"
  |              ^
  = help: print it inside a `solve` body, where every unknown has a value
```

Inside the body of a `solve`, every unknown has been given a value, so there you can print and
compute with it freely:

```rsat
var x: int[0..=9]
require x > 4
solve x {
    print "x is {x}, and x squared is {x * x}"
}
```

```text
x is 9, and x squared is 81
```

## Exact arithmetic, also on unknowns

Arithmetic on unknowns is just as exact as on known values. RedSAT works out the range of every
intermediate result and gives it enough bits, so nothing can overflow:

```rsat
var a, b: uint<8>           // each from 0 to 255
require a + b == 510        // no wraparound: the sum really can be 510
solve a, b
```

```text
a = 255, b = 255
```

Bounds like `uint<8>` limit what a *variable* may hold; they do not make arithmetic wrap. If you
want the modular arithmetic of a CPU, write it out, and it means exactly what it says:

```rsat
var a, b: uint<8>
require a == 200
require (a + b) % 256 == 4      // 8-bit addition that wraps around
solve a, b
```

```text
a = 200, b = 60
```

## Variables change; requirements see the current value

Variables can be assigned again. Each assignment creates a new *version* of the variable, and a
`require` refers to the version current at that point:

```rsat
var x: int[0..=9]
var y = x
require y > 5        // here y is x
y = y * 2
require y < 15       // here y is 2 * x
solve all x
```

```text
x = 6
x = 7
```

This is what makes loops useful. Here `total` is built up one step at a time; in the end it
stands for `1*x + 2*x + 3*x + 4*x`:

```rsat
var x: int[0..=100]
var total = 0
for i in 1..=4 {
    total = total + i * x
}
require total == 50
solve x, total
```

```text
x = 5, total = 50
```

## The known part runs first

A good way to think about a RedSAT program: all the known computation runs first, like a script
that *builds* the problem. Loops over known ranges are unrolled, functions are called, and every
`require` on unknowns is collected. Only then does the solver get to work.

That is why some things must be known when the program runs: array lengths, loop ranges, type
bounds, `limit` counts, exponents and shift amounts. Using an unknown there is an error:

```rsat,error
var n: int[1..=5]
for i in 0..n {
    require i < 10
}
```

```text
error: a range bound must be known at compile time
 --> main.rsat:2:13
  |
2 | for i in 0..n {
  |             ^ this depends on unknowns
```

It also means that ordinary code is a powerful tool for generating constraints. In
[N-Queens](../cookbook/grid-puzzles.md#n-queens), a loop over the rows creates every "these two
queens must not attack each other" requirement; change one number and you get a bigger board.

Next: the different ways to ask for answers, in [Requirements and solving](solving.md).
