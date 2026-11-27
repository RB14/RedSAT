//! A recursive-descent parser from tokens to the [`ast`](crate::ast).
//!
//! Operator precedence, from loosest to tightest binding:
//!
//! | Operators                         | Associativity |
//! |-----------------------------------|---------------|
//! | `iff`                             | left          |
//! | `implies`                         | right         |
//! | `or`                              | left          |
//! | `xor`                             | left          |
//! | `and`                             | left          |
//! | `not`                             | prefix        |
//! | `== != < <= > >=`                 | none (no chaining) |
//! | `+ -`                             | left          |
//! | `* / %`                           | left          |
//! | unary `-`                         | prefix        |
//! | `^`                               | right         |
//! | calls `f(…)`                      | postfix       |

use crate::ast::{
    BinaryOp, Block, DeclId, Expr, ExprKind, Ident, Objective, Program, Range, Solve, SolveMode,
    Stmt, StmtKind, StrPart, TypeExpr, TypeKind, UnaryOp, VarDecl,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{self, Token, TokenKind};
use crate::source::{SourceFile, Span};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Parses a whole program.
///
/// # Errors
///
/// Returns a diagnostic for the first syntax error.
pub fn parse(source: &SourceFile) -> Result<Program> {
    let tokens = lexer::lex(source.text(), 0)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        next_decl: 0,
    };
    let stmts = parser.statements_until_eof()?;
    Ok(Program {
        stmts,
        num_decls: parser.next_decl,
    })
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    next_decl: usize,
}

