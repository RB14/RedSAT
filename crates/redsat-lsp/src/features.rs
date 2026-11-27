//! Editor features computed from a [`Document`]: diagnostics, completion, hover, definition,
//! symbols and inlay hints. Everything here is a pure function of the document, which keeps it
//! testable without the protocol.

use lsp_types::{
    Command, CompletionItem, CompletionItemKind, CompletionItemLabelDetails,
    Diagnostic as LspDiagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Documentation,
    InlayHint, InlayHintKind, InlayHintLabel, InsertTextFormat, Location, MarkupContent,
    MarkupKind, ParameterInformation, ParameterLabel, Range, SignatureHelp, SignatureInformation,
    SymbolInformation, SymbolKind as LspSymbolKind, Uri,
};
use redsat_lang::check::{Symbol, SymbolKind, TypeInfo};
use redsat_lang::signatures::{FunctionInfo, Origin, builtin_functions, call_at};
use redsat_lang::syntax::{BUILTIN_TYPES, CONTEXTUAL_KEYWORDS, KEYWORDS, keyword_doc, type_doc};
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
            // Diagnostics inside another file (an import, the prelude) are shown where the open
            // document leads to them: the first related place in this file.
            let in_document = |span: &Span| span.end <= document.text().len();
            let primary = if in_document(&d.span) {
                d.span
            } else {
                let elsewhere = document.analysis.sources.file_at(d.span.start);
                let (line, column) = elsewhere.line_col(d.span.start);
                message = format!("{message}\n(in {}:{line}:{column})", elsewhere.name());
                d.related
                    .iter()
                    .map(|(span, _)| *span)
                    .find(in_document)
                    .unwrap_or_default()
            };
            let related: Vec<DiagnosticRelatedInformation> = d
                .related
                .iter()
                .filter(|(span, _)| in_document(span))
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
                range: document.range(primary),
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

