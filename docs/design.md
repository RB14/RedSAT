# RedSAT — Design Document (v0)

> **Status:** draft v0, started 2026-11-27. The *semantics* described here are the commitments;
> the concrete *syntax* is a strawman that we expect to polish as the parser comes to life.

---

## 1. Vision

RedSAT is a general-purpose programming language whose programs are **solved, not executed**.
A program declares variables, states the constraints a solution must satisfy, and says which
values it wants to see. The compiler reduces the program **in polynomial time** to a Boolean
satisfiability (SAT) instance, hands it to a state-of-the-art CDCL SAT solver, and decodes the
solver's model back into the program's own values: integers, booleans, characters, strings,
arrays, records.

```text
   program.rsat ──compile (poly-time reduction)──▶ CNF ──SAT solver──▶ model ──decode──▶ answer
```

## 2. Design principles

1. **Bounded by construction.** A polynomial-size reduction exists only when everything is
   finite: integer domains, array sizes, string lengths, loop trip counts. Every such size is
   either known at compile time or inferred by the compiler (§4.5).
2. **Exact mathematics.** Integers are mathematical integers. There is no overflow and no
   wraparound: every intermediate result is sized to hold its exact value. CPU-style modular
   arithmetic is something a user *writes* (`(a + b) % 2^32`), never something that happens to them.
3. **One way to declare a variable.** `var` declares every variable. Whether a variable is an
   unknown, a constant or a derived value follows from how it is used, not from a keyword.
4. **The program says what it wants to find.** `solve x, y` names the variables whose values
   are the answer. Every other unknown is auxiliary: it merely has to *exist* (§4.6).
5. **General purpose.** No language feature is tailored to a particular puzzle. Global
   constraints such as `all_different` live in a standard library written in RedSAT itself; the
   compiler may *recognise* them to pick a better encoding, but they are ordinary functions.
6. **Transparent compilation.** Every stage can be dumped (`--emit ast|ir|aig|cnf|stats`) so
   users can see exactly what their program became, and how big it is.
7. **Wrap the best solvers, own the encodings.** Solving is delegated to proven solvers behind a
   pluggable interface. RedSAT's value is the language and the quality of its encodings.

## 3. A tour of the language

Each example first states the problem in plain words, then shows how RedSAT expresses it. None of
the programs says *how* to find the answer; each one describes what an answer looks like.

### 3.1 Factoring

**The problem.** A number `n` is the product of two primes (a *semiprime*), and we want to find
those two primes. Multiplying is easy, but going backwards from `n` to its factors is hard. The
security of RSA encryption rests on this being infeasible for very large numbers.

**The RedSAT approach.** Declare two unknowns, require that their product is `n`, and rule out
the trivial answer `1 × n`. The compiler turns `x * y == n` into a multiplier circuit, the same
one a CPU uses, and the SAT solver runs that circuit *backwards*: which inputs make it output
`n`?

```rsat
var n = 1000036000099          // = 1000003 × 1000033, known at compile time
var x, y                       // unknowns: types and bit widths are inferred

require x > 1 and y > 1        // rule out the trivial 1 × n
require x <= y                 // symmetry breaking: halves the search space
require x * y == n

solve x, y {
    print "{n} = {x} × {y}"
}
```

Nothing here mentions bits. From the constraints, the compiler infers domains for `x` and `y`
(each factor is at least 2, so neither exceeds `n/2`). It sizes `x * y` so it cannot overflow,
and compares it against the 40-bit constant `n`. Run with `--stats` to see the inferred domains
and bit widths. If `n` is prime, the program prints `no solution`, and that is a proof of
primality.

### 3.2 Cryptarithm: SEND + MORE = MONEY

**The problem.** A classic puzzle from 1924:

```text
      S E N D
  +   M O R E
  -----------
    M O N E Y
```

Each letter stands for a different digit (0–9), and the leading letters `S` and `M` are not 0.
Which digit is each letter?

**The RedSAT approach.** Use one unknown digit per letter, an `all_different` constraint, and
write the column addition as one equation over place values (thousands, hundreds, …).

```rsat
var s, e, n, d, m, o, r, y: int[0..=9]

require all_different([s, e, n, d, m, o, r, y])
require s != 0 and m != 0
require (            1000*s + 100*e + 10*n + d
         +           1000*m + 100*o + 10*r + e
         == 10000*m + 1000*o + 100*n + 10*e + y )

solve s, e, n, d, m, o, r, y     // no body: prints the listed variables
```

The unique answer is 9567 + 1085 = 10652.

### 3.3 N-Queens: compile-time parameters and loops

**The problem.** Place `N` queens on an `N × N` chessboard so that no two attack each other:
no two may share a row, a column or a diagonal. The standard 8 × 8 board has 92 solutions.

**The RedSAT approach.** Every row must hold exactly one queen, so each row's queen is modelled
by a single unknown: the column it stands in. Rows are then distinct by construction, and the
constraints keep columns and diagonals distinct. Queens in rows `r1` and `r2` share a diagonal
exactly when their columns differ by `±(r2 − r1)`. `N` is an ordinary concrete value: set it to
20 and the loops generate the constraints for a 20 × 20 board.

```rsat
var N = 8                               // concrete: drives the shape of the problem
var queen: [int[0..N]; N]               // queen[r] = column of the queen in row r

for r1 in 0..N {
    for r2 in r1 + 1..N {
        var dr = r2 - r1                // concrete: computed by the compiler
        require queen[r1] != queen[r2]          // different columns
        require queen[r1] - queen[r2] != dr     // different diagonals
        require queen[r2] - queen[r1] != dr
    }
}

solve all queen limit 3 {
    print queen
}
```

### 3.4 Knapsack: optimization

**The problem.** A backpack can carry at most 15 kg. Each available item has a weight and a value.
Which items should go in to make the total value as large as possible without exceeding the
weight limit? The same pattern shows up in choosing projects under a budget, loading cargo, and
allocating servers.

**The RedSAT approach.** Use one yes/no unknown per item ("is it taken?"), a constraint on the
total weight, and `maximize` on the total value.

```rsat
var weight   = [12, 2, 1, 1,  4]
var value    = [ 4, 2, 1, 2, 10]
var capacity = 15
var take: [bool; len(weight)]

require sum(weight[i] for i in 0..len(weight) if take[i]) <= capacity

solve take maximize sum(value[i] for i in 0..len(weight) if take[i])
```

