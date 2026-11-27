# Types

Every unknown has a type, and the type says which values the solver may choose from. Types are
often inferred, but writing them down is the easiest way to tell RedSAT (and your readers) what
an unknown stands for.

Here is the whole family at a glance. The sections below show each one in action.

```rsat
var N = 8

var flag: bool                  // true or false
var count: int                  // any integer; RedSAT infers the range
var digit: int[0..=9]           // 0, 1, 2, …, 9
var die: int[1..=6]             // 1, 2, …, 6
var index: int[0..8]            // 0, 1, …, 7: the indices of an 8-element array
var celsius: int[-40..=50]      // negative bounds are fine
var level: int[0..N]            // bounds may be any known expression
var byte: uint<8>               // what an unsigned 8-bit number holds: 0 … 255
var sample: int<16>             // what a signed 16-bit number holds: −32768 … 32767
var letter: char                // a character, such as 'q'
var name: string[1..=10]        // a string of 1 to 10 characters, such as "kayak"
var row: [int[1..=9]; 9]        // nine digits, each from 1 to 9
var board: [[bool; 8]; 8]       // an 8 × 8 grid of yes/no cells

enum Color { Red, Green, Blue }
var paint: Color                // Red, Green or Blue

struct Point { x: int, y: int }
var corner: Point               // a point with integer coordinates
```

## `bool`

A `bool` is `true` or `false`. Yes/no decisions are naturally booleans: is this item packed, is
this person a knight, is it raining?

```rsat
var raining, umbrella: bool
require raining implies umbrella
require raining
solve raining, umbrella
```

```text
raining = true, umbrella = true
```

## `int`

An `int` is a mathematical integer, as small or as large as needed. A plain `int` leaves the
range open, and RedSAT infers it from your requirements:

```rsat
var x: int
require x * x == 49 and x < 0
solve x
```

```text
x = -7
```

