# Introduction

**RedSAT** (**Red**uce → **SAT**) is a programming language whose programs are *solved*, not
executed. You declare variables, state what must be true of them, and say which values you want
to see. RedSAT works out the values for you.

Here is a whole program. It finds the two factors of a number:

```rsat
var n = 4295229443             // known: we picked this number
var x, y                       // unknowns: RedSAT finds them

require x > 1 and y > 1        // rule out the boring answer 1 × n
require x <= y                 // list each pair of factors once
require x * y == n

solve x, y {
    print "{n} = {x} × {y}"
}
```

```text
4295229443 = 65537 × 65539
```

It reads like ordinary code, but nothing in it says *how* to find `x` and `y`: there is no loop
over candidates, no clever algorithm. The program only describes what an answer looks like, and
the `solve` statement asks for one.

## How it works, in a nutshell

1. RedSAT reads your program and runs everything it can run right away: `n` is simply a number.
2. Everything that depends on the *unknowns* `x` and `y` becomes a **Boolean circuit**. The
   multiplication `x * y` turns into a multiplier circuit, much like the one inside a CPU.
3. That circuit, together with the requirements, is turned into a **SAT problem**: a big
   formula over true/false variables. This reduction always takes polynomial time.
4. A state-of-the-art **SAT solver** finds values for those true/false variables, which is like
   running the multiplier *backwards*: which inputs make it output `n`?
5. RedSAT decodes the solver's answer back into your program's values, here two integers, and
   runs the body of `solve` with them.

A few things make this pleasant to use:

- **Integers are exact.** There is no overflow, ever. RedSAT works out how many bits every
  value needs from your constraints, so `x * y` always gets enough bits for its true value.
- **Types are inferred.** `var x, y` is enough: using them in `x * y == n` makes them integers,
  and the requirements bound them.
- **It is a real language.** There are functions, loops, `if`, arrays, strings, enums, structs,
  a standard library and imports. Code that runs on known values works on unknowns too.

## What is it good for?

Anything you can describe more easily than you can compute: puzzles (Sudoku, cryptarithms,
logic riddles), scheduling and timetabling, packing and selection problems, configuration,
colouring and assignment problems, or "running a function backwards" to find an input that
produces a given output. The [Cookbook](cookbook/modelling.md) walks through many of these.

These problems are typically *NP-hard*: no known method solves every instance quickly. SAT
solvers are remarkably good at the instances people actually meet, but they are not magic. The
[Performance tips](under-the-hood/performance.md) explain how to help them.

## How to read this guide

- **[Getting started](getting-started.md)** installs RedSAT and runs your first programs.
- **Learn RedSAT** is a tutorial, one concept per chapter. Read it in order.
- **Cookbook** shows how to model classic problems, starting each one in plain language.
- **Reference** is the place to look things up: syntax, types, operators, every built-in and
  standard library function, the command line and the configuration file.
- **Under the hood** explains how programs become SAT problems, and how to make them fast.

Every RedSAT program in this guide is tested against the compiler, and the output shown below
each one is exactly what it prints.

Want to try RedSAT without installing anything? Open the [online playground](../).
