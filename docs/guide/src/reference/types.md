# Types

| Type                    | Values                                                    | Encoded as                         |
|-------------------------|-----------------------------------------------------------|------------------------------------|
| `bool`                  | `true`, `false`                                           | one SAT variable                   |
| `int`                   | any integer; the range is inferred                        | two's complement, sized to fit     |
| `int[lo..hi]`           | integers from `lo` up to, but **excluding**, `hi`         | sized to the range                 |
| `int[lo..=hi]`          | integers from `lo` up to and **including** `hi`           | sized to the range                 |
| `uint<N>`               | `0` to `2^N − 1`                                          | `N` bits                           |
| `int<N>`                | `−2^(N−1)` to `2^(N−1) − 1`                               | `N` bits                           |
| `char`                  | a Unicode character, such as `'a'`                        | its code, sized to fit             |
| `string`                | a sequence of characters, of bounded length               | characters plus a length           |
| `string[lo..hi]`, `string[lo..=hi]` | strings whose length is in the range          | characters plus a length           |
| `[T; N]`                | exactly `N` values of type `T`                            | `N` encoded elements               |
| `[T]`                   | an array of any length (parameters and known values only) |                                    |
| `Name` (an enum)        | one of the enum's variants                                | a small integer                    |
| `Name` (a struct)       | a record with the struct's fields                         | its fields                         |

All bounds (`lo`, `hi`, `N`) can be any expression that is known when the program runs, such as
`int[0..N]` or `[bool; len(names)]`.

## Integers

Integers are mathematical integers. Arithmetic on them is exact: every intermediate result gets
as many bits as its value can need, so there is no overflow and no wraparound.

A type's bounds restrict the values a **variable** may take, not the arithmetic done with it:
with `a, b: uint<8>`, `a + b` may be 510. Storing a value into a variable with a bounded type,
`var c: uint<8> = a + b`, requires the value to fit.

- **Ranges.** `lo..hi` excludes `hi` and `lo..=hi` includes it, so `int[0..10]` and `int[0..=9]`
  are the same type. A range with no values (`int[5..5]`) is an error.
- **Fixed widths.** In `int<N>` and `uint<N>`, `N` is between 1 and 65536. `uint` always needs a
  width; `int` without one is the unbounded `int`.
- **Inference.** The range of a plain `int` unknown is inferred from the requirements that
  apply unconditionally (those not under a condition that depends on unknowns). An explicit
  range always takes precedence: inference may narrow it, never widen it.
- **Unbounded integers.** If no finite range can be inferred, RedSAT warns and searches with
  growing magnitudes (`|x| < 2^32`, `2^64`, …, up to `max-int-bits`), or reports an error if
  `unbounded-ints = "error"`. See [Configuration](configuration.md).

## Booleans

`true` and `false`. Booleans are not integers: `flag + 1` is a type error. To count booleans, use
`count`.

## Characters

A `char` is a Unicode scalar value, written `'a'`, `'7'` or `'\n'`. Characters can be compared
with `== != < <= > >=` (by their code), tested with `in` against a range of characters
(`c in 'a'..='z'`) or a string (`c in "aeiou"`), and converted with `ord(c)` (the code) and
`chr(n)` (the character with code `n`).

An unconstrained unknown `char` can be any Unicode character, not only a printable one, so give
character unknowns a range when you print them.

## Strings

A string is a sequence of characters. Known strings support `len(s)`, `s[i]` (a `char`),
`s + t`, `reverse(s)`, `==`, `!=`, `c in s` and `for c in s`.

An unknown string has an unknown length within its declared range, and an unknown character in
every position up to its **capacity**, the largest length allowed: `string[2..=5]` holds 2 to 5
characters. A plain `string` unknown has capacity 32, with a warning. Positions beyond a string's
length are unused, so two strings are equal exactly when their lengths and their characters
agree.

The same operations work on unknown strings: `len(w)` is an unknown integer, `w[i]` requires `i`
to be within the length, and `+` joins strings of unknown lengths. Strings cannot be changed in
place: `s[0] = 'x'` is an error.

## Arrays

`[T; N]` holds exactly `N` elements of type `T`, where `N` is known; arrays nest, as in
`[[int[1..=9]; 9]; 9]`. Array values are written `[1, 2, 3]` or with a comprehension
`[x * x for x in 1..=5]`. Elements are read and written as `xs[i]`, and the index may be unknown,
which requires it to be in range. `len(xs)` is always known.

Arrays support `==`, `!=`, `+` (concatenation), `x in xs`, `for x in xs`, `reverse`, and the
aggregates (`sum`, `count`, `all`, `any`, `min`, `max`, `all_different`).

`[T]`, without a length, is the type of an array parameter that accepts arrays of any length. A
known array may also be annotated with it.

## Holes

`?` stands for a fresh, anonymous unknown of whatever type the context expects. It lets a value
mix known and unknown parts:

```rsat
struct Point { x: int[0..=5], y: int[0..=5] }
var p = Point(1, ?)                         // x is known, y is not
var row: [int[1..=4]; 4] = [1, ?, ?, 4]     // two known cells, two unknown ones
require p.y == p.x + 2 and all_different(row) and row[1] < row[2]
solve p, row
```

```text
p = Point(x: 1, y: 3), row = [1, 2, 3, 4]
```

A `?` stands for a single value (a number, a boolean, a character, an enum or a struct), not a
whole array, and its type must be clear from the context.

## Enums

```rsat
enum Suit { Hearts, Diamonds, Clubs, Spades }
var s: Suit
require s != Suit.Hearts and s != Suit.Spades
solve all s
print Suit.Clubs == Suit.Clubs
```

```text
s = Diamonds
s = Clubs
true
```

Variants are written `Name.Variant` and compared with `==` and `!=` only. An enum needs at least
one variant, and variants must be distinct.

## Structs

```rsat
struct Card { rank: int[1..=13], suit: int[0..4] }
var hand: [Card; 2]
require hand[0].rank + hand[1].rank == 3 and hand[0].rank < hand[1].rank
require hand[0].suit == hand[1].suit + 1
solve hand
var c = Card(rank: 12, suit: 0)
c.rank = 13
print c, c == Card(13, 0)
```

```text
hand = [Card(rank: 1, suit: 3), Card(rank: 2, suit: 2)]
Card(rank: 13, suit: 0) true
```

Struct values are built with the struct's name, with all arguments positional
(`Card(13, 0)`) or all named (`Card(rank: 13, suit: 0)`). Fields are read and assigned as
`c.rank`. Structs compare with `==` and `!=`, field by field.

## Type inference

Every expression has a type, worked out by the type checker before the program runs.

- A variable with an initial value takes its type from it (`var n = 42` is an `int`).
- A variable without one takes its type from its annotation or, failing that, from its uses:
  `var x` used in `x * 2` is an `int`, used in `x and y` a `bool`.
- If no use settles it, the program is rejected with "cannot infer the type", asking for an
  annotation.
- With the compiler option `explicit-types = "unknowns"` (or `"all"`), annotations are required.
  See [Configuration](configuration.md).

Equality (`==`, `!=`) is defined for every type; the order comparisons (`<`, …) for integers and
characters only.
