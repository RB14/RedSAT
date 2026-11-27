//! Turning source text into tokens.
//!
//! Statements end at line breaks, so the lexer emits [`TokenKind::Newline`] tokens. Line breaks
//! inside parentheses or square brackets are ignored, which lets long expressions span several
//! lines (as in Python). Consecutive line breaks collapse into one token.

use num_bigint::BigInt;
use num_traits::Num;

use crate::diagnostic::Diagnostic;
use crate::source::Span;

/// The kind of a token, with its payload for literals and names.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)] // the variant names say it all
pub enum TokenKind {
    Ident(String),
    Int(BigInt),
    /// A string literal's raw contents between the quotes (escapes and `{…}` not yet processed).
    Str(String),
    Char(char),

    // Keywords.
    Var,
    Fn,
    Return,
    Require,
    Solve,
    Print,
    If,
    Else,
    For,
    In,
    While,
    Enum,
    Struct,
    And,
    Or,
    Not,
    Xor,
    Implies,
    Iff,
    True,
    False,

    // Punctuation and operators.
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Semicolon,
    Question,
    Dot,
    DotDot,
    DotDotEq,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Amp,
    Pipe,
    Shl,
    Shr,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Assign,

    Newline,
    Eof,
}

impl TokenKind {
    /// How the token is described in error messages.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Ident(name) => format!("name `{name}`"),
            Self::Int(value) => format!("number `{value}`"),
            Self::Str(_) => "string".into(),
            Self::Char(_) => "character".into(),
            Self::Newline => "end of line".into(),
            Self::Eof => "end of file".into(),
            other => format!("`{}`", other.text()),
        }
    }

    fn text(&self) -> &'static str {
        match self {
            Self::Var => "var",
            Self::Fn => "fn",
            Self::Return => "return",
            Self::Require => "require",
            Self::Solve => "solve",
            Self::Print => "print",
            Self::If => "if",
            Self::Else => "else",
            Self::For => "for",
            Self::In => "in",
            Self::While => "while",
            Self::Enum => "enum",
            Self::Struct => "struct",
            Self::And => "and",
            Self::Or => "or",
            Self::Not => "not",
            Self::Xor => "xor",
            Self::Implies => "implies",
            Self::Iff => "iff",
            Self::True => "true",
            Self::False => "false",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::Comma => ",",
            Self::Colon => ":",
            Self::Semicolon => ";",
            Self::Question => "?",
            Self::Dot => ".",
            Self::DotDot => "..",
            Self::DotDotEq => "..=",
            Self::Arrow => "->",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::Star => "*",
            Self::Slash => "/",
            Self::Percent => "%",
            Self::Caret => "^",
            Self::Amp => "&",
            Self::Pipe => "|",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::EqEq => "==",
            Self::NotEq => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Assign => "=",
            _ => "?",
        }
    }
}

/// The keyword token for `word`, if it is one of [`crate::syntax::KEYWORDS`].
#[must_use]
pub fn keyword(word: &str) -> Option<TokenKind> {
    Some(match word {
        "var" => TokenKind::Var,
        "fn" => TokenKind::Fn,
        "return" => TokenKind::Return,
        "require" => TokenKind::Require,
        "solve" => TokenKind::Solve,
        "print" => TokenKind::Print,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "for" => TokenKind::For,
        "in" => TokenKind::In,
        "while" => TokenKind::While,
        "enum" => TokenKind::Enum,
        "struct" => TokenKind::Struct,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "xor" => TokenKind::Xor,
        "implies" => TokenKind::Implies,
        "iff" => TokenKind::Iff,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        _ => return None,
    })
}

/// A token and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    /// What it is.
    pub kind: TokenKind,
    /// Where it is.
    pub span: Span,
}

/// Splits `text` into tokens. `base` is the offset of `text` within its file, which is needed
/// when lexing a fragment such as an interpolated expression inside a string.
///
/// # Errors
///
/// Returns a diagnostic for the first malformed token.
pub fn lex(text: &str, base: usize) -> Result<Vec<Token>, Diagnostic> {
    Lexer {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        base,
        tokens: Vec::new(),
        brackets: Vec::new(),
    }
    .run()
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    base: usize,
    tokens: Vec<Token>,
    brackets: Vec<u8>,
}