The `if take[i]` filter is *symbolic*: it compiles to `take[i] ? weight[i] : 0` inside the sum.
The optimum takes every item except the first: 8 kg, value 15.

### 3.5 Symbolic control flow and mutation

**The problem.** A small riddle: two numbers between 0 and 100 add up to 50, and the larger of
the two is 42. What are they?

**The RedSAT approach.** This example shows that ordinary imperative code works on unknowns.
`biggest` starts as `a` and is overwritten with `b` when `b > a`. Whether `b > a` holds is not
known at compile time, so the compiler encodes both branches, and `biggest` becomes
"`b` if `b > a`, otherwise `a`".

```rsat
var a, b: int[0..=100]

var biggest = a
if b > a {                  // symbolic condition: both branches are encoded
    biggest = b             // ...and biggest becomes  b > a ? b : a
}

require biggest == 42
require a + b == 50

solve all a, b              // → (42, 8) and (8, 42)
```

### 3.6 Projection: auxiliary unknowns

**The problem.** Which numbers from 1 to 50 can be written as a sum of two squares,
`total = a² + b²`? For example `5 = 1² + 2²`, but 3 cannot be written that way.

**The RedSAT approach.** `a` and `b` are *witnesses*: they only need to exist. Many totals have
several witnesses (`25 = 0² + 5² = 3² + 4²`, plus every sign variation). Because only `total` is
listed after `solve all`, each total is still reported exactly once. This is *projection*
(§4.6).

```rsat
var total: int[1..=50]
var a, b                    // auxiliary witnesses
require a * a + b * b == total

solve all total             // 1, 2, 4, 5, 8, 9, 10, 13, … each exactly once
```

### 3.7 Strings

**The problem.** Find five-letter lowercase palindromes: words that read the same forwards and
backwards, like "level" or "kayak".

**The RedSAT approach.** Use one string unknown with a fixed length, an alphabet restriction on
every character, and the palindrome property itself. `limit 10` asks for the first ten answers.

```rsat
var word: string[5..=5]
require all(c in 'a'..='z' for c in word)
require reverse(word) == word

solve all word limit 10
```

## 4. Semantics

### 4.1 Concrete and symbolic values

At compile time every value is either:

- **concrete**: fully known (literals, and anything computed only from concrete values), or
- **symbolic**: depends on at least one unknown; represented as a Boolean circuit.

Concreteness is inferred and propagates: `var s = x + y` is symbolic because `x` is. Operations
on concrete values are simply *evaluated* by the compiler using arbitrary-precision integers,
so constant folding is not an optimisation but the semantics.

Some positions **must** be concrete, and using a symbolic value there is a compile error:
array sizes, loop bounds, type bounds, and `solve` modifiers such as `limit`.

### 4.2 Variables

| Declaration          | Meaning                                                        |
|----------------------|----------------------------------------------------------------|
| `var x`              | a fresh **unknown**; type inferred from usage                  |
| `var x: int[0..=9]`  | a fresh unknown with an explicit type / domain                 |
| `var x = expr`       | a **defined** value: concrete or symbolic, depending on `expr` |

Variables are mutable. Each assignment creates a new *version* (static single assignment):
`require` refers to the versions current at that point, and `solve` reports final versions.

```rsat
var total = 0
for i in 0..len(items) {
    total = total + items[i]        // total₀, total₁, …: no actual mutation in the circuit
}
```

Types may be omitted whenever usage determines them (`var x, y` followed by `x * y == n` makes
both `int`). Ambiguous cases are compile errors asking for an annotation. A project that wants
every unknown to be annotated can say so with the `explicit-types` compiler option (§6.3).

### 4.3 Control flow

- **`if` on a concrete condition:** only the taken branch is compiled.
- **`if` on a symbolic condition:** both branches are compiled. Every variable assigned in
  either branch is merged with a multiplexer (`c ? then_value : else_value`), and a `require φ`
  inside the branch becomes the guarded constraint `c implies φ`.
- **`for i in lo..hi`:** `lo` and `hi` are concrete, the loop is fully unrolled, and `i` is
  concrete in each copy of the body. This is the language's main *metaprogramming* tool.
- **`for x in array`** and **`for c in string`** iterate over elements. For a string whose
  length is unknown, the body runs for every position up to its capacity, guarded by
  `position < len(string)`.
- **`while cond { … }`** loops while a known condition holds. When the condition depends on
  unknowns, the loop needs a bound: `while cond bound K { … }` unrolls at most `K` guarded
  iterations and requires that the loop has exited by then, as in bounded model checking.
- **`a and b`, `a or b`, `a implies b`** evaluate `b` only when `a` does not already decide the
  result. When `a` is unknown, `b` is evaluated *under the condition* `a` (or `not a` for `or`).
  Requirements that `b` implies, such as an index being in range, therefore only apply where
  `b` matters: `i < len(xs) and xs[i] > 0` is safe. An `if cond { a } else { b }` used as a
  value behaves the same way.
- **Functions** (`fn name(params) -> T { … }`, declared at the top level) are inlined at every
  call:
  - parameters need types, and array parameters may omit the length (`xs: [int]`);
  - a function only sees its parameters and its own variables, never the caller's;
  - a function may contain `var` unknowns, `require`, loops and other calls;
  - a `return` under a condition that depends on unknowns records the result for that
    condition and masks the function's later effects on those paths, so ordinary early-return
    code works on unknowns;
  - recursion is allowed when it stops on known values; calls nested more than 200 deep are
    reported as an error.

### 4.4 Types

| Type                          | Values                                                         | Encoding                                  |
|-------------------------------|----------------------------------------------------------------|-------------------------------------------|
| `bool`                        | `true`, `false`                                                | one SAT variable                          |
| `int`                         | any integer; the domain is inferred (§4.5)                     | two's-complement bit-vector sized to fit  |
| `int[lo..hi]`                 | integers from `lo` up to, but **excluding**, `hi`              | sized to the domain                       |
| `int[lo..=hi]`                | integers from `lo` up to and **including** `hi`                | sized to the domain                       |
| `uint<N>`                     | what an unsigned `N`-bit number holds: `0 … 2^N − 1`           | `N` bits                                  |
| `int<N>`                      | what a signed `N`-bit number holds: `−2^(N−1) … 2^(N−1) − 1`   | `N` bits                                  |
| `char`                        | a Unicode character, e.g. `'a'`                                | narrowed by inference (e.g. to 7 bits)    |
| `string`                      | a sequence of characters with a bounded length                 | array of chars + a length                 |
| `[T; N]`                      | exactly `N` values of type `T` (`N` concrete)                  | `N` encoded elements                      |
| `enum E { A, B, … }`          | one of the listed names                                        | small integer or one-hot                  |
| `struct S { f: T, … }`        | a record of named fields                                       | its fields                                |

