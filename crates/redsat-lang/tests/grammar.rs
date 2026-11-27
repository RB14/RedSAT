//! The editor grammars and the compiler must agree on the language's vocabulary.
//!
//! The VS Code grammar lives in `editors/vscode/syntaxes/rsat.tmLanguage.json`, and the user
//! guide's highlight.js language in `docs/guide/assets/rsat-highlight.js`; the compiler's word
//! tables live in `redsat_lang::syntax`. These tests fail when either side gains or loses a word
//! without the other, so highlighting can never silently drift from the language.

use std::collections::BTreeSet;

use redsat_lang::syntax::{
    BUILTIN_FUNCTIONS, BUILTIN_TYPES, CONTEXTUAL_KEYWORDS, KEYWORDS, PRELUDE_FUNCTIONS,
};
use serde_json::Value;

/// Every word listed in a plain alternation group, such as `(?:if|else)` or `(bool|int)`,
/// anywhere in the grammar's regular expressions.
fn grammar_words() -> BTreeSet<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../editors/vscode/syntaxes/rsat.tmLanguage.json"
    );
    let text = std::fs::read_to_string(path).expect("the VS Code grammar exists");
    let json: Value = serde_json::from_str(&text).expect("the grammar is valid JSON");
    let mut words = BTreeSet::new();
    collect(&json, &mut words);
    words
}

fn collect(value: &Value, words: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "match" || key == "begin" {
                    if let Value::String(regex) = value {
                        alternation_words(regex, words);
                    }
                }
                collect(value, words);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect(item, words)),
        _ => {}
    }
}

fn alternation_words(regex: &str, words: &mut BTreeSet<String>) {
    let is_word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    let mut rest = regex;
    while let Some(open) = rest.find('(') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find(')') else { break };
        let group = rest[..close].trim_start_matches("?:");
        if group.split('|').all(is_word) {
            words.extend(group.split('|').map(str::to_string));
        }
    }
}

fn language_words() -> BTreeSet<String> {
    [
        KEYWORDS,
        CONTEXTUAL_KEYWORDS,
        BUILTIN_TYPES,
        BUILTIN_FUNCTIONS,
        PRELUDE_FUNCTIONS,
    ]
    .into_iter()
    .flatten()
    .map(|word| (*word).to_string())
    .collect()
}

#[test]
fn the_grammar_highlights_every_word_of_the_language() {
    let missing: Vec<_> = language_words()
        .difference(&grammar_words())
        .cloned()
        .collect();
    assert!(
        missing.is_empty(),
        "the VS Code grammar does not know: {missing:?}"
    );
}

#[test]
fn the_grammar_highlights_no_word_the_language_lacks() {
    let extra: Vec<_> = grammar_words()
        .difference(&language_words())
        .cloned()
        .collect();
    assert!(
        extra.is_empty(),
        "the VS Code grammar highlights unknown words: {extra:?}"
    );
}

/// The word list `const NAME = `…`;` of the guide's highlighter, split on whitespace.
fn guide_highlighter_words(name: &str) -> BTreeSet<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/guide/assets/rsat-highlight.js"
    );
    let text = std::fs::read_to_string(path).expect("the guide's highlighter exists");
    let start = format!("const {name} = `");
    let rest = text
        .split_once(&start)
        .unwrap_or_else(|| panic!("the guide's highlighter defines {start}…`"))
        .1;
    let list = rest.split('`').next().expect("a closing backtick");
    list.split_whitespace().map(str::to_string).collect()
}

#[test]
fn the_guide_highlighter_knows_exactly_the_words_of_the_language() {
    let tables = [
        ("KEYWORDS", KEYWORDS),
        ("CONTEXTUAL_KEYWORDS", CONTEXTUAL_KEYWORDS),
        ("BUILTIN_TYPES", BUILTIN_TYPES),
        ("BUILTIN_FUNCTIONS", BUILTIN_FUNCTIONS),
        ("PRELUDE_FUNCTIONS", PRELUDE_FUNCTIONS),
    ];
    for (name, table) in tables {
        let expected: BTreeSet<String> = table.iter().map(|word| (*word).to_string()).collect();
        assert_eq!(
            guide_highlighter_words(name),
            expected,
            "docs/guide/assets/rsat-highlight.js: {name} must list exactly syntax::{name}"
        );
    }
}

#[test]
fn the_prelude_list_matches_the_prelude() {
    let defined: BTreeSet<String> = redsat_lang::load::PRELUDE
        .lines()
        .filter_map(|line| line.strip_prefix("fn "))
        .map(|rest| rest.split('(').next().expect("a name").to_string())
        .collect();
    let listed: BTreeSet<String> = PRELUDE_FUNCTIONS
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    assert_eq!(
        defined, listed,
        "syntax::PRELUDE_FUNCTIONS must list exactly the prelude's functions"
    );
}
