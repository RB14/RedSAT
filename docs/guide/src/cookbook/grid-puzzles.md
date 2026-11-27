# Grid puzzles

Grids are arrays of arrays, and loops over rows, columns and boxes generate the rules.

## Sudoku

**The puzzle.** Fill a 9×9 grid with digits so that every row, every column and every 3×3 box
contains each digit from 1 to 9 exactly once. Some cells are given.

**The model.** The grid is a 9×9 array of digits. The given cells are known and the empty ones
are `?`, an anonymous unknown whose type (`int[1..=9]`) comes from the declaration. Then one
`all_different` per row, column and box does the rest:

```rsat
var grid: [[int[1..=9]; 9]; 9] = [
    [5, 3, ?,  ?, 7, ?,  ?, ?, ?],
    [6, ?, ?,  1, 9, 5,  ?, ?, ?],
    [?, 9, 8,  ?, ?, ?,  ?, 6, ?],
    [8, ?, ?,  ?, 6, ?,  ?, ?, 3],
    [4, ?, ?,  8, ?, 3,  ?, ?, 1],
    [7, ?, ?,  ?, 2, ?,  ?, ?, 6],
    [?, 6, ?,  ?, ?, ?,  2, 8, ?],
    [?, ?, ?,  4, 1, 9,  ?, ?, 5],
    [?, ?, ?,  ?, 8, ?,  ?, 7, 9],
]

for i in 0..9 {
    require all_different(grid[i])                          // row i
    require all_different([grid[r][i] for r in 0..9])       // column i
    var top = i / 3 * 3                                     // box i
    var left = i % 3 * 3
    require all_different([grid[top + r][left + c] for r in 0..3 for c in 0..3])
}

solve unique grid {
    for row in grid {
        print row
    }
}
```

```text
[5, 3, 4, 6, 7, 8, 9, 1, 2]
[6, 7, 2, 1, 9, 5, 3, 4, 8]
[1, 9, 8, 3, 4, 2, 5, 6, 7]
[8, 5, 9, 7, 6, 1, 4, 2, 3]
[4, 2, 6, 8, 5, 3, 7, 9, 1]
[7, 1, 3, 9, 2, 4, 8, 5, 6]
[9, 6, 1, 5, 3, 7, 2, 8, 4]
[2, 8, 7, 4, 1, 9, 6, 3, 5]
[3, 4, 5, 2, 8, 6, 1, 7, 9]
```

A few things to notice:

- `top` and `left` are computed from the known loop variable `i`, so they are plain numbers. The
  comprehensions pick out exactly the right cells, and the solver never sees the arithmetic.
- `solve unique` solves the puzzle *and* proves that it has only one solution, which is what
  makes a Sudoku a good one. It takes a few milliseconds.
- Nothing here is specific to 9×9. A 4×4 or 16×16 Sudoku needs only different numbers.

## N-Queens

**The puzzle.** Place `N` queens on an `N`×`N` chessboard so that no two attack each other: no two
share a row, a column or a diagonal. On the standard 8×8 board, there are 92 ways.

**The model.** Every row holds exactly one queen, so each row's queen is a single unknown: the
column it stands in. Rows are then different by construction. Columns must differ too, which is
an `all_different`. Two queens share a diagonal exactly when the distance between their columns
equals the distance between their rows:

```rsat
var N = 8
var queen: [int[0..N]; N]       // queen[r]: the column of row r's queen

require all_different(queen)
for r1 in 0..N {
    for r2 in r1 + 1..N {
        require abs(queen[r1] - queen[r2]) != r2 - r1
    }
}

var count = 0
solve all queen {
    count = count + 1
    if count == 1 {
        for r in 0..N {
            print board_row(queen[r], N)
        }
    }
}
print "{N} queens can be placed in {count} ways"

// One row of the board, with a queen in column q.
fn board_row(q: int, n: int) -> string {
    var line = ""
    for c in 0..n {
        line = line + (if c == q { "Q " } else { ". " })
    }
    return line
}
```

```text
. . . Q . . . .
. Q . . . . . .
. . . . Q . . .
. . . . . . . Q
. . . . . Q . .
Q . . . . . . .
. . Q . . . . .
. . . . . . Q .
8 queens can be placed in 92 ways
```

`N` is an ordinary known value: the loops turn it into the right number of requirements.
`int[0..N]` uses the exclusive range, which is exactly the valid columns `0` to `N - 1`. The body
counts every solution but draws only the first, using a function that builds a string.

## Magic squares

**The puzzle.** Arrange the numbers 1 to 9 in a 3×3 grid so that every row, every column and both
diagonals add up to the same total. (That total must be 15: the numbers add up to 45, spread
over three rows.)

**The model.** A 3×3 grid of numbers from 1 to 9, all different, and one sum per line:

```rsat
var square: [[int[1..=9]; 3]; 3]
var total = 15

require all_different([square[r][c] for r in 0..3 for c in 0..3])
for i in 0..3 {
    require sum(square[i]) == total                     // row i
    require sum(square[r][i] for r in 0..3) == total    // column i
}
require sum(square[i][i] for i in 0..3) == total        // one diagonal
require sum(square[i][2 - i] for i in 0..3) == total    // the other

var count = 0
solve all square {
    count = count + 1
    if count == 1 {
        for row in square {
            print row
        }
    }
}
print "{count} magic squares"
```

```text
[4, 9, 2]
[3, 5, 7]
[8, 1, 6]
8 magic squares
```

There are 8, but they are really one: the rotations and reflections of the same square. The
[symmetry breaking](symmetry.md) recipe shows how to ask for just one of each.