Concrete examples:

```rsat
var flag: bool                  // true or false
var count: int                  // any integer; the compiler works out how many bits it needs
var digit: int[0..=9]           // 0, 1, 2, …, 9
var die: int[1..=6]             // 1, 2, …, 6
var index: int[0..8]            // 0, 1, …, 7: exactly the valid indices of an 8-element array
var celsius: int[-40..=50]      // negative bounds are fine
var level: int[0..N]            // bounds may be any compile-time expression
var byte: uint<8>               // 0 … 255
var sample: int<16>             // −32768 … 32767
var letter: char                // e.g. 'q'
var name: string                // e.g. "kayak"
var row: [int[1..=9]; 9]        // nine digits, each from 1 to 9
var board: [[bool; 8]; 8]       // an 8 × 8 grid of yes/no cells

enum Color { Red, Green, Blue }
var paint: Color

struct Point { x: int, y: int }
var corner: Point
```

**Ranges: `..` versus `..=`.** Both describe a run of consecutive integers and differ only in
whether the upper end is included:

| Range     | Includes `hi`? | Example       | Values           |
|-----------|----------------|---------------|------------------|
| `lo..hi`  | no (half-open) | `int[0..10]`  | 0, 1, …, 9       |
| `lo..=hi` | yes (closed)   | `int[0..=9]`  | 0, 1, …, 9       |

So `int[0..10]` and `int[0..=9]` are the same type. Each form reads naturally in a different
situation:
- **`..` fits counting and indexing.** `for i in 0..N` runs exactly `N` times, and `int[0..N]` is
  exactly the set of valid indices of an array of length `N`.
- **`..=` fits natural bounds.** "A digit from 0 to 9" is `int[0..=9]`, and "a die roll" is
  `int[1..=6]`.

This is the same convention as Rust (Swift spells it `..<` and `...`). A range with no values,
such as `int[5..5]`, is a compile error.

**Bounds restrict variables, not arithmetic.** A bounded type limits which values a *variable*
may hold. Computing with it is still exact: with `a, b: uint<8>`, the expression `a + b` may be
510. Storing a value into a bounded variable (`var total: uint<8> = a + b`) adds the constraint
that the value fits (`total ≤ 255`).

**Explicit bounds always take precedence** over inference. Inference may narrow a domain inside
the declared bounds but never widens it. If the constraints need values outside the declared
bounds, the program correctly has no solution.

**Annotations are optional by default.** `var x, y` followed by `x * y == n` makes both `int`.
The `explicit-types` compiler option (§6.3) can require them.

**Values of the composite types:**
- arrays: `[1, 2, 3]`, `[i * i for i in 1..=5 if i != 3]`, indexed as `grid[r][c]` (the
  index may be unknown);
- enums: `Color.Red`, compared with `==` and `!=`;
- structs: `Point(x: 1, y: 2)` or `Point(1, 2)`, with fields read and assigned as `p.x`;
- characters: `'a'`; strings: `"text"`, with `len(s)`, `s[i]`, `s + t` and `reverse(s)`.

**Strings with unknown parts** have a *capacity*: the largest length declared in
`string[lo..=hi]`, or 32 for a plain `string` (with a warning). Their length is an unknown in
`lo..=hi`. Positions beyond the length hold `'\0'` and positions before it never do, so every
string has exactly one representation and string equality is position-wise equality.

**Operators:**
- arithmetic: `+ - * / % ^` (the exponent of `^` must be known);
- comparison: `== != < <= > >=`, on integers and characters (`==` on every type);
- membership: `x in lo..hi`, `x in array`, `c in string`;
- logic: `and or not xor implies iff`;
- bitwise, on two's-complement integers: `& | xor << >>` (shift amounts must be known);
- ranges: `a..b` (exclusive) and `a..=b` (inclusive).

**Built-in functions:**

| Function | Meaning |
|---|---|
| `abs(x)`, `min(a, b, …)`, `max(a, b, …)` | the usual, on integers |
| `isqrt(n)` | √n rounded down; computed while compiling for a known `n`, else a new unknown `r` with `r² ≤ n < (r + 1)²` |
| `len(xs)` | length of an array or string |
| `sum(xs)`, `count(bs)` | the sum of integers; the number of `true` values |
| `all(bs)`, `any(bs)` | every / some value is `true` |
| `min(xs)`, `max(xs)` | the smallest / largest element |
| `all_different(xs)` | no two elements are equal |
| `reverse(xs)` | an array or string, reversed |
| `ord(c)`, `chr(n)` | a character's code; the character with a code |

Each function that takes a collection also accepts a comprehension directly, with optional
filters: `sum(w[i] for i in 0..n if take[i])`. A filter that depends on unknowns turns into a
condition on each element. This is why array comprehensions (`[… for …]`) reject such filters:
their length would depend on the unknowns.

Division and remainder are **Euclidean**: the remainder always lies in `0..|m|`, so
`-7 / 2 == -4` and `-7 % 2 == 1`. On unknowns they are encoded *relationally*: `a / b` and
`a % b` share a fresh quotient `q` and remainder `r` with `q*b + r == a` and `0 <= r < |b|`, so
no division circuit is needed (powers of two are plain bit slices). Dividing by an unknown adds
the requirement that it is not zero; inside the circuit, division is made total
(`a / 0 = 0`), so the definitions stay valid even in branches that are never taken.

### 4.5 Integer widths: inferred, exact, never overflowing

**Step 1: domain inference.** Every integer unknown starts with the domain `(−∞, +∞)`, which
is narrowed by:
- explicit type bounds;
- initialiser values;
- **bound propagation** over the top-level `require`s (those not under a symbolic condition).
  For example, `x > 1` gives `x ≥ 2`; `x * y == n` with `x, y ≥ 2` gives `x ≤ n/2`;
  `a*a + b*b == total` with `total ≤ 50` gives `|a| ≤ 7`; and `x <= y` together with
  `x * y == n` gives `x*x ≤ n`, so `x ≤ √n` needs only half of n's bits.

