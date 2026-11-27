# Your first program

Let's solve a puzzle that has been around for over 1,500 years:

> A farmer counts 35 heads and 94 legs among the chickens and rabbits in the yard. How many
> chickens and how many rabbits are there?

You could solve this with a bit of algebra. In RedSAT you don't have to: you write down what you
know, and ask for the rest.

## The program

Save this program as `farm.rsat` and run it with `redsat farm.rsat`:

```rsat
var heads = 35
var legs = 94
var chickens, rabbits

require chickens >= 0 and rabbits >= 0
require chickens + rabbits == heads
require 2 * chickens + 4 * rabbits == legs

solve chickens, rabbits
```

```text
chickens = 23, rabbits = 12
```

That's the answer: 23 chickens and 12 rabbits.

## Line by line

**`var heads = 35`** and **`var legs = 94`**: `var` declares a variable. These two have values,
so they are **known**: plain numbers, just as in any other language.

**`var chickens, rabbits`**: these two have no value, so they are **unknowns**. Finding their
values is the whole point of the program. Notice that there is no type: RedSAT sees them used
in arithmetic below and makes them integers.

**`require …`**: each `require` states something that must be true of the answer. A chicken has
one head and two legs, a rabbit one head and four legs. We also say that the counts can't be
negative: RedSAT's integers are real mathematical integers, so without that line `-1` rabbits
would be fair game.

**`solve chickens, rabbits`**: `solve` asks RedSAT to find values for the unknowns that satisfy
every requirement, and to show the ones listed after it. Without a body, it prints them as
`name = value`.

## Make it yours

Programs are short, so experimenting is cheap. Try these changes one at a time.

**Print the answer your way.** A `solve` can have a body. It runs once the unknowns have their
values, so inside it they are ordinary numbers:

```rsat
var heads = 35
var legs = 94
var chickens, rabbits

require chickens >= 0 and rabbits >= 0
require chickens + rabbits == heads
require 2 * chickens + 4 * rabbits == legs

solve chickens, rabbits {
    print "{chickens} chickens and {rabbits} rabbits"
    print "check: {2 * chickens + 4 * rabbits} legs"
}
```

```text
23 chickens and 12 rabbits
check: 94 legs
```

The `{…}` inside a string inserts the value of any expression.

**Make sure the answer is the only one.** `solve unique` finds a solution and then *proves*
that there is no other:

```rsat
var heads = 35
var legs = 94
var chickens, rabbits

require chickens >= 0 and rabbits >= 0
require chickens + rabbits == heads
require 2 * chickens + 4 * rabbits == legs

solve unique chickens, rabbits
```

```text
chickens = 23, rabbits = 12
```

If there were two answers, RedSAT would stop with an error showing both.

**Ask for the impossible.** Change the number of legs to 95 and run it again:

```rsat
var heads = 35
var legs = 95
var chickens, rabbits

require chickens >= 0 and rabbits >= 0
require chickens + rabbits == heads
require 2 * chickens + 4 * rabbits == legs

solve chickens, rabbits
```

```text
no solution
note: no solution: this requirement can never hold
 --> main.rsat:7:9
  |
7 | require 2 * chickens + 4 * rabbits == legs
  |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

RedSAT doesn't just say "no": it points at the requirement responsible. Every animal has an
even number of legs, so the total can never be odd.

## What just happened?

When you ran the program, RedSAT:

1. computed everything it could right away (`heads` and `legs`);
2. worked out from the requirements that `chickens` and `rabbits` lie between 0 and 35, so each
   fits in a handful of bits;
3. turned the requirements into a SAT problem, a formula over true/false variables;
4. handed it to the CaDiCaL SAT solver, and turned the solver's answer back into two integers.

Run `redsat farm.rsat --stats` to see some of this: the inferred ranges of the unknowns, their
size in bits, and how big the SAT problem was. In this tiny puzzle RedSAT narrows both ranges
down to a single value before the solver even starts.

Next, learn the everyday commands in [Running programs](running.md), or dive into the language
with [Knowns and unknowns](../learn/knowns-and-unknowns.md).