impl Lexer<'_> {
    fn run(mut self) -> Result<Vec<Token>, Diagnostic> {
        while let Some(&byte) = self.bytes.get(self.pos) {
            let start = self.pos;
            match byte {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.newline(start);
                    self.pos += 1;
                }
                b'/' if self.peek(1) == Some(b'/') => {
                    while self.bytes.get(self.pos).is_some_and(|&b| b != b'\n') {
                        self.pos += 1;
                    }
                }
                b'/' if self.peek(1) == Some(b'*') => self.block_comment()?,
                b'0'..=b'9' => self.number()?,
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.word(),
                b'"' => self.string()?,
                b'\'' => self.char_literal()?,
                _ => self.punctuation()?,
            }
        }
        let end = self.text.len();
        self.newline(end);
        self.push(TokenKind::Eof, end, end);
        Ok(self.tokens)
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.base + start, self.base + end)
    }

    fn error(&self, start: usize, end: usize, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(self.span(start, end), message)
    }

    fn push(&mut self, kind: TokenKind, start: usize, end: usize) {
        let span = self.span(start, end);
        self.tokens.push(Token { kind, span });
    }

    /// Records a statement-ending line break, unless it is inside `(…)` or `[…]` or redundant.
    fn newline(&mut self, at: usize) {
        if matches!(self.brackets.last(), Some(b'(' | b'[')) {
            return;
        }
        if self
            .tokens
            .last()
            .is_none_or(|token| token.kind == TokenKind::Newline)
        {
            return;
        }
        self.push(TokenKind::Newline, at, at);
    }

    fn block_comment(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        let Some(length) = self.text[start + 2..].find("*/") else {
            return Err(self.error(start, start + 2, "unterminated block comment"));
        };
        let end = start + 2 + length + 2;
        if self.text[start..end].contains('\n') {
            self.newline(start);
        }
        self.pos = end;
        Ok(())
    }

    fn number(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        let (radix, digits_start) = match (self.bytes[start], self.peek(1)) {
            (b'0', Some(b'x')) => (16, start + 2),
            (b'0', Some(b'b')) => (2, start + 2),
            _ => (10, start),
        };
        self.pos = digits_start;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        let digits: String = self.text[digits_start..self.pos]
            .chars()
            .filter(|&c| c != '_')
            .collect();
        let value = BigInt::from_str_radix(&digits, radix).map_err(|_| {
            self.error(
                start,
                self.pos,
                format!("invalid number `{}`", &self.text[start..self.pos]),
            )
        })?;
        self.push(TokenKind::Int(value), start, self.pos);
        Ok(())
    }

    fn word(&mut self) {
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.pos += 1;
        }
        let word = &self.text[start..self.pos];
        let kind = keyword(word).unwrap_or_else(|| TokenKind::Ident(word.to_string()));
        self.push(kind, start, self.pos);
    }

    fn string(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        self.pos += 1;
        loop {
            match self.bytes.get(self.pos) {
                None | Some(b'\n') => {
                    return Err(self
                        .error(start, self.pos, "unterminated string")
                        .with_help("strings must close with `\"` on the same line"));
                }
                Some(b'\\') => self.pos += 2,
                Some(b'"') => break,
                Some(_) => self.pos += 1,
            }
        }
        let contents = self.text[start + 1..self.pos].to_string();
        self.pos += 1;
        self.push(TokenKind::Str(contents), start, self.pos);
        Ok(())
    }

    fn char_literal(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        let rest = &self.text[start + 1..];
        let mut chars = rest.chars();
        let (value, consumed) = match chars.next() {
            Some('\\') => match chars.next().and_then(unescape) {
                Some(value) => (value, 2),
                None => return Err(self.error(start, start + 3, "unknown escape in character")),
            },
            Some(c) if c != '\'' && c != '\n' => (c, c.len_utf8()),
            _ => {
                return Err(self.error(
                    start,
                    start + 1,
                    "empty or unterminated character literal",
                ));
            }
        };
        if rest[consumed..].starts_with('\'') {
            self.pos = start + 1 + consumed + 1;
            self.push(TokenKind::Char(value), start, self.pos);
            Ok(())
        } else {
            Err(self.error(
                start,
                start + 1 + consumed,
                "unterminated character literal",
            ))
        }
    }

    fn punctuation(&mut self) -> Result<(), Diagnostic> {
        let start = self.pos;
        let rest = &self.text[start..];
        const MULTI: &[(&str, TokenKind)] = &[
            ("..=", TokenKind::DotDotEq),
            ("..", TokenKind::DotDot),
            ("->", TokenKind::Arrow),
            ("==", TokenKind::EqEq),
            ("!=", TokenKind::NotEq),
            ("<=", TokenKind::Le),
            (">=", TokenKind::Ge),
            ("<<", TokenKind::Shl),
            (">>", TokenKind::Shr),
        ];
        if let Some((text, kind)) = MULTI.iter().find(|(text, _)| rest.starts_with(text)) {
            self.pos += text.len();
            self.push(kind.clone(), start, self.pos);
            return Ok(());
        }
        for (wrong, right) in [("&&", "and"), ("||", "or")] {
            if rest.starts_with(wrong) {
                return Err(self
                    .error(start, start + 2, format!("unknown operator `{wrong}`"))
                    .with_help(format!("RedSAT spells it `{right}`")));
            }
        }
        let byte = self.bytes[start];
        let kind = match byte {
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b',' => TokenKind::Comma,
            b':' => TokenKind::Colon,
            b';' => TokenKind::Semicolon,
            b'?' => TokenKind::Question,
            b'.' => TokenKind::Dot,
            b'&' => TokenKind::Amp,
            b'|' => TokenKind::Pipe,
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            b'^' => TokenKind::Caret,
            b'<' => TokenKind::Lt,
            b'>' => TokenKind::Gt,
            b'=' => TokenKind::Assign,
            b'!' => {
                return Err(self
                    .error(start, start + 1, "unknown operator `!`")
                    .with_help("use `not` for negation and `!=` for inequality"));
            }
            _ => {
                let found = rest.chars().next().expect("not at end");
                return Err(self.error(
                    start,
                    start + found.len_utf8(),
                    format!("unexpected character `{found}`"),
                ));
            }
        };
        match byte {
            b'(' | b'[' | b'{' => self.brackets.push(byte),
            b')' | b']' | b'}' => {
                self.brackets.pop();
            }
            _ => {}
        }
        self.pos += 1;
        self.push(kind, start, self.pos);
        Ok(())
    }
}

