//! Tested documentation: every RedSAT code block in the user guide (`docs/guide/src/`) is
//! checked against the compiler, so the guide cannot drift from the language.
//!
//! Fenced code blocks are recognised by their info string:
//!
//! | Info string   | Check                                                               |
//! |---------------|---------------------------------------------------------------------|
//! | `rsat`        | a complete program that runs without errors                         |
//! | `rsat,check`  | type-checks without errors, but is not run (for slow programs)      |
//! | `rsat,error`  | fails, with an error from the parser, type checker or elaboration  |
//! | `rsat,ignore` | not checked: a fragment that cannot stand alone (keep these rare)   |
//!
//! A ```` ```text ```` block right after an `rsat` or `rsat,error` block (blank lines allowed in
//! between) is that program's expected output: what it prints, followed by its rendered
//! diagnostics. The two are compared line by line, ignoring trailing whitespace. Run with
//! `REDSAT_BLESS=1` to rewrite those blocks with the actual output, and review the diff.
//!
//! More attributes, after a comma:
//! - `file=NAME` also saves the block as `NAME` next to the page's other programs, so that the
//!   other blocks on the same page can `import "NAME"` it;
//! - compiler options, spelled as on the command line: `strict`, `explicit-types=…`,
//!   `unbounded-ints=…`, `fallback-int-bits=…` and `max-int-bits=…`.
//!
//! Programs are shown as `main.rsat` in diagnostics (or as their `file=` name) and run with
//! CaDiCaL, like `redsat main.rsat`.
//!
//! A second test checks that the reference chapters mention, in code, every word of the
//! language's vocabulary (`redsat_lang::syntax`), so new features cannot go undocumented. A
//! third one checks that every page is listed in `SUMMARY.md`, so none is left out of the book.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use redsat_lang::syntax::{
    BUILTIN_FUNCTIONS, BUILTIN_TYPES, CONTEXTUAL_KEYWORDS, KEYWORDS, PRELUDE_FUNCTIONS,
};
use redsat_lang::{CompilerOptions, OptionLayer, SourceFile};
use redsat_sat::{Cadical, Solver};

/// The guide's Markdown sources.
fn guide_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/src")
}

/// The name a program is shown under in diagnostics, unless it has a `file=` name.
const MAIN_NAME: &str = "main.rsat";

// --- Markdown -------------------------------------------------------------------------------

/// A fenced code block.
#[derive(Debug)]
struct Fence {
    /// The info string after the opening fence, e.g. `rsat,error`.
    info: String,
    /// The block's contents, with the fence's indentation removed.
    code: String,
    /// How far the opening fence is indented (inside a list item, for example).
    indent: usize,
    /// The 0-based line index of the opening fence.
    open: usize,
    /// The 0-based line index of the closing fence (the line count if it is never closed).
    close: usize,
}

impl Fence {
    /// The language: the first word of the info string.
    fn language(&self) -> &str {
        self.info
            .split([',', ' ', '\t', '{'])
            .next()
            .unwrap_or_default()
    }

    /// The attributes after the language.
    fn attributes(&self) -> impl Iterator<Item = &str> {
        self.info
            .split([',', ' ', '\t'])
            .skip(1)
            .filter(|attribute| !attribute.is_empty())
    }
}

/// Every fenced code block of a Markdown file (with ```` ``` ```` or `~~~` fences).
fn fences(text: &str) -> Vec<Fence> {
    let lines: Vec<&str> = text.lines().collect();
    let mut fences = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let indent = line.len() - line.trim_start().len();
        let rest = &line[indent..];
        let marker = match rest.chars().next() {
            Some(c @ ('`' | '~')) => c,
            _ => {
                index += 1;
                continue;
            }
        };
        let length = rest.chars().take_while(|&c| c == marker).count();
        if length < 3 {
            index += 1;
            continue;
        }
        let info = rest[length..].trim().to_string();
        let open = index;
        let mut code = String::new();
        index += 1;
        while index < lines.len() {
            let inner = lines[index].trim();
            if inner.len() >= length && inner.chars().all(|c| c == marker) {
                break;
            }
            let content = lines[index];
            let strip = content.len() - content.trim_start().len();
            code.push_str(&content[strip.min(indent)..]);
            code.push('\n');
            index += 1;
        }
        fences.push(Fence {
            info,
            code,
            indent,
            open,
            close: index,
        });
        index += 1;
    }
    fences
}

