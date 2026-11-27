//! Reading and writing CNF formulas in DIMACS, the plain-text format every SAT solver speaks.
//!
//! ```text
//! c  comment lines start with 'c'
//! p cnf 3 2          <- header: number of variables and clauses
//! 1 -3 0             <- clause (x1 ∨ ¬x3), terminated by 0
//! 2 3 -1 0
//! ```

use std::error::Error;
use std::fmt;
use std::io::{self, BufRead, Write};

use crate::{ClauseSink, Cnf, Lit, Var};

/// Writes `cnf` in DIMACS format.
///
/// # Errors
///
/// Propagates I/O errors from `out`.
pub fn write<W: Write>(cnf: &Cnf, mut out: W) -> io::Result<()> {
    writeln!(out, "p cnf {} {}", cnf.num_vars(), cnf.num_clauses())?;
    for clause in cnf.clauses() {
        for lit in clause {
            write!(out, "{lit} ")?;
        }
        writeln!(out, "0")?;
    }
    out.flush()
}

/// Parses a DIMACS formula.
///
/// The parser is strict about syntax but forgiving about common quirks: the clause count in the
/// header is not enforced, the final clause may omit its terminating `0`, and a SATLIB-style `%`
/// line ends the input.
///
/// # Errors
///
/// Returns [`ParseError`] on I/O failure or malformed input.
pub fn parse<R: BufRead>(input: R) -> Result<Cnf, ParseError> {
    let mut cnf: Option<Cnf> = None;
    let mut clause = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let syntax_error = |message: String| ParseError::Syntax {
            line: line_number,
            message,
        };
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line.starts_with('c') {
            continue;
        }
        if line.starts_with('%') {
            break;
        }
        if line.starts_with('p') {
            if cnf.is_some() {
                return Err(syntax_error("duplicate header".into()));
            }
            cnf = Some(parse_header(line).map_err(syntax_error)?);
            continue;
        }

        let Some(cnf) = cnf.as_mut() else {
            return Err(syntax_error("clause before the 'p cnf' header".into()));
        };
        for token in line.split_whitespace() {
            let value: i32 = token
                .parse()
                .map_err(|_| syntax_error(format!("invalid literal '{token}'")))?;
            if value == 0 {
                cnf.add_clause(&clause);
                clause.clear();
            } else if value.unsigned_abs() > cnf.num_vars() {
                return Err(syntax_error(format!(
                    "literal {value} exceeds the declared {} variables",
                    cnf.num_vars()
                )));
            } else {
                clause.push(Lit::from_dimacs(value));
            }
        }
    }

    let mut cnf = cnf.ok_or(ParseError::MissingHeader)?;
    if !clause.is_empty() {
        cnf.add_clause(&clause);
    }
    Ok(cnf)
}

fn parse_header(line: &str) -> Result<Cnf, String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let [_, "cnf", num_vars, _num_clauses] = fields.as_slice() else {
        return Err(format!(
            "expected 'p cnf <variables> <clauses>', found '{line}'"
        ));
    };
    let num_vars: u32 = num_vars
        .parse()
        .map_err(|_| format!("invalid variable count '{num_vars}'"))?;
    if num_vars > Var::MAX_INDEX + 1 {
        return Err(format!("too many variables: {num_vars}"));
    }
    Ok(Cnf::with_vars(num_vars))
}

/// Why a DIMACS input could not be parsed.
#[derive(Debug)]
pub enum ParseError {
    /// Reading the input failed.
    Io(io::Error),
    /// The input contains no `p cnf` header.
    MissingHeader,
    /// The input is malformed at the given (one-based) line.
    Syntax {
        /// One-based line number.
        line: usize,
        /// What is wrong.
        message: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read DIMACS input: {error}"),
            Self::MissingHeader => write!(f, "missing 'p cnf' header"),
            Self::Syntax { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ParseError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Result<Cnf, ParseError> {
        parse(text.as_bytes())
    }

    #[test]
    fn round_trips_through_text() {
        let mut cnf = Cnf::new();
        let (a, b, c) = (cnf.new_var(), cnf.new_var(), cnf.new_var());
        cnf.add_clause(&[a.pos(), c.neg()]);
        cnf.add_clause(&[b.pos(), c.pos(), a.neg()]);
        cnf.add_clause(&[]);

        let mut text = Vec::new();
        write(&cnf, &mut text).unwrap();
        assert_eq!(
            String::from_utf8(text.clone()).unwrap(),
            "p cnf 3 3\n1 -3 0\n2 3 -1 0\n0\n"
        );
        assert_eq!(parse(text.as_slice()).unwrap(), cnf);
    }

    #[test]
    fn tolerates_comments_multiline_clauses_and_missing_terminator() {
        // The first clause spans two lines, the second lacks its 0, and '%' ends the input.
        let cnf = parse_str("c hello\np cnf 2 2\n1\n -2 0\n\n2\n%\n0\n").unwrap();
        assert_eq!(cnf.num_vars(), 2);
        let expected = [
            vec![Lit::from_dimacs(1), Lit::from_dimacs(-2)],
            vec![Lit::from_dimacs(2)],
        ];
        assert_eq!(cnf.clauses(), expected.as_slice());
    }

    #[test]
    fn reports_malformed_input() {
        assert!(matches!(
            parse_str("1 2 0\n"),
            Err(ParseError::Syntax { line: 1, .. })
        ));
        assert!(matches!(
            parse_str("c only comments\n"),
            Err(ParseError::MissingHeader)
        ));
        assert!(matches!(
            parse_str("p dnf 1 1\n"),
            Err(ParseError::Syntax { line: 1, .. })
        ));
        assert!(matches!(
            parse_str("p cnf 2 1\n1 3 0\n"),
            Err(ParseError::Syntax { line: 2, .. })
        ));
        assert!(matches!(
            parse_str("p cnf 1 1\np cnf 1 1\n"),
            Err(ParseError::Syntax { line: 2, .. })
        ));
    }
}