/// The doc comment (`/// …` lines) of a standard library function.
fn prelude_doc(name: &str) -> Option<String> {
    let lines: Vec<&str> = redsat_lang::load::PRELUDE.lines().collect();
    let declaration = lines
        .iter()
        .position(|line| line.starts_with(&format!("fn {name}(")))?;
    let doc: Vec<&str> = lines[..declaration]
        .iter()
        .rev()
        .take_while(|line| line.starts_with("///"))
        .map(|line| line.trim_start_matches('/').trim())
        .collect();
    (!doc.is_empty()).then(|| doc.into_iter().rev().collect::<Vec<_>>().join(" "))
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
    for symbol in visible(types, offset)
        .into_iter()
        .filter(|s| s.kind != SymbolKind::Function)
    {
        let kind = match symbol.kind {
            SymbolKind::Variable | SymbolKind::Parameter | SymbolKind::LoopVariable => {
                CompletionItemKind::VARIABLE
            }
            SymbolKind::Function => CompletionItemKind::FUNCTION,
            SymbolKind::Enum => CompletionItemKind::ENUM,
            SymbolKind::Struct => CompletionItemKind::STRUCT,
        };
        let mut entry = item(&symbol.name, kind, Some(symbol.detail.clone()), None);
        if symbol.kind == SymbolKind::Struct {
            entry.insert_text = Some(format!("{}($1)", symbol.name));
            entry.insert_text_format = Some(InsertTextFormat::SNIPPET);
        }
        items.push(entry);
    }
    for function in document.last_functions.iter().chain(&builtin_functions()) {
        items.push(function_item(function));
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

/// A completion item that calls `function`, with a placeholder per parameter and its docs.
fn function_item(function: &FunctionInfo) -> CompletionItem {
    CompletionItem {
        label: function.name.clone(),
        kind: Some(CompletionItemKind::FUNCTION),
        detail: Some(function.label()),
        label_details: Some(CompletionItemLabelDetails {
            detail: Some(format!(
                "({})",
                function
                    .params
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            description: Some(origin_name(function.origin).into()),
        }),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: function_markdown(function),
        })),
        insert_text: Some(function.call_snippet()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        // Show the signature as soon as the call is inserted.
        command: Some(Command::new(
            "Signature help".into(),
            "editor.action.triggerParameterHints".into(),
            None,
        )),
        ..CompletionItem::default()
    }
}

fn origin_name(origin: Origin) -> &'static str {
    match origin {
        Origin::Program => "your function",
        Origin::Prelude => "standard library",
        Origin::Builtin => "built-in",
    }
}

/// The signature in a code block, then the doc, then where the function comes from.
fn function_markdown(function: &FunctionInfo) -> String {
    let mut text = format!("```rsat\nfn {}\n```", function.label());
    if !function.doc.is_empty() {
        text.push_str(&format!("\n\n{}", function.doc));
    }
    text.push_str(&format!("\n\n*{}*", origin_name(function.origin)));
    text
}

/// The function called `name`: the program's (or the prelude's), else a built-in.
fn function_named(document: &Document, name: &str) -> Option<FunctionInfo> {
    document
        .last_functions
        .iter()
        .find(|f| f.name == name)
        .cloned()
        .or_else(|| builtin_functions().into_iter().find(|f| f.name == name))
}

/// Signature help inside a call's parentheses at `offset`.
#[must_use]
pub fn signature_help(document: &Document, offset: usize) -> Option<SignatureHelp> {
    let (name, argument, _) = call_at(document.text(), offset)?;
    let function = function_named(document, &name)?;
    let label = function.label();
    let utf16 =
        |byte: usize| u32::try_from(label[..byte].encode_utf16().count()).unwrap_or(u32::MAX);
    let parameters = function
        .param_ranges()
        .into_iter()
        .map(|(start, end)| ParameterInformation {
            label: ParameterLabel::LabelOffsets([utf16(start), utf16(end)]),
            documentation: None,
        })
        .collect::<Vec<_>>();
    let active = u32::try_from(argument.min(parameters.len().saturating_sub(1))).ok();
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation: (!function.doc.is_empty()).then(|| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: function.doc.clone(),
                })
            }),
            parameters: Some(parameters),
            active_parameter: active,
        }],
        active_signature: Some(0),
        active_parameter: active,
    })
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
        documentation: doc.filter(|d| !d.is_empty()).map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d.to_string(),
            })
        }),
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
                type_doc(t),
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
        if symbol.kind == SymbolKind::Function {
            if let Some(function) = function_named(document, word) {
                return Some((range, function_markdown(&function)));
            }
        }
        let types = types.expect("resolved through types");
        return Some((range, describe(document, types, symbol)));
    }
    if let Some(function) = function_named(document, word) {
        return Some((range, function_markdown(&function)));
    }
    if KEYWORDS.contains(&word) || CONTEXTUAL_KEYWORDS.contains(&word) {
        return keyword_doc(word).map(|doc| (range, doc.to_string()));
    }
    type_doc(word).map(|doc| (range, doc.to_string()))
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
    if symbol.kind == SymbolKind::Function {
        if let Some(doc) = prelude_doc(&symbol.name) {
            text.push_str(&format!("\n\n{doc}\n\n*standard library*"));
        }
    }
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
        .filter(|s| {
            s.scope.start == 0 && s.scope.end == usize::MAX && s.span.end <= document.text().len()
        })
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
    fn completes_library_functions_with_parameters_and_docs() {
        let doc = document("var n: int[0..=9999]\nrequire dig");
        let items = completions(&doc, doc.text().len());
        let digits = items
            .iter()
            .find(|i| i.label == "digits")
            .expect("standard library");
        assert_eq!(
            digits.detail.as_deref(),
            Some("digits(n: int, k: int) -> [int]")
        );
        assert_eq!(
            digits.insert_text.as_deref(),
            Some("digits(${1:n}, ${2:k})")
        );
        let Some(Documentation::MarkupContent(doc)) = &digits.documentation else {
            panic!("markdown docs")
        };
        assert!(doc.value.contains("decimal digits") && doc.value.contains("standard library"));
        let abs = items.iter().find(|i| i.label == "abs").expect("built-in");
        assert_eq!(abs.insert_text.as_deref(), Some("abs(${1:x})"));
    }

    #[test]
    fn helps_with_signatures_while_typing_arguments() {
        // As when typing: the program parsed a moment ago, and the call is not finished yet.
        let before = "/// Clamped to 0..=9.\nfn squash(v: int, top: int) -> int { return min(v, top) }\nvar x: int[0..=20]\n";
        let text = format!("{before}require squash(x, ");
        let text = text.as_str();
        let doc = Document::update(Some(document(before)), 2, text.to_string(), None);
        let help = signature_help(&doc, text.len()).expect("inside a call");
        let signature = &help.signatures[0];
        assert_eq!(signature.label, "squash(v: int, top: int) -> int");
        assert_eq!(help.active_parameter, Some(1));
        let Some(ParameterLabel::LabelOffsets([start, end])) =
            signature.parameters.as_ref().map(|p| p[1].label.clone())
        else {
            panic!("offsets")
        };
        assert_eq!(&signature.label[start as usize..end as usize], "top: int");
        let (_, hover_text) = hover(&doc, text.find("squash(x").expect("call")).expect("hover");
        assert!(hover_text.contains("Clamped to 0..=9.") && hover_text.contains("your function"));
        let library = document("require digit_sum(4096, ");
        let help = signature_help(&library, library.text().len()).expect("prelude call");
        assert_eq!(help.active_parameter, Some(1));
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
        assert!(text.contains("**targets**"), "{text}");
        let doc = document("var d: int[0..=9]\nsolve d\n");
        let (_, text) = hover(&doc, position_of(&doc, "int")).unwrap();
        assert!(text.contains("Arithmetic is exact"), "{text}");
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
