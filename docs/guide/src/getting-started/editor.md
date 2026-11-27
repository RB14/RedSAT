# Editor support

RedSAT comes with an extension for [Visual Studio Code](https://code.visualstudio.com/), in
`editors/vscode/` of the repository. It has two layers:

- **Highlighting, snippets and editing support**, which work everywhere with nothing else
  installed.
- **A language server**, built into the `redsat` command (`redsat lsp`), which adds live error
  checking, completion, hovers and more.

## What you get

- **Syntax highlighting** for `.rsat` files, including `{…}` interpolation inside strings, and
  for ```` ```rsat ```` code blocks in Markdown files.
- **Live diagnostics**: syntax, type and strict-mode errors as you type, plus errors that only
  show up when the program is elaborated, such as an index out of range or a loop bound that
  depends on unknowns. A `redsat.toml` next to the file is honoured.
- **Hovers that show inferred ranges.** Hover over an unknown to see the range RedSAT inferred
  for it and how many bits it needs, for example
  `unknown · inferred domain [34, 130158467] · 27 bits`. The same information appears as
  **inlay hints** next to every unknown's declaration. This is the quickest way to spot an
  unknown that is wider than you expected.
- **Autocomplete** for variables, functions, enums, structs and built-ins; types after `:`;
  enum variants after `Color.`; struct fields after `p.`; and `solve` modifiers.
- **Go to definition** and a **document outline**.
- **Snippets** for common shapes: type `solveall`, `for`, `fn`, … and press Tab.

The language server analyses your program exactly as a run would, but stops each `solve` before
calling the SAT solver, so the editor stays responsive. It also gives up quietly after a short
time on programs that take long to elaborate.

## Installing

1. Install the `redsat` command (see [Installation](installation.md)). The extension starts it
   as `redsat lsp`.
2. Build the extension (this needs Node.js 22 or newer):

   ```console
   $ cd editors/vscode
   $ npm install
   $ npm run package
   ```

3. Install the resulting package and reload VS Code:

   ```console
   $ code --install-extension redsat-0.2.0.vsix --force
   ```

If the extension cannot find `redsat`, highlighting and snippets keep working, and the status
bar item next to *RedSAT* tells you why the language server did not start.

### Windows and WSL

If you use VS Code on Windows with RedSAT installed inside WSL, open your project in a **WSL
remote window** and install the extension there too: it then runs next to the Linux `redsat`.
In a local Windows window, the extension falls back to starting `redsat lsp` through `wsl.exe`
when no Windows `redsat` is on the `PATH`.

The extension's own `README.md` documents its settings (such as `redsat.server.path`) and these
details in full.

## Other editors

Any editor with a Language Server Protocol client can use `redsat lsp`: it speaks LSP over
standard input and output.
