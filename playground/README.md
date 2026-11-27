# RedSAT playground

A web page where you can pick an example, edit it and run it. Everything runs in your browser:
the RedSAT compiler and its built-in CDCL SAT solver are Rust, compiled to WebAssembly
(`crates/redsat-wasm`). Nothing is sent to a server.

Features: syntax highlighting, completion and hover docs (built from the compiler's own
vocabulary, so they never drift from the language), live diagnostics while you type, the
domain the compiler inferred for each unknown on hover, `--verbose` progress, strict mode,
streaming output with a Stop button, and share links (`#code=…` holds the compressed program).

## Building

You need Rust with the WebAssembly target, a `wasm-bindgen` CLI matching the version pinned in
`crates/redsat-wasm/Cargo.toml`, and Node.js 20+. `wasm-opt` (binaryen) is optional and
shrinks the module.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked

cd playground
npm install
npm run dev      # builds the compiler, then serves on http://localhost:8000 and rebuilds on change
npm run build    # a production build in dist/
npm test         # checks the example list against ../examples
```

`npm run wasm` rebuilds only the WebAssembly module (into `pkg/`); `npm run bundle` rebuilds
only the page (into `dist/`).

## How it fits together

| File | Role |
|---|---|
| `src/worker.js` | Loads the WebAssembly compiler in a Web Worker and runs or analyses programs, streaming output back. |
| `src/main.js` | The page: examples, Run/Stop (Stop terminates the worker), output, status, sharing. A second worker analyses the program as you type. |
| `src/editor.js` | CodeMirror 6 setup: diagnostics, hover tooltips, keyboard shortcuts. |
| `src/rsat-language.js` | RedSAT highlighting, indentation and completion, built from the compiler's `vocabulary()`. |
| `scripts/examples.mjs` | Turns `../examples/*.rsat` into `examples.json`, in the curated order with titles. New examples appear automatically. |

## Deploying

`npm run deploy` builds everything (the compiler, the solvers, the page and the guide), copies
it into its own folder of the GitHub Pages repository (`RB14/RB14.github.io`, folder `redsat/`),
commits and pushes. The playground is then at <https://rb14.github.io/redsat/> and the guide at
<https://rb14.github.io/redsat/guide/>. `--no-build` publishes the current `dist/` and
`--dry-run` stops before pushing. Other projects in that repository are left alone.

## Solvers

The playground currently uses RedSAT's built-in CDCL solver. It is much slower than CaDiCaL on
hard instances (the factoring and Pell examples are marked as slow). CaDiCaL itself can be
compiled to WebAssembly with Emscripten and offered as a second, selectable solver: the
compiler only needs an incremental IPASIR-style `Solver` (`crates/redsat-sat`), which a
JavaScript-backed implementation can provide.
