//! Errors and warnings, rendered against the source in the style of `rustc`.

use std::fmt::Write;

use crate::source::{SourceFile, Span};

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The program cannot be compiled or run.
    Error,
    /// Something suspicious that does not stop the program.
    Warning,
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
    /// Extra lines printed after the snippet, such as hints.
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

    /// Adds a label under the highlighted span.
    #[must_use]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
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

    /// Renders the diagnostic with a source snippet:
    ///
    /// ```text
    /// error: unknown variable `z`
    ///  --> prog.rsat:3:9
    ///   |
    /// 3 | require z > 1
    ///   |         ^ not declared
    /// ```
    #[must_use]
    pub fn render(&self, source: &SourceFile) -> String {
        let (line, column) = source.line_col(self.span.start);
        let line_text = source.line_text(line);
        let gutter = " ".repeat(line.to_string().len());
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        // Underline the span's part on its first line, at least one character wide.
        let line_end = source.line_end(line);
        let underline_end = self
            .span
            .end
            .clamp(self.span.start, line_end.max(self.span.start));
        let underline_len = source.text()[self.span.start..underline_end]
            .chars()
            .count()
            .max(1);

        let mut out = String::new();
        let _ = writeln!(out, "{severity}: {}", self.message);
        let _ = writeln!(out, "{gutter}--> {}:{line}:{column}", source.name());
        let _ = writeln!(out, "{gutter} |");
        let _ = writeln!(out, "{line} | {line_text}");
        let _ = write!(
            out,
            "{gutter} | {}{}",
            " ".repeat(column - 1),
            "^".repeat(underline_len)
        );
        if let Some(label) = &self.label {
            let _ = write!(out, " {label}");
        }
        out.push('\n');
        for note in &self.notes {
            let _ = writeln!(out, "{gutter} = {note}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_snippet_with_caret_and_notes() {
        let source = SourceFile::new("prog.rsat", "var x\nrequire z > 1\n");
        let diagnostic = Diagnostic::error(Span::new(14, 15), "unknown variable `z`")
            .with_label("not declared")
            .with_help("declare it with `var z`");
        assert_eq!(
            diagnostic.render(&source),
            "error: unknown variable `z`\n --> prog.rsat:2:9\n  |\n2 | require z > 1\n  |         ^ \
             not declared\n  = help: declare it with `var z`\n"
        );
    }
}
