# Expressions and operators

Expressions work the same on known values and on unknowns: `x + 1` means the same whether `x`
is `41` or something the solver has yet to find. This chapter tours the operators; the
[expressions reference](../reference/expressions.md) has the full precedence table.

## Arithmetic

`+`, `-`, `*`, `/`, `%` and `^` (power) work on integers, and are **exact**: results are never
truncated to some number of bits.

```rsat
print 1 + 2 * 3, 2 ^ 10, (1 + 2) * 3
print 10 / 3, 10 % 3
```

```text
7 1024 9
3 1
```

**Division rounds so that the remainder is never negative** (this is called *Euclidean*
division). For positive numbers that is the usual answer; for negative ones it differs from
languages like C, Java or Rust:

```rsat
print -7 / 2, -7 % 2
print 7 / -2, 7 % -2
```

```text
-4 1
-3 1
```

The rule is simple: `a == (a / b) * b + a % b`, and `a % b` lies in `0..abs(b)`. In particular
`x % 2` is always `0` or `1`, so `x % 2 == 1` is a reliable test for odd numbers, even negative
ones.

`/` and `%` work on unknowns too, also by unknown divisors. Dividing by an unknown adds the
requirement that it is not zero:

```rsat
var d: int[-4..=4]
require 12 / d == -3
solve all d
```

```text
d = -4
```

The exponent of `^` must be known (`x ^ 3` is fine, `2 ^ x` is not), and so must the shift
amount of `<<` and `>>`. See [Performance tips](../under-the-hood/performance.md) for why
multiplying two unknowns is costlier than it looks.

## Comparisons

`==` and `!=` work on every type: numbers, booleans, characters, strings, arrays, enums and
structs. `<`, `<=`, `>` and `>=` work on integers and characters.

Comparisons do not chain. Mathematicians write `0 < x < 10`, but RedSAT asks you to spell it
out:

```rsat,error
var x: int[0..=20]
require 0 < x < 10
```

```text
error: comparisons cannot be chained
 --> main.rsat:2:15
  |
2 | require 0 < x < 10
  |               ^
  = help: write `a < b and b < c` instead of `a < b < c`
```

