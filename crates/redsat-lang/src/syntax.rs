//! The vocabulary of the language, in one place.
//!
//! The lexer, the editor grammar (checked by a test, so the two cannot drift) and the language
//! server's autocomplete all use these tables.

/// Reserved words: they can never be used as names.
pub const KEYWORDS: &[&str] = &[
    "var", "fn", "return", "require", "solve", "print", "if", "else", "for", "in", "while", "enum",
    "struct", "import", "and", "or", "not", "xor", "implies", "iff", "true", "false",
];

/// Words that are keywords only in one position (elsewhere they are ordinary names): the
/// `solve` modifiers, and `bound` in `while cond bound K`.
pub const CONTEXTUAL_KEYWORDS: &[&str] =
    &["all", "unique", "limit", "minimize", "maximize", "bound"];

/// Names of the built-in types.
pub const BUILTIN_TYPES: &[&str] = &["bool", "int", "uint", "char", "string"];

/// Names of the built-in functions.
pub const BUILTIN_FUNCTIONS: &[&str] = &[
    "abs",
    "min",
    "max",
    "len",
    "sum",
    "count",
    "all",
    "any",
    "all_different",
    "reverse",
    "ord",
    "chr",
];

/// Functions of the standard library prelude (`prelude.rsat`), written in RedSAT itself and
/// available in every program.
pub const PRELUDE_FUNCTIONS: &[&str] = &[
    "sign",
    "clamp",
    "divides",
    "is_even",
    "is_odd",
    "gcd",
    "digits",
    "digit_sum",
    "bit",
    "popcount",
    "exactly",
    "at_least",
    "at_most",
    "count_of",
    "product",
    "all_equal",
    "increasing",
    "strictly_increasing",
    "decreasing",
    "strictly_decreasing",
    "all_different_except",
    "index_of",
    "lex_less",
    "lex_less_eq",
    "sorted",
    "is_permutation",
    "is_digit",
    "is_lower",
    "is_upper",
    "is_letter",
    "to_upper",
    "to_lower",
    "digit_value",
    "starts_with",
    "ends_with",
    "count_char",
];