From `x * x == 49`, RedSAT works out that `x` lies between −7 and 7 and gives it just enough
bits. When the requirements leave an integer unbounded, RedSAT warns you and searches with
growing bounds: see [Unbounded integers](no-solution.md#unbounded-integers).

## Integer ranges: `int[lo..hi]` and `int[lo..=hi]`

Most of the time you know the range of an unknown, and saying so is a good idea: smaller ranges
mean smaller problems. There are two ways to write a range, and they differ only in whether the
upper end is included:

| Range      | Includes `hi`? | Example       | Values      |
|------------|----------------|---------------|-------------|
| `lo..hi`   | no             | `int[0..10]`  | 0, 1, …, 9  |
| `lo..=hi`  | yes            | `int[0..=9]`  | 0, 1, …, 9  |

So `int[0..10]` and `int[0..=9]` are the same type. Ranges look the same in loops and
comprehensions, so you can see the difference directly:

```rsat
print [i for i in 1..4]
print [i for i in 1..=4]
```

```text
[1, 2, 3]
[1, 2, 3, 4]
```

Each form reads naturally in a different situation:

- **`..` (exclusive) fits counting and indexing.** `for i in 0..N` runs exactly `N` times, and
  `int[0..N]` is exactly the set of valid indices of an array of length `N`.
- **`..=` (inclusive) fits natural bounds.** "A digit from 0 to 9" is `int[0..=9]`, and "a roll
  of a die" is `int[1..=6]`.

If you are used to Rust, this is the same convention. A range with no values is an error, which
catches the classic off-by-one:

```rsat,error
var digit: int[5..5]
```

```text
error: the range `5..5` contains no values
 --> main.rsat:1:16
  |
1 | var digit: int[5..5]
  |                ^^^^
  = note: `lo..hi` excludes `hi`, while `lo..=hi` includes it
```

## Fixed-width integers: `int<N>` and `uint<N>`

`uint<N>` holds what an unsigned `N`-bit number can hold, `0` to `2^N − 1`, and `int<N>` what a
signed one can hold, `−2^(N−1)` to `2^(N−1) − 1`. They are handy when a problem is about
machine words:

```rsat
var smallest: int<8>
solve smallest minimize smallest
var largest: uint<8>
solve largest maximize largest
```

```text
smallest = -128
largest = 255
```

## Bounds restrict variables, not arithmetic

A bounded type limits which values a *variable* may hold. Arithmetic is always exact: with
`a, b: uint<8>`, the sum `a + b` can be 510. Storing a value *into* a bounded variable adds the
requirement that it fits:

```rsat
var a, b: uint<8>
var total: uint<8> = a + b      // requires a + b <= 255
require a == 200
solve a, b, total maximize b
```

```text
a = 200, b = 55, total = 255
```

Explicit bounds always win: RedSAT may infer a *narrower* range inside them, but never a wider
one. If your requirements need a value outside the declared range, the correct answer is "no
solution". A known value outside the range is reported right away:

```rsat,error
var level: int[0..10] = 10
```

```text
error: 10 is outside the declared range [0, 9]
 --> main.rsat:1:25
  |
1 | var level: int[0..10] = 10
  |                         ^^
```

## `char`

A `char` is a Unicode character, written in single quotes: `'a'`, `'Z'`, `'7'`, `'\n'`.
Characters can be compared (`<`, `>=`, …), tested against a range or a string with `in`, and
converted to and from their code with `ord` and `chr`:

```rsat
var c: char
require c in 'a'..='z'
require ord(c) - ord('a') == 7
solve c
```

```text
c = 'h'
```

## `string`

A string is a sequence of characters, written in double quotes. Known strings work as in any
language: `len(s)`, `s[i]`, `s + t` and `reverse(s)`.

An unknown string needs a **range of lengths**, written like an integer range:
`string[3..=5]` holds strings of 3 to 5 characters. The length itself is an unknown, and so is
every character:

```rsat
var word: string[3..=5]
require all(c in 'a'..='c' for c in word)
require reverse(word) == word and len(word) == 4
solve all word limit 3
```

```text
word = "aaaa"
word = "abba"
word = "acca"
```

A plain `string` works too, but RedSAT then has to guess a maximum length, 32, and warns you:

```rsat
var s: string
require s + "!" == "hi!"
solve s
```

```text
s = "hi"
warning: `s` has no length bounds; lengths up to 32 are considered
 --> main.rsat:1:5
  |
1 | var s: string
  |     ^
  = help: declare its lengths, e.g. `var s: string[1..=10]`
```

Strings cannot be changed in place (`s[0] = 'x'` is an error): build a new one with `+`.

## Arrays: `[T; N]`

`[T; N]` holds exactly `N` values of type `T`. The length must be known when the program runs,
but the elements can be unknowns, and so can an index:

```rsat
var row: [int[1..=4]; 4]
require all_different(row)
require row[0] == 2 and row[3] < row[1]
solve all row limit 3
```

```text
row = [2, 3, 4, 1]
row = [2, 4, 3, 1]
row = [2, 4, 1, 3]
```

Arrays nest: `[[int[1..=9]; 9]; 9]` is a Sudoku grid, read as `grid[r][c]`. Known arrays are
written as lists, `[4, 8, 15]`, and their type, length included, comes from the list. For
functions that accept arrays of any length, see [Functions](functions.md).

## Mixing knowns and unknowns: `?`

Sometimes part of a value is given and part is to be found, like a Sudoku grid with some digits
filled in. Write `?` for each missing piece. Every `?` is a fresh, anonymous unknown, whose type
comes from the declaration around it:

```rsat
var row: [int[1..=4]; 4] = [1, ?, ?, 4]
require all_different(row) and row[1] < row[2]
solve row
```

```text
row = [1, 2, 3, 4]
```

`?` works anywhere a single value of a known type is expected: an array element, a struct field
(`Point(1, ?)`), or a whole value (`var p: Point = ?`).

## Enums

An enum is a type with a fixed set of named values. Enums make models read like the problem
they describe:

```rsat
enum Drink { Tea, Coffee, Juice }
var alice, bob: Drink
require alice != bob
require alice != Drink.Coffee
require bob == Drink.Coffee or bob == Drink.Juice
solve all alice, bob
```

```text
alice = Juice, bob = Coffee
alice = Tea, bob = Juice
alice = Tea, bob = Coffee
```

Enum values are written `Drink.Tea` and compared with `==` and `!=`. Enums are declared at the
top level of a file.

## Structs

A struct groups named fields into one value. Fields can have any type, including bounded
integers, arrays and other structs:

```rsat
struct Point { x: int[-5..=5], y: int[-5..=5] }

var p: Point
require p.x * p.x + p.y * p.y == 25 and p.x > 0 and p.y > 0
solve all p

var origin = Point(x: 0, y: 0)      // fields by name…
var corner = Point(3, 4)            // …or by position
corner.x = corner.x + 1
print origin, corner, corner.x
```

```text
p = Point(x: 4, y: 3)
p = Point(x: 3, y: 4)
Point(x: 0, y: 0) Point(x: 4, y: 4) 4
```

An unknown struct is simply a bundle of unknowns, one per field. Structs, too, are declared at
the top level.

## Type inference

You can leave out the type of an unknown whenever its use settles it: `var x, y` followed by
`x * y == n` makes both `int`, and using a variable with `and` makes it a `bool`. When nothing
settles it, RedSAT asks you to annotate it:

```rsat,error
var mystery
solve mystery
```

```text
error: cannot infer the type of `mystery`
 --> main.rsat:1:5
  |
1 | var mystery
  |     ^^^^^^^ declared here
  = help: use it in a way that determines its type, or annotate it: `var mystery: int` or `var mystery: bool`
```

Some teams prefer every unknown to carry a type. The `explicit-types` compiler option, part of
[strict mode](../reference/configuration.md), makes annotations mandatory.

The [types reference](../reference/types.md) has the complete rules.