Tighter domains mean smaller circuits. In the Phase 0 factoring demo on a 40-bit semiprime, the
`√n` bound halved the circuit (12,083 → 6,155 AND gates) and the proof that the factorization is
unique (50 s → 23 s). See §7 for why the time to *find* the factors did not simply improve too.

Propagation runs to a fixpoint with an iteration cap. The cap matters: on something like
`x < y and y < x`, bounds would otherwise shrink by one per round, which takes time exponential
in the bit width. The cap keeps compile time polynomial.

**Step 2: exact sizing.** Interval arithmetic flows forward through every expression. Each
intermediate value gets exactly the bits its range needs, in two's complement when it can be
negative. For example, `x ∈ [2, 2^39)` and `y ∈ [2, 2^39)` give `x*y ∈ [4, 2^78)`, so the
product gets 78 bits. Overflow cannot occur by construction. The bit widths of
constants grow with their number of digits, so this sizing preserves polynomiality.

**Step 3: the unbounded fallback.** No compiler can always infer finite bounds that keep every
solution. By Hilbert's tenth problem (Matiyasevich, 1970), deciding whether an arbitrary
polynomial equation has an integer solution is *undecidable*. When an unknown is still
unbounded after step 1 (e.g. `require x > y` and nothing else), RedSAT:

1. emits a **warning** naming the variable and suggesting an explicit bound;
2. uses **iterative deepening**: it solves with a 32-bit magnitude, and on UNSAT doubles the
   bound and retries, up to `--max-int-bits` (default 256);
3. reports results honestly. Any solution found is a **genuine** solution in ℤ, because all
   arithmetic is exact. "No solution" is printed as *"no solution with |x| < 2^256"* unless
   every unknown was bounded, in which case it is a proof.

### 4.6 Solving, projection and iterating solutions

```text
solve [all | unique] <vars> [limit K] [minimize <expr> | maximize <expr>] [{ body }] [else { body }]
```

- **`<vars>`, the projection set.** Two solutions are different if and only if they differ on
  at least one listed variable. All other unknowns are existentially quantified: they only have
  to exist. Enumeration therefore adds *blocking clauses over the listed variables only*
  (projected model enumeration), so the same answer is never shown twice merely because an
  auxiliary variable took a different value. A bare `solve` means "all top-level variables".
- **Listed variables may be derived.** `solve x + y` and `solve biggest` are fine. The solver
  then enumerates distinct values of those expressions.
- **The body** runs once per solution. Inside it the listed variables are bound to *concrete*
  values, so it is ordinary computation: printing, formatting, arithmetic. Without a body,
  RedSAT prints the listed variables. The `else` body runs when there is no solution.
- **Modes:**
  - default: the first solution found (`limit K` asks for the first `K` instead);
  - `all`: every solution, cut off by `limit K` if given;
  - `unique`: one solution plus a proof that no other exists (a second SAT call);
  - `minimize` / `maximize`: an optimal solution, found by **binary search**. The search runs
    between the best value found so far and the objective's bound from domain inference. Each
    probe ("is there one with objective ≤ m?") is a temporary *assumption*, so an
    unsatisfiable probe leaves no trace. A 40-bit objective needs about 40 SAT calls. The
    objective is then fixed at its optimum, so `solve all … minimize e` lists **every** optimal
    solution.
- **Self-check.** Every decoded model is re-evaluated against all constraints using the
  language's own semantics before it is reported. A mismatch is reported as an internal
  compiler error, never silently as an answer.
- **Interactive iteration.** `redsat prog.rsat --interactive` pauses after each solution of
  every `solve` (including plain ones): Enter shows the next solution, `a` shows all remaining
  ones, and `q` moves on.
- **Don't-cares.** If a listed variable is not constrained at all (no requirement or
  objective mentions it), the solver picks an arbitrary value, and RedSAT warns that it is
  *unconstrained*.
- **Several `solve` statements.** Constraints accumulate: each `solve` sees *everything*
  required so far, not just the requirements written since the previous `solve`. Concrete code
  between solves can therefore drive the solver, e.g. a puzzle generator that removes clues as
  long as the solution stays `unique`. The flip side: a file holding two unrelated problems
  will see the first problem's constraints while solving the second. A contradiction in the
  first then makes the second report `no solution`.

### 4.7 Explaining "no solution"

When a `solve` without an `else` block has no solution, RedSAT explains which requirements
contradict each other:

```text
no solution
note: no solution: these requirements cannot all hold together
 --> shop.rsat:5:9
  |
5 | require x + y == 12
  |         ^^^^^^^^^^^
 ::: shop.rsat:7:9
  |
7 | require x > 9
  |         ^^^^^
 ::: shop.rsat:8:9
  |
8 | require y > 5
  |         ^^^^^
```

How it works:
- Each source location that contributed requirements gets a *selector literal* `s`, and its
  clauses are guarded as `¬s ∨ φ`.
- The solver runs with every selector assumed. Its unsat core (the failed assumptions) names
  the locations involved.
- Deletion-based minimisation then drops every location the contradiction does not need.
- The explanation uses each unknown's *declared* domain, not the inferred one: inferred
  domains already encode the requirements, so dropping a requirement would not relax them.
- Explaining repeats the proof a few times. It is skipped when the original proof took more
  than 10 s, and minimisation stops after 5 s.
- A "no solution" that holds only within an iterative-deepening bound is not explained,
  because it is not a proof.

### 4.8 Functions, imports and the standard library

**Functions are inlined.** A call elaborates the function's body with the arguments bound to
its parameters. Known arguments are computed directly; unknown ones flow into the circuit.
Multi-statement bodies, local variables, loops and early `return`s under unknown conditions
work. They are merged through the same guards as a symbolic `if` (§4.3). Recursion is allowed
when it stops on values known at compile time (`gcd(84, 36)`); calls nested more than 200 deep
are an error that points back through the call chain.

**Imports.** `import "path.rsat"` brings in another file's functions, enums and structs. Paths
are relative to the importing file, imports are transitive, and each file is loaded once. An
imported file may only *declare* things: running statements there would make the meaning of
`import` depend on the order of imports. All files share one offset space, so every
diagnostic shows the right file.

