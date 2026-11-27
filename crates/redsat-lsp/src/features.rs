//! Editor features computed from a [`Document`]: diagnostics, completion, hover, definition,
//! symbols and inlay hints. Everything here is a pure function of the document, which keeps it
//! testable without the protocol.

use lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic as LspDiagnostic, DiagnosticRelatedInformation,
    DiagnosticSeverity, Documentation, InlayHint, InlayHintKind, InlayHintLabel, InsertTextFormat,
    Location, Range, SymbolInformation, SymbolKind as LspSymbolKind, Uri,
};
use redsat_lang::check::{Symbol, SymbolKind, TypeInfo};
use redsat_lang::syntax::{BUILTIN_FUNCTIONS, BUILTIN_TYPES, CONTEXTUAL_KEYWORDS, KEYWORDS};
use redsat_lang::{Severity, Span};

use crate::document::Document;

/// The document's diagnostics in LSP form.
#[must_use]
pub fn diagnostics(document: &Document, uri: &Uri) -> Vec<LspDiagnostic> {
    document
        .analysis
        .diagnostics
        .iter()
        .map(|d| {
            let mut message = d.message.clone();
            if let Some(label) = &d.label {
                message.push_str(&format!(" ({label})"));
            }
            for note in &d.notes {
                message.push('\n');
                message.push_str(note);
            }
            let related: Vec<DiagnosticRelatedInformation> = d
                .related
                .iter()
                .map(|(span, label)| DiagnosticRelatedInformation {
                    location: Location::new(uri.clone(), document.range(*span)),
                    message: if label.is_empty() {
                        "related".into()
                    } else {
                        label.clone()
                    },
                })
                .collect();
            LspDiagnostic {
                range: document.range(d.span),
                severity: Some(match d.severity {
                    Severity::Error => DiagnosticSeverity::ERROR,
                    Severity::Warning => DiagnosticSeverity::WARNING,
                    Severity::Note => DiagnosticSeverity::INFORMATION,
                }),
                source: Some("redsat".into()),
                message,
                related_information: (!related.is_empty()).then_some(related),
                ..LspDiagnostic::default()
            }
        })
        .collect()
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The identifier around `offset`, with its span.
fn word_at(text: &str, offset: usize) -> Option<(Span, &str)> {
    let offset = offset.min(text.len());
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_ident(c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = text[offset..]
        .char_indices()
        .find(|&(_, c)| !is_ident(c))
        .map_or(text.len(), |(i, _)| offset + i);
    (start < end).then(|| (Span::new(start, end), &text[start..end]))
}

/// The identifier that ends just before a `.` at `dot`, if any.
fn word_before_dot(text: &str, dot: usize) -> Option<&str> {
    if dot == 0 || !text[..dot].ends_with(|c: char| is_ident(c) || c == ']' || c == ')') {
        return None;
    }
    let before = text[..dot].trim_end_matches([']', ')']);
    let start = before
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_ident(c))
        .last()
        .map(|(i, _)| i)?;
    Some(&before[start..])
}

/// The symbols visible at `offset`: declared before it in an enclosing scope (functions, enums
/// and structs are visible everywhere). Inner declarations shadow outer ones.
fn visible(types: &TypeInfo, offset: usize) -> Vec<&Symbol> {
    let mut found: Vec<&Symbol> = Vec::new();
    for symbol in &types.symbols {
        let hoisted = matches!(
            symbol.kind,
            SymbolKind::Function | SymbolKind::Enum | SymbolKind::Struct
        );
        let in_scope = symbol.scope.start <= offset
            && offset <= symbol.scope.end
            && symbol.span.start < offset;
        if !(hoisted || in_scope) {
            continue;
        }
        match found.iter().position(|s| s.name == symbol.name) {
            // Prefer the innermost (narrowest) scope.
            Some(i)
                if (symbol.scope.end - symbol.scope.start)
                    < (found[i].scope.end - found[i].scope.start) =>
            {
                found[i] = symbol
            }
            Some(_) => {}
            None => found.push(symbol),
        }
    }
    found
}

/// The symbol a name at `offset` refers to, or the declaration at `offset`.
fn resolve<'a>(types: &'a TypeInfo, name: &str, span: Span) -> Option<&'a Symbol> {
    types.symbols.iter().find(|s| s.span == span).or_else(|| {
        visible(types, span.start)
            .into_iter()
            .find(|s| s.name == name)
    })
}

