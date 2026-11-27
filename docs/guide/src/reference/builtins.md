# Built-in functions

These functions are part of the language. All of them work on known values and on unknowns. On
top of them, the [standard library](standard-library.md) adds many more, written in RedSAT.

| Function                  | Result   | Meaning                                                   |
|---------------------------|----------|-----------------------------------------------------------|
| `abs(x)`                  | `int`    | the absolute value of `x`                                 |
| `isqrt(n)`                | `int`    | √n rounded down: the largest `r` with `r * r <= n`        |
| `min(a, b, …)`, `min(xs)` | `int`    | the smallest of the arguments, or of the elements of `xs` |
| `max(a, b, …)`, `max(xs)` | `int`    | the largest of the arguments, or of the elements of `xs`  |
| `len(xs)`                 | `int`    | the length of an array or a string                        |
| `sum(xs)`                 | `int`    | the sum of the elements (0 for none)                      |
| `count(bs)`               | `int`    | how many elements are `true`                              |
| `all(bs)`                 | `bool`   | every element is `true` (true for none)                   |
| `any(bs)`                 | `bool`   | at least one element is `true` (false for none)           |
| `all_different(xs)`       | `bool`   | no two elements of the array are equal                    |
| `reverse(xs)`             | array or `string` | the array or string, back to front               |
| `ord(c)`                  | `int`    | the code of character `c`                                 |
| `chr(n)`                  | `char`   | the character with code `n`                               |

```rsat
print abs(-7), abs(7)
print isqrt(10), isqrt(16), isqrt(10 ^ 100) == 10 ^ 50
print min(3, 1, 2), max(3, 1, 2), min([5, 9]), max(x % 7 for x in 10..20)
print len([1, 2, 3]), len("hello"), len([[1], [2, 3]])
print sum([1, 2, 3]), sum(x for x in 1..=100 if x % 3 == 0), sum([])
print count([true, false, true]), count(x > 2 for x in [1, 5, 9])
print all([true, true]), all([]), any([false]), any([])
print all_different([1, 2, 3]), all_different(["a", "b", "a"])
print reverse([1, 2, 3]), reverse("stressed")
print ord('A'), chr(97), chr(ord('a') + 25)
```

```text
7 7
3 4 true
1 3 5 6
3 5 2
6 1683 0
2 2
true true false false
true false
[3, 2, 1] desserts
65 a z
```

## Integer square root

`isqrt(n)` is √n rounded down; `n` must not be negative. With a known `n`, it is computed exactly
while the program compiles, however large `n` is, so it costs the solver nothing: use it to
bound a search, such as the smaller factor of a number (`require x <= isqrt(n)`). With an
unknown `n`, the result is an unknown that the solver keeps equal to √n rounded down, and `n`
is required not to be negative:

```rsat
var n: int[0..=100]
require isqrt(n) == 5 and n % 2 == 0
solve n maximize n
```

```text
n = 34
```

## Aggregates and comprehensions

`min`, `max`, `len`, `sum`, `count`, `all`, `any`, `all_different` and `reverse` take one array,
which can be written as a comprehension directly inside the parentheses:

```rsat
var take: [bool; 4]
var weight = [3, 5, 2, 7]
require count(take) == 2
require sum(weight[i] for i in 0..4 if take[i]) == 9
solve all take
```

```text
take = [false, false, true, true]
```

In `sum`, `count`, `all`, `any`, `len` and `all_different`, a filter (`if …`) may depend on
unknowns. An element that is filtered out counts as 0 for `sum` and `count`, as `true` for `all`
and as `false` for `any`; `len` counts the elements that pass, and `all_different` requires the
elements that pass to differ. In `min`, `max` and `reverse`, filters must be known, since the
elements taking part must be.

```rsat
var v = [1, 1, 2]
var pick: [bool; 3]
require all_different(v[i] for i in 0..3 if pick[i])    // the picked values differ
require len(i for i in 0..3 if pick[i]) == 2            // two are picked
solve all pick
```

```text
pick = [true, false, true]
pick = [false, true, true]
```

## Notes

- **`min` and `max`** with several arguments compare those arguments; with one, it must be an
  array (`min(3)` is an error). The array must not be empty.
- **`len`** of an array is always known. Of an unknown string, it is an unknown integer.
- **`sum` and `count`** add their terms with a balanced tree of adders, so the result needs only a
  few more bits than the largest term.
- **`all_different`** works on arrays of integers, booleans, characters, strings, enums and
  structs. It is encoded as "every pair differs".
- **`reverse`** of an unknown string reverses its first `len(s)` characters.
- **`ord` and `chr`** convert between characters and their Unicode code points. `chr` of a known
  number that is not a valid code point is an error; on an unknown number, it requires a valid
  one.

These names cannot be used for your own functions, enums or structs. (They can still be used as
variable names, although that is rarely a good idea.)