**The standard library is written in RedSAT.** `prelude.rsat` is loaded before every program.
It holds helpers such as `digits`, `sorted` (an odd–even transposition network, which works on
unknowns), `lex_less`, `exactly`, `is_permutation` and string helpers. Only what needs the
compiler's help stays native: `sum`, `count`, `all_different`, `len`, `min`/`max`, `ord`/`chr`
and the like. Your own function with a standard-library name shadows it, so adding to the
library never breaks a program.

**No macros.** A macro system earns its keep where functions cannot reach: generating code
from compile-time values, avoiding call overhead, or abstracting over syntax. RedSAT functions
already do the first two. They are expanded at compile time, run on compile-time values like
any macro, and leave no call at run time. Inlining also makes them hygienic for free: a
function's locals cannot capture the caller's names. The remaining case, new syntax, would cost
error messages and tooling (the language server and the playground both understand functions)
for little gain. So there are no macros for now; if a pattern keeps appearing that functions
cannot express, it should become a language feature or a library function instead.

## 5. Compiler architecture

```text
 source.rsat
   │  lexer + parser
   ▼
 AST ──── name resolution, type inference, concrete/symbolic analysis
   │  elaboration: evaluate concrete code, unroll loops, inline functions, SSA
   ▼
 word-level IR (ints, bools, arrays, guarded constraints)
   │  domain inference & exact sizing (§4.5), constant folding, CSE, encoding selection
   ▼  bit-blasting
 Boolean circuit: And-Inverter Graph (hash-consed, constant-folded)
   │  Tseitin / Plaisted–Greenbaum encoding (linear size)
   ▼
 CNF ──▶ solver backend (CaDiCaL · Kissat · external DIMACS solver · our own, later)
   │
   ▼
 model ──▶ decode bits into program values ──▶ solve bodies / printing
```

### 5.1 Workspace layout

| Crate            | Responsibility                                                              | Phase |
|------------------|-----------------------------------------------------------------------------|-------|
| `redsat-sat`     | variables, literals, CNF, DIMACS I/O, the `Solver` trait, solver backends   | 0     |
| `redsat-circuit` | And-Inverter Graph, CNF encoder, exact bit-vector gadgets, circuit solving  | 0     |
| `redsat-lang`    | lexer, parser, AST, inference, elaboration, word-level IR, bit-blasting, the standard library prelude, loading imports | 1 |
| `redsat-lsp`     | the language server (`redsat lsp`)                                          | 2     |
| `redsat-cli`     | the `redsat` command-line tool and REPL                                     | 0+    |
| `redsat-wasm`    | the compiler as WebAssembly (wasm-bindgen) for the web playground; bridges JavaScript solvers | 8 |

Dependencies point strictly downwards (`cli → lsp → lang → circuit → sat`, `wasm → lang`). Only
`redsat-sat` touches native code, and only behind its `cadical` feature, so `redsat-lang`
builds for `wasm32-unknown-unknown` unchanged (timing goes through `web-time`).

### 5.2 Boolean circuits: And-Inverter Graphs

Every Boolean function is represented using only 2-input AND gates and *complemented edges*
(inverters are free). OR, XOR, ITE and the rest are derived. Construction is:

- **hash-consed** ("structural hashing"): `and(a, b)` is created at most once per operand pair;
- **locally simplified**: `a∧0 = 0`, `a∧1 = a`, `a∧a = a`, `a∧¬a = 0`.

As a result, constant propagation through arithmetic happens for free. A multiplier fed a
constant operand collapses to adders of only the relevant rows.

### 5.3 CNF encoding: Tseitin with Plaisted–Greenbaum polarity

Naive CNF conversion (distributing ∨ over ∧) is exponential. Tseitin's transformation gives
each gate a fresh variable, which keeps the CNF linear in circuit size and equisatisfiable.
On top of that:

- **Polarity awareness (Plaisted–Greenbaum).** A gate that is only ever required to be true
  needs only the clauses `v → (a ∧ b)`; one only required false needs only `(a ∧ b) → v`.
  This roughly halves the clause count.
- **Lazy, incremental emission.** The encoder remembers which directions it has emitted per
  gate and adds missing ones on demand. This matters for enumeration, optimisation and
  assumptions, which ask for exact literals later on.
- **Top-level flattening.** Required conjunctions are split into separate constraints, and
  required disjunctions become single clauses, without auxiliary variables.
- **Decoding.** Models are decoded by reading the circuit *inputs* and re-evaluating the AIG.
  The values of internal gate variables are not trusted, since with one-sided encodings they
  need not match their gate's function.

### 5.4 Bit-vector gadgets

Bit-vectors are little-endian vectors of AIG edges. Every gadget is polynomial, so the whole
reduction is polynomial in (program size × unrolled size × bit widths):

| Operation       | Circuit                                                  | Size     |
|-----------------|----------------------------------------------------------|----------|
| `a + b`         | ripple-carry chain of full adders, carry kept (exact)    | O(n)     |
| `a - b`         | add the two's complement                                 | O(n)     |
| `a * b`         | shift-and-add array multiplier, full product (exact)     | O(n·m)   |
| `==`, `<`, `<=` | XNOR conjunction / borrow chain comparator               | O(n)     |
| `c ? a : b`     | bitwise multiplexer                                      | O(n)     |
| `/`, `%`        | relational: fresh `q, r` with `q·b + r = a`, `0 ≤ r < b` | O(n·m)   |

### 5.5 Encodings, measured

