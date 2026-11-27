//! The abstract syntax tree produced by the parser.

use std::rc::Rc;

use num_bigint::BigInt;

use crate::source::Span;

/// A whole program: a sequence of statements.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// The top-level statements, in order.
    pub stmts: Vec<Stmt>,
    /// The number of variable declarations ([`DeclId`]s are `0..num_decls`).
    pub num_decls: usize,
    /// Where the standard library prelude lies, if it was loaded. Your compiler options (such
    /// as `explicit-types`) do not apply to it.
    pub library: Option<Span>,
}

/// Identifies one declared variable name (`var x, y` declares two).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeclId(pub usize);

/// A name and where it appears.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    /// The name.
    pub name: String,
    /// Where it appears.
    pub span: Span,
}

/// A `{ … }` block.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The statements, in order.
    pub stmts: Vec<Stmt>,
    /// From `{` to `}`.
    pub span: Span,
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    /// What kind of statement.
    pub kind: StmtKind,
    /// Where it is.
    pub span: Span,
}

/// The kinds of statement.
#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// `var a, b: T = init`
    Var(VarDecl),
    /// `place = value`, e.g. `x = 1`, `grid[r][c] = 0`, `p.x = 3`
    Assign {
        /// What is assigned to.
        target: Place,
        /// The new value.
        value: Expr,
    },
    /// `require condition`
    Require(Expr),
    /// `print a, b, …`
    Print(Vec<Expr>),
    /// `if cond { … } else { … }`
    If {
        /// The condition.
        cond: Expr,
        /// Executed when it holds.
        then_block: Block,
        /// Executed otherwise (an `else if` is an `else` block holding one `if`).
        else_block: Option<Block>,
    },
    /// `for name in iterable { … }`
    For {
        /// The loop variable.
        var: Ident,
        /// What it ranges over.
        iter: Iterable,
        /// The loop body.
        body: Block,
    },
    /// `while cond bound K { … }`
    While {
        /// The loop condition.
        cond: Expr,
        /// The maximum number of iterations, required when `cond` depends on unknowns.
        bound: Option<Expr>,
        /// The loop body.
        body: Block,
    },
    /// `solve …`
    Solve(Solve),
    /// `fn name(params) -> T { … }`
    Fn(Rc<FnDecl>),
    /// `return` or `return value`
    Return(Option<Expr>),
    /// A call used as a statement, e.g. `ordered(xs)`.
    Expr(Expr),
    /// `enum Name { A, B, … }`
    Enum(EnumDecl),
    /// `struct Name { field: T, … }`
    Struct(StructDecl),
    /// `import "file.rsat"`: the functions, enums and structs of another file.
    Import(String),
}

/// An assignable location: a variable followed by indexing and field accesses.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    /// The variable.
    pub root: Ident,
    /// The accesses applied to it, in order.
    pub path: Vec<Access>,
    /// The whole place.
    pub span: Span,
}

/// One step of a [`Place`].
#[derive(Clone, Debug, PartialEq)]
pub enum Access {
    /// `[index]`
    Index(Expr),
    /// `.field`
    Field(Ident),
}

/// What a `for` loop or comprehension clause ranges over.
#[derive(Clone, Debug, PartialEq)]
pub enum Iterable {
    /// `lo..hi` or `lo..=hi`
    Range(Range),
    /// An array or a string.
    Expr(Expr),
}

/// `var a, b: T = init`
#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    /// The declared names.
    pub names: Vec<(Ident, DeclId)>,
    /// The type annotation, if any.
    pub ty: Option<TypeExpr>,
    /// The initial value, if any (only allowed with a single name).
    pub init: Option<Expr>,
}

/// `fn name(params) -> T { body }`
#[derive(Clone, Debug, PartialEq)]
pub struct FnDecl {
    /// The function's name.
    pub name: Ident,
    /// Its parameters.
    pub params: Vec<Param>,
    /// Its result type; `None` for a function that returns nothing.
    pub ret: Option<TypeExpr>,
    /// Its body.
    pub body: Block,
}

/// A function parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// The parameter name.
    pub name: Ident,
    /// Its type.
    pub ty: TypeExpr,
}

/// `enum Name { A, B, … }`
#[derive(Clone, Debug, PartialEq)]
pub struct EnumDecl {
    /// The enum's name.
    pub name: Ident,
    /// Its variants, in order.
    pub variants: Vec<Ident>,
}

/// `struct Name { field: T, … }`
#[derive(Clone, Debug, PartialEq)]
pub struct StructDecl {
    /// The struct's name.
    pub name: Ident,
    /// Its fields, in order.
    pub fields: Vec<(Ident, TypeExpr)>,
}

