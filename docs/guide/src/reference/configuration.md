# Configuration: `redsat.toml`

Like `tsconfig.json` for TypeScript, an optional `redsat.toml` file configures the compiler for
every program in its directory and below. RedSAT looks for it in the program's directory, then
in each parent directory, and uses the first one it finds. `--config <file>` names one
explicitly instead.

```toml
[compiler]
strict = true                # turn on every strict check below
explicit-types = "unknowns"  # "off" | "unknowns" | "all"   (default "off"; strict: "unknowns")
unbounded-ints = "error"     # "deepen" | "error"           (default "deepen"; strict: "error")
fallback-int-bits = 32       # first magnitude tried for unbounded integers
max-int-bits = 256           # stop deepening beyond this
```

Every key is optional, and an empty file is fine. Unknown keys are errors, so a typo cannot go
unnoticed.

## Options

| Key                 | Values                          | Default    | Command line          |
|---------------------|---------------------------------|------------|-----------------------|
| `strict`            | `true`, `false`                 | `false`    | `--strict`            |
| `explicit-types`    | `"off"`, `"unknowns"`, `"all"`  | `"off"`    | `--explicit-types`    |
| `unbounded-ints`    | `"deepen"`, `"error"`           | `"deepen"` | `--unbounded-ints`    |
| `fallback-int-bits` | 1 to `max-int-bits`             | `32`       | `--fallback-int-bits` |
| `max-int-bits`      | up to 65536                     | `256`      | `--max-int-bits`      |

### `explicit-types`

Which variable declarations need a type annotation.

- `"off"`: types may always be inferred.
- `"unknowns"`: every unknown (a `var` without an initial value) needs a type, like
  TypeScript's `noImplicitAny`.
- `"all"`: every `var` needs a type, even `var n: int = 42`.

> **Known issue:** in the current version, `"all"` also checks the variables inside the
> standard library's own functions, which are not annotated, so it rejects every program. Use
> `"unknowns"` (the strict-mode setting) until this is fixed.

```rsat,error,explicit-types=unknowns
var x, y
var n = 42
require x + y == n
```

```text
error: `x` needs a type annotation
 --> main.rsat:1:5
  |
1 | var x, y
  |     ^ no type given
  = note: required by the compiler option explicit-types = "unknowns"
  = help: annotate it, e.g. `var x: int`
error: `y` needs a type annotation
 --> main.rsat:1:8
  |
1 | var x, y
  |        ^ no type given
  = note: required by the compiler option explicit-types = "unknowns"
  = help: annotate it, e.g. `var x: int`
```

### `unbounded-ints`

What to do with an integer unknown whose range cannot be inferred.

- `"deepen"`: warn, and search with growing bounds: `|x| < 2^fallback-int-bits` first, doubling
  the number of bits after each "no solution", up to `2^max-int-bits`. Solutions found are
  genuine; "no solution" only covers the bounds searched. See
  [Unbounded integers](../learn/no-solution.md#unbounded-integers).
- `"error"`: reject the program, asking for bounds.

### `fallback-int-bits` and `max-int-bits`

The first and the largest magnitude, in bits, tried for unbounded integers with
`unbounded-ints = "deepen"`.

## Strict mode

`strict = true` (or `--strict`) turns on the whole family of strict checks at once:
`explicit-types = "unknowns"` and `unbounded-ints = "error"`. A strictly checked program declares
the type of every unknown and gives every integer a finite range, inferred or declared:

```rsat,strict
var x: int[0..=100]
var y: int[0..=100]
require x + y == 42 and x == 2 * y
solve x, y
```

```text
x = 28, y = 14
```

```rsat,error,strict
var x: int
require x > 5
solve x
```

```text
error: cannot infer a finite domain for `x`
 --> main.rsat:1:5
  |
1 | var x: int
  |     ^ unbounded
  = note: required by the compiler option unbounded-ints = "error"
  = help: give it bounds, e.g. `var x: int[0..=1000]`, or a constraint that limits it
```

## Precedence

Settings are combined from four layers, each overriding the ones before:

1. the built-in defaults;
2. what `strict` implies;
3. keys set explicitly in `redsat.toml`;
4. command-line flags.

So a strict project can still opt out of a single check, by setting its key explicitly:

```toml
[compiler]
strict = true
unbounded-ints = "deepen"    # strict, except for this one check
```