Integers use binary (two's complement) encoding with exact, interval-derived widths. Each
choice below was **measured** with the benchmark suite in `bench/` (`bench/run.sh`), not assumed:

- **`all_different`** is a dedicated IR term (it folds constants and detects repeated operands),
  encoded as pairwise inequalities. Adding a *one-hot view* (an indicator `x == v` per value,
  with at-most-one and, for permutations, at-least-one per value) made quasigroup completion
  and Sudoku 2–4× slower with CaDiCaL. It helped 10-Queens and 5×5 magic squares by up to 2×.
  The pigeonhole-only variant (at-least-one for permutations) was neutral, within noise. Plain
  pairwise inequality was kept.
- **`sum` and `count`** build balanced addition trees, so intermediate widths grow with the
  logarithm of the number of terms.
- **Counting constraints.** A count compared with a constant (`count(…) <= k`, `exactly`,
  `at_least`, `at_most`) is encoded with a *totalizer* (Bailleux and Boufkhad, 2003), whose
  unary outputs `count ≥ j` are truncated at `k + 1`, instead of an adder and a comparator.
  Unit propagation then enforces the bound on its own (arc consistency). On N-Queens over a
  30×30 Boolean board the CNF shrank from 29,396 variables and 84,944 clauses to 4,148 and 9,978,
  and solving went from 398 ms to 40 ms. A 14-nurse roster went from 414 ms to 277 ms. Bounds
  that are trivially true or false are decided outright.
- **Weighted sums** compared with a constant (`sum(w[i] for i … if b[i]) <= K`) use the same
  machinery generalized to weights (GTE: Joshi, Martins and Manquinho, CP 2015), whose nodes
  keep one output per achievable partial sum. Counts are the unit-weight case and produce
  exactly the totalizer. Weighted sums get it only while it stays within 8× the binary
  encoding's size, because its size grows with the number of distinct partial sums. Measured:
  on the 30- and 50-item knapsacks (weights up to 54, capacity 300) it made the CNF 40–80×
  larger and solving 7–27× slower, the "large weights favor binary" effect reported by
  Ulrich-Oltean, Nightingale and Walker (Constraints 2023), so they keep the binary sum. The
  small knapsack example's CNF shrinks from 408 to 87 clauses.
- **Division** by a constant power of two is a bit slice; other divisions are relational.
- **Two-level AIG rewriting.** Creating `a ∧ b` applies the local rules of Brummayer and
  Biere ("Local Two-Level And-Inverter Graph Minimization without Blowup", 2006):
  contradiction, idempotence, subsumption, substitution and resolution across two levels. CNF
  sizes shrink by 2–7%; the effect on solve times is real but uneven (best of 5 runs, one
  thread):

  | Benchmark | Before | After |
  |---|---|---|
  | factoring a 36-bit semiprime | 7.97 s | 4.16 s |
  | five 32–34-bit semiprimes (total) | 11.6 s | 4.2 s (one instance 7× slower, two 14–22× faster) |
  | 4×4 magic square | 0.07 s | 0.03 s |
  | 30-item knapsack | 0.24 s | 0.30 s |
  | all 10-queens solutions | 1.29 s | 1.51 s |

  The gains come from *substitution* (`¬(x ∧ y) ∧ x = x ∧ ¬y`), which simplifies the partial
  products of multipliers; without it the rules were neutral. The rules are kept for the large
  wins on arithmetic. The portfolio (`--threads`) also evens out such per-instance variance.
- **Comparisons decided by ranges** (`x >= 0` for an `x` in `0..=35`) fold to constants before
  any circuit is built, so entailed requirements cost nothing (25% fewer clauses on the
  scheduling example).
- **CaDiCaL options.** Options are fixed when a solver is created. Measured on the whole suite
  (`node bench/ab.mjs`, medians of 5 runs, one thread; differences under ±5% overall are noise,
  single programs can swing ±35% between identical builds):
  - **Kept: incremental lazy backtracking (`ilb=2`, from IntelSAT, Nadel 2022)**, which keeps the
    trail between calls. 7% faster overall; 10-queens enumeration (725 calls) 1.9×, quasigroup
    completion 1.3×. On by default, in the browser too.
  - **In one portfolio member: bounded variable addition (`factor=1`, the core of SBVA, SAT
    Competition 2023).** Too erratic for a default (38% slower overall, 24× slower to 9× faster
    on factoring, 4× faster on a 50-item knapsack), which is exactly what a race exploits.
  - **Rejected:** no elimination (`elim=0`, slower everywhere) and no lucky phases (`lucky=0`,
    enumeration 30% faster but factoring 2–6× slower).
- **Optimization strategies, measured and rejected.** Bit-level objective search (Nadel and
  Ryvchin, TACAS 2016), which fixes the objective's bits most significant first under
  assumptions, was 11% slower than our binary search (45% on Golomb-7); the published gains rely on
  a branching preference for the objective bits that CaDiCaL does not offer. Keeping each proven
  bound as a clause was neutral: CaDiCaL learns it from the failed assumption anyway.

`--stats` splits each `solve` into *build* time (domains, circuit, CNF) and *search* time (SAT
calls, decoding). On the largest benchmark (quasigroup completion of order 20: 123k AND gates)
build takes 60 ms and search 530 ms. The SAT solver dominates, as it should.

Automatic symmetry breaking remains an open experiment.

### 5.6 Solver backends

The `Solver` trait follows the IPASIR conventions: add clauses, solve under assumptions, read
the model, and query failed assumptions.

| Backend          | Status    | Notes                                                       |
|------------------|-----------|-------------------------------------------------------------|
| CaDiCaL 2.2      | ✅ Phase 0 | default; incremental; via the `rustsat-cadical` crate      |
| Built-in CDCL    | ✅ Phase 7 | pure Rust (watched literals, VSIDS, first-UIP, LBD, Luby restarts, assumptions); `--solver builtin` |
| Portfolio        | ✅ Phase 9 | `--threads N`: N diversified members race on hard calls (see below) |
| Browser solvers  | ✅ Phase 8 | CaDiCaL (and MiniSat) compiled with Emscripten into separate WebAssembly modules; `redsat-wasm` calls them through a JavaScript `Solver` |
| Kissat           | later     | often fastest one-shot; not incremental                     |
| External process | later     | any competition solver through a DIMACS file                |

The built-in solver is much slower than CaDiCaL on hard instances (factoring a 33-bit
semiprime: 13 s against 0.04 s). Typical CDCL tuning (initial phase, a Glucose-style VSIDS decay
ramp) did not close the gap, which comes from CaDiCaL's inprocessing, rephasing and
stabilisation. In the browser, the answer is therefore to offer CaDiCaL itself, compiled to
WebAssembly, rather than to tune the built-in solver.

**Multithreading: an adaptive portfolio.** CaDiCaL and MiniSat are sequential. Truly parallel
solvers (gimsatul, Painless, Mallob) share clauses between threads, but none supports the
incremental interface (clauses that accumulate, assumptions, failed assumptions) that
enumeration, optimization and explanations rely on. So RedSAT parallelises the way portfolio
solvers such as ManySAT do: N differently configured members race and the first answer wins.
The members vary CaDiCaL's preset (`sat`/`unsat`), seed, initial phase and variable shuffling.
Races are rare by design: the first member solves alone, on the calling thread, and the
helpers join only when a call is still running after 25 ms. The helpers are persistent threads
(created on the first race, from a log of the formula) that receive every clause in order. A
watchdog thread times calls, starts races and keeps stopping the losers until they report,
because a stop that reaches a search before it begins may be ignored. Measured on 8 cores:

| Program | 1 thread | 4 threads |
|---|---|---|
| factoring a 36-bit semiprime | 7.84 s | 0.20 s |
| all 724 solutions of 10-queens (724 quick calls) | 1.18 s | 1.26 s |
| 30-item knapsack (binary-search optimization) | 0.26 s | 0.30 s |

On heavy-tailed instances, one of the members is usually lucky; on easy calls the cost is a
message to the watchdog. With several threads, which of several valid solutions is found can
vary from run to run (`-j 1` is reproducible).

## 6. Tooling

### 6.1 Command line

Running a program is the default action, so there is no `run` subcommand:

```text
redsat prog.rsat [options]           # compile and run the program
redsat check prog.rsat [options]     # parse and type-check only: quick feedback for editors and CI
redsat repl                          # build a program one statement at a time
redsat lsp                           # the language server (used by editors)
redsat dimacs problem.cnf            # solve a raw DIMACS file (exit code 10 = SAT, 20 = UNSAT)

options:
  --strict                           # enable every strict check (§6.3)
  --explicit-types <off|unknowns|all>
  --unbounded-ints <deepen|error>
  --max-int-bits <N>
  --config <redsat.toml>             # instead of searching for one
  --stats                            # per solve: domains, bit widths, circuit and CNF sizes, time
  -v, --verbose                      # progress as it happens: files loaded, each solve's unknowns,
                                     # decision bits and search space, circuit and CNF sizes
                                     # (auxiliary variables included), SAT calls, timings, summary
  --interactive                      # pause after each solution: next / all / quit
  --solver <cadical|builtin>         # the SAT solver
```

`--verbose` reports through a hook (`Hooks::trace`) rather than printing, so the playground
shows the same lines.

Later: `--emit ast|ir|aig|cnf`.

### 6.2 VS Code support

Programs should be pleasant to read and write in an editor. The extension lives in
`editors/vscode/` and grows in two steps:

1. **Highlighting, snippets and Markdown (Phase 1).** A declarative extension that provides:
   - a TextMate grammar (`syntaxes/rsat.tmLanguage.json`) covering keywords, types, numeric and
     string literals with `{…}` interpolation, comments, operators and ranges;
   - **Markdown support**: fenced code blocks tagged ```` ```rsat ```` (or ```` ```redsat ````) are
     highlighted in any Markdown file, including this document, through a grammar injected into
     VS Code's Markdown grammar;
   - snippets with tab stops for the common shapes (`var`, `require`, `solve`, `solve all`,
     `for`, `if`, `fn`, …), a first taste of autocomplete;
   - `language-configuration.json` with comment toggling, bracket matching, auto-closing pairs
     and indentation rules.

   A unit test in `redsat-lang` checks that the grammar's keyword lists match the lexer's
   keyword table, so the two cannot drift.