/// A short description of a built-in function, for completion and hover.
fn builtin_doc(name: &str) -> &'static str {
    match name {
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
        _ => "",
    }
}

fn keyword_doc(word: &str) -> Option<&'static str> {
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

/// Completion items at `offset`.
#[must_use]
pub fn completions(document: &Document, offset: usize) -> Vec<CompletionItem> {
    let text = document.text();
    let offset = offset.min(text.len());
    let word_start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_ident(c))
        .last()
        .map_or(offset, |(i, _)| i);
    let before = &text[..word_start];
    // Without any symbols yet (e.g. the first version does not parse), still complete the
    // language's own vocabulary.
    let empty = TypeInfo::default();
    let types = document
        .last_types
        .as_ref()
        .or(document.analysis.types.as_ref())
        .unwrap_or(&empty);

    // After `name.`: enum variants or struct fields.
    if let Some(dot) = before.strip_suffix('.').map(str::len) {
        return member_items(types, word_before_dot(text, dot), offset);
    }

    let trimmed = before.trim_end();
    let line = &trimmed[trimmed.rfind('\n').map_or(0, |i| i + 1)..];
    // After `:` in a declaration: types.
    if trimmed.ends_with(':')
        && (line.trim_start().starts_with("var")
            || line.trim_start().starts_with("fn")
            || !line.contains('('))
    {
        return type_items(types);
    }

    let mut items = Vec::new();
    if trimmed.ends_with("solve") {
        for modifier in ["all", "unique"] {
            items.push(item(
                modifier,
                CompletionItemKind::KEYWORD,
                Some("solve modifier".into()),
                None,
            ));
        }
    }
    let line_has_solve = line.trim_start().starts_with("solve");
    for symbol in visible(types, offset) {
        let kind = match symbol.kind {
            SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::LoopVariable => {
                CompletionItemKind::VARIABLE
            }
            SymbolKind::Function => CompletionItemKind::FUNCTION,
            SymbolKind::Enum => CompletionItemKind::ENUM,
            SymbolKind::Struct => CompletionItemKind::STRUCT,
        };
        let mut entry = item(&symbol.name, kind, Some(symbol.detail.clone()), None);
        if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Struct) {
            entry.insert_text = Some(format!("{}($1)", symbol.name));
            entry.insert_text_format = Some(InsertTextFormat::SNIPPET);
        }
        items.push(entry);
    }
    for function in BUILTIN_FUNCTIONS {
        let mut entry = item(
            function,
            CompletionItemKind::FUNCTION,
            Some("built-in".into()),
            Some(builtin_doc(function)),
        );
        entry.insert_text = Some(format!("{function}($1)"));
        entry.insert_text_format = Some(InsertTextFormat::SNIPPET);
        items.push(entry);
    }
    items.extend(keyword_items());
    if line_has_solve {
        for modifier in ["limit", "minimize", "maximize"] {
            items.push(item(
                modifier,
                CompletionItemKind::KEYWORD,
                Some("solve modifier".into()),
                None,
            ));
        }
    }
    items
}

fn item(
    label: &str,
    kind: CompletionItemKind,
    detail: Option<String>,
    doc: Option<&str>,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail,
        documentation: doc
            .filter(|d| !d.is_empty())
            .map(|d| Documentation::String(d.to_string())),
        ..CompletionItem::default()
    }
}

fn keyword_items() -> Vec<CompletionItem> {
    KEYWORDS
        .iter()
        .map(|k| item(k, CompletionItemKind::KEYWORD, None, keyword_doc(k)))
        .collect()
}

