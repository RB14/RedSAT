//! What editors need to know about functions: signatures with parameter names, and docs.
//!
//! Functions come from three places: the program (and the files it imports), the standard
//! library prelude, and the built-ins implemented in the compiler. A function's doc is the block
//! of `///` comments right above it, for your own functions as for the prelude's.

use crate::ast::{Program, StmtKind};
use crate::source::{SourceMap, Span};
use crate::syntax::{BUILTIN_FUNCTIONS, builtin_doc};

/// Where a function is defined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// In the program or a file it imports.
    Program,
    /// In the standard library prelude.
    Prelude,
    /// Inside the compiler.
    Builtin,
}

/// A parameter: its name and its type as written (empty when the signature does not say).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parameter {
    /// The parameter's name.
    pub name: String,
    /// Its type, as written in the source.
    pub ty: String,
}

/// One function, as editors show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionInfo {
    /// The function's name.
    pub name: String,
    /// Its parameters, in order.
    pub params: Vec<Parameter>,
    /// The result type as written, if any.
    pub result: Option<String>,
    /// What it does (its `///` comments, joined into a paragraph).
    pub doc: String,
    /// Where it is defined.
    pub origin: Origin,
    /// Its name in the source (none for built-ins).
    pub span: Option<Span>,
    /// For built-ins, the signature as documented (e.g. `min(a, b, …) or min(xs)`).
    pub written: Option<String>,
}

impl FunctionInfo {
    /// The signature, e.g. `digits(n: int, k: int) -> [int]`.
    #[must_use]
    pub fn label(&self) -> String {
        if let Some(written) = &self.written {
            return written.clone();
        }
        let params: Vec<String> = self.params.iter().map(Parameter::label).collect();
        let result = self
            .result
            .as_ref()
            .map_or_else(String::new, |r| format!(" -> {r}"));
        format!("{}({}){result}", self.name, params.join(", "))
    }

    /// The byte ranges of each parameter within [`label`](Self::label), for signature help.
    #[must_use]
    pub fn param_ranges(&self) -> Vec<(usize, usize)> {
        let label = self.label();
        let Some(open) = label.find('(') else {
            return Vec::new();
        };
        let mut ranges = Vec::new();
        let mut from = open + 1;
        for param in &self.params {
            let text = param.label();
            match label[from..].find(&text) {
                Some(at) => {
                    ranges.push((from + at, from + at + text.len()));
                    from += at + text.len();
                }
                None => break,
            }
        }
        ranges
    }

    /// A snippet that calls the function with a placeholder per parameter, in the LSP/TextMate
    /// syntax: `digits(${1:n}, ${2:k})`.
    #[must_use]
    pub fn call_snippet(&self) -> String {
        if self.params.is_empty() {
            return format!("{}($0)", self.name);
        }
        let placeholders: Vec<String> = self
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| format!("${{{}:{}}}", i + 1, p.name))
            .collect();
        format!("{}({})", self.name, placeholders.join(", "))
    }
}

impl Parameter {
    /// `name: type`, or `name` when the type is unknown.
    #[must_use]
    pub fn label(&self) -> String {
        if self.ty.is_empty() {
            self.name.clone()
        } else {
            format!("{}: {}", self.name, self.ty)
        }
    }
}

/// The functions a program declares (its own and the prelude's), with their docs.
#[must_use]
pub fn program_functions(program: &Program, sources: &SourceMap) -> Vec<FunctionInfo> {
    let prelude = sources
        .files()
        .iter()
        .find(|file| file.name() == crate::load::PRELUDE_NAME);
    program
        .stmts
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Fn(decl) => Some((stmt.span, decl)),
            _ => None,
        })
        .map(|(span, decl)| {
            let file = sources.file_at(span.start);
            let in_prelude = prelude.is_some_and(|p| p.base() == file.base());
            FunctionInfo {
                name: decl.name.name.clone(),
                params: decl
                    .params
                    .iter()
                    .map(|param| Parameter {
                        name: param.name.name.clone(),
                        ty: sources.slice(param.ty.span).to_string(),
                    })
                    .collect(),
                result: decl
                    .ret
                    .as_ref()
                    .map(|ty| sources.slice(ty.span).to_string()),
                doc: doc_comment(file.text(), span.start - file.base()),
                origin: if in_prelude {
                    Origin::Prelude
                } else {
                    Origin::Program
                },
                span: Some(decl.name.span),
                written: None,
            }
        })
        .collect()
}

/// The standard library's functions (parsed once), for when a program does not parse.
#[must_use]
pub fn prelude_functions() -> &'static [FunctionInfo] {
    static PRELUDE: std::sync::OnceLock<Vec<FunctionInfo>> = std::sync::OnceLock::new();
    PRELUDE.get_or_init(|| {
        let (sources, program) =
            crate::load::load(&crate::source::SourceFile::new("empty.rsat", ""));
        program
            .map(|program| program_functions(&program, &sources))
            .unwrap_or_default()
    })
}