2. **Language server (Phase 2).** `redsat lsp` (crate `redsat-lsp`) speaks the Language Server
   Protocol over stdio. The extension's client starts it automatically.

   **What it does.** On every change, the document goes through
   [`redsat_lang::analyze`](../crates/redsat-lang/src/lib.rs): it is parsed, type-checked and
   elaborated exactly as a run would elaborate it. At each `solve`, analysis stops after domain
   inference and never calls the SAT solver. Elaboration gives up after 400 ms, so heavy
   programs keep the editor responsive. This powers:
   - **live diagnostics**: syntax, type and strict-mode errors, plus errors found while
     elaborating, such as an out-of-range index or a loop bound that depends on unknowns;
   - **autocomplete**:
     - variables visible at the cursor, functions, enums, structs and built-ins;
     - types after `:`, enum variants after `Color.`, fields after `p.`;
     - `solve` modifiers;
     - while the text is broken mid-edit, completion keeps using the symbols of the last
       version that type-checked;
   - **hovers** with the declaration and the unknown's **inferred domain and bit width**
     (`unknown · inferred domain [34, 130158467] · 27 bits`);
   - **inlay hints** with the same information next to every unknown declaration;
   - **go-to-definition** and the **document outline**.

   **Where the server runs.** The extension runs on the workspace side: in a WSL remote window
   it runs inside WSL, next to the Linux `redsat` binary. In a local Windows window it starts
   the server through `wsl.exe` when no Windows `redsat` is on the PATH. URIs that come through
   that bridge (`file:///c%3A/…`, `file://wsl.localhost/…`) are mapped to Linux paths so that
   `redsat.toml` is still found.

   Later: a code lens with the CNF size of each `solve`.

### 6.3 Compiler options: `redsat.toml`

Like `tsconfig.json` in TypeScript, an optional `redsat.toml` configures the compiler for every
program in its directory and below. The compiler looks in the program's directory, then its
parent directories, and uses the first file it finds (or the one given with `--config`).

```toml
[compiler]
strict = true                # turn on the whole strict family below
explicit-types = "unknowns"  # "off" | "unknowns" | "all"     (default "off"; strict: "unknowns")
unbounded-ints = "error"     # "deepen" | "error"             (default "deepen"; strict: "error")
fallback-int-bits = 32       # first magnitude tried for unbounded integers (§4.5)
max-int-bits = 256           # stop deepening beyond this
```

Precedence, lowest to highest: built-in defaults, then what `strict` implies, then keys set
explicitly in `redsat.toml`, then command-line flags. This means a strict project can still opt
out of one check by setting its key explicitly.

| Option              | What it enforces                                                                 |
|---------------------|----------------------------------------------------------------------------------|
| `explicit-types`    | `"unknowns"`: every unknown (`var` without an initializer) needs a type, like TypeScript's `noImplicitAny`. `"all"`: every `var` needs one, even `var n: int = 42`. |
| `unbounded-ints`    | `"error"`: an integer unknown whose domain cannot be inferred is a compile error, instead of being solved with iterative deepening (§4.5). |

```rsat
// with explicit-types = "unknowns"
var x                   // error: `x` needs a type annotation, e.g. `var x: int`
var y: int              // ok
var n = 42              // ok: the initializer determines the type
```

