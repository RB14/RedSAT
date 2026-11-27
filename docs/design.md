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

### 3.1 Factoring

```rsat
// Find the two prime factors of a semiprime.
var n = 1000036000099          // = 1000003 × 1000033, known at compile time
var x, y                       // unknowns: types and bit widths are inferred

require x > 1 and y > 1        // rule out the trivial 1 × n
require x <= y                 // symmetry breaking: halves the search space
require x * y == n

solve x, y {
    print "{n} = {x} × {y}"
}
```

Nothing here mentions bits. From the constraints the compiler infers `x, y ∈ [2, n/2]`
(39 bits each), sizes `x * y` at 78 bits so that it cannot overflow, and compares it against
the 40-bit constant `n`.

### 3.2 Cryptarithm: SEND + MORE = MONEY

```rsat
var s, e, n, d, m, o, r, y: int[0..=9]

require all_different([s, e, n, d, m, o, r, y])
require s != 0 and m != 0
require (            1000*s + 100*e + 10*n + d
         +           1000*m + 100*o + 10*r + e
         == 10000*m + 1000*o + 100*n + 10*e + y )

solve s, e, n, d, m, o, r, y     // no body: prints the listed variables
```

### 3.3 N-Queens: compile-time parameters and loops

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

```rsat
var weight   = [12, 2, 1, 1,  4]
var value    = [ 4, 2, 1, 2, 10]
var capacity = 15
var take: [bool; len(weight)]

require sum(weight[i] for i in 0..len(weight) if take[i]) <= capacity

solve take maximize sum(value[i] for i in 0..len(weight) if take[i])
```

The `if take[i]` filter is *symbolic*: it compiles to `take[i] ? weight[i] : 0` inside the sum.

### 3.5 Symbolic control flow and mutation

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

```rsat
var total: int[1..=50]
var a, b                    // auxiliary witnesses
require a * a + b * b == total

solve all total             // 1, 2, 4, 5, 8, 9, 10, 13, … each exactly once,
                            // although e.g. 25 = 0² + 5² = 3² + 4² = (−3)² + 4² = …
```

### 3.7 Strings (later phase, a teaser)

