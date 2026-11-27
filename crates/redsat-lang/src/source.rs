//! Source files, byte spans, and line/column lookup.

/// A half-open byte range `start..end` in a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// Offset of the first byte.
    pub start: usize,
    /// Offset one past the last byte.
    pub end: usize,
}

impl Span {
    /// The span `start..end`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The smallest span covering both `self` and `other`.
    #[must_use]
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// A named source text with fast line lookup.
#[derive(Clone, Debug)]
pub struct SourceFile {
    name: String,
    text: String,
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// Wraps `text`, which came from the file (or other origin) called `name`.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(index, _)| index + 1))
            .collect();
        Self {
            name: name.into(),
            text,
            line_starts,
        }
    }

    /// The name shown in diagnostics.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The full source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text covered by `span`.
    #[must_use]
    pub fn slice(&self, span: Span) -> &str {
        &self.text[span.start..span.end]
    }

    /// The one-based line and column (in characters) of byte `offset`.
    #[must_use]
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let column = self.text[self.line_starts[line]..offset].chars().count();
        (line + 1, column + 1)
    }

    /// The text of the one-based line `line`, without its line break.
    #[must_use]
    pub fn line_text(&self, line: usize) -> &str {
        self.text[self.line_starts[line - 1]..self.line_end(line)].trim_end_matches('\r')
    }

    /// The byte offset where the one-based line `line` ends (before its line break).
    #[must_use]
    pub fn line_end(&self, line: usize) -> usize {
        self.line_starts
            .get(line)
            .map_or(self.text.len(), |&next| next - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_offsets_to_lines_and_columns() {
        let file = SourceFile::new("t.rsat", "var x\nrequire x × 2\n");
        assert_eq!(file.line_col(0), (1, 1));
        assert_eq!(file.line_col(4), (1, 5));
        assert_eq!(file.line_col(6), (2, 1));
        let times = file.text().find('2').unwrap();
        assert_eq!(
            file.line_col(times),
            (2, 13),
            "columns count characters, not bytes"
        );
        assert_eq!(file.line_text(2), "require x × 2");
    }
}