impl Parser {
    // --- token helpers -------------------------------------------------------------------

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident(name) if name == word)
    }

    fn bump(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        if token.kind != TokenKind::Eof {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        let found = self.at(kind);
        if found {
            self.bump();
        }
        found
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token> {
        if self.at(kind) {
            Ok(self.bump())
        } else {
            Err(self.unexpected(what))
        }
    }

    fn unexpected(&self, expected: &str) -> Diagnostic {
        Diagnostic::error(
            self.peek_span(),
            format!("expected {expected}, found {}", self.peek().describe()),
        )
    }

    fn previous_span(&self) -> Span {
        self.tokens[self.pos.saturating_sub(1)].span
    }

    fn ident(&mut self, what: &str) -> Result<Ident> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            _ => Err(self.unexpected(what)),
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), TokenKind::Newline | TokenKind::Semicolon) {
            self.bump();
        }
    }

    // --- statements ----------------------------------------------------------------------

    fn statements_until_eof(&mut self) -> Result<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            self.skip_separators();
            if self.at(&TokenKind::Eof) {
                return Ok(stmts);
            }
            if self.at(&TokenKind::RBrace) {
                return Err(Diagnostic::error(self.peek_span(), "unmatched `}`"));
            }
            stmts.push(self.statement()?);
            self.end_of_statement()?;
        }
    }

    fn block(&mut self) -> Result<Block> {
        let open = self.expect(&TokenKind::LBrace, "`{`")?.span;
        let mut stmts = Vec::new();
        loop {
            self.skip_separators();
            if self.at(&TokenKind::RBrace) {
                let close = self.bump().span;
                return Ok(Block {
                    stmts,
                    span: open.to(close),
                });
            }
            if self.at(&TokenKind::Eof) {
                return Err(Diagnostic::error(open, "this `{` is never closed"));
            }
            stmts.push(self.statement()?);
            self.end_of_statement()?;
        }
    }

    fn end_of_statement(&mut self) -> Result<()> {
        match self.peek() {
            TokenKind::Newline | TokenKind::Semicolon => {
                self.bump();
                Ok(())
            }
            TokenKind::RBrace | TokenKind::Eof => Ok(()),
            _ => Err(self.unexpected("the end of the statement (a line break or `;`)")),
        }
    }

    fn statement(&mut self) -> Result<Stmt> {
        let start = self.peek_span();
        let kind = match self.peek() {
            TokenKind::Var => self.var_decl()?,
            TokenKind::Require => {
                self.bump();
                StmtKind::Require(self.expr()?)
            }
            TokenKind::Print => self.print()?,
            TokenKind::If => self.if_stmt()?,
            TokenKind::For => self.for_stmt()?,
            TokenKind::Solve => StmtKind::Solve(self.solve()?),
            TokenKind::Fn | TokenKind::Return => {
                return Err(Diagnostic::error(start, "functions are not available yet")
                    .with_note("user-defined functions arrive in Phase 3 of the roadmap"));
            }
            TokenKind::Ident(_) => self.assignment()?,
            _ => return Err(self.unexpected("a statement")),
        };
        Ok(Stmt {
            kind,
            span: start.to(self.previous_span()),
        })
    }

    fn var_decl(&mut self) -> Result<StmtKind> {
        self.bump(); // var
        let mut names = vec![self.declared_name()?];
        while self.eat(&TokenKind::Comma) {
            names.push(self.declared_name()?);
        }
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let init = if self.at(&TokenKind::Assign) {
            let assign = self.bump().span;
            if names.len() > 1 {
                return Err(Diagnostic::error(
                    assign,
                    "only a single variable can have an initial value",
                )
                .with_help("declare the variables on separate lines"));
            }
            Some(self.expr()?)
        } else {
            None
        };
        Ok(StmtKind::Var(VarDecl { names, ty, init }))
    }

    fn declared_name(&mut self) -> Result<(Ident, DeclId)> {
        let ident = self.ident("a variable name")?;
        let id = DeclId(self.next_decl);
        self.next_decl += 1;
        Ok((ident, id))
    }

    fn type_expr(&mut self) -> Result<TypeExpr> {
        let start = self.peek_span();
        if self.at(&TokenKind::LBracket) {
            return Err(
                Diagnostic::error(start, "array types are not available yet")
                    .with_note("arrays arrive in Phase 3 of the roadmap"),
            );
        }
        let name = self.ident("a type")?;
        let kind = match name.name.as_str() {
            "bool" => TypeKind::Bool,
            "int" if self.eat(&TokenKind::LBracket) => {
                let range = self.range()?;
                self.expect(&TokenKind::RBracket, "`]`")?;
                TypeKind::Int(Some(range))
            }
            "int" | "uint" if self.at(&TokenKind::Lt) => {
                self.bump();
                let bits = self.primary()?;
                self.expect(&TokenKind::Gt, "`>`")?;
                TypeKind::IntBits {
                    signed: name.name == "int",
                    bits: Box::new(bits),
                }
            }
            "int" => TypeKind::Int(None),
            "uint" => {
                return Err(self.unexpected("`<` after `uint` (e.g. `uint<8>`)"));
            }
            "char" | "string" => {
                return Err(Diagnostic::error(
                    name.span,
                    format!("`{}` is not available yet", name.name),
                )
                .with_note("characters and strings arrive in Phase 4 of the roadmap"));
            }
            other => {
                return Err(Diagnostic::error(name.span, format!("unknown type `{other}`"))
                    .with_help("the available types are `bool`, `int`, `int[lo..hi]`, `int<N>` and `uint<N>`"));
            }
        };
        Ok(TypeExpr {
            kind,
            span: start.to(self.previous_span()),
        })
    }

    fn range(&mut self) -> Result<Range> {
        let lo = self.expr()?;
        let inclusive = match self.peek() {
            TokenKind::DotDot => false,
            TokenKind::DotDotEq => true,
            _ => return Err(self.unexpected("`..` or `..=`")),
        };
        self.bump();
        let hi = self.expr()?;
        let span = lo.span.to(hi.span);
        Ok(Range {
            lo: Box::new(lo),
            hi: Box::new(hi),
            inclusive,
            span,
        })
    }

    fn print(&mut self) -> Result<StmtKind> {
        self.bump(); // print
        let mut args = Vec::new();
        if !matches!(
            self.peek(),
            TokenKind::Newline | TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
        ) {
            args.push(self.expr()?);
            while self.eat(&TokenKind::Comma) {
                args.push(self.expr()?);
            }
        }
        Ok(StmtKind::Print(args))
    }

    fn if_stmt(&mut self) -> Result<StmtKind> {
        self.bump(); // if
        let cond = self.expr()?;
        let then_block = self.block()?;
        let else_block = if self.eat(&TokenKind::Else) {
            if self.at(&TokenKind::If) {
                let start = self.peek_span();
                let kind = self.if_stmt()?;
                let span = start.to(self.previous_span());
                Some(Block {
                    stmts: vec![Stmt { kind, span }],
                    span,
                })
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };
        Ok(StmtKind::If {
            cond,
            then_block,
            else_block,
        })
    }

    fn for_stmt(&mut self) -> Result<StmtKind> {
        self.bump(); // for
        let var = self.ident("a loop variable")?;
        self.expect(&TokenKind::In, "`in`")?;
        let range = self.range()?;
        let body = self.block()?;
        Ok(StmtKind::For { var, range, body })
    }

    fn solve(&mut self) -> Result<Solve> {
        self.bump(); // solve
        let mode = if self.at_word("all") {
            self.bump();
            SolveMode::All
        } else if self.at_word("unique") {
            self.bump();
            SolveMode::Unique
        } else {
            SolveMode::First
        };
        let mut targets = Vec::new();
        if !self.at_solve_clause_end() {
            targets.push(self.expr()?);
            while self.eat(&TokenKind::Comma) {
                targets.push(self.expr()?);
            }
        }
        let mut limit = None;
        let mut objective = None;
        loop {
            if self.at_word("limit") && limit.is_none() {
                self.bump();
                limit = Some(self.expr()?);
            } else if (self.at_word("minimize") || self.at_word("maximize")) && objective.is_none()
            {
                let direction = if self.at_word("minimize") {
                    Objective::Minimize
                } else {
                    Objective::Maximize
                };
                self.bump();
                objective = Some((direction, self.expr()?));
            } else {
                break;
            }
        }
        let body = if self.at(&TokenKind::LBrace) {
            Some(self.block()?)
        } else {
            None
        };
        let else_block = if self.eat(&TokenKind::Else) {
            Some(self.block()?)
        } else {
            None
        };
        Ok(Solve {
            mode,
            targets,
            limit,
            objective,
            body,
            else_block,
        })
    }

    fn at_solve_clause_end(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Newline
                | TokenKind::Semicolon
                | TokenKind::LBrace
                | TokenKind::RBrace
                | TokenKind::Else
                | TokenKind::Eof
        ) || ["limit", "minimize", "maximize"]
            .iter()
            .any(|word| self.at_word(word))
    }

    fn assignment(&mut self) -> Result<StmtKind> {
        let target = self.ident("a variable")?;
        if !self.at(&TokenKind::Assign) {
            return Err(self.unexpected("`=`").with_help(
                "a bare expression does nothing on its own; did you mean `require …` or `print …`?",
            ));
        }
        self.bump();
        let value = self.expr()?;
        Ok(StmtKind::Assign { target, value })
    }

    // --- expressions ---------------------------------------------------------------------

    fn expr(&mut self) -> Result<Expr> {
        self.iff()
    }

    fn binary(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        let span = lhs.span.to(rhs.span);
        Expr {
            kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
            span,
        }
    }

    /// Parses a left-associative chain `next (op next)*` for the operators in `table`.
    fn left_assoc(
        &mut self,
        table: &[(TokenKind, BinaryOp)],
        next: fn(&mut Self) -> Result<Expr>,
    ) -> Result<Expr> {
        let mut lhs = next(self)?;
        while let Some(&(_, op)) = table.iter().find(|(kind, _)| self.at(kind)) {
            self.bump();
            let rhs = next(self)?;
            lhs = Self::binary(op, lhs, rhs);
        }
        Ok(lhs)
    }

    fn iff(&mut self) -> Result<Expr> {
        self.left_assoc(&[(TokenKind::Iff, BinaryOp::Iff)], Self::implies)
    }

    fn implies(&mut self) -> Result<Expr> {
        let lhs = self.or()?;
        if self.eat(&TokenKind::Implies) {
            let rhs = self.implies()?;
            return Ok(Self::binary(BinaryOp::Implies, lhs, rhs));
        }
        Ok(lhs)
    }

    fn or(&mut self) -> Result<Expr> {
        self.left_assoc(&[(TokenKind::Or, BinaryOp::Or)], Self::xor)
    }

    fn xor(&mut self) -> Result<Expr> {
        self.left_assoc(&[(TokenKind::Xor, BinaryOp::Xor)], Self::and)
    }

    fn and(&mut self) -> Result<Expr> {
        self.left_assoc(&[(TokenKind::And, BinaryOp::And)], Self::not)
    }

    fn not(&mut self) -> Result<Expr> {
        if self.at(&TokenKind::Not) {
            let start = self.bump().span;
            let operand = self.not()?;
            let span = start.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary(UnaryOp::Not, Box::new(operand)),
                span,
            });
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr> {
        const OPS: &[(TokenKind, BinaryOp)] = &[
            (TokenKind::EqEq, BinaryOp::Eq),
            (TokenKind::NotEq, BinaryOp::Ne),
            (TokenKind::Lt, BinaryOp::Lt),
            (TokenKind::Le, BinaryOp::Le),
            (TokenKind::Gt, BinaryOp::Gt),
            (TokenKind::Ge, BinaryOp::Ge),
        ];
        let lhs = self.additive()?;
        let Some(&(_, op)) = OPS.iter().find(|(kind, _)| self.at(kind)) else {
            return Ok(lhs);
        };
        self.bump();
        let rhs = self.additive()?;
        if OPS.iter().any(|(kind, _)| self.at(kind)) {
            return Err(
                Diagnostic::error(self.peek_span(), "comparisons cannot be chained")
                    .with_help("write `a < b and b < c` instead of `a < b < c`"),
            );
        }
        Ok(Self::binary(op, lhs, rhs))
    }

    fn additive(&mut self) -> Result<Expr> {
        self.left_assoc(
            &[
                (TokenKind::Plus, BinaryOp::Add),
                (TokenKind::Minus, BinaryOp::Sub),
            ],
            Self::multiplicative,
        )
    }

    fn multiplicative(&mut self) -> Result<Expr> {
        self.left_assoc(
            &[
                (TokenKind::Star, BinaryOp::Mul),
                (TokenKind::Slash, BinaryOp::Div),
                (TokenKind::Percent, BinaryOp::Mod),
            ],
            Self::unary,
        )
    }

    fn unary(&mut self) -> Result<Expr> {
        if self.at(&TokenKind::Minus) {
            let start = self.bump().span;
            let operand = self.unary()?;
            let span = start.to(operand.span);
            return Ok(Expr {
                kind: ExprKind::Unary(UnaryOp::Neg, Box::new(operand)),
                span,
            });
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr> {
        let base = self.postfix()?;
        if self.eat(&TokenKind::Caret) {
            let exponent = self.unary()?;
            return Ok(Self::binary(BinaryOp::Pow, base, exponent));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Expr> {
        let expr = self.primary()?;
        if self.at(&TokenKind::LBracket) {
            return Err(
                Diagnostic::error(self.peek_span(), "indexing is not available yet")
                    .with_note("arrays arrive in Phase 3 of the roadmap"),
            );
        }
        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr> {
        let span = self.peek_span();
        let kind = match self.peek().clone() {
            TokenKind::Int(value) => ExprKind::Int(value),
            TokenKind::True => ExprKind::Bool(true),
            TokenKind::False => ExprKind::Bool(false),
            TokenKind::Str(raw) => ExprKind::Str(string_parts(&raw, span.start + 1)?),
            TokenKind::Ident(name) => {
                self.bump();
                let ident = Ident { name, span };
                if self.at(&TokenKind::LParen) {
                    return self.call(ident);
                }
                return Ok(Expr {
                    kind: ExprKind::Var(ident),
                    span,
                });
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.expr()?;
                let close = self.expect(&TokenKind::RParen, "`)`")?.span;
                return Ok(Expr {
                    kind: inner.kind,
                    span: span.to(close),
                });
            }
            TokenKind::LBracket => {
                return Err(
                    Diagnostic::error(span, "array literals are not available yet")
                        .with_note("arrays arrive in Phase 3 of the roadmap"),
                );
            }
            TokenKind::Char(_) => {
                return Err(Diagnostic::error(span, "characters are not available yet")
                    .with_note("characters and strings arrive in Phase 4 of the roadmap"));
            }
            _ => return Err(self.unexpected("an expression")),
        };
        self.bump();
        Ok(Expr { kind, span })
    }

    fn call(&mut self, name: Ident) -> Result<Expr> {
        self.bump(); // (
        let mut args = Vec::new();
        if !self.at(&TokenKind::RParen) {
            args.push(self.expr()?);
            while self.eat(&TokenKind::Comma) {
                args.push(self.expr()?);
            }
        }
        let close = self.expect(&TokenKind::RParen, "`,` or `)`")?.span;
        let span = name.span.to(close);
        Ok(Expr {
            kind: ExprKind::Call(name, args),
            span,
        })
    }
}

/// Splits a string literal's raw contents into text and `{expression}` parts. `base` is the
/// file offset of the first character after the opening quote.
fn string_parts(raw: &str, base: usize) -> Result<Vec<StrPart>> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = raw.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        match c {
            '\\' => {
                let (_, escaped) = chars.next().expect("the lexer never ends a string on `\\`");
                let value = lexer::unescape(escaped).ok_or_else(|| {
                    Diagnostic::error(
                        Span::new(base + index, base + index + 2),
                        format!("unknown escape `\\{escaped}`"),
                    )
                    .with_help("the escapes are \\n \\t \\r \\0 \\\\ \\\" \\{ \\}")
                })?;
                text.push(value);
            }
            '{' => {
                let Some(length) = raw[index + 1..].find('}') else {
                    return Err(Diagnostic::error(
                        Span::new(base + index, base + index + 1),
                        "unclosed `{` in string",
                    )
                    .with_help("write `\\{` for a literal brace"));
                };
                let inner_start = index + 1;
                let inner = &raw[inner_start..inner_start + length];
                let expr = parse_fragment(inner, base + inner_start)?;
                if !text.is_empty() {
                    parts.push(StrPart::Text(std::mem::take(&mut text)));
                }
                parts.push(StrPart::Expr(expr));
                while chars
                    .peek()
                    .is_some_and(|&(i, _)| i <= inner_start + length)
                {
                    chars.next();
                }
            }
            '}' => {
                return Err(Diagnostic::error(
                    Span::new(base + index, base + index + 1),
                    "unmatched `}` in string",
                )
                .with_help("write `\\}` for a literal brace"));
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(StrPart::Text(text));
    }
    Ok(parts)
}

/// Parses an interpolated expression from inside a string literal.
fn parse_fragment(text: &str, base: usize) -> Result<Expr> {
    let tokens = lexer::lex(text, base)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        next_decl: 0,
    };
    if parser.at(&TokenKind::Newline) || parser.at(&TokenKind::Eof) {
        return Err(Diagnostic::error(
            Span::new(base - 1, base + text.len() + 1),
            "empty `{}` in string",
        )
        .with_help("put an expression inside the braces, or write `\\{\\}` for literal braces"));
    }
    let expr = parser.expr()?;
    parser.skip_separators();
    if !parser.at(&TokenKind::Eof) {
        return Err(parser.unexpected("`}` to end the interpolation"));
    }
    Ok(expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(text: &str) -> Program {
        parse(&SourceFile::new("t.rsat", text)).unwrap_or_else(|d| panic!("{}", d.message))
    }

    fn parse_err(text: &str) -> Diagnostic {
        parse(&SourceFile::new("t.rsat", text)).expect_err("should fail")
    }

    /// Renders an expression with full parentheses, to check precedence.
    fn show(expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(v) => v.to_string(),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Var(ident) => ident.name.clone(),
            ExprKind::Str(_) => "\"…\"".into(),
            ExprKind::Unary(UnaryOp::Neg, e) => format!("(-{})", show(e)),
            ExprKind::Unary(UnaryOp::Not, e) => format!("(not {})", show(e)),
            ExprKind::Binary(op, a, b) => format!("({} {} {})", show(a), op.symbol(), show(b)),
            ExprKind::Call(f, args) => {
                format!(
                    "{}({})",
                    f.name,
                    args.iter().map(show).collect::<Vec<_>>().join(", ")
                )
            }
        }
    }

    fn expr_of(text: &str) -> String {
        let program = parse_ok(&format!("require {text}"));
        match &program.stmts[0].kind {
            StmtKind::Require(expr) => show(expr),
            _ => unreachable!(),
        }
    }

    #[test]
    fn respects_precedence_and_associativity() {
        assert_eq!(expr_of("1 + 2 * 3 == 7"), "((1 + (2 * 3)) == 7)");
        assert_eq!(expr_of("a - b - c"), "((a - b) - c)");
        assert_eq!(expr_of("-x ^ 2"), "(-(x ^ 2))");
        assert_eq!(expr_of("2 ^ 3 ^ 2"), "(2 ^ (3 ^ 2))");
        assert_eq!(expr_of("not a == b and c"), "((not (a == b)) and c)");
        assert_eq!(expr_of("a or b and c"), "(a or (b and c))");
        assert_eq!(
            expr_of("a implies b implies c"),
            "(a implies (b implies c))"
        );
        assert_eq!(expr_of("a iff b or c"), "(a iff (b or c))");
        assert_eq!(
            expr_of("max(a, b + 1) > abs(-c)"),
            "(max(a, (b + 1)) > abs((-c)))"
        );
    }

    #[test]
    fn rejects_chained_comparisons() {
        assert!(parse_err("require a < b < c").message.contains("chained"));
    }

    #[test]
    fn parses_declarations_with_types_and_ranges() {
        let program = parse_ok("var a, b: int[0..=9]\nvar c: uint<8>\nvar n = 42\nvar f: bool");
        assert_eq!(program.num_decls, 5);
        let StmtKind::Var(decl) = &program.stmts[0].kind else {
            panic!()
        };
        let Some(TypeExpr {
            kind: TypeKind::Int(Some(range)),
            ..
        }) = &decl.ty
        else {
            panic!()
        };
        assert!(range.inclusive);
        assert!(
            parse_err("var a, b = 1")
                .message
                .contains("single variable")
        );
    }

    #[test]
    fn parses_solve_modes_targets_and_clauses() {
        let program = parse_ok(
            "solve all x, y limit 3 { print x }\nsolve unique\nsolve x minimize x + y else { print 0 }",
        );
        let StmtKind::Solve(first) = &program.stmts[0].kind else {
            panic!()
        };
        assert_eq!((first.mode, first.targets.len()), (SolveMode::All, 2));
        assert!(first.limit.is_some() && first.body.is_some());
        let StmtKind::Solve(second) = &program.stmts[1].kind else {
            panic!()
        };
        assert_eq!((second.mode, second.targets.len()), (SolveMode::Unique, 0));
        let StmtKind::Solve(third) = &program.stmts[2].kind else {
            panic!()
        };
        assert!(matches!(third.objective, Some((Objective::Minimize, _))));
        assert!(third.else_block.is_some());
    }

    #[test]
    fn parses_control_flow() {
        let program =
            parse_ok("for i in 0..n { if i > 2 { x = x + i } else if i == 0 { print i } }");
        let StmtKind::For { range, body, .. } = &program.stmts[0].kind else {
            panic!()
        };
        assert!(!range.inclusive);
        assert_eq!(body.stmts.len(), 1);
    }

    #[test]
    fn splits_interpolated_strings() {
        let program = parse_ok(r#"print "{n} = {x} \{literal\} × {y + 1}""#);
        let StmtKind::Print(args) = &program.stmts[0].kind else {
            panic!()
        };
        let ExprKind::Str(parts) = &args[0].kind else {
            panic!()
        };
        assert_eq!(parts.len(), 5); // {n}, " = ", {x}, " {literal} × ", {y + 1}
        assert_eq!(parts[3], StrPart::Text(" {literal} × ".into()));
        let StrPart::Expr(expr) = &parts[4] else {
            panic!()
        };
        assert_eq!(show(expr), "(y + 1)");
    }

    #[test]
    fn interpolation_spans_point_into_the_file() {
        let text = r#"print "a{zz}""#;
        let program = parse_ok(text);
        let StmtKind::Print(args) = &program.stmts[0].kind else {
            panic!()
        };
        let ExprKind::Str(parts) = &args[0].kind else {
            panic!()
        };
        let StrPart::Expr(expr) = &parts[1] else {
            panic!()
        };
        assert_eq!(&text[expr.span.start..expr.span.end], "zz");
    }

    #[test]
    fn statements_need_separators() {
        assert!(
            parse_err("require a require b")
                .message
                .contains("end of the statement")
        );
        parse_ok("require a; require b");
    }

    #[test]
    fn explains_unavailable_features() {
        assert!(parse_err("fn f() {}").message.contains("functions"));
        assert!(parse_err("var a: [int; 3]").message.contains("array"));
        assert!(parse_err("x\n").notes[0].contains("require"));
    }
}