/// What a RedSAT block must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Run,
    Check,
    Error,
    Ignore,
}

/// A RedSAT block to test, with everything needed to run it.
#[derive(Debug)]
struct Case {
    /// The page, relative to the guide's `src/` directory.
    page: PathBuf,
    /// The 1-based line of the opening fence, for messages.
    line: usize,
    mode: Mode,
    code: String,
    /// The name the program is shown (and saved) under.
    name: String,
    options: OptionLayer,
    /// The expected output (the ```text block after it).
    expected: Option<Expected>,
}

/// A program's expected output, and where it is in the page.
#[derive(Debug)]
struct Expected {
    text: String,
    /// The 0-based line range of the block's contents.
    lines: std::ops::Range<usize>,
    /// The block's indentation, kept when it is rewritten.
    indent: usize,
}

impl Case {
    fn location(&self) -> String {
        format!("docs/guide/src/{}:{}", self.page.display(), self.line)
    }
}

/// Parses a block's attributes; returns its mode, `file=` name and options.
fn parse_attributes(fence: &Fence) -> Result<(Mode, Option<String>, OptionLayer), String> {
    let mut mode = Mode::Run;
    let mut name = None;
    let mut options = OptionLayer::default();
    for attribute in fence.attributes() {
        let (key, value) = attribute
            .split_once('=')
            .map_or((attribute, None), |(key, value)| (key, Some(value)));
        let text = value.ok_or_else(|| format!("`{attribute}` needs a value"));
        let number = || -> Result<Option<u32>, String> {
            let text = text.clone()?;
            text.parse()
                .map(Some)
                .map_err(|_| format!("`{attribute}` needs a number"))
        };
        match key {
            "check" => mode = Mode::Check,
            "error" => mode = Mode::Error,
            "ignore" => mode = Mode::Ignore,
            "file" => name = Some(text?.to_string()),
            "strict" => options.strict = Some(true),
            "explicit-types" => options.explicit_types = Some(text?.parse()?),
            "unbounded-ints" => options.unbounded_ints = Some(text?.parse()?),
            "fallback-int-bits" => options.fallback_int_bits = number()?,
            "max-int-bits" => options.max_int_bits = number()?,
            _ => return Err(format!("unknown attribute `{attribute}`")),
        }
    }
    Ok((mode, name, options))
}

/// Every Markdown page of the guide, relative to `src/`, sorted.
fn pages() -> Vec<PathBuf> {
    fn walk(directory: &Path, root: &Path, found: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(directory)
            .expect("readable directory")
            .map(|entry| entry.expect("an entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, root, found);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                found.push(
                    path.strip_prefix(root)
                        .expect("inside the guide")
                        .to_path_buf(),
                );
            }
        }
    }
    let root = guide_dir();
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found
}

