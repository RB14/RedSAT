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

/// A short description of a built-in function, for completion and hover.
#[must_use]
pub fn builtin_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "abs" => "abs(x): the absolute value",
        "min" => "min(a, b, …) or min(xs): the smallest value",
        "max" => "max(a, b, …) or max(xs): the largest value",
        "len" => "len(xs): the length of an array or string",
        "sum" => "sum(xs) or sum(e for x in xs if c): the total",
        "count" => "count(bs) or count(c for x in xs): how many are true",
        "all" => "all(bs) or all(c for x in xs): every one is true",
        "any" => "any(bs) or any(c for x in xs): at least one is true",
        "all_different" => "all_different(xs): no two elements are equal",
        "reverse" => "reverse(xs): the array or string, reversed",
        "ord" => "ord(c): the code of a character",
        "chr" => "chr(n): the character with code n",
        _ => return None,
    })
}

/// A short description of a keyword, for hover.
#[must_use]
pub fn keyword_doc(word: &str) -> Option<&'static str> {
    Some(match word {
        "var" => {
            "`var x` declares an unknown (types may be inferred); `var x = e` defines a value."
        }
        "require" => "`require condition`: every solution must satisfy the condition.",
        "solve" => {
            "`solve [all | unique] targets [limit K] [minimize e | maximize e] { body } else { … }`: \
             find solutions; the body runs once per solution."
        }
        "print" => "`print a, b, …`: prints values (unknowns only inside a `solve` body).",
        "fn" => "`fn name(p: T, …) -> T { … }`: a function, inlined at every call.",
        "while" => {
            "`while cond bound K { … }`: a loop; `bound` is required when `cond` depends on unknowns."
        }
        "for" => "`for x in lo..hi`, `for x in array`: a loop, unrolled at compile time.",
        "enum" => "`enum Name { A, B }`: a type with named values, used as `Name.A`.",
        "struct" => "`struct Name { field: T }`: a record, built as `Name(field: value)`.",
        "implies" | "iff" | "xor" | "and" | "or" | "not" => "a logical operator",
        "all" | "unique" | "limit" | "minimize" | "maximize" => "a `solve` modifier",
        "bound" => "`while cond bound K`: the maximum number of iterations",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_function_is_documented() {
        for name in BUILTIN_FUNCTIONS {
            assert!(
                builtin_doc(name).is_some_and(|doc| doc.starts_with(name)),
                "{name}"
            );
        }
    }
}