### 6.4 The web playground

`playground/` is a static site: GitHub Pages hosts it, and no server is involved. Programs run in
the visitor's browser.

- **Compiler.** `redsat-wasm` (Rust → WebAssembly) runs in a Web Worker. Output and `--verbose`
  lines stream back line by line. *Stop* terminates the worker, because a running solve cannot
  be interrupted from inside. A second worker analyses the program as you type, for
  diagnostics, inferred domains, symbols, structs and enums.
- **Solvers.** The built-in CDCL solver is part of the compiler module. CaDiCaL (and MiniSat)
  are separate Emscripten modules, loaded on first use. They are single-threaded, because
  GitHub Pages cannot send the cross-origin isolation headers that WebAssembly threads need.
  Rust calls them synchronously through an IPASIR-like JavaScript object: `add(clauses)`,
  `solve(assumptions)`, `value(lit)`, `failed(lit)` and `release()`.
- **Parallelism without threads.** Single-threaded modules lose nothing per solver, because
  CaDiCaL is sequential natively too. The cost that remains is WebAssembly against native
  code, typically 1.3–2×. Parallelism comes from a *portfolio* instead: *Race* runs the program
  in one Web Worker per solver and keeps the first to finish. Workers are separate threads that
  need no shared memory or special headers. Solvers that share clauses across threads would
  need cross-origin isolation, which a service worker (`coi-serviceworker`) can provide on
  static hosting if one is ever worth shipping.
- **Editor.** CodeMirror 6, with highlighting, completion and hover docs built from the
  compiler's own vocabulary (`vocabulary()`), so they cannot drift from the language. It adds
  snippets, signature help, member completion (struct fields, enum variants) and
  go-to-definition.
- **Documents.** The examples are generated from `examples/` at build time. Your own files and
  your edits live in `localStorage`. Share links carry the program, compressed, in the URL
  fragment, which never reaches a server.
- **Phones.** Below 760 px the layout changes: Code and Output become tabs, Run moves to a
  bottom bar with a row of symbol keys above the on-screen keyboard, and the app is sized to
  the visual viewport, so the keyboard shrinks the editor instead of covering it.

## 7. Testing strategy

- **Differential testing of every operator.** Each operator has a concrete semantics (used by
  the compiler to evaluate concrete code) and a circuit. For small widths, exhaustive tests
  check that the two agree on every input.
- **Encoder equivalence.** For random small circuits and every input assignment, check that
  "CNF + inputs fixed" is satisfiable exactly when the circuit evaluates to true.
- **Round trips.** DIMACS write → parse → identical formula.
- **Golden end-to-end tests.** Every program in `crates/redsat-lang/tests/programs/` (errors,
  features, strict mode with its own `redsat.toml`, imports) and every file in `examples/` is
  run, and its output plus rendered diagnostics are compared with a checked-in `.out` file (for
  test programs) or with the `// Output:` comment block at the bottom of the example (so readers
  see what each example prints). After a reviewed change in behaviour,
  `REDSAT_BLESS=1 cargo test -p redsat-lang --test programs` rewrites the expectations.
- **Benchmarks (Phase 5).** Encoding size and solve time, compared against MiniZinc and Z3 on
  shared problems. Solve times on *satisfiable* instances are heavy-tailed and sensitive to the
  exact encoding. In the factoring experiment of §4.5, the smaller circuit proved uniqueness
  twice as fast, yet took 34 s instead of 2.5 s to *find* the factors. Benchmarks must therefore
  use many instances and solver seeds, and report distributions rather than single runs.

## 8. Roadmap

| Phase | Scope                                                                                         | Milestone                       |
|-------|-----------------------------------------------------------------------------------------------|---------------------------------|
| 0 ✅  | AIG, Tseitin/PG encoder, DIMACS I/O, `Solver` trait + CaDiCaL, exact unsigned gadgets          | factoring via hand-built circuit |
| 1 ✅  | lexer, parser, type inference, `var`/`require`/`solve`/`print`, bool + int with inferred widths, signed gadgets, compile-time `for`/`if`, **symbolic `if`** (pulled forward), `solve all/unique/limit`, **`minimize`/`maximize`** (pulled forward), `redsat.toml` + strict mode, VS Code grammar + Markdown fences + snippets | factoring, triples, logic puzzles in `.rsat` |
| 2 ✅  | language server: live diagnostics, autocomplete, hovers and inlay hints with inferred domains and widths, definitions, outline; VS Code client | editor intelligence |
| 3 ✅  | arrays (unknown indices too), functions with early returns, comprehensions, built-ins (`all_different`, `sum`, `count`, …), `in`, `if` expressions, bounded `while` | N-Queens, cryptarithms, Sudoku, magic squares |
| 4 ✅  | `/` `%` on unknowns, bitwise ops, chars, strings with unknown lengths, enums, structs           | palindromes, graph colouring, scheduling |
| 5 ✅  | interactive mode, explanations of "no solution" (minimised unsat cores), binary-search optimization | debuggable models               |
| 6     | optimiser: encoding selection, symmetry breaking, CSE; benchmark suite                           | performance                     |
| 7 ✅  | REPL, built-in CDCL solver, `?` holes (knowns and unknowns side by side)                         | stretch                         |
| 8 ✅  | standard library in RedSAT, `import`, `--verbose`, the web playground (compiler and solvers as WebAssembly), the RedSAT Guide with tested samples | usable by others |

## 9. Open questions

- Syntax bikeshedding: word operators (`and`, `or`, `not`) versus symbols (`&&`, `||`, `!`);
  where `solve` modifiers go; statement terminators (currently newline-terminated, with newlines
  ignored inside brackets).
- The default starting magnitude and the cap for the unbounded fallback (§4.5).
- Fixed-point or rational numbers? Bit-precise IEEE-754 floats are possible but heavy. Out of
  scope for v0.
- Sets and relations as first-class types (Alloy-style)?

## 10. Prior art we learn from

MiniZinc (separating parameters from decision variables) · Essence/Conjure + Savile Row (models
compiled to SAT) · Alloy/Kodkod (bounded relational logic → SAT) · clingo/ASP (grounding, then
solving) · Rosette (solver-aided programming) · CBMC (bounded model checking of C via SAT) ·
Z3/Bitwuzla (bit-vector theories via bit-blasting) · ABC (And-Inverter Graphs).