```rsat
var word: string
require len(word) == 5
require all(c in 'a'..='z' for c in word)
require reverse(word) == word           // palindromes

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
both `int`). Ambiguous cases are compile errors asking for an annotation.

### 4.3 Control flow

- **`if` on a concrete condition:** only the taken branch is compiled.
- **`if` on a symbolic condition:** both branches are compiled. Every variable assigned in
  either branch is merged with a multiplexer (`c ? then_value : else_value`), and a `require φ`
  inside the branch becomes the guarded constraint `c implies φ`.
- **`for i in lo..hi`:** `lo` and `hi` are concrete, the loop is fully unrolled, and `i` is
  concrete in each copy of the body. This is the language's main *metaprogramming* tool.
- **Bounded `while` (later):** `while cond bound K { … }` with a symbolic `cond` unrolls `K`
  guarded iterations and requires that the loop has exited by then (as in bounded model checking).
- **Functions** (`fn name(params) -> T { … }`) are inlined at every call. Recursion is allowed
  only when it terminates on concrete arguments; the compiler enforces an evaluation budget.

### 4.4 Types

| Type                              | Meaning                                                          | Encoding                                   |
|-----------------------------------|------------------------------------------------------------------|--------------------------------------------|
| `bool`                            | truth value                                                      | one SAT variable                           |
| `int`                             | mathematical integer; domain inferred (§4.5)                     | two's-complement bit-vector sized to fit   |
| `int[lo..hi]`, `int[lo..=hi]`     | integer with an explicit domain                                  | same, sized to the domain                  |
| `int<N>`, `uint<N>`               | explicit bit bound: `[−2^(N−1), 2^(N−1))`, `[0, 2^N)`            | N bits                                     |
| `char`                            | Unicode scalar; a subtype of `int[0..=0x10FFFF]`                 | narrowed by inference (e.g. to 7 bits)     |
| `string`                          | bounded sequence of `char` plus a symbolic length                | array of chars + length                    |
| `[T; N]`                          | fixed-size array (`N` concrete)                                  | N encoded elements                         |
| `enum E { A, B, C }`              | finite enumeration                                               | small integer or one-hot                   |
| `struct S { … }`                  | record                                                           | its fields                                 |

Explicit bounds (`int[…]`, `int<N>`, `uint<N>`) **always take precedence** over inference:
inference may narrow inside them but never widen them. If the constraints need values outside
an explicit bound, the program correctly has no solution.

Operators (strawman): arithmetic `+ - * / % ^`(power with concrete exponent); comparison
`== != < <= > >=`; logic `and or not xor implies iff`; bitwise on non-negative integers
`& | ~ << >>`; ranges `a..b` (exclusive) and `a..=b` (inclusive); comprehensions
`sum(e for i in r if c)`, `all(…)`, `any(…)`, `count(…)`.

Division and modulo are defined mathematically (floor semantics, `x % m ∈ [0, |m|)`) and are
encoded *relationally*: `q = a / b` becomes fresh unknowns with `q*b + r == a, 0 <= r < |b|`,
so no division circuit is ever needed.

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
  - default: the first solution found;
  - `all`: every solution, cut off by `limit K` if given;
  - `unique`: one solution plus a proof that no other exists (a second SAT call);
  - `minimize` / `maximize`: an optimal solution, found with incremental SAT by tightening the
    bound on the objective until the problem becomes UNSAT.
- **Interactive iteration.** `redsat run prog.rsat --interactive` pauses after each solution:
  `[n]ext · [a]ll · [q]uit`.
- **Don't-cares.** If a listed variable is not constrained at all, the solver picks an
  arbitrary value; RedSAT flags it as *unconstrained* in the default output.
- **Several `solve` statements (later).** Constraints accumulate; each `solve` sees everything
  required so far and reuses the solver incrementally. This turns the concrete level into a
  scripting language that can drive the solver, e.g. a puzzle generator that removes clues as
  long as the solution stays `unique`.

### 4.7 Explaining "no solution"

Each top-level `require` gets its own *selector literal* `sᵢ`, and its clauses are guarded as
`¬sᵢ ∨ φᵢ`. The solver runs under the assumptions `s₁ … sₖ`. On UNSAT it reports which
assumptions it needed (the unsat core), and these map straight back to source spans:

```text
error: no solution — these constraints conflict:
  --> shop.rsat:12   require total <= budget
  --> shop.rsat:17   require count(take) >= 4