/// The character an escape sequence `\c` stands for.
#[must_use]
pub fn unescape(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        '\\' | '"' | '\'' | '{' | '}' => c,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::KEYWORDS;

    fn kinds(text: &str) -> Vec<TokenKind> {
        lex(text, 0)
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn every_keyword_lexes_to_its_token() {
        for &word in KEYWORDS {
            let kind = keyword(word).expect("keyword table is complete");
            assert_eq!(
                kinds(word),
                [kind.clone(), TokenKind::Newline, TokenKind::Eof]
            );
            assert_eq!(kind.text(), word);
        }
    }

    #[test]
    fn lexes_numbers_in_three_bases() {
        use TokenKind::{Eof, Int, Newline};
        assert_eq!(
            kinds("1_000 0xff 0b1010"),
            [
                Int(1000.into()),
                Int(255.into()),
                Int(10.into()),
                Newline,
                Eof
            ]
        );
        assert!(lex("12abc", 0).is_err());
    }

    #[test]
    fn ranges_are_not_mistaken_for_numbers() {
        use TokenKind::{DotDot, DotDotEq, Eof, Int, Newline};
        assert_eq!(
            kinds("0..9"),
            [Int(0.into()), DotDot, Int(9.into()), Newline, Eof]
        );
        assert_eq!(
            kinds("1..=6"),
            [Int(1.into()), DotDotEq, Int(6.into()), Newline, Eof]
        );
    }

    #[test]
    fn newlines_end_statements_except_inside_parentheses() {
        use TokenKind::{Eof, Ident, LParen, Newline, Plus, RParen};
        let name = |n: &str| Ident(n.into());
        assert_eq!(
            kinds("a\n\n\nb"),
            [name("a"), Newline, name("b"), Newline, Eof]
        );
        assert_eq!(
            kinds("(a\n+ b)\nc"),
            [
                LParen,
                name("a"),
                Plus,
                name("b"),
                RParen,
                Newline,
                name("c"),
                Newline,
                Eof
            ]
        );
    }

    #[test]
    fn comments_are_skipped() {
        use TokenKind::{Eof, Ident, Newline};
        assert_eq!(
            kinds("a // note\nb /* x */ c"),
            [
                Ident("a".into()),
                Newline,
                Ident("b".into()),
                Ident("c".into()),
                Newline,
                Eof
            ]
        );
        assert!(lex("/* never closed", 0).is_err());
    }

    #[test]
    fn strings_keep_their_raw_contents() {
        assert_eq!(
            kinds(r#""x = {x}\n""#)[0],
            TokenKind::Str(r"x = {x}\n".into())
        );
        assert!(lex("\"open", 0).is_err());
        assert_eq!(
            kinds(r"'\n' 'q'")[..2],
            [TokenKind::Char('\n'), TokenKind::Char('q')]
        );
    }

    #[test]
    fn suggests_word_operators() {
        let error = lex("a && b", 0).unwrap_err();
        assert!(error.notes[0].contains("`and`"));
        assert!(lex("!a", 0).is_err());
    }

    #[test]
    fn spans_include_the_base_offset() {
        let tokens = lex("x", 10).unwrap();
        assert_eq!(tokens[0].span, Span::new(10, 11));
    }
}
