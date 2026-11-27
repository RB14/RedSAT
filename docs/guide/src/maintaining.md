# Keeping the docs up to date

This guide is tested, so that it cannot silently drift away from the compiler. This page is for
people changing RedSAT or the guide.

## Building the book

The guide is an [mdBook](https://rust-lang.github.io/mdBook/). From the repository root:

```console
$ cargo install mdbook        # once
$ mdbook build docs/guide     # writes docs/guide/book/ (git-ignored)
$ mdbook serve docs/guide     # serves it on http://localhost:3000, rebuilding on change
```

The book is meant to be published under `/guide/`, next to the online playground at the site
root.

## Tested code blocks

`crates/redsat-lang/tests/docs.rs` reads every page under `docs/guide/src/` and checks its RedSAT
code blocks against the compiler, using CaDiCaL like `redsat` does:

````text
```rsat            a complete program; it must run without errors
```rsat,check      it must type-check (for programs too slow to run in the tests)
```rsat,error      it must fail
```rsat,ignore     not checked: a fragment that cannot stand alone (keep these rare)
````

A ```` ```text ```` block right after an `rsat` or `rsat,error` block, with only blank lines in
between, is the program's expected output: what it prints, followed by its diagnostics. The test
compares them line by line, ignoring trailing whitespace. Programs are named `main.rsat` in
diagnostics. To show output that is *not* checked (a shell session, say), use a ```` ```console ````
block, or put a sentence between the program and its output.

More attributes can follow the language, separated by commas:

- `file=NAME` also saves the block as `NAME` next to the page's other programs, so that other
  blocks on the same page can `import "NAME"` it (see the [Imports](learn/imports.md) chapter);
- compiler options, spelled like the command-line flags: `strict`, `explicit-types=…`,
  `unbounded-ints=…`, `fallback-int-bits=…`, `max-int-bits=…`.

Run the tests with:

```console
$ cargo test -p redsat-lang --test docs
```

A failure names the page and line of the block. When a change in behaviour is intended (a
better error message, a different but equally valid first solution), regenerate the expected
outputs, then **review the diff** before committing:

```console
$ REDSAT_BLESS=1 cargo test -p redsat-lang --test docs
$ git diff docs/guide
```

Outputs that list several solutions depend on the order in which the solver finds them, so
changes to encodings can reorder them. That is expected; blessing updates them. Make sure the
prose around a block does not depend on that order.

## When the language changes

Two more tests keep the guide complete:

- `the_reference_documents_every_word_of_the_language` (in `docs.rs`) fails when a keyword, type,
  built-in function or standard library function from `redsat_lang::syntax` is not mentioned,
  in code, in the reference chapters (`docs/guide/src/reference/`).
- `the_guide_highlighter_knows_exactly_the_words_of_the_language` (in
  `crates/redsat-lang/tests/grammar.rs`) fails when the guide's syntax highlighter,
  `docs/guide/assets/rsat-highlight.js`, does not list exactly the words of `redsat_lang::syntax`.
  mdBook highlights code with highlight.js, which does not know RedSAT; that script teaches it.

`every_page_is_in_the_summary` also checks that every page is listed in `src/SUMMARY.md`, since
mdBook leaves unlisted pages out.

So, when you add to the language:

1. **A keyword, type or built-in function**: add it to the word lists in `rsat-highlight.js`,
   document it in the right reference chapter, and, if it is something users will reach for,
   show it in the tutorial ("Learn RedSAT").
2. **A standard library function**: add it to the table in
   `reference/standard-library.md`, with its signature and a known-value example.
3. **A new error message or a changed one**: run the docs test; bless and review. Consider adding
   it to `reference/diagnostics.md`.
4. **A command-line flag or a `redsat.toml` key**: update `reference/cli.md` or
   `reference/configuration.md`.
5. **A new example in `examples/`**: consider a cookbook recipe, starting with the problem in
   plain words.

The design document, `docs/design.md`, remains the place for rationale and decisions; the guide
links to it rather than repeating it.
