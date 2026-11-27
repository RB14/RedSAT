# Expressions and operators

Every operator works on known values and on unknowns, with the same meaning. A few operands
must be known, as noted below.

## Precedence

From loosest to tightest binding:

| Operators                                   | Associativity        |
|---------------------------------------------|----------------------|
| `iff`                                       | left                 |
| `implies`                                   | right                |
| `or`                                        | left                 |
| `xor`                                       | left                 |
| `and`                                       | left                 |
| `not`                                       | prefix               |
| `==` `!=` `<` `<=` `>` `>=` `in`            | none: cannot chain   |
| `\|` (bitwise or)                           | left                 |
| `&` (bitwise and)                           | left                 |
| `<<` `>>`                                   | left                 |
| `+` `-`                                     | left                 |
| `*` `/` `%`                                 | left                 |
| `-` (negation)                              | prefix               |
| `^` (power)                                 | right                |
| `f(…)` `a[i]` `p.x` `E.V`                   | postfix              |

Some consequences:

```rsat
print 1 + 2 * 3 ^ 2             // 1 + (2 * (3 ^ 2))
print -2 ^ 2, 2 ^ 3 ^ 2         // -(2 ^ 2) and 2 ^ (3 ^ 2)
print 6 & 3 == 2                // (6 & 3) == 2: bitwise binds tighter than ==
print not 1 == 2                // not (1 == 2)
print true or false and false   // true or (false and false)
print false implies false implies false   // false implies (false implies false)
```

```text
19
-4 512
true
true
true
true
```

## Arithmetic

| Expression | Operands              | Result | Notes                                              |
|------------|-----------------------|--------|----------------------------------------------------|
| `a + b`    | `int`                 | `int`  | exact                                              |
| `a - b`    | `int`                 | `int`  | exact                                              |
| `a * b`    | `int`                 | `int`  | exact                                              |
| `a / b`    | `int`                 | `int`  | Euclidean division (see below)                     |
| `a % b`    | `int`                 | `int`  | Euclidean remainder: always in `0..abs(b)`         |
| `a ^ b`    | `int`                 | `int`  | `b` must be known and non-negative; at most 64 when `a` is unknown |
| `-a`       | `int`                 | `int`  |                                                    |

Integers never overflow: every result is exact, whatever its size.

**Euclidean division.** `a / b` and `a % b` satisfy `a == (a / b) * b + a % b` with
`0 <= a % b < abs(b)`. For positive operands this is ordinary integer division; for negative ones
it differs from C, Java or Rust:

| `a`  | `b`  | `a / b` | `a % b` |
|------|------|---------|---------|
| 7    | 2    | 3       | 1       |
| -7   | 2    | -4      | 1       |
| 7    | -2   | -3      | 1       |
| -7   | -2   | 4       | 1       |

**Division by zero.** Dividing a known value by zero is an error. Dividing by an unknown adds the
requirement that it is not zero. In a branch that may not be taken (`if d != 0 { n / d } else { 0 }`),
the division is guarded like everything else in that branch.

## Comparison

| Expression          | Operands                         | Result |
|---------------------|----------------------------------|--------|
| `a == b`, `a != b`  | any two values of the same type  | `bool` |
| `a < b`, `a <= b`, `a > b`, `a >= b` | `int` or `char` | `bool` |

Comparisons do not chain: write `a < b and b < c`, or `b in a + 1..c`.

## Logic

| Expression      | Operands | True when                               |
|-----------------|----------|-----------------------------------------|
| `not a`         | `bool`   | `a` is false                            |
| `a and b`       | `bool`   | both are true                           |
| `a or b`        | `bool`   | at least one is true                    |
| `a xor b`       | `bool`   | exactly one is true                     |
| `a implies b`   | `bool`   | `a` is false or `b` is true             |
| `a iff b`       | `bool`   | both have the same value                |

`and`, `or` and `implies` evaluate `b` only where it matters: if `a` decides the result, `b` is
not evaluated at all, and if `a` is unknown, `b` is evaluated *under the condition* that it
matters (`a` for `and` and `implies`, `not a` for `or`). Requirements that `b` brings along,
such as an index being in range, then only apply under that condition.

## Bitwise operators

| Expression | Operands | Result | Notes                                        |
|------------|----------|--------|----------------------------------------------|
| `a & b`    | `int`    | `int`  | bitwise and                                  |
| `a \| b`   | `int`    | `int`  | bitwise or                                   |
| `a xor b`  | `int`    | `int`  | bitwise exclusive or                         |
| `a << k`   | `int`    | `int`  | `a * 2^k`; `k` must be known and non-negative |
| `a >> k`   | `int`    | `int`  | `a / 2^k`, rounding down; `k` must be known and non-negative |

Bitwise operators act on the two's-complement representation, as if integers had infinitely
many bits: `-5 & 3 == 3`, `-5 | 3 == -5`, `-5 xor 3 == -8`.

## Membership: `in`

| Expression        | True when                                          |
|-------------------|----------------------------------------------------|
| `x in lo..hi`     | `lo <= x and x < hi` (`x` an `int` or a `char`)    |
| `x in lo..=hi`    | `lo <= x and x <= hi`                              |
| `x in array`      | some element equals `x`                            |
| `c in string`     | the character `c` occurs in the string             |

## Ranges

`lo..hi` (excluding `hi`) and `lo..=hi` (including it) are not values of their own: they appear
in types (`int[0..10]`), loops (`for i in 0..n`), comprehensions and `in`.

## Indexing and fields

- `xs[i]` reads element `i` (counting from 0) of an array or string. The index may be unknown:
  that requires it to be in range.
- `p.f` reads field `f` of a struct.
- `E.V` is variant `V` of enum `E`.

A known index out of range is an error.

## `if` expressions

```text
if condition { value1 } else { value2 }
if c1 { v1 } else if c2 { v2 } else { v3 }
```

Both branches are required and must have the same type. With an unknown condition, the result is
`value1` where the condition holds and `value2` elsewhere, and each branch is evaluated under its
condition.

## Comprehensions

```text
[element for x in iterable]
[element for x in iterable if condition for y in iterable if condition …]
```

Builds an array by looping, with any number of `for` clauses (nested from left to right), each
followed by any number of `if` filters. An array comprehension's filters must be known, because
the array's length must be.

The same clauses can be written directly inside the call of an aggregate, or of a function whose
only parameter is an array: `sum(w[i] for i in 0..n if take[i])`. Inside `sum`, `count`, `all`
and `any`, filters may depend on unknowns: a filtered-out element contributes 0 to `sum` and
`count`, `true` to `all` and `false` to `any`.

## Strings

`"text {expression} text"` inserts the value of each `{expression}`, which may be any expression
without braces or string literals of its own. Inserted values are shown as `print` shows them.
Write `\{` and `\}` for literal braces.

`a + b` joins two strings, or two arrays.

## Function calls

`name(argument, …)` calls a built-in, standard library or user function, with positional
arguments. `Name(…)` with a struct's name builds a struct, with positional or named arguments.
See [Built-in functions](builtins.md) and [Standard library](standard-library.md).

## Holes

`?` is an expression that stands for a fresh unknown of the type the context expects. See
[Types](types.md#holes).