/// The RedSAT blocks of a page, or the problems found while reading them.
fn cases(page: &Path, text: &str, problems: &mut Vec<String>) -> Vec<Case> {
    let fences = fences(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut cases = Vec::new();
    for (index, fence) in fences.iter().enumerate() {
        let location = format!("docs/guide/src/{}:{}", page.display(), fence.open + 1);
        if fence.close >= lines.len() {
            problems.push(format!("{location}: this code block is never closed"));
            continue;
        }
        if !matches!(fence.language(), "rsat" | "redsat") {
            continue;
        }
        let (mode, name, options) = match parse_attributes(fence) {
            Ok(parsed) => parsed,
            Err(problem) => {
                problems.push(format!("{location}: {problem}"));
                continue;
            }
        };
        // The output block must follow directly, with only blank lines in between.
        let expected = fences.get(index + 1).filter(|next| {
            next.language() == "text"
                && lines[fence.close + 1..next.open]
                    .iter()
                    .all(|line| line.trim().is_empty())
        });
        if expected.is_some() && matches!(mode, Mode::Check | Mode::Ignore) {
            problems.push(format!(
                "{location}: the ```text block after this block would not be verified; \
                 run the program (```rsat) or put a sentence in between"
            ));
            continue;
        }
        cases.push(Case {
            page: page.to_path_buf(),
            line: fence.open + 1,
            mode,
            code: fence.code.clone(),
            name: name.unwrap_or_else(|| MAIN_NAME.to_string()),
            options,
            expected: expected.map(|next| Expected {
                text: next.code.clone(),
                lines: next.open + 1..next.close,
                indent: next.indent,
            }),
        });
    }
    cases
}

// --- running --------------------------------------------------------------------------------

/// Where a page's programs are saved, so that they can import each other.
fn scratch_dir(page: &Path) -> PathBuf {
    let slug = page
        .with_extension("")
        .to_string_lossy()
        .replace(['/', '\\'], "__");
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("docs")
        .join(slug)
}

/// A case's output (what it printed, then its diagnostics) and whether it failed, or why it
/// could not run.
type Outcome = Result<(String, bool), String>;

/// Runs (or checks) a case.
fn execute(case: &Case) -> Outcome {
    let options = CompilerOptions::resolve(&OptionLayer::default(), &case.options)?;
    let path = scratch_dir(&case.page).join(&case.name);
    let source = SourceFile::new(case.name.clone(), case.code.clone()).with_path(path);
    if case.mode == Mode::Check {
        let outcome = redsat_lang::check(&source, &options);
        return Ok((outcome.render_diagnostics(), outcome.has_errors()));
    }
    let factory = || -> Box<dyn Solver> { Box::new(Cadical::new()) };
    let mut out = Vec::new();
    let outcome = redsat_lang::run(&source, &options, &factory, &mut out);
    let mut text = String::from_utf8(out).map_err(|error| error.to_string())?;
    text.push_str(&outcome.render_diagnostics());
    Ok((text, outcome.has_errors()))
}

/// Outputs compared line by line, ignoring trailing whitespace and trailing blank lines.
fn normalized(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

/// What went wrong with a case, if anything.
fn verify(case: &Case, output: &str, failed: bool) -> Option<String> {
    let location = case.location();
    match (case.mode, failed) {
        (Mode::Run, true) => {
            return Some(format!(
                "{location}: this program should run without errors:\n{output}"
            ));
        }
        (Mode::Check, true) => {
            return Some(format!(
                "{location}: this program should type-check:\n{output}"
            ));
        }
        (Mode::Error, false) => {
            return Some(format!(
                "{location}: this program should fail (it is marked `rsat,error`), but it \
                 printed:\n{output}"
            ));
        }
        _ => {}
    }
    let expected = &case.expected.as_ref()?.text;
    (normalized(expected) != normalized(output)).then(|| {
        format!("{location}: the output differs\n--- expected (the ```text block)\n{expected}--- actual\n{output}")
    })
}

/// Rewrites the ```text blocks of a page with the actual outputs.
fn bless(page: &Path, outputs: &[(&Case, String)]) {
    let path = guide_dir().join(page);
    let text = fs::read_to_string(&path).expect("readable page");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut replacements: Vec<(std::ops::Range<usize>, Vec<String>)> = outputs
        .iter()
        .filter_map(|(case, output)| {
            let expected = case.expected.as_ref()?;
            let margin = " ".repeat(expected.indent);
            let new = normalized(output)
                .into_iter()
                .map(|line| {
                    if line.is_empty() {
                        String::new()
                    } else {
                        format!("{margin}{line}")
                    }
                })
                .collect();
            Some((expected.lines.clone(), new))
        })
        .collect();
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, new) in replacements {
        lines.splice(range, new);
    }
    let mut result = lines.join("\n");
    result.push('\n');
    if result != text {
        fs::write(&path, result).expect("writable page");
    }
}

#[test]
fn every_code_block_in_the_guide_works() {
    let mut problems = Vec::new();
    let mut all_cases = Vec::new();
    for page in pages() {
        let text = fs::read_to_string(guide_dir().join(&page)).expect("readable page");
        let cases = cases(&page, &text, &mut problems);
        // Save the programs other blocks may import.
        let directory = scratch_dir(&page);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("a scratch directory");
        for case in cases.iter().filter(|case| case.name != MAIN_NAME) {
            let path = directory.join(&case.name);
            fs::create_dir_all(path.parent().expect("a directory")).expect("a directory");
            fs::write(&path, &case.code).expect("a writable scratch file");
        }
        all_cases.extend(cases);
    }

    // Run the cases in parallel; each run is independent.
    let todo: Vec<&Case> = all_cases
        .iter()
        .filter(|c| c.mode != Mode::Ignore)
        .collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Outcome>>> = Mutex::new(todo.iter().map(|_| None).collect());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = todo.get(index) else { break };
                    let result = execute(case);
                    results.lock().expect("no panics")[index] = Some(result);
                }
            });
        }
    });
    let results = results.into_inner().expect("no panics");

    let bless_mode = std::env::var_os("REDSAT_BLESS").is_some();
    let mut outputs: Vec<(&Case, String)> = Vec::new();
    for (case, result) in todo.iter().zip(results) {
        match result.expect("every case ran") {
            Err(problem) => problems.push(format!("{}: {problem}", case.location())),
            Ok((output, failed)) => {
                let wrong_kind = matches!(
                    (case.mode, failed),
                    (Mode::Run | Mode::Check, true) | (Mode::Error, false)
                );
                if bless_mode && !wrong_kind {
                    outputs.push((case, output));
                } else if let Some(problem) = verify(case, &output, failed) {
                    problems.push(problem);
                }
            }
        }
    }
    if bless_mode {
        let pages: BTreeSet<&Path> = outputs
            .iter()
            .map(|(case, _)| case.page.as_path())
            .collect();
        for page in pages {
            let page_outputs: Vec<(&Case, String)> = outputs
                .iter()
                .filter(|(case, _)| case.page == page)
                .map(|(case, output)| (*case, output.clone()))
                .collect();
            bless(page, &page_outputs);
        }
    }

    let count = |mode: Mode| all_cases.iter().filter(|case| case.mode == mode).count();
    let compared = all_cases
        .iter()
        .filter(|case| case.expected.is_some())
        .count();
    println!(
        "guide code blocks: {} run, {} type-checked, {} expected to fail, {} ignored; {} outputs \
         compared",
        count(Mode::Run),
        count(Mode::Check),
        count(Mode::Error),
        count(Mode::Ignore),
        compared
    );
    assert!(
        problems.is_empty(),
        "{} problem(s) in the guide's code blocks:\n\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

// --- completeness ---------------------------------------------------------------------------

/// The pages that make up the language reference.
const REFERENCE_DIR: &str = "reference";

/// Every name-like word that appears in code (inline code spans and code blocks) in `text`.
fn words_in_code(text: &str) -> BTreeSet<String> {
    let mut code = String::new();
    let mut in_fence = vec![false; text.lines().count()];
    for fence in fences(text) {
        code.push_str(&fence.code);
        let end = fence.close.min(in_fence.len() - 1);
        in_fence[fence.open..=end].fill(true);
    }
    // Inline code spans: text between backticks, outside code blocks.
    for (line, _) in text.lines().zip(&in_fence).filter(|(_, inside)| !**inside) {
        for (index, span) in line.split('`').enumerate() {
            if index % 2 == 1 {
                code.push_str(span);
                code.push('\n');
            }
        }
    }
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn the_reference_documents_every_word_of_the_language() {
    let reference: BTreeSet<String> = pages()
        .into_iter()
        .filter(|page| page.starts_with(REFERENCE_DIR))
        .flat_map(|page| {
            words_in_code(&fs::read_to_string(guide_dir().join(page)).expect("readable page"))
        })
        .collect();
    let tables = [
        ("KEYWORDS", KEYWORDS),
        ("CONTEXTUAL_KEYWORDS", CONTEXTUAL_KEYWORDS),
        ("BUILTIN_TYPES", BUILTIN_TYPES),
        ("BUILTIN_FUNCTIONS", BUILTIN_FUNCTIONS),
        ("PRELUDE_FUNCTIONS", PRELUDE_FUNCTIONS),
    ];
    let missing: Vec<String> = tables
        .iter()
        .flat_map(|(table, words)| {
            words
                .iter()
                .filter(|word| !reference.contains(**word))
                .map(move |word| format!("{word} (syntax::{table})"))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "the reference chapters (docs/guide/src/{REFERENCE_DIR}/) never mention these words in \
         code: {missing:?}"
    );
}

#[test]
fn every_page_is_in_the_summary() {
    let summary = fs::read_to_string(guide_dir().join("SUMMARY.md")).expect("a SUMMARY.md");
    let listed: BTreeSet<PathBuf> = summary
        .split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .filter(|target| target.ends_with(".md"))
        .map(PathBuf::from)
        .collect();
    let orphans: Vec<PathBuf> = pages()
        .into_iter()
        .filter(|page| page != Path::new("SUMMARY.md") && !listed.contains(page))
        .collect();
    assert!(
        orphans.is_empty(),
        "these pages are not listed in docs/guide/src/SUMMARY.md, so the book leaves them out: \
         {orphans:?}"
    );
}
