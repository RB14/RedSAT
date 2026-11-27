# RedSAT for Visual Studio Code

Editor support for RedSAT programs (`.rsat`): a language whose programs are
*solved, not executed*. The extension has two layers:

- **Declarative features** (grammar, Markdown code blocks, editing support, snippets) that need no
  running code and work everywhere: local Windows windows and WSL / SSH / container remote windows.
- **A language client** for the RedSAT language server (`redsat lsp`), which adds diagnostics,
  completion, hover and more. If the server is not installed, the declarative features keep
  working and the extension says so once.

## Features

### Language server

With the `redsat` CLI installed (see [Language server setup](#language-server-setup)):

- **Diagnostics** while you type (parse, type and strict-mode errors), honouring a `redsat.toml`
  next to the file;
- **Autocomplete** for keywords, variables and functions in scope, types after `:`, and solve
  modifiers (also triggered by typing `.` or `:`);
- **Hover** showing each variable's inferred **domain and bit width**;
- **Go to definition**;
- **Outline**: document symbols for the Outline view, breadcrumbs and *Go to Symbol* (`Ctrl+Shift+O`);
- **Inlay hints** with inferred domains and bit widths.

The server's state is shown in the language status item (the `{}` next to *RedSAT* in the status bar
of a `.rsat` editor), which also offers a restart. The command palette has
**RedSAT: Restart Language Server**. Logs are in the **RedSAT Language Server** output channel.

### Syntax highlighting

`source.rsat` covers:

- reserved keywords: `var fn return if else for in require solve print`, word operators
  `and or not xor implies iff`, and the constants `true false`;
- contextual solve modifiers: `all` / `unique` directly after `solve`, and `limit`, `minimize`,
  `maximize`;
- built-in types `bool int uint char string`, including `int[0..=9]`, `int<32>`, `uint<8>`
  and array types such as `[[int[1..=9]; 9]; 9]`;
- built-in functions (`abs isqrt min max len sum count all any all_different reverse`) and other
  function calls, function definitions with their parameters, and the names declared by
  `var` and bound by `for … in`;
- decimal, hex (`0xFF_FF`) and binary (`0b1010`) numbers; `'c'` character literals;
- single-line strings with escapes (`\n \t \r \0 \\ \" \' \{ \}`) and `{ expression }` **interpolation**,
  whose contents are highlighted as RedSAT code;
- `//` and (non-nesting) `/* */` comments; range operators `..` / `..=`; all other operators.

### Markdown code blocks

Fenced blocks tagged `rsat` or `redsat` (with ```` ``` ```` or `~~~` fences) are highlighted
inside Markdown files and the Markdown preview editor.

### Editing support

Comment toggling (`Ctrl+/`, `Shift+Alt+A`), bracket matching and colorization, auto-closing and
surrounding pairs, indentation after `{` / `(` / `[`, and `// #region` folding markers.

### Snippets

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

## Language server setup

Install the `redsat` CLI, which contains the server, from the root of the RedSAT repository:

```sh
cargo install --path crates/redsat-cli
```

This puts `redsat` in `~/.cargo/bin`. The extension starts it as `redsat lsp`, talking LSP over
stdio. Change a `redsat.server.*` setting, or run **RedSAT: Restart Language Server**, after
installing or updating it.

### How the server is found

The extension prefers to run where the workspace is (`extensionKind: ["workspace", "ui"]`):

- **Linux / macOS, and WSL / SSH / container remote windows:** the extension runs on that side
  and spawns `<redsat.server.path> lsp` directly. A bare name such as the default `redsat` is
  looked up on `PATH`. In a WSL remote window this is the Linux binary inside WSL.
- **Local Windows windows:** `redsat.exe` is tried on the Windows `PATH` first. If it cannot be
  started and `redsat.server.useWsl` is `auto` (the default), the server runs in the default WSL
  distribution instead:

  ```text
  wsl.exe -e bash -lc "exec redsat lsp"
  ```

  The login shell (`-l`) puts `~/.cargo/bin` on `PATH`. Your login-shell startup files
  (`~/.profile`, `~/.bash_profile`, …) must not print anything to stdout, because that would
  corrupt the LSP stream. When the server runs this way it sees Windows paths, so prefer a WSL
  remote window for projects that live in WSL.

Before starting the client, the extension checks that the executable can be spawned. If it
cannot, it shows one warning (with **Don't Show Again**) and leaves highlighting and snippets
working. The status item still shows the reason, and an explicit restart reports it again.

### Settings

| Setting                 | Default    | Description |
|-------------------------|------------|-------------|
| `redsat.server.enabled` | `true`     | Start the language server. |
| `redsat.server.path`    | `"redsat"` | The `redsat` executable (invoked with `lsp`): a name looked up on `PATH`, or a path. Through WSL, `~/` is expanded. |
| `redsat.server.useWsl`  | `"auto"`   | Local Windows windows only: `auto` uses WSL when `redsat.server.path` cannot be started directly, `always` always uses WSL, `never` never does. Ignored elsewhere. |
| `redsat.trace.server`   | `"messages"` | Detail of the LSP trace (`messages` or `verbose`). Tracing is on while the *RedSAT Language Server* output channel's log level is **Trace** (gear icon in the Output panel, or *Developer: Set Log Level…*). |

Changing a `redsat.server.*` setting restarts the server. In Restricted Mode (an untrusted
workspace), workspace values of `redsat.server.path` and `redsat.server.useWsl` are ignored, so
a cloned repository cannot choose which executable the extension starts.

## Build

Requires Node.js 22 or newer. From this directory (`editors/vscode/`):

```sh
npm install
npm run package
```

`npm run package` type-checks, bundles `src/` with esbuild into a minified `dist/extension.js`
(through the `vscode:prepublish` script) and writes `redsat-0.2.0.vsix` here. `dist/`,
`node_modules/` and `*.vsix` are git-ignored. To write the `.vsix` elsewhere, run
`npm run package -- -o <dir>/`.

Other scripts:

| Script              | Does                                                        |
|---------------------|-------------------------------------------------------------|
| `npm run typecheck` | `tsc --noEmit`                                              |
| `npm run build`     | development bundle `dist/extension.js` with a source map    |
| `npm run watch`     | the same, rebuilding on change                              |

`vsce` runs with `--allow-missing-repository --skip-license --no-dependencies`, so it needs no
prompts. The extension has no repository URL and no `LICENSE` file, and everything it needs at
runtime is bundled into `dist/extension.js`.

## Install

```sh
code --install-extension redsat-0.2.0.vsix --force
```

Then reload the window (*Developer: Reload Window*).

- Installed from **Windows** (`code` in PowerShell / cmd, or *Extensions → … → Install from
  VSIX…* in a local window), it serves local windows. In WSL remote windows VS Code can still run
  it on the Windows side for highlighting, but the language server only runs when the extension
  runs on the WSL side; until then the status item says so.
- For **WSL remote windows**, also install it into WSL: run `code --install-extension …` from a
  WSL terminal inside a VS Code window (there `code` is the remote CLI), or use *Install in WSL*
  on the extension's page. It then runs next to the Linux `redsat` binary.

## Layout

| Path                                    | Purpose                                         |
|-----------------------------------------|-------------------------------------------------|
| `package.json`                          | extension manifest, settings, scripts           |
| `src/extension.ts`                      | entry point: output channel, command, settings  |
| `src/serverManager.ts`                  | language client lifecycle, status item, warning |
| `src/launch.ts`                         | finding the server (PATH, WSL fallback)         |
| `tsconfig.json`                         | type checking (esbuild does the bundling)       |
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
