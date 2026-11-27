# RedSAT for Visual Studio Code

Editor support for RedSAT programs (`.rsat`): a language whose programs are
*solved, not executed*. This extension is **purely declarative** (no extension code runs), so it
works the same in local Windows windows and in WSL / SSH / container remote windows.

## Features

- **Syntax highlighting** (`source.rsat`) for:
  - reserved keywords: `var fn return if else for in require solve print`, word operators
    `and or not xor implies iff`, and the constants `true false`;
  - contextual solve modifiers: `all` / `unique` directly after `solve`, and `limit`, `minimize`,
    `maximize`;
  - built-in types `bool int uint char string`, including `int[0..=9]`, `int<32>`, `uint<8>`
    and array types such as `[[int[1..=9]; 9]; 9]`;
  - built-in functions (`abs min max len sum count all any all_different reverse`) and other
    function calls, function definitions with their parameters, and the names declared by
    `var` and bound by `for … in`;
  - decimal, hex (`0xFF_FF`) and binary (`0b1010`) numbers; `'c'` character literals;
  - single-line strings with escapes (`\n \t \r \0 \\ \" \' \{ \}`) and `{ expression }` **interpolation**,
    whose contents are highlighted as RedSAT code;
  - `//` and (non-nesting) `/* */` comments; range operators `..` / `..=`; all other operators.
- **Markdown code blocks**: fenced blocks tagged `rsat` or `redsat` (with ```` ``` ```` or
  `~~~` fences) are highlighted inside Markdown files and the Markdown preview editor.
- **Editing support**: comment toggling (`Ctrl+/`, `Shift+Alt+A`), bracket matching and
  colorization, auto-closing and surrounding pairs, indentation after `{` / `(` / `[`, and
  `// #region` folding markers.
- **Snippets** (a first step towards completion; a language server comes later):

  | Prefix        | Expands to                                  |
  |---------------|---------------------------------------------|
  | `var`         | `var x: int` (type is a choice)             |
  | `varr`        | `var x: int[0..=9]`                         |
  | `require`     | `require condition`                         |
  | `alldiff`     | `require all_different([a, b, c])` (Phase 3) |
  | `solve`       | `solve x { print "x = {x}" }`               |
  | `solveall`    | `solve all x limit 10 { … }`                |
  | `solveunique` | `solve unique x`                            |
  | `solvemin`    | `solve x minimize objective`                |
  | `solvemax`    | `solve x maximize objective`                |
  | `for`         | `for i in 0..N { … }`                       |
  | `if` / `ife`  | `if condition { … }` (with `else`)          |
  | `fn`          | `fn name(x: int) -> int { return x }` (Phase 3) |
  | `print`       | `print "{x}"`                               |

## Build

Requires Node.js (for `npx`). From this directory (`editors/vscode/`):

```sh
npx --yes @vscode/vsce package --allow-missing-repository --skip-license --no-dependencies
```

This writes `redsat-0.1.0.vsix` here (it is git-ignored). The flags let `vsce` run
non-interactively: the extension has no repository URL, no `LICENSE` file and no npm
dependencies.

## Install

```sh
code --install-extension redsat-0.1.0.vsix --force
```

Because the extension is declarative, VS Code can run it on either side of a remote
connection:

- Installed from **Windows** (`code` in PowerShell / cmd, or *Extensions → … → Install from
  VSIX…* in a local window), it highlights `.rsat` files in local windows **and** in WSL remote
  windows.
- Installed from a **WSL terminal inside a VS Code window**, `code` is the remote CLI and installs
  it into that WSL server, so it applies to WSL remote windows.

Reload the window afterwards (*Developer: Reload Window*).

## Layout

| Path                                    | Purpose                                         |
|-----------------------------------------|-------------------------------------------------|
| `package.json`                          | extension manifest                              |
| `language-configuration.json`           | comments, brackets, pairs, indentation, folding |
| `syntaxes/rsat.tmLanguage.json`         | TextMate grammar (`source.rsat`)                |
| `syntaxes/rsat.markdown.tmLanguage.json`| Markdown fenced-block injection grammar         |
| `snippets/rsat.json`                    | snippets                                        |
| `samples/showcase.rsat`, `showcase.md`  | samples exercising every token class            |

## Keeping the grammar in sync with the lexer

The grammar follows the compiler's lexer exactly. One invariant is load-bearing: **the
reserved keywords are exactly the words listed in rules whose `match` has the shape
`\b(?:kw1|kw2|…)\b`** (the `keywords` repository entry). A unit test in `redsat-lang` parses
that shape and compares it to the lexer's keyword table. Contextual keywords, types and builtins
therefore use capturing groups `\b(…)\b` instead. When adding a keyword, update both the
lexer and that list.

To check highlighting by hand, open `samples/showcase.rsat` and run
*Developer: Inspect Editor Tokens and Scopes*.