```

The core is shrunk further by deletion-based minimisation so explanations stay short.

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
| `redsat-lang`    | lexer, parser, AST, inference, elaboration, word-level IR, bit-blasting     | 1+    |
| `redsat-cli`     | the `redsat` command-line tool                                              | 0+    |

Dependencies point strictly downwards (`cli → lang → circuit → sat`). Only `redsat-sat` touches
native code, and only behind its `cadical` feature.

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

### 5.5 Integer encodings (optimisation, later phases)

Binary (log) encoding is the default. Small domains that are used mostly with `==`/`!=`
benefit from **one-hot** (direct) encoding; domains used mostly with `<` benefit from **order**
(unary) encoding. Cardinality constraints (`count(…) <= k`) use sequential counters or
totalizers. The compiler chooses per variable from its domain size and usage. Encoding choice
often matters more for solve time than the choice of solver.

### 5.6 Solver backends

The `Solver` trait follows the IPASIR conventions: add clauses, solve under assumptions, read
the model, and query failed assumptions.

| Backend          | Status    | Notes                                                       |
|------------------|-----------|-------------------------------------------------------------|
| CaDiCaL 2.2      | ✅ Phase 0 | default; incremental; via the `rustsat-cadical` crate      |
| Kissat           | later     | often fastest one-shot; not incremental                     |
| External process | later     | any competition solver through a DIMACS file                |
| Built-in CDCL    | stretch   | educational; same trait, so it is a drop-in                 |

## 6. Tooling

### 6.1 Command line

```text
redsat run prog.rsat [--interactive] [--solver cadical] [--max-int-bits 256]
redsat emit prog.rsat --stage ast|ir|aig|cnf|stats
redsat dimacs problem.cnf          # solve a raw DIMACS file (available since Phase 0 ✅)
```

### 6.2 VS Code support

Programs should be pleasant to read in an editor. This is delivered in two steps:

1. **Syntax highlighting (Phase 1, shipped with the lexer).** A declarative VS Code extension
   in `editors/vscode/` containing:
   - a TextMate grammar (`syntaxes/rsat.tmLanguage.json`) covering keywords, types, numeric
     and string literals with `{…}` interpolation, comments, operators, and range literals;
   - `language-configuration.json` with comment toggling (`//`, `/* */`), bracket matching,
     auto-closing pairs and indentation rules;
   - the `.rsat` file association.

   It installs from a locally built `.vsix`. Because it is purely declarative it works both in
   local Windows windows and in WSL remote windows. A unit test in `redsat-lang` checks that the
   grammar's keyword list matches the lexer's keyword table, so the two cannot drift.
2. **Language server (later).** `redsat lsp` will provide live diagnostics (including UNSAT
   explanations), go-to-definition, and RedSAT-specific views. Examples are hovers and inlay
   hints showing each variable's **inferred domain and bit width**
   (`x: int[2..=500018000049] · 39 bits`), and a code lens with the CNF size of each `solve`.

## 7. Testing strategy

- **Differential testing of every operator.** Each operator has a concrete semantics (used by
  the compiler to evaluate concrete code) and a circuit. For small widths, exhaustive tests
  check that the two agree on every input.
- **Encoder equivalence.** For random small circuits and every input assignment, check that
  "CNF + inputs fixed" is satisfiable exactly when the circuit evaluates to true.
- **Round trips.** DIMACS write → parse → identical formula.
- **Golden end-to-end tests (Phase 1+).** `.rsat` programs paired with their expected output.
- **Benchmarks (Phase 5).** Encoding size and solve time, compared against MiniZinc and Z3 on
  shared problems. Solve times on *satisfiable* instances are heavy-tailed and sensitive to the
  exact encoding. In the factoring experiment of §4.5, the smaller circuit proved uniqueness
  twice as fast, yet took 34 s instead of 2.5 s to *find* the factors. Benchmarks must therefore
  use many instances and solver seeds, and report distributions rather than single runs.

## 8. Roadmap

| Phase | Scope                                                                                         | Milestone                       |
|-------|-----------------------------------------------------------------------------------------------|---------------------------------|
| 0 ✅  | AIG, Tseitin/PG encoder, DIMACS I/O, `Solver` trait + CaDiCaL, exact unsigned gadgets          | factoring via hand-built circuit |
| 1     | lexer, parser, `var`/`require`/`solve`/`print`, bool + int with inferred widths, signed gadgets, VS Code grammar | factoring in `.rsat`  |
| 2     | arrays, compile-time `for`/`if`, functions, comprehensions, stdlib (`all_different`, `sum`, `count`), cardinality encodings | N-Queens, cryptarithms, graph colouring |
| 3     | symbolic `if`/assignment merging, `/` `%`, bitwise ops, chars, strings, enums, structs          | string puzzles, scheduling      |
| 4     | `solve all/unique/limit/minimize/maximize`, interactive mode, UNSAT explanations, multiple `solve`s | knapsack, puzzle generation |
| 5     | optimiser: encoding selection, symmetry breaking, CSE; benchmark suite                           | performance                     |
| 6     | language server, REPL, built-in CDCL solver                                                     | stretch                         |

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
