# How to model a problem

The cookbook shows how to express classic problems in RedSAT. Each recipe starts with the
problem in plain language, then builds the model step by step. The full programs, and more, are
in the `examples/` directory of the repository.

Writing a model is a skill of its own, different from writing an algorithm. These questions
guide almost every model in this cookbook.

## 1. What does an answer look like?

Pick the **unknowns**: the smallest set of values that, once known, tell you the whole answer.

- In Sudoku, the answer is the digit in each cell: `[[int[1..=9]; 9]; 9]`.
- In N-Queens, every row holds exactly one queen, so the answer is the *column* of each row's
  queen: `[int[0..N]; N]`. Choosing this over "a yes/no for every square" makes "one queen per
  row" true by construction, so it never has to be required.
- In a knapsack problem, the answer is a yes/no per item: `[bool; n]`.

A good choice of unknowns makes many rules automatic. Give each unknown the **tightest type**
you can: `int[1..=9]` rather than `int`.

## 2. What makes an answer valid?

Turn every rule of the problem into a `require`. Write them the way the problem states them, and
let loops generate the repetitive ones. "Neighbouring regions have different colours":

```rsat
struct Border { a: int, b: int }
var borders = [Border(0, 1), Border(1, 2), Border(0, 2), Border(2, 3)]
var color: [int[0..3]; 4]           // three colours, four regions

for border in borders {
    require color[border.a] != color[border.b]
}
solve color
```

```text
color = [1, 2, 0, 1]
```

The standard library has ready-made building blocks for common rules: `all_different`,
`exactly`, `at_most`, `sorted`, `lex_less` and more.

## 3. What do you want to see?

- One answer? `solve`.
- Proof that the answer is the only one? `solve unique`.
- Every answer, or a count? `solve all`, with a counter in the body.
- The best answer? `minimize` or `maximize`.

List only the unknowns you care about after `solve`: the others then only need to exist, and
answers that differ only in them are not repeated.

## 4. Separate the data from the model

Put the instance, such as the numbers, the names and the grid, in known variables at the top, and
write the rules in terms of them. Then the same model solves any instance: change `var N = 8` to
`var N = 20` and N-Queens solves a bigger board.

## 5. Start small, then grow

Solve a tiny instance first and print the answer in a readable form. When it looks right, scale
up. If something is slow, see the [Performance tips](../under-the-hood/performance.md); if there
is no solution, see [When there is no solution](../learn/no-solution.md).

## The recipes

- [Logic puzzles](logic-puzzles.md): knights and knaves, a who-owns-what puzzle, and
  SEND + MORE = MONEY.
- [Grid puzzles](grid-puzzles.md): Sudoku, N-Queens and magic squares.
- [Number puzzles](number-puzzles.md): factoring, Pythagorean triples, digit puzzles and bit
  patterns.
- [Graph colouring](graph-colouring.md): colouring a map with enums and structs.
- [Knapsack](knapsack.md): picking the most valuable items, with side conditions.
- [Scheduling](scheduling.md): ordering jobs on a machine to finish as early as possible.
- [Counting constraints](counting.md): a weekly duty roster.
- [Symmetry breaking](symmetry.md): avoiding answers that are the same in disguise.
