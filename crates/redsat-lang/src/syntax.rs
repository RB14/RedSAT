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
    "isqrt",
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

/// The documentation of a built-in function, for completion and hover. The first line is
/// `signature: summary` (see [`crate::signatures::builtin_functions`]); details follow after a
/// blank line, in light Markdown (inline code, `- ` lists).
#[must_use]
pub fn builtin_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "abs" => {
            "abs(x): the absolute value of the integer `x`.\n\n\
             Works on unknowns too: `require abs(x - y) == 3` holds when `x` and `y` are three \
             apart, in either order.\n\n\
             Example: `abs(-7)` is `7`."
        }
        "isqrt" => {
            "isqrt(n): the integer square root of `n`: the largest `r` with `r * r <= n`, that              is √n rounded down. `n` must not be negative.\n\n             - With a known `n`, it is computed exactly when the program is compiled, at no cost              to the solver, however large `n` is (`isqrt(10 ^ 100)` is `10 ^ 50`).\n             - With an unknown `n`, the result is an unknown `r` that the solver keeps equal to              √n rounded down (`r * r <= n < (r + 1) * (r + 1)`), and `n` is required not to be              negative.\n\n             Typical uses: bounding a search (`require x <= isqrt(n)`: the smaller of two              factors of `n`), and perfect squares (`isqrt(n) * isqrt(n) == n`).\n\n             Example: `isqrt(10)` is `3`, `isqrt(16)` is `4`."
        }
        "min" => {
            "min(a, b, …) or min(xs): the smallest value.\n\n\
             Takes two or more integers, an array of integers, or a comprehension such as \
             `min(cost[i] for i in 0..n)`. With unknowns, the result is an unknown that equals \
             the smallest candidate. A comprehension's `if` filter must not depend on unknowns \
             here (use `sum`, `count`, `all`, `any`, `len` or `all_different` for that).\n\n\
             Example: `min(3, 9, 4)` is `3`."
        }
        "max" => {
            "max(a, b, …) or max(xs): the largest value.\n\n\
             Takes two or more integers, an array of integers, or a comprehension such as \
             `max(finish[j] for j in 0..n)`. With unknowns, the result is an unknown that equals \
             the largest candidate. A comprehension's `if` filter must not depend on unknowns \
             here.\n\n\
             Example: `max(3, 9, 4)` is `9`."
        }
        "len" => {
            "len(xs): the number of elements of an array, or of characters in a string.\n\n\
             For a string of unknown length (`var w: string[2..=5]`), `len(w)` is an unknown you \
             can constrain: `require len(w) == 4`. With a comprehension, it counts the elements \
             that pass the filter, even when the filter depends on unknowns: \
             `len(x for x in xs if x > 0)`.\n\n\
             Example: `len([4, 5, 6])` is `3`."
        }
        "sum" => {
            "sum(xs) or sum(e for x in xs if c): the total of integers.\n\n\
             The filter may depend on unknowns: `sum(weight[i] for i in 0..n if take[i])` adds \
             the weights of the items taken. A sum of booleans' weights compared with a constant \
             (`sum(…) <= capacity`) gets a dedicated encoding when that is smaller.\n\n\
             Example: `sum([1, 2, 3])` is `6`."
        }
        "count" => {
            "count(bs) or count(c for x in xs): how many of the booleans are true.\n\n\
             Count conditions over collections: `count(q[i] for i in 0..n) == 1`. A count \
             compared with a constant is encoded so that the solver propagates it directly, so \
             `count(…) <= k` (and the standard library's `exactly`, `at_least`, `at_most`) beat \
             hand-written sums.\n\n\
             Example: `count([true, false, true])` is `2`."
        }
        "all" => {
            "all(bs) or all(c for x in xs): whether every boolean is true (true when there are \
             none).\n\n\
             Example: `require all(x > 0 for x in xs)` makes every element positive. (As a `solve` \
             modifier, `solve all` asks for every solution.)"
        }
        "any" => {
            "any(bs) or any(c for x in xs): whether at least one boolean is true (false when \
             there are none).\n\n\
             Example: `require any(x == 7 for x in xs)` puts a 7 somewhere in `xs`."
        }
        "all_different" => {
            "all_different(xs): whether no two elements are equal.\n\n\
             The classic constraint of puzzles: Sudoku rows, permutations, cryptarithms. Works on \
             integers, characters and enum values, known or unknown, and accepts a \
             comprehension: `all_different(grid[r][c] for r in 0..9)`.\n\n\
             Example: `require all_different([s, e, n, d, m, o, r, y])`."
        }
        "reverse" => {
            "reverse(xs): the array or string in reverse order.\n\n\
             Example: `require w == reverse(w)` makes the string `w` a palindrome."
        }
        "ord" => {
            "ord(c): the code of the character `c` (its Unicode scalar value), an integer.\n\n\
             Arithmetic on characters goes through codes: `ord(c) - ord('0')` is the value of a \
             digit character.\n\n\
             Example: `ord('A')` is `65`."
        }
        "chr" => {
            "chr(n): the character whose code is `n` (the inverse of `ord`).\n\n\
             Example: `chr(ord('a') + 1)` is `'b'`."
        }
        _ => return None,
    })
}

