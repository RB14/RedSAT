//! The vocabulary of the language, in one place.
//!
//! The lexer, the editor grammar (checked by a test, so the two cannot drift) and, later, the
//! language server's autocomplete all use these tables.

/// Reserved words: they can never be used as names.
pub const KEYWORDS: &[&str] = &[
    "var", "fn", "return", "require", "solve", "print", "if", "else", "for", "in", "and", "or",
    "not", "xor", "implies", "iff", "true", "false",
];

/// Words that are keywords only inside a `solve` statement (elsewhere they are ordinary names).
pub const SOLVE_MODIFIERS: &[&str] = &["all", "unique", "limit", "minimize", "maximize"];

/// Names of the built-in types.
pub const BUILTIN_TYPES: &[&str] = &["bool", "int", "uint", "char", "string"];

/// Names of the built-in functions (some arrive in later phases).
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
];
