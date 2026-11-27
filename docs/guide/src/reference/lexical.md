# Lexical structure

How the text of a RedSAT program is split into words, numbers and symbols.

## Source files

A program is a UTF-8 text file, conventionally with the extension `.rsat`. Non-ASCII characters
may appear in strings, characters and comments, but not in names.

## Comments

```rsat
// A line comment runs to the end of the line.
print 1 /* a block comment can sit inside a line */ + 2
/* Block comments
   can span lines. */
```

```text
3
```

Block comments do not nest: a block comment ends at the first `*/`.

## Statements and line breaks

A statement ends at the end of its line, or at a `;`, which separates statements on one line:

```rsat
var a = 1; var b = 2; print a + b
```

```text
3
```

Line breaks inside parentheses `(…)` and square brackets `[…]` are ignored, so a long expression
can continue on the next line when it is wrapped in parentheses, and a list can be written one
element per line, with an optional trailing comma:

```rsat
var total = (1 + 2
             + 3 + 4)
var primes = [
    2, 3, 5,
    7, 11,
]
print total, primes
```

```text
10 [2, 3, 5, 7, 11]
```

Braces `{…}` delimit blocks, whose statements end at line breaks as usual.

## Names

A name starts with an ASCII letter or `_`, followed by letters, digits and `_`: `x`, `queen`,
`max_weight`, `_tmp2`. Names are case-sensitive.

## Keywords

These words are reserved and can never be used as names:

| Kind                  | Keywords                                                       |
|-----------------------|----------------------------------------------------------------|
| declarations          | `var`, `fn`, `enum`, `struct`, `import`                        |
| statements            | `require`, `solve`, `print`, `return`                          |
| control flow          | `if`, `else`, `for`, `in`, `while`                             |
| logical operators     | `and`, `or`, `not`, `xor`, `implies`, `iff`                    |
| literals              | `true`, `false`                                                |

These words are keywords only in one position, and ordinary names everywhere else:

| Word       | Keyword in                                           |
|------------|------------------------------------------------------|
| `all`      | `solve all …`                                        |
| `unique`   | `solve unique …`                                     |
| `limit`    | `solve … limit K`                                    |
| `minimize` | `solve … minimize e`                                 |
| `maximize` | `solve … maximize e`                                 |
| `bound`    | `while cond bound K { … }`                           |

So `var limit = 30` is fine, and `all(…)` is still the built-in function.

The built-in type names (`bool`, `int`, `uint`, `char`, `string`) and the names of the
[built-in functions](builtins.md) cannot be used to name functions, enums or structs.

## Literals

| Literal     | Examples                            | Notes                                         |
|-------------|-------------------------------------|-----------------------------------------------|
| integer     | `42`, `1_000_000`, `0xFF`, `0b1010` | decimal, hexadecimal or binary; `_` separates digits; any size |
| boolean     | `true`, `false`                     |                                               |
| character   | `'a'`, `'7'`, `'\n'`, `'\''`        | exactly one character, or an escape           |
| string      | `"hello"`, `"x = {x}"`              | on one line; `{…}` interpolates an expression |
| hole        | `?`                                 | an anonymous unknown (see [Types](types.md#holes)) |

There are no negative literals: `-5` is the unary minus applied to `5`, which comes to the same.

The escapes, in characters and strings, are:

| Escape | Means                 |
|--------|-----------------------|
| `\n`   | line feed             |
| `\t`   | tab                   |
| `\r`   | carriage return       |
| `\0`   | the null character    |
| `\\`   | a backslash           |
| `\"`   | a double quote        |
| `\'`   | a single quote        |
| `\{`   | a literal `{`         |
| `\}`   | a literal `}`         |

```rsat
var x = 0xFF_FF + 0b1010 + 1_000
print x, "x is {x}", 'q', "a \{literal\} brace"
```

```text
66545 x is 66545 q a {literal} brace
```

## Operators and punctuation

```text
+  -  *  /  %  ^          arithmetic
== != <  <= >  >=         comparison
&  |  <<  >>              bitwise (and the keyword xor)
..  ..=                   ranges
=                         assignment
->                        function result type
.                         field access, enum variants
,  :  ;  ?                separators, type annotations, statement separator, hole
( ) [ ] { }               grouping, arrays and indexing, blocks
```

`&&`, `||` and `!` are not RedSAT operators: the compiler suggests `and`, `or` and `not` (or
`!=`) instead.