(`x in 1..10` is a neat alternative, see [Membership](#membership-in) below.)

## Logic

The logical operators are words:

| Operator      | True when                                  |
|---------------|--------------------------------------------|
| `not a`       | `a` is false                               |
| `a and b`     | both are true                              |
| `a or b`      | at least one is true                       |
| `a xor b`     | exactly one is true                        |
| `a implies b` | `a` is false, or `b` is true               |
| `a iff b`     | both are true or both are false            |

`implies` and `iff` are rarely found in programming languages, but they are everyday tools for
stating requirements. "If it rains, take an umbrella" is `require raining implies umbrella`,
and "a knight's statement is true" is `require knight iff statement`.

Typing `&&`, `||` or `!` gets you a friendly reminder of the word to use instead.

### Guarded evaluation

`and`, `or` and `implies` only evaluate their right-hand side where it matters. When the
left-hand side is an unknown, the right-hand side only has to make sense *when the left-hand
side allows it*. This makes guards like this one safe, even though `i` may be out of range:

```rsat
var xs = [3, -1, 4]
var i: int[0..=5]
require i < len(xs) and xs[i] > 0
solve all i
```

```text
i = 0
i = 2
```

Without the guard, `xs[i]` alone would require `i` to be a valid index. An `if` expression
(below) guards its branches the same way.

## Precedence, briefly

Operators bind in the order you would expect from mathematics: `^` before `*` before `+`, then
comparisons, then `not`, `and`, `or`. Two differences from C-like languages are worth knowing:

- **Bitwise operators bind tighter than comparisons**, so `x & 1 == 1` means `(x & 1) == 1`.
- **`not` binds looser than comparisons**, so `not x == 3` means `not (x == 3)`.

```rsat
var x: int[0..=7]
require not x == 3 and x & 1 == 1 and x > 1
solve all x
```

```text
x = 7
x = 5
```

When in doubt, add parentheses.

## Bitwise operators

`&` (and), `|` (or), `xor`, `<<` and `>>` work on integers, bit by bit, as on two's-complement
numbers. `xor` does double duty: on booleans it is logical, on integers bitwise.

```rsat
print 12 & 10, 12 | 10, 12 xor 10
print 1 << 4, 256 >> 4, -16 >> 2
```

```text
8 14 6
16 16 -4
```

`>>` rounds towards minus infinity, like division by a power of two. The standard library has
[`bit` and `popcount`](../reference/standard-library.md#numbers) for working with single bits.

## Membership: `in`

`x in …` tests whether a value lies in a range, an array or a string:

```rsat
print 3 in 1..3, 3 in 1..=3
print 4 in [1, 4, 9], 'e' in "aeiou"
```

```text
false true
true true
```

## Comprehensions and aggregates

A **comprehension** builds an array from a loop, with optional `if` filters, and several `for`s
nest:

```rsat
print [x * x for x in 1..=5 if x != 3]
print [[i, j] for i in 0..3 for j in i + 1..3]
```

```text
[1, 4, 16, 25]
[[0, 1], [0, 2], [1, 2]]
```

The **aggregates** `sum`, `count`, `all`, `any`, `min` and `max` take an array, or a
comprehension written straight inside the parentheses:

```rsat
var xs = [4, 8, 15, 16, 23, 42]
print sum(xs), min(xs), max(xs)
print count(x % 2 == 0 for x in xs), all(x > 0 for x in xs), any(x > 40 for x in xs)
```

```text
108 4 42
4 true true
```

This is where models get compact. A filter that depends on unknowns is allowed inside `sum`,
`count`, `all` and `any`: it decides, per element, whether that element takes part. Pick three
of the numbers so that they add up to 47:

```rsat
var xs = [4, 8, 15, 16, 23, 42]
var pick: [bool; 6]
require count(pick) == 3
require sum(xs[i] for i in 0..6 if pick[i]) == 47
solve all pick {
    print [xs[i] for i in 0..6 if pick[i]]
}
```

```text
[8, 16, 23]
```

Inside the `solve` body, `pick` is known, so the comprehension `[… if pick[i]]` is fine there.
Outside a body, an *array* comprehension cannot filter on unknowns, because the length of the
array would depend on them:

```rsat,error
var take: [bool; 3]
var chosen = [i for i in 0..3 if take[i]]
```

```text
error: an array comprehension cannot filter on unknowns: its length would depend on them
 --> main.rsat:2:14
  |
2 | var chosen = [i for i in 0..3 if take[i]]
  |              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  = help: aggregate with `sum`, `count`, `all`, `any`, `len` or `all_different`, or move the condition into the element
```

## `if` as a value

`if` can be used as an expression. It needs both branches, and `else if` chains work:

```rsat
var a, b: int[0..=9]
var bigger = if a > b { a } else { b }
var trend = if a == b { "flat" } else if a < b { "rising" } else { "falling" }
require bigger == 7 and a + b == 10
solve all a, b {
    print "{a}, {b}: the bigger one is {bigger}, {trend}"
}
```

```text
3, 7: the bigger one is 7, rising
7, 3: the bigger one is 7, falling
```

When the condition depends on unknowns, both branches are computed, and the result is "the
first if the condition holds, otherwise the second".

## Strings and interpolation

`{…}` inside a string inserts the value of any expression. To write a literal brace, escape it:

```rsat
var name = "Ada"
print "Hello, {name}! Your name has {len(name)} letters."
print "braces: \{ and \}, a quote: \", a backslash: \\"
print "a tab:\tthen more"
```

```text
Hello, Ada! Your name has 3 letters.
braces: { and }, a quote: ", a backslash: \
a tab:	then more
```

The escapes are `\n`, `\t`, `\r`, `\0`, `\\`, `\"`, `\'`, `\{` and `\}`.

`+` joins strings, and also arrays:

```rsat
print "ab" + "cd", [1, 2] + [3]
```

```text
abcd [1, 2, 3]
```

## Built-in functions

A handful of functions are built into the language: `abs`, `min`, `max`, `len`, `sum`,
`count`, `all`, `any`, `all_different`, `reverse`, `ord` and `chr`. On top of them, a
[standard library](../reference/standard-library.md) written in RedSAT itself adds
`exactly`, `sorted`, `lex_less`, `is_digit` and many more. All of them work on unknowns. The
[built-in functions reference](../reference/builtins.md) describes each one.