/// The documentation of a keyword (or `solve` modifier), for completion and hover, in light
/// Markdown: a signature line, then explanations and an example.
#[must_use]
pub fn keyword_doc(word: &str) -> Option<&'static str> {
    Some(match word {
        "var" => {
            "`var name`, `var a, b: T`, `var name = value`: declares variables.\n\n\
             - Without a value, the variable is an *unknown*: the solver finds its value. Its \
             type may be written (`var x: int[0..=9]`) or inferred from how it is used.\n\
             - With a value (`var total = 0`), it is an ordinary variable that can be reassigned \
             (`total = total + x`).\n\
             - `?` is an anonymous unknown inside a value: `var row: [int[1..=9]; 3] = [5, ?, ?]`.\n\n\
             Example: `var x, y: int[1..=100]`"
        }
        "require" => {
            "`require condition`: every solution must make the condition true.\n\n\
             The condition is a boolean expression over known values and unknowns. Requirements \
             accumulate: each `solve` sees all of them so far. Inside an `if` whose condition \
             depends on unknowns, a requirement only applies when that branch is taken.\n\n\
             Example: `require x * y == 91 and x < y`"
        }
        "solve" => {
            "`solve [all | unique] targets [limit K] [minimize e | maximize e] { body } else { … }`\n\n\
             Asks the SAT solver for values of the unknowns that satisfy every `require` so far.\n\n\
             - **targets**: the unknowns (or expressions over them) whose values you want, \
             separated by commas: `solve x, y`. Two solutions count as different only when the \
             targets differ. Without targets, every unknown in scope is a target.\n\
             - **all**: every solution, not just the first. **unique**: one solution, proven to \
             be the only one (finding a second is an error that shows both).\n\
             - **limit K**: at most `K` solutions.\n\
             - **minimize e** / **maximize e**: a solution with the smallest / largest value of \
             the expression `e` (with `all`: every such solution).\n\
             - **{ body }**: runs once per solution, with the targets holding its values. \
             Without a body, the targets are printed (`x = 3, y = 4`).\n\
             - **else { … }**: runs when there is no solution. Without it, RedSAT prints \
             `no solution` and explains which requirements conflict.\n\n\
             Example: `solve all x, y limit 3 { print x * y }`"
        }
        "print" => {
            "`print a, b, …`: prints the values, separated by spaces.\n\n\
             Strings interpolate expressions in braces: `print \"x = {x}\"`. Unknowns have values \
             only inside a `solve` body, where they hold the current solution.\n\n\
             Example: `solve x { print \"found {x}\" }`"
        }
        "if" => {
            "`if cond { … } else { … }`, and as a value: `if cond { a } else { b }`.\n\n\
             When the condition depends on unknowns, both branches are compiled: variables \
             assigned in them hold one value or the other depending on the condition, and a \
             `require` inside a branch only applies when that branch is taken.\n\n\
             Example: `var biggest = if a > b { a } else { b }`"
        }
        "else" => {
            "`else`: the other branch of an `if` (`else if …` chains conditions), or what a \
             `solve` does when there is no solution: `solve x { … } else { print \"none\" }`."
        }
        "for" => {
            "`for x in lo..hi { … }`, `for x in lo..=hi`, `for x in array`, `for c in string`\n\n\
             The range or collection must be known: the loop is unrolled when the program is \
             compiled, so it can declare unknowns and requirements in bulk. `lo..hi` stops \
             before `hi`; `lo..=hi` includes it.\n\n\
             Example: `for i in 0..n { require queen[i] != i }`"
        }
        "in" => {
            "`in`: the range or collection of a `for` loop, or a membership test that works on \
             unknowns too: `x in 1..=9`, `x in [2, 3, 5]`, `c in \"aeiou\"`."
        }
        "while" => {
            "`while cond { … }` and `while cond bound K { … }`\n\n\
             With a known condition it loops as in any language. When the condition depends on \
             unknowns, `bound K` is required: the loop is unrolled `K` times, each iteration \
             guarded by the condition, and it must finish within `K` iterations (solutions that \
             would need more are excluded).\n\n\
             Example: `while n > 0 bound 10 { n = n / 10 }`"
        }
        "bound" => {
            "`bound K`: the most iterations of a `while` loop whose condition depends on \
             unknowns; the loop must have finished within them. `K` must be known."
        }
        "fn" => {
            "`fn name(p: T, …) -> T { … }`: declares a function.\n\n\
             Calls are inlined: the body runs at compile time with the arguments, so a function \
             works on known values and unknowns alike, and may contain `var`, `require`, loops \
             and early `return`s. Parameters need types; array parameters may leave out the \
             length (`xs: [int]`). Recursion must stop on known values. `///` comment lines just \
             above document the function. Your function hides a standard-library one of the \
             same name, in your code only.\n\n\
             Example: `fn square(x: int) -> int { return x * x }`"
        }
        "return" => {
            "`return value` (or `return` in a function without a result): leaves the function.\n\n\
             An early `return` under a condition that depends on unknowns works too: the result \
             depends on the condition, and the rest of the function only applies when it is \
             false."
        }
        "enum" => {
            "`enum Name { A, B, … }`: a type with a fixed set of named values, written `Name.A`.\n\n\
             Unknowns can be enums (`var color: Color` lets the solver pick one); compare them \
             with `==` and `!=`, and use them with `all_different`.\n\n\
             Example: `enum Color { Red, Green, Blue }`"
        }
        "struct" => {
            "`struct Name { field: T, … }`: a record type.\n\n\
             Build one with `Name(1, 2)` or `Name(x: 1, y: 2)` and read fields with `p.x`. Fields \
             can be unknown: `var p: Point` leaves every field to the solver.\n\n\
             Example: `struct Point { x: int[0..=9], y: int[0..=9] }`"
        }
        "import" => {
            "`import \"file.rsat\"`: brings in the functions, enums and structs of another file.\n\n\
             The path is relative to the importing file. Each file is loaded once, and imported \
             files may import further files, but they may only declare things (functions, \
             enums, structs, imports), not run statements."
        }
        "and" => {
            "`a and b`: true when both are. `b` is only evaluated where `a` holds, so \
             `i < len(xs) and xs[i] > 0` is safe even when `i` is unknown."
        }
        "or" => "`a or b`: true when either is. `b` is only evaluated where `a` is false.",
        "not" => "`not a`: the opposite of the boolean `a`.",
        "xor" => "`a xor b`: true when exactly one of `a` and `b` is.",
        "implies" => {
            "`a implies b`: false only when `a` holds and `b` does not (the same as \
             `not a or b`). Handy in requirements: `require x > 5 implies y == 0`."
        }
        "iff" => {
            "`a iff b`: true when `a` and `b` are both true or both false. Typical of logic \
             puzzles: `require knight iff statement` (a knight's statements are true)."
        }
        "true" | "false" => "`true` and `false`: the boolean constants.",
        "all" => {
            "`solve all targets`: every solution, one after another; the body runs once for \
             each. Two solutions differ when the targets differ. Add `limit K` to stop after `K`. \
             (`all(…)` as a function: whether every boolean is true.)"
        }
        "unique" => {
            "`solve unique targets`: one solution, proven to be the only one. If there is a \
             second, it is an error that shows both, which helps to check that a puzzle is \
             well-posed."
        }
        "limit" => {
            "`limit K`: at most `K` solutions, e.g. `solve all x limit 3`. `K` must be known."
        }
        "minimize" => {
            "`minimize e`: a solution with the smallest value of the expression `e`, e.g. \
             `solve x, y minimize x + y`. With `all`: every solution with that optimal value."
        }
        "maximize" => {
            "`maximize e`: a solution with the largest value of the expression `e`, e.g. \
             `solve take maximize value`. With `all`: every solution with that optimal value."
        }
        _ => return None,
    })
}

