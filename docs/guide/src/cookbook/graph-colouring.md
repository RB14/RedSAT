# Graph colouring

**The problem.** Colour every region of a map so that regions sharing a border get different
colours. With few colours this is hard in general, and many real problems are colouring in
disguise: assigning exams to time slots so that no student has two at once, or frequencies to
radio masts so that neighbours do not interfere.

Here is mainland Australia's states and territories, plus Tasmania, with three colours:

```text
WA ─ NT ─ QLD
  ╲  │  ╱  │
    SA ── NSW
      ╲    │
        VIC          TAS (an island)
```

**The model.** An enum names the colours, and a struct describes a border between two regions,
by their index. The unknowns are one colour per region; one requirement per border does the
rest:

```rsat
enum Color { Red, Green, Blue }
struct Border { a: int, b: int }

var regions = ["WA", "NT", "SA", "QLD", "NSW", "VIC", "TAS"]
var borders = [
    Border(0, 1), Border(0, 2), Border(1, 2), Border(1, 3),
    Border(2, 3), Border(2, 4), Border(2, 5), Border(3, 4), Border(4, 5),
]

var color: [Color; len(regions)]
for border in borders {
    require color[border.a] != color[border.b]
}
require color[0] == Color.Red            // break the symmetry between colours

solve color {
    for i in 0..len(regions) {
        print "{regions[i]}: {color[i]}"
    }
}
```

```text
WA: Red
NT: Green
SA: Blue
QLD: Red
NSW: Green
VIC: Red
TAS: Red
warning: `color[6]` is unconstrained: no requirement mentions it
  --> main.rsat:10:5
   |
10 | var color: [Color; len(regions)]
   |     ^^^^^
   = note: any value in its domain is a solution, so the value shown is arbitrary
```

The data (regions and borders) is separate from the model (the loop over borders), so colouring
another map means changing only the data.

## Why fix the first colour?

The colours are interchangeable: swap every Red with every Green in an answer, and you get
another valid answer. Fixing the colour of one region removes these "same answer in disguise"
duplicates. Here is the difference it makes when counting:

```rsat
enum Color { Red, Green, Blue }
struct Border { a: int, b: int }

var regions = ["WA", "NT", "SA", "QLD", "NSW", "VIC", "TAS"]
var borders = [
    Border(0, 1), Border(0, 2), Border(1, 2), Border(1, 3),
    Border(2, 3), Border(2, 4), Border(2, 5), Border(3, 4), Border(4, 5),
]
var color: [Color; len(regions)]
for border in borders {
    require color[border.a] != color[border.b]
}

var count = 0
solve all color {
    count = count + 1
}
print "{count} colourings"

require color[0] == Color.Red
count = 0
solve all color {
    count = count + 1
}
print "{count} colourings with WA red"
```

```text
18 colourings
6 colourings with WA red
warning: `color[6]` is unconstrained: no requirement mentions it
 --> main.rsat:9:5
  |
9 | var color: [Color; len(regions)]
  |     ^^^^^
  = note: any value in its domain is a solution, so the value shown is arbitrary
warning: `color[6]` is unconstrained: no requirement mentions it
 --> main.rsat:9:5
  |
9 | var color: [Color; len(regions)]
  |     ^^^^^
  = note: any value in its domain is a solution, so the value shown is arbitrary
```

(The second `solve` sees the first one's requirements plus the new one: constraints
[accumulate](../learn/solving.md#several-solves-constraints-accumulate).) On a big map with no
solution, symmetry makes the solver prove the same thing many times over; breaking it can be the
difference between seconds and hours. [Symmetry breaking](symmetry.md) has more on this.
