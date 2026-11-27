//! Source files, byte spans, and line/column lookup.
//!
//! A program can span several files: the main file, the standard library prelude and imported
//! files. Every file occupies its own range of one shared offset space (the main file starts at
//! 0, the others after it), so a [`Span`] identifies a location in any file without naming the
//! file. [`SourceMap::file_at`] finds the file an offset belongs to.

use std::path::{Path, PathBuf};

/// A half-open byte range `start..end` in the shared offset space of a [`SourceMap`].
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

/// A named source text with fast line lookup. Offsets passed to its methods are global (they
/// include the file's [`base`](SourceFile::base)).
#[derive(Clone, Debug)]
pub struct SourceFile {
    name: String,
    text: String,
    line_starts: Vec<usize>,
    base: usize,
    path: Option<PathBuf>,
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
            base: 0,
            path: None,
        }
    }

    /// Records the file's location on disk, which `import`s are resolved against.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
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

    /// The file's location on disk, if it has one.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The global offset of the file's first byte.
    #[must_use]
    pub fn base(&self) -> usize {
        self.base
    }

    /// Whether the global `offset` lies in this file (its end included).
    #[must_use]
    pub fn contains(&self, offset: usize) -> bool {
        (self.base..=self.base + self.text.len()).contains(&offset)
    }

    /// The text covered by `span`.
    #[must_use]
    pub fn slice(&self, span: Span) -> &str {
        &self.text[span.start - self.base..span.end - self.base]
    }

    /// The one-based line and column (in characters) of the global `offset`.
    #[must_use]
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.saturating_sub(self.base).min(self.text.len());
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let column = self.text[self.line_starts[line]..offset].chars().count();
        (line + 1, column + 1)
    }

    /// The text of the one-based line `line`, without its line break.
    #[must_use]
    pub fn line_text(&self, line: usize) -> &str {
        let end = self.line_end(line) - self.base;
        self.text[self.line_starts[line - 1]..end].trim_end_matches('\r')
    }

    /// The global offset where the one-based line `line` ends (before its line break).
    #[must_use]
    pub fn line_end(&self, line: usize) -> usize {
        self.base
            + self
                .line_starts
                .get(line)
                .map_or(self.text.len(), |&next| next - 1)
    }
}

/// All the files of a program, each in its own range of the shared offset space.
#[derive(Clone, Debug)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// A map holding `main` (at offset 0).
    #[must_use]
    pub fn new(mut main: SourceFile) -> Self {
        main.base = 0;
        Self { files: vec![main] }
    }

    /// Adds a file after the existing ones and returns it.
    pub fn add(&mut self, mut file: SourceFile) -> &SourceFile {
        let last = self.files.last().expect("the main file");
        file.base = last.base + last.text.len() + 1;
        self.files.push(file);
        self.files.last().expect("just pushed")
    }

    /// The main file.
    #[must_use]
    pub fn main(&self) -> &SourceFile {
        &self.files[0]
    }

    /// All files, the main one first.
    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// The file containing the global `offset`.
    #[must_use]
    pub fn file_at(&self, offset: usize) -> &SourceFile {
        let index = self
            .files
            .partition_point(|file| file.base <= offset)
            .saturating_sub(1);
        &self.files[index]
    }

    /// The text covered by `span`.
    #[must_use]
    pub fn slice(&self, span: Span) -> &str {
        self.file_at(span.start).slice(span)
    }
}

impl From<SourceFile> for SourceMap {
    fn from(file: SourceFile) -> Self {
        Self::new(file)
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

    #[test]
    fn files_share_one_offset_space() {
        let mut map = SourceMap::new(SourceFile::new("main.rsat", "abc\n"));
        let lib = map.add(SourceFile::new("lib.rsat", "fn f()\n{}\n"));
        let base = lib.base();
        assert_eq!(base, 5);
        assert_eq!(map.file_at(2).name(), "main.rsat");
        assert_eq!(map.file_at(base + 3).name(), "lib.rsat");
        assert_eq!(map.file_at(base + 7).line_col(base + 7), (2, 1));
        assert_eq!(map.slice(Span::new(base, base + 2)), "fn");
    }
}
