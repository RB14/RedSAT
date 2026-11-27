# Installation

RedSAT is written in Rust and builds into a single command-line program, `redsat`.

## Prerequisites

- **Rust**, version 1.85 or newer. If you do not have it yet, install it with
  [rustup](https://rustup.rs):

  ```console
  $ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

- **A C++ compiler and libclang.** RedSAT bundles the [CaDiCaL](https://github.com/arminbiere/cadical)
  SAT solver, which is written in C++ and compiled as part of the build. On Debian or Ubuntu:

  ```console
  $ sudo apt install build-essential libclang-dev
  ```

## Build and install

From a checkout of the RedSAT repository:

```console
$ cargo install --path crates/redsat-cli
```

This compiles RedSAT in release mode and puts `redsat` into `~/.cargo/bin`, which rustup adds
to your `PATH`. Check that it works:

```console
$ redsat --version
redsat 0.1.0
```

To update RedSAT later, pull the latest sources and run the same `cargo install` command again.

## Running without installing

Inside the repository you can also run RedSAT through Cargo, without installing it:

```console
$ cargo run --release -p redsat-cli -- examples/factor.rsat
4295229443 = 65537 × 65539
```

Everything after `--` is passed to `redsat`. Use `--release`: an unoptimised build is much
slower at solving.

## Running the tests

If you are working on RedSAT itself, the whole test suite runs with:

```console
$ cargo test --workspace
```

It includes the examples in `examples/` and every code sample in this guide.

## Next steps

- Write [your first program](first-program.md).
- Install the [VS Code extension](editor.md) for highlighting and live feedback.