fn type_items(types: &TypeInfo) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = BUILTIN_TYPES
        .iter()
        .map(|t| {
            item(
                t,
                CompletionItemKind::TYPE_PARAMETER,
                Some("built-in type".into()),
                None,
            )
        })
        .collect();
    items.extend(types.enums.iter().map(|e| {
        item(
            &e.name,
            CompletionItemKind::ENUM,
            Some(format!("enum {}", e.name)),
            None,
        )
    }));
    items.extend(types.structs.iter().map(|s| {
        item(
            &s.name,
            CompletionItemKind::STRUCT,
            Some(format!("struct {}", s.name)),
            None,
        )
    }));
    items
}

fn member_items(types: &TypeInfo, owner: Option<&str>, offset: usize) -> Vec<CompletionItem> {
    let Some(owner) = owner else {
        return Vec::new();
    };
    if let Some(info) = types.enums.iter().find(|e| e.name == owner) {
        return info
            .variants
            .iter()
            .map(|v| {
                item(
                    v,
                    CompletionItemKind::ENUM_MEMBER,
                    Some(format!("{owner}.{v}")),
                    None,
                )
            })
            .collect();
    }
    // A variable whose type is a struct (or an array of structs).
    let struct_name = visible(types, offset)
        .into_iter()
        .find(|s| s.name == owner)
        .map(|s| {
            s.detail
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });
    let Some(info) = struct_name.and_then(|name| types.structs.iter().find(|s| s.name == name))
    else {
        return Vec::new();
    };
    info.fields
        .iter()
        .map(|(field, _)| {
            item(
                field,
                CompletionItemKind::FIELD,
                Some(info.name.clone()),
                None,
            )
        })
        .collect()
}

/// Hover text at `offset`.
#[must_use]
pub fn hover(document: &Document, offset: usize) -> Option<(Range, String)> {
    let text = document.text();
    let (span, word) = word_at(text, offset)?;
    let types = document
        .analysis
        .types
        .as_ref()
        .or(document.last_types.as_ref());
    let range = document.range(span);

    if span.start > 0 && text[..span.start].ends_with('.') {
        if let (Some(types), Some(owner)) = (types, word_before_dot(text, span.start - 1)) {
            if types.enums.iter().any(|e| e.name == owner) {
                return Some((
                    range,
                    format!("```rsat\n{owner}.{word}\n```\nvariant of enum `{owner}`"),
                ));
            }
        }
    }
    if let Some(symbol) = types.and_then(|types| resolve(types, word, span)) {
        let types = types.expect("resolved through types");
        return Some((range, describe(document, types, symbol)));
    }
    if BUILTIN_FUNCTIONS.contains(&word) {
        return Some((
            range,
            format!("```rsat\n{word}\n```\n{}", builtin_doc(word)),
        ));
    }
    if KEYWORDS.contains(&word) || CONTEXTUAL_KEYWORDS.contains(&word) {
        return keyword_doc(word).map(|doc| (range, doc.to_string()));
    }
    None
}

