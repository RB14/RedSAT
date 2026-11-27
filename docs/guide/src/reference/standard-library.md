# Standard library

The standard library (the *prelude*) is available in every program, without an `import`. It is
written in RedSAT itself: every function is an ordinary RedSAT function, inlined at each call
like your own, so it works on known values and on unknowns alike. Its source is
`crates/redsat-lang/src/prelude.rsat`, and reading it is a good way to learn idiomatic RedSAT.

If you declare a function with the same name, yours is used instead, everywhere (see
[Functions](../learn/functions.md#your-functions-and-the-standard-library)).

Some parameters must be known when the function is called, because they set a loop range, an
array length or a shift amount; the tables below say which.

## Numbers

| Function                                    | Description                                                          |
|---------------------------------------------|----------------------------------------------------------------------|
| `sign(x: int) -> int`                       | −1, 0 or 1: the sign of `x`.                                         |
| `clamp(x: int, lo: int, hi: int) -> int`    | `x`, limited to the range `lo..=hi`.                                 |
| `divides(d: int, n: int) -> bool`           | Whether `d` divides `n`. `d` must not be 0.                          |
| `is_even(x: int) -> bool`                   | Whether `x` is even.                                                 |
| `is_odd(x: int) -> bool`                    | Whether `x` is odd (also for negative `x`).                          |
| `gcd(a: int, b: int) -> int`                | The greatest common divisor. Both arguments must be known.           |
| `digits(n: int, k: int) -> [int]`           | The `k` decimal digits of `n`, most significant first, with leading zeros. `n` is non-negative with at most `k` digits; `k` must be known. |
| `digit_sum(n: int, k: int) -> int`          | The sum of the `k` decimal digits of `n`; `k` must be known.         |
| `bit(x: int, i: int) -> int`                | Bit `i` of `x` (two's complement): 0 or 1. `i` must be known.        |
| `popcount(x: int, width: int) -> int`       | The number of one bits among the lowest `width` bits of `x`; `width` must be known. |

```rsat
print sign(-5), sign(0), sign(7)
print clamp(15, 0, 10), clamp(-3, 0, 10), clamp(4, 0, 10)
print divides(3, 12), divides(5, 12), is_even(-4), is_odd(-3)
print gcd(84, 36), digits(4096, 4), digits(42, 4), digit_sum(4096, 4)
print bit(6, 0), bit(6, 1), bit(-1, 100), popcount(255, 8), popcount(-1, 4)
```

```text
-1 0 1
10 0 4
true false true true
12 [4, 0, 9, 6] [0, 0, 4, 2] 19
0 1 1 8 4
```

On unknowns: which three-digit odd multiple of 7 has digits adding up to 25?

```rsat
var n: int[100..=999]
require digit_sum(n, 3) == 25 and is_odd(n) and divides(7, n)
solve all n
```

```text
n = 889
```

## Counting

| Function                                          | Description                                      |
|---------------------------------------------------|--------------------------------------------------|
| `exactly(k: int, conditions: [bool]) -> bool`     | Whether exactly `k` of the conditions hold.      |
| `at_least(k: int, conditions: [bool]) -> bool`    | Whether at least `k` of the conditions hold.     |
| `at_most(k: int, conditions: [bool]) -> bool`     | Whether at most `k` of the conditions hold.      |
| `count_of(xs: [int], value: int) -> int`          | How many elements of `xs` equal `value`.         |

```rsat
var flags = [true, false, true, true]
print exactly(3, flags), at_least(4, flags), at_most(3, flags)
print count_of([1, 2, 1, 3], 1)
```

```text
true false true
2
```

The [counting constraints](../cookbook/counting.md) recipe uses these to build a duty roster.

## Arrays

| Function                                                | Description                                                         |
|---------------------------------------------------------|---------------------------------------------------------------------|
| `product(xs: [int]) -> int`                             | The product of the elements (1 for none).                           |
| `all_equal(xs: [int]) -> bool`                          | Whether all elements are equal.                                     |
| `increasing(xs: [int]) -> bool`                         | Whether each element is at most the next one.                       |
| `strictly_increasing(xs: [int]) -> bool`                | Whether each element is smaller than the next one.                  |
| `decreasing(xs: [int]) -> bool`                         | Whether each element is at least the next one.                      |
| `strictly_decreasing(xs: [int]) -> bool`                | Whether each element is larger than the next one.                   |
| `all_different_except(xs: [int], ignored: int) -> bool` | Whether no two elements are equal, except for copies of `ignored` (e.g. 0 for "empty"). |
| `index_of(xs: [int], value: int) -> int`                | The first position of `value` in `xs`, or −1.                        |
| `lex_less(xs: [int], ys: [int]) -> bool`                | Whether `xs` comes before `ys` in lexicographic (dictionary) order. Useful to [break symmetries](../cookbook/symmetry.md). |
| `lex_less_eq(xs: [int], ys: [int]) -> bool`             | Whether `xs` comes before `ys` in lexicographic order, or equals it. |
| `sorted(xs: [int]) -> [int]`                            | The elements in increasing order. Built as a sorting network, so it works on unknown elements. |
| `is_permutation(xs: [int], ys: [int]) -> bool`          | Whether `ys` is a rearrangement of `xs`.                            |

```rsat
print product([2, 3, 7]), all_equal([4, 4, 4])
print increasing([1, 2, 2]), strictly_increasing([1, 2, 2])
print decreasing([3, 2, 2]), strictly_decreasing([3, 2, 1])
print all_different_except([0, 1, 0, 2], 0), index_of([5, 7, 9, 7], 7), index_of([5, 7], 8)
print lex_less([1, 2], [1, 3]), lex_less([1, 3], [1, 3]), lex_less([1], [1, 0])
print lex_less_eq([1, 3], [1, 3]), sorted([3, 1, 2, 1])
print is_permutation([1, 2, 3], [3, 1, 2]), is_permutation([1, 2], [1, 1])
```

```text
42 true
true false
true true
true 1 -1
true false true
true [1, 1, 2, 3]
true false
```

`sorted` on unknowns: which arrays of three digits sort to `[2, 5, 7]`, with the first element
larger than the last?

```rsat
var xs: [int[0..=9]; 3]
require sorted(xs) == [2, 5, 7] and xs[0] > xs[2]
solve all xs
```

```text
xs = [7, 5, 2]
xs = [5, 7, 2]
xs = [7, 2, 5]
```

`sorted` uses an odd–even transposition network: about `n²/2` compare-and-swap steps for `n`
elements, so it suits short arrays.

## Characters and strings

| Function                                         | Description                                                 |
|--------------------------------------------------|-------------------------------------------------------------|
| `is_digit(c: char) -> bool`                      | Whether `c` is one of `'0'` … `'9'`.                        |
| `is_lower(c: char) -> bool`                      | Whether `c` is one of `'a'` … `'z'`.                        |
| `is_upper(c: char) -> bool`                      | Whether `c` is one of `'A'` … `'Z'`.                        |
| `is_letter(c: char) -> bool`                     | Whether `c` is an ASCII letter.                             |
| `to_upper(c: char) -> char`                      | `c` in upper case (ASCII letters only; others unchanged).   |
| `to_lower(c: char) -> char`                      | `c` in lower case (ASCII letters only; others unchanged).   |
| `digit_value(c: char) -> int`                    | The value of a digit character: `'7'` → 7.                  |
| `starts_with(s: string, prefix: string) -> bool` | Whether `s` starts with `prefix`, whose length must be known. |
| `ends_with(s: string, suffix: string) -> bool`   | Whether `s` ends with `suffix`, whose length must be known. |
| `count_char(s: string, c: char) -> int`          | How many times `c` occurs in `s`.                           |

```rsat
print is_digit('7'), is_lower('a'), is_upper('a'), is_letter('Z'), is_letter('?')
print to_upper('q'), to_lower('Q'), to_upper('!'), digit_value('8')
print starts_with("hello", "he"), ends_with("hello", "lo"), count_char("banana", 'a')
```

```text
true true false true false
Q q ! 8
true true 3
```

On an unknown string: a word of 4 to 6 lowercase letters that starts with "ab", ends with "ba"
and has three `a`s:

```rsat
var w: string[4..=6]
require all(is_lower(c) for c in w)
require starts_with(w, "ab") and ends_with(w, "ba") and count_char(w, 'a') == 3
solve w
```

```text
w = "ababa"
```
