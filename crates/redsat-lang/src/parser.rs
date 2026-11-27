//! A recursive-descent parser from tokens to the [`ast`](crate::ast).
//!
//! Operator precedence, from loosest to tightest binding:
//!
//! | Operators                         | Associativity       |
//! |-----------------------------------|---------------------|
//! | `iff`                             | left                |
//! | `implies`                         | right               |
//! | `or`                              | left                |
//! | `xor`                             | left                |
//! | `and`                             | left                |
//! | `not`                             | prefix              |
//! | `== != < <= > >=`, `in`           | none (no chaining)  |
//! | `\|` (bitwise or)                 | left                |
//! | `&` (bitwise and)                 | left                |
//! | `<< >>`                           | left                |
//! | `+ -`                             | left                |
//! | `* / %`                           | left                |
//! | unary `-`                         | prefix              |
//! | `^` (power)                       | right               |
//! | calls `f(…)`, indexing `a[i]`, fields `p.x` | postfix   |

use std::rc::Rc;

use crate::ast::{
    Access, Arg, BinaryOp, Block, Clause, DeclId, EnumDecl, Expr, ExprKind, FnDecl, Ident,
    Iterable, Objective, Param, Place, Program, Range, Solve, SolveMode, Stmt, StmtKind, StrPart,
    StructDecl, TypeExpr, TypeKind, UnaryOp, VarDecl,
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

const COMPARISONS: &[(TokenKind, BinaryOp)] = &[
    (TokenKind::EqEq, BinaryOp::Eq),
    (TokenKind::NotEq, BinaryOp::Ne),
    (TokenKind::Lt, BinaryOp::Lt),
    (TokenKind::Le, BinaryOp::Le),
    (TokenKind::Gt, BinaryOp::Gt),
    (TokenKind::Ge, BinaryOp::Ge),
];

impl Parser {
    // --- token helpers -------------------------------------------------------------------

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, ahead: usize) -> &TokenKind {
        &self.tokens[(self.pos + ahead).min(self.tokens.len() - 1)].kind
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

    fn skip_newlines(&mut self) {
        while self.at(&TokenKind::Newline) {
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
            TokenKind::While => self.while_stmt()?,
            TokenKind::Solve => StmtKind::Solve(self.solve()?),
            TokenKind::Fn => StmtKind::Fn(Rc::new(self.fn_decl()?)),
            TokenKind::Return => {
                self.bump();
                let value = if self.at_statement_end() {
                    None
                } else {
                    Some(self.expr()?)
                };
                StmtKind::Return(value)
            }
            TokenKind::Enum => StmtKind::Enum(self.enum_decl()?),
            TokenKind::Struct => StmtKind::Struct(self.struct_decl()?),
            TokenKind::Ident(_) => self.assignment_or_call()?,
            _ => return Err(self.unexpected("a statement")),
        };
        Ok(Stmt {
            kind,
            span: start.to(self.previous_span()),
        })
    }

    fn at_statement_end(&self) -> bool {
        matches!(
            self.peek(),
            TokenKind::Newline | TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
        )
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
        if self.eat(&TokenKind::LBracket) {
            let element = self.type_expr()?;
            let length = if self.eat(&TokenKind::Semicolon) {
                Some(Box::new(self.expr()?))
            } else {
                None
            };
            self.expect(&TokenKind::RBracket, "`]`")?;
            let span = start.to(self.previous_span());
            return Ok(TypeExpr {
                kind: TypeKind::Array(Box::new(element), length),
                span,
            });
        }
        let name = self.ident("a type")?;
        let kind = match name.name.as_str() {
            "bool" => TypeKind::Bool,
            "char" => TypeKind::Char,
            "int" if self.eat(&TokenKind::LBracket) => {
                let range = self.range()?;
                self.expect(&TokenKind::RBracket, "`]`")?;
                TypeKind::Int(Some(range))
            }
            "string" if self.eat(&TokenKind::LBracket) => {
                let range = self.range()?;
                self.expect(&TokenKind::RBracket, "`]`")?;
                TypeKind::Str(Some(range))
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
            "string" => TypeKind::Str(None),
            "uint" => return Err(self.unexpected("`<` after `uint` (e.g. `uint<8>`)")),
            _ => TypeKind::Named(name),
        };
        Ok(TypeExpr {
            kind,
            span: start.to(self.previous_span()),
        })
    }

    fn range_from(&mut self, lo: Expr) -> Result<Range> {
        let inclusive = match self.peek() {
            TokenKind::DotDot => false,
            TokenKind::DotDotEq => true,
            _ => return Err(self.unexpected("`..` or `..=`")),
        };
        self.bump();
        let hi = self.bit_or()?;
        let span = lo.span.to(hi.span);
        Ok(Range {
            lo: Box::new(lo),
            hi: Box::new(hi),
            inclusive,
            span,
        })
    }

    fn range(&mut self) -> Result<Range> {
        let lo = self.bit_or()?;
        self.range_from(lo)
    }

    /// An iterable: `lo..hi`, `lo..=hi`, or an expression (array or string).
    fn iterable(&mut self) -> Result<Iterable> {
        let first = self.bit_or()?;
        if matches!(self.peek(), TokenKind::DotDot | TokenKind::DotDotEq) {
            return Ok(Iterable::Range(self.range_from(first)?));
        }
        Ok(Iterable::Expr(first))
    }

    fn print(&mut self) -> Result<StmtKind> {
        self.bump(); // print
        let mut args = Vec::new();
        if !self.at_statement_end() {
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
        let iter = self.iterable()?;
        let body = self.block()?;
        Ok(StmtKind::For { var, iter, body })
    }

    fn while_stmt(&mut self) -> Result<StmtKind> {
        self.bump(); // while
        let cond = self.expr()?;
        let bound = if self.at_word("bound") {
            self.bump();
            Some(self.expr()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(StmtKind::While { cond, bound, body })
    }

    fn fn_decl(&mut self) -> Result<FnDecl> {
        self.bump(); // fn
        let name = self.ident("a function name")?;
        self.expect(&TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
            let param = self.ident("a parameter name")?;
            if !self.at(&TokenKind::Colon) {
                return Err(self
                    .unexpected("`:` and the parameter's type")
                    .with_help(format!(
                        "parameters need a type, e.g. `{}: int`",
                        param.name
                    )));
            }
            self.bump();
            let ty = self.type_expr()?;
            params.push(Param { name: param, ty });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "`,` or `)`")?;
        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.type_expr()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(FnDecl {
            name,
            params,
            ret,
            body,
        })
    }

    /// Items separated by commas and/or line breaks, inside `{ … }`.
    fn braced_list<T>(&mut self, mut item: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        let mut items = Vec::new();
        loop {
            while matches!(self.peek(), TokenKind::Newline | TokenKind::Comma) {
                self.bump();
            }
            if self.eat(&TokenKind::RBrace) {
                return Ok(items);
            }
            items.push(item(self)?);
        }
    }

    fn enum_decl(&mut self) -> Result<EnumDecl> {
        self.bump(); // enum
        let name = self.ident("an enum name")?;
        let variants = self.braced_list(|this| this.ident("a variant name"))?;
        Ok(EnumDecl { name, variants })
    }

    fn struct_decl(&mut self) -> Result<StructDecl> {
        self.bump(); // struct
        let name = self.ident("a struct name")?;
        let fields = self.braced_list(|this| {
            let field = this.ident("a field name")?;
            this.expect(&TokenKind::Colon, "`:` and the field's type")?;
            Ok((field, this.type_expr()?))
        })?;
        Ok(StructDecl { name, fields })
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

    fn assignment_or_call(&mut self) -> Result<StmtKind> {
        let target = self.postfix()?;
        if self.at(&TokenKind::Assign) {
            self.bump();
            let place = to_place(target)?;
            let value = self.expr()?;
            return Ok(StmtKind::Assign {
                target: place,
                value,
            });
        }
        if matches!(target.kind, ExprKind::Call(..)) {
            return Ok(StmtKind::Expr(target));
        }
        Err(self.unexpected("`=`").with_help(
            "a bare expression does nothing on its own; did you mean `require …` or `print …`?",
        ))
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
        let lhs = self.bit_or()?;
        let result = if self.eat(&TokenKind::In) {
            let iterable = self.iterable()?;
            let end = match &iterable {
                Iterable::Range(range) => range.span,
                Iterable::Expr(expr) => expr.span,
            };
            let span = lhs.span.to(end);
            Expr {
                kind: ExprKind::In(Box::new(lhs), Box::new(iterable)),
                span,
            }
        } else if let Some(&(_, op)) = COMPARISONS.iter().find(|(kind, _)| self.at(kind)) {
            self.bump();
            let rhs = self.bit_or()?;
            Self::binary(op, lhs, rhs)
        } else {
            return Ok(lhs);
        };
        if COMPARISONS.iter().any(|(kind, _)| self.at(kind)) || self.at(&TokenKind::In) {
            return Err(
                Diagnostic::error(self.peek_span(), "comparisons cannot be chained")
                    .with_help("write `a < b and b < c` instead of `a < b < c`"),
            );
        }
        Ok(result)
    }

    fn bit_or(&mut self) -> Result<Expr> {
        self.left_assoc(&[(TokenKind::Pipe, BinaryOp::BitOr)], Self::bit_and)
    }

    fn bit_and(&mut self) -> Result<Expr> {
        self.left_assoc(&[(TokenKind::Amp, BinaryOp::BitAnd)], Self::shift)
    }

    fn shift(&mut self) -> Result<Expr> {
        self.left_assoc(
            &[
                (TokenKind::Shl, BinaryOp::Shl),
                (TokenKind::Shr, BinaryOp::Shr),
            ],
            Self::additive,
        )
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
        let mut expr = self.primary()?;
        loop {
            if self.eat(&TokenKind::LBracket) {
                let index = self.expr()?;
                let close = self.expect(&TokenKind::RBracket, "`]`")?.span;
                let span = expr.span.to(close);
                expr = Expr {
                    kind: ExprKind::Index(Box::new(expr), Box::new(index)),
                    span,
                };
            } else if self.eat(&TokenKind::Dot) {
                let field = self.ident("a field name")?;
                let span = expr.span.to(field.span);
                expr = Expr {
                    kind: ExprKind::Field(Box::new(expr), field),
                    span,
                };
            } else {
                return Ok(expr);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        let span = self.peek_span();
        let kind = match self.peek().clone() {
            TokenKind::Int(value) => ExprKind::Int(value),
            TokenKind::True => ExprKind::Bool(true),
            TokenKind::False => ExprKind::Bool(false),
            TokenKind::Char(value) => ExprKind::Char(value),
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
            TokenKind::LBracket => return self.array(),
            TokenKind::If => return self.if_expr(),
            _ => return Err(self.unexpected("an expression")),
        };
        self.bump();
        Ok(Expr { kind, span })
    }

    fn if_expr(&mut self) -> Result<Expr> {
        let start = self.bump().span; // if
        let cond = self.expr()?;
        let then_value = self.braced_expr()?;
        self.expect(
            &TokenKind::Else,
            "`else` (an `if` used as a value needs both branches)",
        )?;
        let else_value = if self.at(&TokenKind::If) {
            self.if_expr()?
        } else {
            self.braced_expr()?
        };
        let span = start.to(else_value.span);
        Ok(Expr {
            kind: ExprKind::If(Box::new(cond), Box::new(then_value), Box::new(else_value)),
            span,
        })
    }

    fn braced_expr(&mut self) -> Result<Expr> {
        self.expect(&TokenKind::LBrace, "`{`")?;
        self.skip_newlines();
        let value = self.expr()?;
        self.skip_newlines();
        self.expect(&TokenKind::RBrace, "`}`")?;
        Ok(value)
    }

    fn array(&mut self) -> Result<Expr> {
        let open = self.bump().span; // [
        let mut elements = Vec::new();
        if !self.at(&TokenKind::RBracket) {
            let first = self.expr()?;
            if self.at(&TokenKind::For) {
                let clauses = self.clauses()?;
                let close = self.expect(&TokenKind::RBracket, "`]`")?.span;
                let kind = ExprKind::Comprehension {
                    func: None,
                    body: Box::new(first),
                    clauses,
                };
                return Ok(Expr {
                    kind,
                    span: open.to(close),
                });
            }
            elements.push(first);
            while self.eat(&TokenKind::Comma) {
                if self.at(&TokenKind::RBracket) {
                    break; // trailing comma
                }
                elements.push(self.expr()?);
            }
        }
        let close = self.expect(&TokenKind::RBracket, "`,` or `]`")?.span;
        Ok(Expr {
            kind: ExprKind::Array(elements),
            span: open.to(close),
        })
    }

    /// `for x in … (if …)*`, repeated.
    fn clauses(&mut self) -> Result<Vec<Clause>> {
        let mut clauses = Vec::new();
        while self.eat(&TokenKind::For) {
            let var = self.ident("a loop variable")?;
            self.expect(&TokenKind::In, "`in`")?;
            clauses.push(Clause::For(var, self.iterable()?));
            while self.eat(&TokenKind::If) {
                clauses.push(Clause::If(self.or()?));
            }
        }
        Ok(clauses)
    }

    fn call(&mut self, name: Ident) -> Result<Expr> {
        self.bump(); // (
        let mut args = Vec::new();
        if !self.at(&TokenKind::RParen) {
            let first = self.argument()?;
            if first.name.is_none() && self.at(&TokenKind::For) {
                let clauses = self.clauses()?;
                let close = self.expect(&TokenKind::RParen, "`)`")?.span;
                let span = name.span.to(close);
                let kind = ExprKind::Comprehension {
                    func: Some(name),
                    body: Box::new(first.value),
                    clauses,
                };
                return Ok(Expr { kind, span });
            }
            args.push(first);
            while self.eat(&TokenKind::Comma) {
                args.push(self.argument()?);
            }
        }
        let close = self.expect(&TokenKind::RParen, "`,` or `)`")?.span;
        let span = name.span.to(close);
        Ok(Expr {
            kind: ExprKind::Call(name, args),
            span,
        })
    }

    fn argument(&mut self) -> Result<Arg> {
        if matches!(self.peek(), TokenKind::Ident(_)) && self.peek_at(1) == &TokenKind::Colon {
            let name = self.ident("an argument name")?;
            self.bump(); // :
            return Ok(Arg {
                name: Some(name),
                value: self.expr()?,
            });
        }
        Ok(Arg {
            name: None,
            value: self.expr()?,
        })
    }
}

/// Turns the left side of `=` into a [`Place`].
fn to_place(expr: Expr) -> Result<Place> {
    let span = expr.span;
    let mut path = Vec::new();
    let mut current = expr;
    loop {
        match current.kind {
            ExprKind::Var(root) => {
                path.reverse();
                return Ok(Place { root, path, span });
            }
            ExprKind::Index(base, index) => {
                path.push(Access::Index(*index));
                current = *base;
            }
            ExprKind::Field(base, field) => {
                path.push(Access::Field(field));
                current = *base;
            }
            _ => {
                return Err(
                    Diagnostic::error(current.span, "cannot assign to this expression")
                        .with_help("assign to a variable, an element `a[i]` or a field `p.x`"),
                );
            }
        }
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
                    .with_help("the escapes are \\n \\t \\r \\0 \\\\ \\\" \\' \\{ \\}")
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

    fn show_iterable(iterable: &Iterable) -> String {
        match iterable {
            Iterable::Range(range) => {
                format!(
                    "{}{}{}",
                    show(&range.lo),
                    if range.inclusive { "..=" } else { ".." },
                    show(&range.hi)
                )
            }
            Iterable::Expr(expr) => show(expr),
        }
    }

    /// Renders an expression with full parentheses, to check precedence.
    fn show(expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Int(v) => v.to_string(),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Char(c) => format!("{c:?}"),
            ExprKind::Var(ident) => ident.name.clone(),
            ExprKind::Str(_) => "\"…\"".into(),
            ExprKind::Unary(UnaryOp::Neg, e) => format!("(-{})", show(e)),
            ExprKind::Unary(UnaryOp::Not, e) => format!("(not {})", show(e)),
            ExprKind::Binary(op, a, b) => format!("({} {} {})", show(a), op.symbol(), show(b)),
            ExprKind::Call(f, args) => {
                let args: Vec<String> = args
                    .iter()
                    .map(|arg| match &arg.name {
                        Some(name) => format!("{}: {}", name.name, show(&arg.value)),
                        None => show(&arg.value),
                    })
                    .collect();
                format!("{}({})", f.name, args.join(", "))
            }
            ExprKind::Index(a, i) => format!("{}[{}]", show(a), show(i)),
            ExprKind::Field(a, f) => format!("{}.{}", show(a), f.name),
            ExprKind::Array(items) => format!(
                "[{}]",
                items.iter().map(show).collect::<Vec<_>>().join(", ")
            ),
            ExprKind::Comprehension {
                func,
                body,
                clauses,
            } => {
                let clauses: Vec<String> = clauses
                    .iter()
                    .map(|clause| match clause {
                        Clause::For(var, iter) => {
                            format!("for {} in {}", var.name, show_iterable(iter))
                        }
                        Clause::If(cond) => format!("if {}", show(cond)),
                    })
                    .collect();
                let inner = format!("{} {}", show(body), clauses.join(" "));
                match func {
                    Some(func) => format!("{}({inner})", func.name),
                    None => format!("[{inner}]"),
                }
            }
            ExprKind::In(e, iter) => format!("({} in {})", show(e), show_iterable(iter)),
            ExprKind::If(c, a, b) => format!("(if {} then {} else {})", show(c), show(a), show(b)),
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
        assert_eq!(expr_of("a & b == 0"), "((a & b) == 0)");
        assert_eq!(expr_of("a | b & c << 1 + 2"), "(a | (b & (c << (1 + 2))))");
    }

    #[test]
    fn parses_postfix_chains_and_membership() {
        assert_eq!(expr_of("grid[r][c].x == p.y"), "(grid[r][c].x == p.y)");
        assert_eq!(expr_of("c in 'a'..='z'"), "(c in 'a'..='z')");
        assert_eq!(
            expr_of("x in xs and y in 0..n + 1"),
            "((x in xs) and (y in 0..(n + 1)))"
        );
        assert_eq!(expr_of("Color.Red != paint"), "(Color.Red != paint)");
    }

    #[test]
    fn parses_comprehensions_and_arrays() {
        assert_eq!(
            expr_of("sum(w[i] for i in 0..n if take[i]) <= cap"),
            "(sum(w[i] for i in 0..n if take[i]) <= cap)"
        );
        assert_eq!(
            expr_of("all(x != y for x in xs for y in ys)"),
            "all((x != y) for x in xs for y in ys)"
        );
        assert_eq!(
            expr_of("[1, 2, 3,] == [i * i for i in 1..=3]"),
            "([1, 2, 3] == [(i * i) for i in 1..=3])"
        );
        assert_eq!(
            expr_of("Point(x: 1, y: 2) == q"),
            "(Point(x: 1, y: 2) == q)"
        );
        assert_eq!(
            expr_of("if a > b { a } else if b > 0 { b } else { 0 }"),
            "(if (a > b) then a else (if (b > 0) then b else 0))"
        );
    }

    #[test]
    fn rejects_chained_comparisons() {
        assert!(parse_err("require a < b < c").message.contains("chained"));
        assert!(parse_err("require a in b in c").message.contains("chained"));
    }

    #[test]
    fn parses_declarations_with_types() {
        let program = parse_ok(
            "var a, b: int[0..=9]\nvar c: uint<8>\nvar n = 42\nvar f: bool\nvar g: [[int[1..=9]; 9]; 9]\n\
             var w: string[1..=5]\nvar p: Point",
        );
        assert_eq!(program.num_decls, 8);
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
        let StmtKind::Var(grid) = &program.stmts[4].kind else {
            panic!()
        };
        assert!(matches!(
            &grid.ty,
            Some(TypeExpr {
                kind: TypeKind::Array(_, Some(_)),
                ..
            })
        ));
        assert!(
            parse_err("var a, b = 1")
                .message
                .contains("single variable")
        );
    }

    #[test]
    fn parses_functions_enums_and_structs() {
        let program = parse_ok(
            "fn clamp(v: int, lo: int, hi: int) -> int {\n  if v < lo { return lo }\n  return min(v, hi)\n}\n\
             fn check(xs: [int]) { require xs[0] > 0 }\n\
             enum Color { Red, Green,\n Blue }\nstruct Point {\n  x: int\n  y: int[0..=9],\n}\ncheck([1])",
        );
        let StmtKind::Fn(clamp) = &program.stmts[0].kind else {
            panic!()
        };
        assert_eq!((clamp.params.len(), clamp.ret.is_some()), (3, true));
        let StmtKind::Enum(color) = &program.stmts[2].kind else {
            panic!()
        };
        assert_eq!(color.variants.len(), 3);
        let StmtKind::Struct(point) = &program.stmts[3].kind else {
            panic!()
        };
        assert_eq!(point.fields.len(), 2);
        assert!(matches!(program.stmts[4].kind, StmtKind::Expr(_)));
        assert!(parse_err("fn f(x) {}").notes[0].contains("parameters need a type"));
    }

    #[test]
    fn parses_places_and_loops() {
        let program = parse_ok(
            "grid[r][c] = 1\np.x = 2\nfor v in values { total = total + v }\nwhile n > 1 bound 10 { n = n - 1 }",
        );
        let StmtKind::Assign { target, .. } = &program.stmts[0].kind else {
            panic!()
        };
        assert_eq!((target.root.name.as_str(), target.path.len()), ("grid", 2));
        assert!(matches!(
            &program.stmts[2].kind,
            StmtKind::For {
                iter: Iterable::Expr(_),
                ..
            }
        ));
        assert!(matches!(
            &program.stmts[3].kind,
            StmtKind::While { bound: Some(_), .. }
        ));
        assert!(parse_err("f(x) = 1").message.contains("cannot assign"));
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
        assert!(parse_err("x\n").notes[0].contains("require"));
    }
}
