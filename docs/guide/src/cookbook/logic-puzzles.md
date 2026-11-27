# Logic puzzles

Logic puzzles are a natural fit: their clues are requirements, and their answer is whatever
satisfies all of them.

## Knights and knaves

**The puzzle.** On an island, knights always tell the truth and knaves always lie. You meet two
islanders.

- A says: "B is a knave."
- B says: "A and I are of the same kind."

Who is what?

**The model.** One `bool` per islander: is this one a knight? The key insight is that a
statement is true *exactly when* its speaker is a knight, whatever the statement is. That is
`speaker iff statement`:

```rsat
var a_is_knight, b_is_knight: bool

require a_is_knight iff not b_is_knight                 // A: "B is a knave"
require b_is_knight iff (a_is_knight iff b_is_knight)   // B: "we are alike"

solve unique a_is_knight, b_is_knight {
    if a_is_knight { print "A is a knight" } else { print "A is a knave" }
    if b_is_knight { print "B is a knight" } else { print "B is a knave" }
}
```

```text
A is a knight
B is a knave
```

`solve unique` also proves there is no other reading of the clues. Inside the body, every value
is known, so the `if`s are ordinary code.

## Who owns what

**The puzzle.** Ann, Ben and Cleo each own a different pet (a cat, a dog and a fish) and each
prefers a different drink (tea, coffee and juice).

1. Ann does not own the dog.
2. Ben drinks coffee.
3. Whoever owns the fish drinks tea, and whoever owns the dog drinks juice.

Who owns what, and who drinks what?

**The model.** Enums name the pets and drinks. For each person, one unknown says which pet they
own and one which drink they prefer, and `all_different` says that no two people share. Each
clue becomes a requirement; clue 3 is about "whoever", so a loop checks it for every person:

```rsat
enum Pet { Cat, Dog, Fish }
enum Drink { Tea, Coffee, Juice }

var people = ["Ann", "Ben", "Cleo"]
var pet: [Pet; 3]           // pet[p]: the pet of person p
var drink: [Drink; 3]       // drink[p]: what person p drinks

require all_different(pet)
require all_different(drink)

// 1. Ann does not own the dog.
require pet[0] != Pet.Dog
// 2. Ben drinks coffee.
require drink[1] == Drink.Coffee
// 3. Whoever owns the fish drinks tea, and whoever owns the dog drinks juice.
for p in 0..3 {
    require pet[p] == Pet.Fish implies drink[p] == Drink.Tea
    require pet[p] == Pet.Dog implies drink[p] == Drink.Juice
}

solve unique pet, drink {
    for p in 0..3 {
        print "{people[p]} owns the {pet[p]} and drinks {drink[p]}"
    }
}
```

```text
Ann owns the Fish and drinks Tea
Ben owns the Cat and drinks Coffee
Cleo owns the Dog and drinks Juice
```

Bigger puzzles of this kind, like the famous *Zebra puzzle*, work exactly the same way, with
more people, more attributes and more clues. `solve unique` is the puzzle-setter's friend: it
tells you whether your clues pin down a single answer, and if they don't, it shows you two
answers that a further clue must tell apart.

## SEND + MORE = MONEY

**The puzzle.** A classic from 1924. Each letter stands for a different digit, and the leading
letters `S` and `M` are not zero:

```text
      S E N D
  +   M O R E
  -----------
    M O N E Y
```

**The model.** One digit per letter, `all_different` over all eight, and the column addition
written as a single equation over place values: thousands, hundreds, tens and ones.

```rsat
var s, e, n, d, m, o, r, y: int[0..=9]

require all_different([s, e, n, d, m, o, r, y])
require s != 0 and m != 0
require (            1000*s + 100*e + 10*n + d
         +           1000*m + 100*o + 10*r + e
         == 10000*m + 1000*o + 100*n + 10*e + y )

solve unique s, e, n, d, m, o, r, y {
    print "  {s}{e}{n}{d}"
    print "+ {m}{o}{r}{e}"
    print "= {m}{o}{n}{e}{y}"
}
```

```text
  9567
+ 1085
= 10652
```

The parentheses let the equation span several lines: inside `(…)` and `[…]`, line breaks do not
end the statement.

You could also model the addition column by column, with a carry per column (each `carry` an
`int[0..=1]`). Both work; the single equation is shorter, and RedSAT's exact arithmetic makes
it safe.
