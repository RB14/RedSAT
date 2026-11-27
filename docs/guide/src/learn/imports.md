# Imports

As models grow, you will want to share functions, enums and structs between programs. `import`
brings in everything another file declares.

## A module and a program

A file meant to be imported, a *module*, contains declarations only. Here are two. The first
holds a small helper:

```rsat,file=lib/squares.rsat
fn square(x: int) -> int {
    return x * x
}
```

The second builds on it, so it imports it:

```rsat,file=geometry.rsat
import "lib/squares.rsat"

struct Point { x: int[-10..=10], y: int[-10..=10] }

fn distance_squared(a: Point, b: Point) -> int {
    return square(a.x - b.x) + square(a.y - b.y)
}
```

A program next to `geometry.rsat` can now use `Point` and `distance_squared`. Which points with
positive coordinates lie at distance 5 from the origin?

```rsat
import "geometry.rsat"

var origin = Point(0, 0)
var q: Point
require distance_squared(origin, q) == 25 and q.x > 0 and q.y > 0
solve all q
```

```text
q = Point(x: 4, y: 3)
q = Point(x: 3, y: 4)
```

## The rules

- **Paths are relative to the importing file.** `geometry.rsat` says `import
  "lib/squares.rsat"`, which means the `lib` directory next to `geometry.rsat`, wherever the
  program that imports `geometry.rsat` lives.
- **Imports are transitive.** Importing `geometry.rsat` also brings in what *it* imports, so the
  program above could call `square` too.
- **Each file is loaded once**, however many times it is imported.
- **Everything shares one namespace.** Declaring the same name in two files is an error, just
  like declaring it twice in one file.
- **Imports go at the top level** of a file.

## Modules only declare

An imported file may only contain `fn`, `enum`, `struct` and further `import`s. Statements that
*do* something, like `var`, `require`, `print` or `solve`, belong in the program, so a module
can be imported by many programs without side effects. Here is a file that breaks the rule:

```rsat,file=greeting.rsat
fn greeting() -> string {
    return "hello"
}
print greeting()
```

Run on its own, it is a fine program. Imported, it is rejected, with a pointer to the offending
statement and to the import:

```rsat,error
import "greeting.rsat"
```

```text
error: an imported file may only declare functions, enums and structs
 --> greeting.rsat:4:1
  |
4 | print greeting()
  | ^^^^^^^^^^^^^^^^
 ::: main.rsat:1:1
  |
1 | import "greeting.rsat"
  | ^^^^^^^^^^^^^^^^^^^^^^ imported here
```

Importing a file that does not exist is an error too, which names the file and the reason it
could not be opened.

## The standard library

Every program implicitly has access to RedSAT's
[standard library](../reference/standard-library.md), with no `import` needed. It is written in
RedSAT, so you can read it to see how its functions work, and your own functions with the same
name take precedence over it (see [Functions](functions.md#your-functions-and-the-standard-library)).
