# The RedSAT Guide

The user guide and reference for RedSAT, written with [mdBook](https://rust-lang.github.io/mdBook/).

```console
$ cargo install mdbook        # once
$ mdbook build docs/guide     # writes docs/guide/book/ (git-ignored)
$ mdbook serve docs/guide     # serves it on http://localhost:3000, rebuilding on change
```

- `src/`: the chapters; `src/SUMMARY.md` is the table of contents.
- `assets/rsat-highlight.js`: teaches highlight.js (mdBook's highlighter) the RedSAT language.
- `assets/rsat.css`: small styling for RedSAT code blocks.

Every RedSAT code block in the guide is tested against the compiler by
`crates/redsat-lang/tests/docs.rs` (`cargo test -p redsat-lang --test docs`; bless changed
outputs with `REDSAT_BLESS=1`). The rules for code blocks, and what to update when the language
changes, are in the guide's own chapter
[Keeping the docs up to date](src/maintaining.md).
