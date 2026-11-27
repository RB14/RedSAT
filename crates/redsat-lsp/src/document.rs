//! Open documents: their text, line index and latest analysis.

use std::path::PathBuf;
use std::time::Duration;

use lsp_types::{Position, Range};
use redsat_lang::check::TypeInfo;
use redsat_lang::config::{find_config, load_config};
use redsat_lang::signatures::FunctionInfo;
use redsat_lang::{Analysis, CompilerOptions, OptionLayer, SourceFile, Span};

/// How long elaborating a program for editor features may take.
const ANALYSIS_BUDGET: Duration = Duration::from_millis(400);

/// Maps byte offsets to LSP positions (lines and UTF-16 code units) and back.
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    /// Indexes `text`.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            starts: std::iter::once(0)
                .chain(text.match_indices('\n').map(|(i, _)| i + 1))
                .collect(),
        }
    }

    /// The position of byte `offset`.
    #[must_use]
    pub fn position(&self, text: &str, offset: usize) -> Position {
        let offset = offset.min(text.len());
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let character = text[self.starts[line]..offset].encode_utf16().count();
        Position::new(
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(character).unwrap_or(u32::MAX),
        )
    }

    /// The byte offset of `position`, clamped to the text.
    #[must_use]
    pub fn offset(&self, text: &str, position: Position) -> usize {
        let Some(&start) = self.starts.get(position.line as usize) else {
            return text.len();
        };
        let end = self
            .starts
            .get(position.line as usize + 1)
            .map_or(text.len(), |&next| next);
        let mut units = 0;
        for (index, c) in text[start..end].char_indices() {
            if units >= position.character as usize || c == '\n' {
                return start + index;
            }
            units += c.len_utf16();
        }
        end
    }

    /// The range of `span`.
    #[must_use]
    pub fn range(&self, text: &str, span: Span) -> Range {
        Range::new(
            self.position(text, span.start),
            self.position(text, span.end),
        )
    }
}

/// An open document.
pub struct Document {
    /// The document's version, as sent by the client.
    pub version: i32,
    /// The source text.
    pub source: SourceFile,
    /// Line index of the text.
    pub index: LineIndex,
    /// The latest analysis.
    pub analysis: Analysis,
    /// Types and symbols from the latest version that type-checked, used for completion while
    /// the text is temporarily broken.
    pub last_types: Option<TypeInfo>,
    /// Functions (with signatures and docs) from the latest version that parsed.
    pub last_functions: Vec<FunctionInfo>,
}

impl Document {
    /// Analyses a new version of the document at `path` (if it is a local file).
    pub fn update(
        previous: Option<Document>,
        version: i32,
        text: String,
        path: Option<PathBuf>,
    ) -> Self {
        let name = path.as_ref().and_then(|path| path.file_name()).map_or_else(
            || "untitled.rsat".to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let options = options_for(path.as_ref());
        let index = LineIndex::new(&text);
        let mut source = SourceFile::new(name, text);
        if let Some(path) = &path {
            source = source.with_path(path);
        }
        let analysis = redsat_lang::analyze(&source, &options, ANALYSIS_BUDGET);
        let has_errors = analysis
            .diagnostics
            .iter()
            .any(redsat_lang::Diagnostic::is_error);
        let previous_functions = previous
            .as_ref()
            .map(|previous| previous.last_functions.clone());
        let last_types = match (&analysis.types, previous) {
            (Some(types), _) if !has_errors || !types.symbols.is_empty() => Some(types.clone()),
            (_, Some(previous)) => previous.last_types,
            _ => None,
        };
        // While the text does not parse, keep the functions of the last version that did (or at
        // least the standard library's).
        let last_functions = if !analysis.functions.is_empty() {
            analysis.functions.clone()
        } else {
            previous_functions
                .filter(|functions| !functions.is_empty())
                .unwrap_or_else(|| redsat_lang::signatures::prelude_functions().to_vec())
        };
        Self {
            version,
            source,
            index,
            analysis,
            last_types,
            last_functions,
        }
    }

    /// The text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.source.text()
    }

    /// The range of `span`.
    #[must_use]
    pub fn range(&self, span: Span) -> Range {
        self.index.range(self.text(), span)
    }

    /// The byte offset of `position`.
    #[must_use]
    pub fn offset(&self, position: Position) -> usize {
        self.index.offset(self.text(), position)
    }
}

/// The compiler options for a document: from the nearest `redsat.toml`, if any.
fn options_for(path: Option<&PathBuf>) -> CompilerOptions {
    let layer = path
        .and_then(|path| path.parent())
        .and_then(find_config)
        .and_then(|config| load_config(&config).ok())
        .unwrap_or_default();
    CompilerOptions::resolve(&layer, &OptionLayer::default()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units() {
        let text = "var x\nprint \"× {x}\"\n";
        let index = LineIndex::new(text);
        let brace = text.find('{').unwrap();
        assert_eq!(index.position(text, brace), Position::new(1, 9));
        assert_eq!(index.offset(text, Position::new(1, 9)), brace);
        assert_eq!(
            index.offset(text, Position::new(0, 99)),
            5,
            "clamped to the line end"
        );
        assert_eq!(index.offset(text, Position::new(9, 0)), text.len());
        let emoji = "😀x";
        assert_eq!(
            LineIndex::new(emoji).position(emoji, 4),
            Position::new(0, 2)
        );
    }
}
