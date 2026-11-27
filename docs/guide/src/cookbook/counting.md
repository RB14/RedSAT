# Counting constraints

Many rules are about *how many*: exactly two people on duty, at most four shifts a week, at
least one vowel. RedSAT's `count` and the standard library's `exactly`, `at_least`, `at_most`
and `count_of` say these directly.

| You want                          | Write                                  |
|-----------------------------------|----------------------------------------|
| how many conditions hold          | `count(conditions)`                    |
| exactly `k` of them hold          | `exactly(k, conditions)`               |
| at least / at most `k` hold       | `at_least(k, conditions)`, `at_most(k, conditions)` |
| how many elements equal a value   | `count_of(xs, value)`                  |
| none / all / some hold            | `not any(…)`, `all(…)`, `any(…)`       |

`conditions` is an array of booleans, which you can write inline as a comprehension:
`count(x > 0 for x in xs)`.

## A duty roster

**The problem.** Four people share a duty over a week. Every day, exactly two of them are on
duty. Everyone works three or four days, and nobody works three days in a row. Ann is away on
Monday, and Ben has asked to work the weekend. Who works when?

**The model.** A grid of yes/no unknowns, one per person and day. Each rule becomes a counting
requirement over a row or a column of the grid:

```rsat
var people = ["Ann", "Ben", "Cleo", "Dev"]
var days = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
var P = len(people)
var D = len(days)
var on: [[bool; D]; P]          // on[p][d]: person p works on day d

// Two people every day.
for d in 0..D {
    require exactly(2, [on[p][d] for p in 0..P])
}
for p in 0..P {
    // Three or four days each, and never three days in a row.
    require at_least(3, on[p]) and at_most(4, on[p])
    for d in 0..D - 2 {
        require not (on[p][d] and on[p][d + 1] and on[p][d + 2])
    }
}
require not on[0][0]            // Ann is away on Monday
require on[1][5] and on[1][6]   // Ben works the weekend

solve on {
    print "      Mo Tu We Th Fr Sa Su"
    for p in 0..P {
        var line = people[p]
        while len(line) < 6 {
            line = line + " "
        }
        for d in 0..D {
            line = line + (if on[p][d] { "X  " } else { ".  " })
        }
        print "{line}({count(on[p])} days)"
    }
}
```

```text
      Mo Tu We Th Fr Sa Su
Ann   .  X  X  .  X  X  .  (4 days)
Ben   X  X  .  .  .  X  X  (4 days)
Cleo  X  .  X  X  .  .  .  (3 days)
Dev   .  .  .  X  X  .  X  (3 days)
```

The body pads names with a `while` loop. That loop's condition is known (inside a body,
everything is), so it needs no `bound`.

## How counting is encoded

`count` adds up its conditions, each worth 0 or 1, with a balanced tree of adders, so the sum
of `n` conditions needs only about `log2(n)` bits. `exactly(k, …)` is just `count(…) == k`, and
you can write that yourself whenever it reads better.

Counting also works on characters in strings. Which four-letter words over `a`, `b` and `c`
contain exactly two `a`s, and never two equal letters in a row?

```rsat
var w: string[4..=4]
require all(c in 'a'..='c' for c in w)
require count_char(w, 'a') == 2
require all(w[i] != w[i + 1] for i in 0..3)
solve all w
```

```text
w = "baba"
w = "caba"
w = "acba"
w = "caca"
w = "baca"
w = "abca"
w = "acac"
w = "acab"
w = "abab"
w = "abac"
```
