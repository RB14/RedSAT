//! Errors, warnings and notes, rendered against the source in the style of `rustc`.

use std::fmt::Write;

use crate::source::{SourceMap, Span};

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The program cannot be compiled or run.
    Error,
    /// Something suspicious that does not stop the program.
    Warning,
    /// Information, such as why a problem has no solution.
    Note,
}

/// A message about a span of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// How serious it is.
    pub severity: Severity,
    /// The main message.
    pub message: String,
    /// Where it applies.
    pub span: Span,
    /// A short label printed under the highlighted span.
    pub label: Option<String>,
    /// Further places that belong to the message, each with a label.
    pub related: Vec<(Span, String)>,
    /// Extra lines printed after the snippets, such as hints.
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// An error at `span`.
    #[must_use]
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
            label: None,
            related: vec![],
            notes: vec![],
        }
    }

    /// A warning at `span`.
    #[must_use]
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(span, message)
        }
    }

    /// A note at `span`.
    #[must_use]
    pub fn note(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Note,
            ..Self::error(span, message)
        }
    }

    /// Adds a label under the highlighted span.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds another highlighted place.
    #[must_use]
    pub fn with_related(mut self, span: Span, label: impl Into<String>) -> Self {
        self.related.push((span, label.into()));
        self
    }

    /// Adds a `help:` line.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.notes.push(format!("help: {}", help.into()));
        self
    }

    /// Adds a `note:` line.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(format!("note: {}", note.into()));
        self
    }

    /// Whether this is an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// Renders the diagnostic with source snippets, each taken from the file it points into:
    ///
    /// ```text
    /// error: unknown variable `z`
    ///  --> prog.rsat:3:9
    ///   |
    /// 3 | require z > 1
    ///   |         ^ not declared
    /// ```
    #[must_use]
    pub fn render(&self, sources: &SourceMap) -> String {
        let places: Vec<(Span, Option<&str>)> = std::iter::once((self.span, self.label.as_deref()))
            .chain(
                self.related
                    .iter()
                    .map(|(span, label)| (*span, Some(label.as_str()))),
            )
            .collect();
        let widest_line = places
            .iter()
            .map(|(span, _)| sources.file_at(span.start).line_col(span.start).0)
            .max()
            .unwrap_or(1);
        let gutter = " ".repeat(widest_line.to_string().len());
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        };

        let mut out = String::new();
        let _ = writeln!(out, "{severity}: {}", self.message);
        for (index, (span, label)) in places.iter().enumerate() {
            let file = sources.file_at(span.start);
            let (line, column) = file.line_col(span.start);
            let line_text = file.line_text(line);
            // Underline the span's part on its first line, at least one character wide.
            let line_end = file.line_end(line);
            let underline_end = span.end.clamp(span.start, line_end.max(span.start));
            let underline_len = file
                .slice(Span::new(span.start, underline_end))
                .chars()
                .count()
                .max(1);
            let arrow = if index == 0 { "-->" } else { ":::" };
            let _ = writeln!(out, "{gutter}{arrow} {}:{line}:{column}", file.name());
            let _ = writeln!(out, "{gutter} |");
            let _ = writeln!(out, "{line:>width$} | {line_text}", width = gutter.len());
            let _ = write!(
                out,
                "{gutter} | {}{}",
                " ".repeat(column - 1),
                "^".repeat(underline_len)
            );
            if let Some(label) = label.filter(|label| !label.is_empty()) {
                let _ = write!(out, " {label}");
            }
            out.push('\n');
        }
        for note in &self.notes {
            let _ = writeln!(out, "{gutter} = {note}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceFile;

    #[test]
    fn renders_a_snippet_with_caret_and_notes() {
        let source = SourceFile::new("prog.rsat", "var x\nrequire z > 1\n");
        let diagnostic = Diagnostic::error(Span::new(14, 15), "unknown variable `z`")
            .with_label("not declared")
            .with_help("declare it with `var z`");
        assert_eq!(
            diagnostic.render(&source.into()),
            "error: unknown variable `z`\n --> prog.rsat:2:9\n  |\n2 | require z > 1\n  |         ^ \
             not declared\n  = help: declare it with `var z`\n"
        );
    }

    #[test]
    fn renders_related_places() {
        let source = SourceFile::new("prog.rsat", "require a\nrequire b\n");
        let diagnostic = Diagnostic::note(Span::new(8, 9), "these conflict")
            .with_related(Span::new(18, 19), "and this");
        assert_eq!(
            diagnostic.render(&source.into()),
            "note: these conflict\n --> prog.rsat:1:9\n  |\n1 | require a\n  |         ^\n \
             ::: prog.rsat:2:9\n  |\n2 | require b\n  |         ^ and this\n"
        );
    }
}