/// The built-in functions, with the parameters of their first documented form.
#[must_use]
pub fn builtin_functions() -> Vec<FunctionInfo> {
    BUILTIN_FUNCTIONS
        .iter()
        .map(|name| {
            let doc = builtin_doc(name).unwrap_or_default();
            let (written, meaning) = doc.split_once(": ").unwrap_or((doc, ""));
            let params = written
                .strip_prefix(&format!("{name}("))
                .and_then(|rest| rest.split_once(')'))
                .map(|(inside, _)| {
                    inside
                        .split(',')
                        .map(str::trim)
                        .filter(|p| !p.is_empty() && *p != "…")
                        .map(|p| Parameter {
                            name: p.to_string(),
                            ty: String::new(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            FunctionInfo {
                name: (*name).to_string(),
                params,
                result: None,
                doc: meaning.to_string(),
                origin: Origin::Builtin,
                span: None,
                written: Some(written.to_string()),
            }
        })
        .collect()
}

/// The `///` comment block that ends right above the line containing `offset` in `text`.
#[must_use]
pub fn doc_comment(text: &str, offset: usize) -> String {
    let line_start = text[..offset.min(text.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let lines: Vec<&str> = text[..line_start]
        .lines()
        .rev()
        .map(str::trim)
        .take_while(|line| line.starts_with("///"))
        .map(|line| line.trim_start_matches('/').trim())
        .collect();
    join_doc_lines(lines.into_iter().rev())
}

/// Joins the lines of a `///` comment into light Markdown: the lines of a paragraph join with
/// spaces, an empty line separates paragraphs, and a line starting with `- ` (a list item)
/// starts a new line.
#[must_use]
pub fn join_doc_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::new();
    let mut paragraph_break = false;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            paragraph_break = !text.is_empty();
            continue;
        }
        if paragraph_break {
            text.push_str("\n\n");
        } else if line.starts_with("- ") && !text.is_empty() {
            text.push('\n');
        } else if !text.is_empty() {
            text.push(' ');
        }
        paragraph_break = false;
        text.push_str(line);
    }
    text
}

/// The call that `offset` is inside of: the function's name, which argument (from 0) the
/// offset is in, and where the `(` is. Brackets inside arguments are skipped.
#[must_use]
pub fn call_at(text: &str, offset: usize) -> Option<(String, usize, usize)> {
    let mut depth = 0usize;
    let mut argument = 0;
    let bytes = text.as_bytes();
    let end = offset.min(text.len());
    let start = end.saturating_sub(4000);
    for i in (start..end).rev() {
        match bytes[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'(' => {
                let before = text[..i].trim_end();
                let name_start = before
                    .char_indices()
                    .rev()
                    .take_while(|&(_, c)| c.is_ascii_alphanumeric() || c == '_')
                    .last()
                    .map(|(at, _)| at)?;
                let name = &before[name_start..];
                return (!name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()))
                    .then(|| (name.to_string(), argument, i));
            }
            b'[' | b'{' => return None,
            b',' if depth == 0 => argument += 1,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceFile;

    #[test]
    fn reads_signatures_and_doc_comments() {
        let text =
            "/// Twice `x`.\n/// Exactly.\nfn double(x: int[0..9]) -> int {\n    return 2 * x\n}\n";
        let (sources, program) = crate::load::load(&SourceFile::new("t.rsat", text));
        let functions = program_functions(&program.expect("parses"), &sources);
        let double = functions
            .iter()
            .find(|f| f.name == "double")
            .expect("declared");
        assert_eq!(double.label(), "double(x: int[0..9]) -> int");
        assert_eq!(double.doc, "Twice `x`. Exactly.");
        assert_eq!(double.origin, Origin::Program);
        assert_eq!(double.call_snippet(), "double(${1:x})");
        let digits = functions
            .iter()
            .find(|f| f.name == "digits")
            .expect("from the prelude");
        assert_eq!(digits.origin, Origin::Prelude);
        assert_eq!(digits.label(), "digits(n: int, k: int) -> [int]");
        assert!(digits.doc.contains("decimal digits"));
        let ranges = digits.param_ranges();
        assert_eq!(&digits.label()[ranges[1].0..ranges[1].1], "k: int");
    }

    #[test]
    fn describes_the_builtins() {
        let builtins = builtin_functions();
        assert_eq!(builtins.len(), BUILTIN_FUNCTIONS.len());
        let min = builtins.iter().find(|f| f.name == "min").expect("min");
        assert_eq!(min.label(), "min(a, b, …) or min(xs)");
        assert_eq!(min.params.len(), 2);
        assert!(!min.doc.is_empty());
    }

    #[test]
    fn finds_the_call_around_the_cursor() {
        let text = "require clamp(x + f(1, 2), [3, 4], ";
        assert_eq!(call_at(text, text.len()), Some(("clamp".into(), 2, 13)));
        let inner = "require f(g(1, ";
        assert_eq!(
            call_at(inner, inner.len()).map(|c| (c.0, c.1)),
            Some(("g".into(), 1))
        );
        assert_eq!(call_at("var xs = [1, ", 13), None);
    }
}
