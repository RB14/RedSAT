# Number puzzles

RedSAT's integers are exact, so number puzzles can be stated just as a mathematician would state
them.

## Factoring: running multiplication backwards

**The problem.** A number is the product of two primes, and we want to find them. Multiplying
is easy; going back from the product to its factors is hard. The security of RSA encryption
rests on that being infeasible for very large numbers.

**The model.** Two unknowns whose product is `n`, excluding the trivial `1 × n`:

```rsat
var n = 4295229443             // = 65537 × 65539
var x, y

require x > 1 and y > 1        // rule out the trivial 1 × n
require x <= y                 // symmetry breaking: each pair once
require x * y == n

solve x, y {
    print "{n} = {x} × {y}"
} else {
    print "{n} is prime"
}
```

```text
4295229443 = 65537 × 65539
```

Nothing mentions bits. From `x > 1`, `y > 1` and `x * y == n`, RedSAT infers that neither factor
exceeds `n / 2`, and `x <= y` tightens `x` to at most `√n`. It then builds a multiplier circuit for
`x * y`, and the SAT solver runs it backwards: which inputs make it output `n`?

If `n` is prime, there is no solution, and that *is* a proof that `n` is prime:

```rsat
var n = 1000003
var x, y
require x > 1 and y > 1
require x <= y
require x * y == n
solve x, y {
    print "{n} = {x} × {y}"
} else {
    print "{n} is prime"
}
```

```text
1000003 is prime
```

Factoring is a good way to feel the limits of SAT solving: products of two 32-bit primes take a
while, and products of two 64-bit primes are far out of reach. See
[Performance tips](../under-the-hood/performance.md).

## Pythagorean triples

**The problem.** Which right triangles have whole-number sides, `a² + b² = c²`, with sides up
to 30?

**The model.** Three bounded unknowns and the equation. `a < b` lists each triangle once
instead of twice:

```rsat
var limit = 30
var a, b, c: int[1..=limit]

require a < b
require a * a + b * b == c * c

var count = 0
solve all a, b, c {
    count = count + 1
}
print "{count} triples with sides up to {limit}"
```

```text
11 triples with sides up to 30
```

## Sums of two squares

**The problem.** Which numbers up to 50 can be written as a sum of two squares, `a² + b²`? For
example, `5 = 1² + 2²`, but 3 cannot be written that way.

**The model.** `a` and `b` are *witnesses*: they only need to exist. Because only `total` is
listed after `solve all`, each total is reported once, however many pairs produce it:

```rsat
var total: int[1..=50]
var a, b
require a * a + b * b == total

var found = 0
solve all total {
    found = found + 1
}
print "{found} numbers up to 50 are sums of two squares"
```

```text
24 numbers up to 50 are sums of two squares
```

`a` and `b` have no declared range at all. From `a * a + b * b == total` and `total <= 50`,
RedSAT infers that both lie between −7 and 7.

## A digit puzzle

**The problem.** What is the smallest four-digit multiple of 7 whose digits add up to 30?

**The model.** One unknown number. The digit sum is computed the way you would compute it for a
known number, with a `while` loop that strips off one digit at a time. Because the loop's
condition depends on the unknown, it needs a `bound`; a four-digit number needs four
iterations:

```rsat
var n: int[1000..=9999]

var digit_sum = 0
var rest = n
while rest > 0 bound 4 {
    digit_sum = digit_sum + rest % 10
    rest = rest / 10
}

require digit_sum == 30
require n % 7 == 0

solve n minimize n
```

```text
n = 4998
```

`/` and `%` on unknowns are encoded as a fresh quotient and remainder that must satisfy
`rest == 10 * quotient + remainder`, so no division circuit is needed. (The standard library's
[`digit_sum`](../reference/standard-library.md#numbers) does the same job in one call.)

## Bit patterns

**The problem.** Which 8-bit numbers have exactly four one bits, and bits that read the same in
both directions?

**The model.** One `uint<8>` unknown, with the standard library's `bit` and `popcount`:

```rsat
var x: uint<8>

require all(bit(x, i) == bit(x, 7 - i) for i in 0..4)
require popcount(x, 8) == 4

solve all x {
    print "{x}: {binary(x)}"
}

fn binary(x: int) -> string {
    var digits = ""
    for i in 0..8 {
        digits = (if bit(x, i) == 1 { "1" } else { "0" }) + digits
    }
    return digits
}
```

```text
60: 00111100
195: 11000011
102: 01100110
165: 10100101
90: 01011010
153: 10011001
```

The function `binary` runs in the `solve` body, on a known `x`, so it is ordinary code.