fn describe(document: &Document, types: &TypeInfo, symbol: &Symbol) -> String {
    let signature = match symbol.kind {
        SymbolKind::Variable => format!("var {}: {}", symbol.name, symbol.detail),
        SymbolKind::Parameter => format!("{}: {}  (parameter)", symbol.name, symbol.detail),
        SymbolKind::LoopVariable => format!("{}: {}  (loop variable)", symbol.name, symbol.detail),
        SymbolKind::Function => format!(
            "fn {}{}",
            symbol.name,
            symbol.detail.trim_start_matches("fn")
        ),
        SymbolKind::Enum => {
            let variants = types
                .enums
                .iter()
                .find(|e| e.name == symbol.name)
                .map(|e| e.variants.join(", "))
                .unwrap_or_default();
            format!("enum {} {{ {variants} }}", symbol.name)
        }
        SymbolKind::Struct => {
            let fields = types
                .structs
                .iter()
                .find(|s| s.name == symbol.name)
                .map(|s| {
                    s.fields
                        .iter()
                        .map(|(name, _)| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!("struct {} {{ {fields} }}", symbol.name)
        }
    };
    let mut text = format!("```rsat\n{signature}\n```");
    if let Some(domain) = document
        .analysis
        .domains
        .iter()
        .find(|d| d.span == symbol.span)
    {
        let bits = if domain.width == 1 {
            "1 bit".to_string()
        } else {
            format!("{} bits", domain.width)
        };
        if domain.unknowns == 1 {
            text.push_str(&format!(
                "\n\nunknown · inferred domain `{}` · {bits}",
                domain.domain
            ));
        } else {
            text.push_str(&format!(
                "\n\n{} unknowns · each in `{}` · up to {bits}",
                domain.unknowns, domain.domain
            ));
        }
    }
    text
}

/// Where the name at `offset` is declared.
#[must_use]
pub fn definition(document: &Document, offset: usize) -> Option<Span> {
    let (span, word) = word_at(document.text(), offset)?;
    let types = document
        .analysis
        .types
        .as_ref()
        .or(document.last_types.as_ref())?;
    resolve(types, word, span).map(|symbol| symbol.span)
}

/// The document outline: functions, types and top-level variables.
#[must_use]
#[allow(deprecated)] // `SymbolInformation::deprecated` is deprecated but required.
pub fn symbols(document: &Document, uri: &Uri) -> Vec<SymbolInformation> {
    let Some(types) = document
        .analysis
        .types
        .as_ref()
        .or(document.last_types.as_ref())
    else {
        return Vec::new();
    };
    types
        .symbols
        .iter()
        .filter(|s| s.scope.start == 0 && s.scope.end == usize::MAX)
        .map(|s| SymbolInformation {
            name: s.name.clone(),
            kind: match s.kind {
                SymbolKind::Function => LspSymbolKind::FUNCTION,
                SymbolKind::Enum => LspSymbolKind::ENUM,
                SymbolKind::Struct => LspSymbolKind::STRUCT,
                _ => LspSymbolKind::VARIABLE,
            },
            tags: None,
            deprecated: None,
            location: Location::new(uri.clone(), document.range(s.span)),
            container_name: None,
        })
        .collect()
}

/// Inlay hints showing the inferred domain of each unknown declaration in `range`.
#[must_use]
pub fn inlay_hints(document: &Document, range: Range) -> Vec<InlayHint> {
    let text = document.text();
    let (start, end) = (document.offset(range.start), document.offset(range.end));
    document
        .analysis
        .domains
        .iter()
        .filter(|d| d.span.start >= start && d.span.start <= end)
        .map(|d| {
            // Annotated declarations get the hint at the end of the line instead of between the
            // name and its type.
            let after_name = text[d.span.end..].trim_start_matches([' ', '\t']);
            let at = if after_name.starts_with(':') {
                d.span.end
                    + text[d.span.end..]
                        .find('\n')
                        .unwrap_or(text.len() - d.span.end)
            } else {
                d.span.end
            };
            let label = if d.domain == "bool" {
                "∈ bool".to_string()
            } else if d.unknowns == 1 {
                format!(
                    "∈ {} · {} bit{}",
                    d.domain,
                    d.width,
                    if d.width == 1 { "" } else { "s" }
                )
            } else {
                format!(
                    "each ∈ {} · {} bit{}",
                    d.domain,
                    d.width,
                    if d.width == 1 { "" } else { "s" }
                )
            };
            InlayHint {
                position: document.index.position(text, at),
                label: InlayHintLabel::String(label),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            }
        })
        .collect()
}

/// The position of `offset` (for tests).
#[cfg(test)]
fn position_of(document: &Document, needle: &str) -> usize {
    document.text().find(needle).expect("needle in text")
}

#[cfg(test)]
mod tests {
    use lsp_types::Position;

    use super::*;

    fn document(text: &str) -> Document {
        Document::update(None, 1, text.to_string(), None)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn completes_variables_in_scope_functions_and_keywords() {
        let doc = document(
            "fn double(v: int) -> int { return v * 2 }\nvar count: int[0..=9]\nrequire co",
        );
        let items = completions(&doc, doc.text().len());
        let labels = labels(&items);
        assert!(
            labels.contains(&"count")
                && labels.contains(&"double")
                && labels.contains(&"all_different")
        );
        assert!(
            labels.contains(&"require") && !labels.contains(&"v"),
            "parameters are not visible outside"
        );
    }

    #[test]
    fn completes_enum_variants_and_struct_fields() {
        let doc = document(
            "enum Color { Red, Green }\nstruct P { x: int, y: int }\nvar p: P\nvar c = Color.Red\n",
        );
        let mut text = doc.text().to_string();
        text.push_str("require Color.");
        let with_enum = Document::update(Some(doc), 2, text.clone(), None);
        assert_eq!(
            labels(&completions(&with_enum, text.len())),
            ["Red", "Green"]
        );
        text.push_str("Red == c and p.");
        let with_field = Document::update(Some(with_enum), 3, text.clone(), None);
        assert_eq!(labels(&completions(&with_field, text.len())), ["x", "y"]);
    }

    #[test]
    fn completes_types_after_a_colon() {
        // The previous version parsed; the current one is mid-edit.
        let parsed = document("enum Color { Red }\nvar d = Color.Red\n");
        let text = format!("{}var c: ", parsed.text());
        let doc = Document::update(Some(parsed), 2, text.clone(), None);
        let items = completions(&doc, text.len());
        let labels = labels(&items);
        assert!(
            labels.contains(&"int") && labels.contains(&"Color") && !labels.contains(&"require")
        );
    }

    #[test]
    fn hovers_show_types_and_inferred_domains() {
        let doc = document(
            "var n = 1000\nvar x, y\nrequire x > 1 and y > 1\nrequire x * y == n\nsolve x, y\n",
        );
        let (_, text) = hover(&doc, position_of(&doc, "x, y")).unwrap();
        assert!(text.contains("var x: int"), "{text}");
        assert!(text.contains("inferred domain `[2, 500]`"), "{text}");
        let (_, text) = hover(&doc, position_of(&doc, "solve")).unwrap();
        assert!(text.contains("find solutions"));
    }

    #[test]
    fn finds_definitions_and_outline() {
        let doc =
            document("fn f(a: int) -> int { return a }\nvar total = 3\nrequire f(total) == 3\n");
        let use_site = doc.text().rfind("total").unwrap();
        let declared = definition(&doc, use_site).unwrap();
        assert_eq!(&doc.text()[declared.start..declared.end], "total");
        assert_eq!(declared.start, doc.text().find("total").unwrap());
        let uri: Uri = "file:///t.rsat".parse().unwrap();
        let names: Vec<String> = symbols(&doc, &uri).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["f", "total"]);
    }

    #[test]
    fn inlay_hints_show_domains() {
        let doc = document(
            "var x, y: int[0..=9]\nvar b: bool\nrequire x + y == 3 and b\nsolve x, y, b\n",
        );
        let all = Range::new(Position::new(0, 0), Position::new(10, 0));
        let labels: Vec<String> = inlay_hints(&doc, all)
            .into_iter()
            .map(|h| match h.label {
                InlayHintLabel::String(s) => s,
                InlayHintLabel::LabelParts(_) => String::new(),
            })
            .collect();
        assert_eq!(labels, ["∈ [0, 3] · 2 bits", "∈ [0, 3] · 2 bits", "∈ bool"]);
    }

    #[test]
    fn reports_diagnostics_with_ranges() {
        let doc = document("var x: int[0..=9]\nrequire x + z > 3\n");
        let uri: Uri = "file:///t.rsat".parse().unwrap();
        let diagnostics = diagnostics(&doc, &uri);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start, Position::new(1, 12));
        assert!(diagnostics[0].message.starts_with("unknown variable `z`"));
    }
}
