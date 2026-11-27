# Statements

A program is a sequence of statements, run from top to bottom. Declarations of functions, enums
and structs are the exception: they apply to the whole file, wherever they appear.

```text
var x, y: T                    declare unknowns (or, with "= e", a defined value)
x = e                          assign (also a[i] = e, p.f = e)
require e                      every answer must satisfy e
print e1, e2, …                print values
solve …                        find answers (see below)
if c { … } else { … }          branch
for x in r { … }               loop over a range, array or string
while c bound K { … }          loop ("bound K" when c depends on unknowns)
fn f(p: T, …) -> T { … }       declare a function
return e                       return from a function
enum E { A, B }                declare an enum
struct S { f: T, … }           declare a struct
import "file.rsat"             bring in another file's declarations
f(…)                           call a function for its effects
```

## `var`

```rsat
var x                       // an unknown; its type is inferred from its use below
var a, b: int[0..=9]        // several unknowns of one type
var n = 42                  // a defined value (known here)
var m: int[0..=100] = n     // a defined value with a type, which it must fit
require x == a + b
print n, m
```

```text
42 42
```

- Without `= e`, each declared name is a fresh **unknown** of the given (or inferred) type. An
  unknown array or string needs a type with a length.
- With `= e`, the variable is defined by `e`: known if `e` is, otherwise derived from unknowns.
  Only a single variable can have an initial value.
- With both a type and `= e`, the value must fit the type: a known value outside the range is an
  error, and an unknown one is *required* to fit.
- A name can be declared only once per block. An inner block may declare a name again, hiding
  the outer one until the block ends.

## Assignment

`place = e` gives a variable a new value. The place may be a variable, an array element or a
field, nested as deep as needed: `grid[r][c] = 0`, `jobs[i].start = 3`. Each assignment creates a
new *version* of the variable; requirements made earlier keep referring to the earlier version.

An array index in an assignment may be unknown: `slots[k] = 7` writes 7 into whichever element
`k` selects. Strings cannot be changed in place, and a loop variable cannot be assigned to.

## `require`

`require e` adds the requirement that `e`, a `bool`, holds in every answer. Inside an `if`
whose condition `c` depends on unknowns, it means `c implies e`. Not allowed inside a `solve`
body.

## `print`

`print e1, e2, …` prints its arguments separated by spaces, then a line break (`print` alone
prints an empty line). Strings and characters print without quotes at the top level and with
quotes inside arrays; enums print as their variant name; structs as `Name(field: value, …)`.

Every printed value must be known, so outside `solve` bodies you cannot print unknowns, nor
print under a condition that depends on unknowns.

## `solve`

```text
solve [all | unique] [e1, e2, …] [limit K] [minimize e | maximize e] [{ body }] [else { body }]
```

| Part              | Meaning                                                                    |
|-------------------|----------------------------------------------------------------------------|
| `e1, e2, …`       | what to show; two answers differ when one of these differs. Default: every variable that depends on unknowns |
| (no mode)         | the first answer (or the first `K` with `limit K`)                         |
| `all`             | every answer (at most `K` with `limit K`)                                  |
| `unique`          | the answer, and a proof that it is the only one; an error if there are more. Cannot have a `limit` |
| `limit K`         | at most `K` answers; `K` is a known positive number                        |
| `minimize e`      | an answer with the smallest value of `e`; with `all`, every such answer     |
| `maximize e`      | an answer with the largest value of `e`; with `all`, every such answer      |
| `{ body }`        | runs once per answer, with every unknown known; without a body, the listed expressions are printed as `e = value, …` |
| `else { body }`   | runs when there is no answer; otherwise `no solution` and an explanation are printed |

Each `solve` takes into account every requirement made before it in the program. `solve` must
be at the top level: not inside a function, nor under a condition that depends on unknowns. A
`solve` body cannot contain `require` or another `solve`, but it can assign to variables declared
outside it.

## `if`

```text
if condition { … } else if condition { … } else { … }
```

When the condition is known, only the branch taken runs. When it depends on unknowns, every
branch runs under its condition, and each variable assigned in a branch ends up as "the value
from the branch whose condition holds". See
[Control flow over unknowns](../learn/control-flow.md).

## `for`

```text
for name in lo..hi { … }      for name in lo..=hi { … }
for name in array { … }       for name in string { … }
```

The range bounds must be known, and so must the length of an array. A string of unknown length
is looped over up to its maximum length, each iteration guarded by "this position exists". The
loop is unrolled: the body runs once per element, with `name` bound to it. At most ten million
iterations are allowed.

## `while`

```text
while condition { … }
while condition bound K { … }
```

While the condition is known, the loop runs as usual, up to ten million iterations. Once the
condition depends on unknowns, `bound K` (a known, non-negative number) is required: the loop
runs at most `K` times in total, each iteration guarded by the condition, and the loop is
required to have finished after the last one.

## `fn` and `return`

```text
fn name(param: Type, …) -> ResultType { … }
fn name(param: Type, …) { … }
```

Declares a function, at the top level only. Every parameter needs a type; array parameters may
leave out the length (`xs: [int]`). Without `-> ResultType`, the function returns nothing and is
called as a statement. Functions only see their parameters and their own variables.

`return e` (or a bare `return` in a function without a result) leaves the function, also under
conditions that depend on unknowns. A function with a result type must return on every path.

Calls are inlined, nested at most 200 deep. A user function shadows a
[standard library](standard-library.md) function of the same name. See
[Functions](../learn/functions.md).

## `enum` and `struct`

```text
enum Name { Variant1, Variant2, … }
struct Name { field1: Type, field2: Type, … }
```

Declared at the top level only. Variants and fields are separated by commas or line breaks.
Variants are written `Name.Variant`; struct values are built with `Name(v1, v2)` or
`Name(field1: v1, field2: v2)`, all positional or all named. See [Types](types.md).

## `import`

```text
import "relative/path.rsat"
```

At the top level only. Loads the functions, enums and structs declared in another file, which
may contain nothing else (and further imports). Paths are relative to the importing file; each
file is loaded once. See [Imports](../learn/imports.md).

## Call statements

A function call on its own line, such as `ascending(digits)`, calls the function for its effects
(typically, the requirements it adds). Any other expression on its own line is an error: write
`require …` or `print …`.
