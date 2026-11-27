# Functions

Functions let you name a piece of a model and reuse it. They look like functions in any
language, and they work on known values and on unknowns alike.

```rsat
fn area(width: int, height: int) -> int {
    return width * height
}

print area(6, 7)

var w, h: int[1..=10]
require area(w, h) == 24 and w < h
solve all w, h
```

```text
42
w = 3, h = 8
w = 4, h = 6
```

## Declaring functions

- `fn name(parameter: type, …) -> result_type { … }` declares a function. Every parameter needs
  a type; the result type is left out for functions that return nothing.
- Functions are declared at the top level of a file, and can be used anywhere in it, also
  above their declaration.
- An array parameter may leave out the length, so `xs: [int]` accepts arrays of any length.
- A function body can contain anything a program can: variables (including unknowns), `if`,
  loops, `require` and calls to other functions. It cannot contain `solve`.

```rsat
fn mean(xs: [int]) -> int {
    return sum(xs) / len(xs)
}
print mean([2, 4, 9]), mean([1, 2, 3, 4, 5])
```

```text
5 3
```

A function whose only parameter is an array can also be called with a comprehension written
directly inside its parentheses, like the built-in aggregates:

```rsat
fn mean(xs: [int]) -> int {
    return sum(xs) / len(xs)
}
print mean(x * x for x in 1..=3)
```

```text
4
```

## Functions that add requirements

A function without a result type is useful for packaging up requirements. Each call creates its
own copies of the function's variables, so here every call gets a fresh `root`:

```rsat
// Requires that n is a perfect square (n at most 10000).
fn require_square(n: int) {
    var root: int[0..=100]
    require root * root == n
}

var x: int[1..=100]
require_square(x)
require_square(x + 9)
solve all x
```

```text
x = 16
```

## Functions only see their parameters

A function sees its parameters and its own variables, and nothing else. That keeps functions
self-contained, and a mistake here gets a clear message:

```rsat,error
var limit = 10
fn small(x: int) -> bool {
    return x < limit
}
```

```text
error: unknown variable `limit`
 --> main.rsat:3:16
  |
3 |     return x < limit
  |                ^^^^^ not declared
  = note: functions can only use their own parameters and variables
  = help: pass `limit` to the function as a parameter
```

## Early returns work on unknowns

You can `return` from the middle of a function, also under a condition that depends on
unknowns. RedSAT keeps track of "has the function returned yet?" for every path, so ordinary
early-return code means what it says:

```rsat
fn clamp_to(v: int, lo: int, hi: int) -> int {
    if v < lo { return lo }
    if v > hi { return hi }
    return v
}

var x: int[-20..=20]
require clamp_to(x, 0, 10) == 10 and x < 13
solve all x
```

```text
x = 10
x = 11
x = 12
```

This works inside loops too. Find arrays whose first positive element is at position 2:

```rsat
fn first_positive(xs: [int]) -> int {
    for i in 0..len(xs) {
        if xs[i] > 0 { return i }
    }
    return -1
}

var v: [int[-1..=1]; 3]
require first_positive(v) == 2
solve all v
```

```text
v = [0, 0, 1]
v = [-1, -1, 1]
v = [-1, 0, 1]
v = [0, -1, 1]
```

A function with a result type must return a value on every path; RedSAT checks this for you.

## How calls work: inlining

RedSAT does not "call" functions at run time the way a CPU does. Every call is **inlined**: the
function's body is run right there, with the arguments in place of its parameters. On known
arguments that is just computation. On unknown arguments, the body's work becomes part of the
circuit, once per call.

Two consequences are worth knowing:

- **Recursion is fine when it stops on known values.** Here every call has a known `n`, so the
  recursion unfolds completely while the program runs:

  ```rsat
  fn fib(n: int) -> int {
      if n < 2 { return n }
      return fib(n - 1) + fib(n - 2)
  }
  print [fib(i) for i in 0..12]
  ```

  ```text
  [0, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89]
  ```

  If the stopping condition depends on unknowns, the unfolding never ends. RedSAT stops after
  200 nested calls and tells you:

  ```rsat,error
  fn down(n: int) -> int {
      if n == 0 { return 0 }
      return down(n - 1)
  }
  var x: int[0..=5]
  require down(x) == 0
  ```

  ```text
  error: calls nested more than 200 deep
   --> main.rsat:3:12
    |
  3 |     return down(n - 1)
    |            ^^^^^^^^^^^
   ::: main.rsat:6:9
    |
  6 | require down(x) == 0
    |         ^^^^^^^ in this call to `down`
    = note: functions are inlined, so recursion must stop based on values known at compile time
  ```

  Rewrite such a function with a loop over a known range instead, or with `while … bound`.

- **Each call adds to the problem.** A function that multiplies two unknowns adds a multiplier
  circuit every time it is called on unknowns. That is usually fine, but it is worth keeping in
  mind inside big loops.

## Arguments

Arguments are passed by position. Named arguments, `f(x: 1)`, are only for building
[structs](types.md#structs).

## Your functions and the standard library

RedSAT's [standard library](../reference/standard-library.md) is written in RedSAT and
available in every program. If you declare a function with the same name as one of its
functions, **yours wins**, so adding a function to the standard library can never break your
program:

```rsat
fn sign(x: int) -> int {
    return 42
}
print sign(-3)
```

```text
42
```

Your version replaces the standard one in your code only. Shadowing is *lexical*: the
standard library's own functions keep calling the library's versions. For example, the
standard `is_letter` calls `is_lower`, and redefining `is_lower` does not change `is_letter`:

```rsat
fn is_lower(c: char) -> bool {
    return false
}

print is_lower('a'), is_letter('a')
```

```text
false true
```

The built-in functions such as `sum` and `len` are part of the language itself, and their names
cannot be reused for functions.

## Documenting your functions

Lines starting with `///` right above a function are its documentation. Editors show it when
you hover the function's name, in the completion list, and in the signature help that pops up
while you type its arguments (in VS Code and in the playground). The standard library documents
its functions the same way.

```rsat
/// The larger of `a` and `b`, but never above 9.
fn capped_max(a: int, b: int) -> int {
    return min(max(a, b), 9)
}

print capped_max(4, 12)
```

```text
9
```
