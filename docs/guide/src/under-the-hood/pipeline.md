# How RedSAT works

You never need to know any of this to use RedSAT, but it helps to understand why some models
are fast and others slow. The full design, with the reasoning behind each decision, is in
`docs/design.md` in the repository.

## The pipeline

```text
 program.rsat
   │  lexer, parser
   ▼
 syntax tree ── names, type inference, strict-mode checks
   │  elaboration: run known code, unroll loops, inline calls
   ▼
 word-level constraints over integers and booleans
   │  at each solve: domain inference, exact sizing
   ▼  bit-blasting
 Boolean circuit (an And-Inverter Graph)
   │  Tseitin encoding
   ▼
 CNF formula ──▶ SAT solver (CaDiCaL, or the built-in solver)
   │
   ▼
 model ──▶ decode into program values ──▶ self-check ──▶ solve body
```

Every step takes polynomial time in the size of the program, the number of loop iterations and
inlined calls, and the bit widths involved. The hard part, the actual search, is left to the SAT
solver.

## 1. Parsing and type checking

The program, the files it imports and the standard library are parsed into one syntax tree. The
type checker infers the type of every variable and expression, and applies the
[strict-mode](../reference/configuration.md) checks. `redsat check` stops here.

## 2. Elaboration: running the known part

Next, RedSAT *runs* the program, with a twist: values may be unknown.

- Known values are computed right away, with arbitrary-precision integers.
- Loops over known ranges are unrolled, and function calls inlined.
- Each assignment creates a new version of a variable, so mutation needs no special handling.
- An `if` on an unknown condition runs both branches, and merges the variables they assign.
- Operations on unknowns are not computed but *recorded*, as terms like
  `x * y == 4295229443`, and each `require` on unknowns adds a constraint.

When a `solve` is reached, the recorded constraints are handed to the next stages.

## 3. Domain inference and exact sizing

To build a circuit, RedSAT needs to know how many bits every integer has. It works that out in
two steps:

**Narrowing the unknowns.** Every integer unknown starts with its declared range, or with
"any integer". RedSAT then propagates bounds through the requirements that always apply (those
not under an unknown condition), until nothing changes or an iteration cap is reached (the cap
keeps compile time polynomial). For example:

- `x > 1` gives `x ≥ 2`;
- `x * y == n` with `x, y ≥ 2` gives `x ≤ n / 2`;
- `a*a + b*b == total` with `total ≤ 50` gives `|a| ≤ 7`;
- `x <= y` together with `x * y == n` gives `x * x ≤ n`, so `x ≤ √n`.

**Sizing everything else.** From the unknowns' ranges, interval arithmetic gives every
intermediate result a range it is guaranteed to lie in, and so its exact number of bits, in
two's complement when it can be negative. With `x` and `y` below `2^27`, `x * y` gets 54 bits.
Overflow is impossible by construction.

`--stats`, `--verbose` and the editor's hovers show the inferred ranges.

## 4. Bit-blasting: building the circuit

Each word-level operation becomes a small circuit over the bits of its operands, built from
2-input AND gates and inverters (an *And-Inverter Graph*):

| Operation         | Circuit                                                     | Size    |
|-------------------|-------------------------------------------------------------|---------|
| `a + b`, `a - b`  | ripple-carry adder, carry kept                              | O(n)    |
| `a * b`           | shift-and-add multiplier, full product                      | O(n·m)  |
| `==`, `<`, `<=`   | comparators                                                 | O(n)    |
| `if c {a} else {b}` | a multiplexer per bit                                     | O(n)    |
| `a / b`, `a % b`  | a fresh quotient and remainder with `q·b + r == a`, `0 ≤ r < abs(b)`; powers of two are bit slices | O(n·m) |
| `xs[i]`, `i` unknown | a chain of multiplexers over the elements                | O(len·n) |
| `all_different`   | an inequality per pair of elements                          | O(len²·n) |
| `sum`, `count`    | a balanced tree of adders                                   | O(len·n) |

The graph is *hash-consed*: the same gate is never built twice. It is also simplified as it is
built (`a ∧ 0 = 0`, `a ∧ a = a`, …), so constants propagate for free: a multiplier with one
known operand shrinks to a few adders.

## 5. CNF encoding

SAT solvers read formulas in *conjunctive normal form* (CNF): a list of clauses, each an "or" of
literals. RedSAT translates the circuit with the *Tseitin* encoding, which gives every gate a
fresh variable and keeps the formula linear in the size of the circuit. On top of that:

- a gate that is only ever required to be true (or false) only gets the clauses for that
  direction (the *Plaisted–Greenbaum* refinement), roughly halving the clause count;
- required "and"s are split into separate clauses, and required "or"s become single clauses.

## 6. Solving

The CNF formula goes to the SAT solver, a *conflict-driven clause learning* (CDCL) solver. It
assigns variables, propagates their consequences, and when it hits a contradiction, learns a new
clause that rules out the cause. Modern solvers handle formulas with millions of clauses.

RedSAT uses [CaDiCaL](https://github.com/arminbiere/cadical) by default. Its own pure-Rust CDCL
solver (`--solver builtin`) behaves the same, more slowly. Both are *incremental*: clauses can be
added between calls, and a call can make temporary assumptions. RedSAT relies on that for:

- **`solve all`**: after each answer, a *blocking clause* says "differ from this answer in at
  least one of the listed variables", and the solver is called again. Blocking only the listed
  variables is what makes [projection](../learn/solving.md#projection-only-the-listed-variables-count)
  work.
- **`solve unique`**: one call for the answer, one more to show there is no other.
- **`minimize` / `maximize`**: a binary search, each probe a temporary assumption
  ("objective ≤ m?"). See [Optimization](../learn/optimization.md#how-it-works).
- **Explanations**: each source line's requirements get a *selector* literal, and the solver
  reports which selectors its proof of "no solution" used. See
  [When there is no solution](../learn/no-solution.md#how-explanations-are-found).

## 7. Decoding and the self-check

The solver's answer is a true/false value for each variable. RedSAT reads the bits of the
unknowns, turns them back into integers, booleans, characters, strings, arrays and structs, and
then **checks the answer against every requirement**, using the language's own rules for known
values. Only then is the answer shown, or handed to the `solve` body. An answer that failed the
check would be reported as an internal error, never printed as if it were right.
