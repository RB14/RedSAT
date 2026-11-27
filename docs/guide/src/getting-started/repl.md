# The REPL

`redsat repl` starts an interactive session. Each statement you type is added to a program, and
RedSAT shows you what it prints. It is a handy way to try out an operator, a standard library
function, or a model you are still shaping.

```console
$ redsat repl
RedSAT 0.1.0 — type :help for help, :quit to leave
rsat> var x, y: int[0..=9]
rsat> require x + y == 10
rsat> require x > y
rsat> solve all x, y
x = 9, y = 1
x = 6, y = 4
x = 7, y = 3
x = 8, y = 2
rsat> 2 ^ 100
1267650600228229401496703205376
rsat> sorted([3, 1, 2])
[1, 2, 3]
rsat> :quit
```

A few things to know:

- **A bare expression is printed**, as if you had written `print` in front of it. That
  includes a function call on its own line, like `sorted([3, 1, 2])`, when the function returns
  a value; a function that returns nothing is just run.
- **Blocks can span several lines.** Input continues (with a `...` prompt) while a bracket is
  open, so you can type a `solve` body, an `if` or a function over several lines.
- **Mistakes are not kept.** An input with an error is reported and dropped; the program stays as
  it was.
- **Constraints accumulate**, exactly as in a file: each `solve` sees every `require` you have
  entered so far. Use `:undo` to take back the last input, or `:reset` to start over.

Behind the scenes, the REPL runs the whole program again for each input and shows only the new
output. Runs are deterministic, so earlier output never changes.

## Commands

| Command        | What it does                                   |
|----------------|------------------------------------------------|
| `:help`        | show the commands                              |
| `:show`        | print the program entered so far               |
| `:undo`        | remove the last accepted input                 |
| `:reset`       | start over with an empty program               |
| `:load <file>` | append a file's contents to the program        |
| `:quit`        | leave the REPL (Ctrl-D works too)              |

`redsat repl` accepts the same compiler options and `--solver` choice as running a file. It
looks for a [`redsat.toml`](../reference/configuration.md) in the current directory and its
parents.