/// A type annotation.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeExpr {
    /// Which type.
    pub kind: TypeKind,
    /// Where it is written.
    pub span: Span,
}

/// The types that can be written.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    /// `bool`
    Bool,
    /// `int` or `int[lo..hi]` / `int[lo..=hi]`
    Int(Option<Range>),
    /// `int<N>` (signed) or `uint<N>` (unsigned)
    IntBits {
        /// `int<N>` rather than `uint<N>`.
        signed: bool,
        /// `N`.
        bits: Box<Expr>,
    },
    /// `char`
    Char,
    /// `string` or `string[lo..=hi]` (a range of lengths)
    Str(Option<Range>),
    /// `[T; N]` or `[T]` (any length, for parameters)
    Array(Box<TypeExpr>, Option<Box<Expr>>),
    /// An enum or struct name.
    Named(Ident),
}

/// `lo..hi` or `lo..=hi`.
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    /// The first value.
    pub lo: Box<Expr>,
    /// The end: excluded for `..`, included for `..=`.
    pub hi: Box<Expr>,
    /// Whether written with `..=`.
    pub inclusive: bool,
    /// Where it is written.
    pub span: Span,
}

/// A `solve` statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Solve {
    /// Which solutions to look for.
    pub mode: SolveMode,
    /// What to solve for (the projection); empty means "every unknown in scope".
    pub targets: Vec<Expr>,
    /// `limit K`.
    pub limit: Option<Expr>,
    /// `minimize e` / `maximize e`.
    pub objective: Option<(Objective, Expr)>,
    /// Runs once per solution.
    pub body: Option<Block>,
    /// Runs when there is no solution.
    pub else_block: Option<Block>,
}

/// How many solutions a `solve` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveMode {
    /// One solution (or the first `limit` ones).
    First,
    /// Every solution.
    All,
    /// One solution, proven to be the only one.
    Unique,
}

/// The direction of optimization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Objective {
    /// Find the smallest value.
    Minimize,
    /// Find the largest value.
    Maximize,
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    /// What kind of expression.
    pub kind: ExprKind,
    /// Where it is.
    pub span: Span,
}

/// The kinds of expression.
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// An integer literal.
    Int(BigInt),
    /// `true` or `false`.
    Bool(bool),
    /// A character literal.
    Char(char),
    /// A string literal, split into text and interpolated `{…}` expressions.
    Str(Vec<StrPart>),
    /// A variable (or an enum/struct name in `Color.Red` / `Point(…)`).
    Var(Ident),
    /// `?`: a fresh anonymous unknown whose type comes from the context.
    Hole,
    /// `-e` or `not e`.
    Unary(UnaryOp, Box<Expr>),
    /// `a op b`.
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `f(a, b, …)` or `Point(x: 1, y: 2)`.
    Call(Ident, Vec<Arg>),
    /// `a[i]`.
    Index(Box<Expr>, Box<Expr>),
    /// `p.x` (also `Color.Red`).
    Field(Box<Expr>, Ident),
    /// `[a, b, c]`.
    Array(Vec<Expr>),
    /// `[e for …]` (no function) or `f(e for …)`.
    Comprehension {
        /// The aggregating function, or `None` for an array comprehension.
        func: Option<Ident>,
        /// The element expression.
        body: Box<Expr>,
        /// `for … in …` and `if …` clauses, in order.
        clauses: Vec<Clause>,
    },
    /// `x in lo..hi` or `x in array`.
    In(Box<Expr>, Box<Iterable>),
    /// `if cond { a } else { b }` used as a value.
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

/// A call argument, optionally named (`Point(x: 1, y: 2)`).
#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    /// The name, for struct constructors.
    pub name: Option<Ident>,
    /// The value.
    pub value: Expr,
}

/// A comprehension clause.
#[derive(Clone, Debug, PartialEq)]
pub enum Clause {
    /// `for var in iterable`
    For(Ident, Iterable),
    /// `if condition`
    If(Expr),
}

/// A piece of a string literal.
#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    /// Literal text (escapes already processed).
    Text(String),
    /// An interpolated `{expression}`.
    Expr(Expr),
}

/// Prefix operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// `-e`
    Neg,
    /// `not e`
    Not,
}

/// Infix operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Xor,
    Implies,
    Iff,
}

impl BinaryOp {
    /// The operator as written in source.
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Mod => "%",
            Self::Pow => "^",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "and",
            Self::Or => "or",
            Self::Xor => "xor",
            Self::Implies => "implies",
            Self::Iff => "iff",
        }
    }
}
