# Diagnostics

RedSAT reports problems as *diagnostics*, which point into your source code. When running a
program, they are printed to standard error after the program's output.

## Anatomy of a diagnostic

```rsat,error
var flag: bool
var n = 3
require flag + n > 2
```

```text
error: expected `int`, found `bool`
 --> main.rsat:3:9
  |
3 | require flag + n > 2
  |         ^^^^ this is `bool`
```

- The first line gives the **severity** and the message.
- `-->` gives the file, line and column.
- The source line follows, with `^^^^` under the code in question and, sometimes, a label.
- Further locations, introduced by `:::`, show related places: the other requirements of a
  contradiction, the call that led to an error inside a function, or the `import` that brought
  in a file.
- Lines starting with `= note:` add context, and `= help:` suggests a fix.

There are three severities:

| Severity  | Means                                                                                |
|-----------|--------------------------------------------------------------------------------------|
| `error`   | The program is wrong and stops. `redsat` exits with status 1.                        |
| `warning` | Something is probably not what you meant, but the program runs to the end.          |
| `note`    | Information, such as the [explanation](../learn/no-solution.md) of a "no solution". |

## When errors are found

- **Syntax and type errors** are found before anything runs, by `redsat check`, by the editor
  and by every run. All of them are reported at once.
- **Elaboration errors** are found while the known part of the program runs: an index out of
  range, a value that must be known but depends on unknowns, runaway recursion. The program stops
  at the first one, after printing whatever it printed before. The editor's language server finds
  these too, because it elaborates the program the same way.

## Common messages

### Syntax

| Message                                                        | What to do                                                         |
|----------------------------------------------------------------|--------------------------------------------------------------------|
| ``unknown operator `&&` `` (or `\|\|`, `!`)                    | Use `and`, `or`, `not` (or `!=`).                                   |
| `comparisons cannot be chained`                                | Write `a < b and b < c`.                                            |
| `expected the end of the statement (a line break or ;)`        | One statement per line, or separate them with `;`.                  |
| `expected =` with *a bare expression does nothing on its own*  | Write `require …` or `print …` in front of the expression.          |
| `only a single variable can have an initial value`             | Declare the variables on separate lines.                            |
| `unterminated string`                                          | Strings must close on the same line.                                |
| ``unclosed `{` in string``                                     | Close the interpolation, or write `\{` for a literal brace.          |

### Names and types

| Message                                                     | What to do                                                            |
|-------------------------------------------------------------|-----------------------------------------------------------------------|
| ``unknown variable `x` ``                                   | Declare it first. Inside a function, pass it as a parameter.           |
| ``expected `int`, found `bool` ``                           | The operands of an operator, or the arguments of a call, have the wrong types. |
| ``cannot infer the type of `x` ``                           | Annotate it: `var x: int`.                                             |
| ``cannot infer the type of this `?` ``                      | Use `?` where the type is known, e.g. `var x: int = ?`.                |
| ``an unknown array needs a length``                         | Give the length: `[int; 5]`.                                           |
| ``` `x` is already declared in this scope ```               | Assign with `=` instead of declaring again, or pick another name.     |
| ``` `sum` is a built-in name ```                            | Give your function, enum or struct another name.                       |
| ``` `f` might end without returning a value ```             | Make every path end with `return`.                                     |
| ``` `f` takes 2 arguments, but 3 were given ```             | Check the call against the declaration.                                |
| ``` `Point` has no field `z` ```                            | The help line lists the fields.                                        |
| ``named arguments are only for struct constructors``        | Pass function arguments by position.                                   |
| ``` `x` needs a type annotation ```                         | Required by the `explicit-types` option: annotate it.                  |

### Known versus unknown

| Message                                                                     | What to do                                                      |
|-----------------------------------------------------------------------------|-----------------------------------------------------------------|
| `this value depends on unknowns, so it cannot be shown here`                | Print it inside a `solve` body.                                 |
| `cannot print under a condition that depends on unknowns`                   | Print inside a `solve` body.                                    |
| `a range bound must be known at compile time` (also: a loop bound, an array length, `limit`, the exponent, the shift amount, a bit width) | Use a known value here; see [Knowns and unknowns](../learn/knowns-and-unknowns.md#the-known-part-runs-first). |
| `this loop's condition depends on unknowns`                                 | Add a bound: `while … bound K { … }`.                           |
| `an array comprehension cannot filter on unknowns: its length would depend on them` | Use `sum`, `count`, `all` or `any`, or move the condition into the element. |
| `calls nested more than 200 deep`                                           | Recursion must stop on known values. Use a loop instead.        |
| `` `solve` must be a top-level statement ``                                 | Move the `solve` out of the function or symbolic `if`.          |
| `` `require` is not allowed inside a `solve` body ``                        | Move it before the `solve`.                                     |

### Values

| Message                                                   | What to do                                                       |
|-----------------------------------------------------------|------------------------------------------------------------------|
| `index 3 is out of range for length 3`                    | Indices count from 0.                                            |
| `10 is outside the declared range [0, 9]`                 | Check the range: `int[0..10]` excludes 10.                       |
| ``the range `5..5` contains no values``                   | `lo..hi` excludes `hi`; use `lo..=hi` to include it.             |
| `division by zero`                                        | A known value was divided by 0.                                  |
| `strings cannot be changed in place`                      | Build a new string with `+`.                                     |
| `the loop did not finish within 10000000 iterations`      | A known `while` loop never ends; check its condition.            |

### Solving

| Message                                                            | Means                                                        |
|--------------------------------------------------------------------|--------------------------------------------------------------|
| `` `solve unique` found more than one solution ``                  | The notes show two different solutions.                      |
| `` `limit` cannot be combined with `unique` ``                     | `unique` always looks for exactly two.                       |
| ``cannot infer a finite domain for `x` ``                          | Required by `unbounded-ints = "error"`: give `x` bounds.     |
| `warning: could not infer bounds for …; searched \|value\| < 2^32` | The search was not complete: give the unknowns bounds.       |
| ``warning: `s` has no length bounds; lengths up to 32 are considered`` | Declare the lengths: `string[1..=10]`.                   |
| `warning: this requirement is always false, so the problem has no solution` | A requirement on known values is false.              |
| `note: no solution: these requirements cannot all hold together`   | See [When there is no solution](../learn/no-solution.md).    |
| `note: no solution: this requirement can never hold`               | That requirement is impossible on its own.                   |

### Imports

| Message                                                               | What to do                                              |
|-----------------------------------------------------------------------|---------------------------------------------------------|
| ``cannot find `file.rsat`: …``                                        | Paths are relative to the importing file.               |
| `an imported file may only declare functions, enums and structs`      | Move the other statements into the program.            |
| `` `square` is declared twice ``                                      | Two files declare the same name.                        |

## Internal errors

Before reporting an answer, RedSAT checks it against every requirement, using the language's own
rules for known values. If an answer ever fails that check, RedSAT reports
`internal error: the solver's answer violates this constraint` instead of printing a wrong
answer. That would be a bug in RedSAT: please report it, with the program.