/// The documentation of a built-in type, for completion and hover.
#[must_use]
pub fn type_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "bool" => {
            "`bool`: `true` or `false`.\n\n\
             An unknown boolean (`var b: bool`) is one yes/no decision for the solver."
        }
        "int" => {
            "`int`, `int[lo..hi]`, `int[lo..=hi]`, `int<N>`: an integer.\n\n\
             Arithmetic is exact: there is no overflow. Without bounds, RedSAT infers a range \
             from the requirements (`x + y == 35` with `x, y >= 0` bounds both by 35) and sizes \
             the circuit to it. `int[lo..hi]` stops before `hi`; `int[lo..=hi]` includes it. \
             `int<N>` is an `N`-bit signed range, from -2^(N-1) to 2^(N-1) - 1.\n\n\
             Examples: `var digit: int[0..=9]`, `var day: int[0..7]`"
        }
        "uint" => {
            "`uint<N>`: an integer from 0 to 2^N - 1 (`N` bits, unsigned).\n\n\
             The range is a bound, not a wrap-around: arithmetic stays exact.\n\n\
             Example: `var byte: uint<8>`"
        }
        "char" => {
            "`char`: a character, written `'a'`.\n\n\
             Characters compare and order by their codes; `ord` and `chr` convert to and from \
             codes, and ranges work: `c in 'a'..='z'`."
        }
        "string" => {
            "`string`, `string[lo..=hi]`: text, written `\"…\"`.\n\n\
             An unknown string (`var w: string[3..=5]`) has unknown characters and a length in \
             the given range. Index it (`w[0]`), measure it (`len(w)`), join with `+`, compare \
             with `==`, and interpolate values: `\"x = {x}\"`."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyword_and_type_is_documented() {
        for word in KEYWORDS.iter().chain(CONTEXTUAL_KEYWORDS) {
            assert!(
                keyword_doc(word).is_some_and(|doc| doc.len() > 40),
                "{word}"
            );
        }
        for name in BUILTIN_TYPES {
            assert!(type_doc(name).is_some_and(|doc| doc.len() > 40), "{name}");
        }
    }

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
