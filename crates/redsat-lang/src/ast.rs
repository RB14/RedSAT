//! The abstract syntax tree produced by the parser.

use num_bigint::BigInt;

use crate::source::Span;

/// A whole program: a sequence of statements.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    /// The top-level statements, in order.
    pub stmts: Vec<Stmt>,
    /// The number of variable declarations ([`DeclId`]s are `0..num_decls`).
    pub num_decls: usize,
}

/// Identifies one declared name (`var x, y` declares two).
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
    /// `name = value`
    Assign {
        /// The variable assigned to.
        target: Ident,
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
    /// `for name in range { … }`
    For {
        /// The loop variable.
        var: Ident,
        /// The values it takes.
        range: Range,
        /// The loop body.
        body: Block,
    },
    /// `solve …`
    Solve(Solve),
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
    /// What to solve for (the projection); empty means "every unknown".
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
    /// A string literal, split into text and interpolated `{…}` expressions.
    Str(Vec<StrPart>),
    /// A variable.
    Var(Ident),
    /// `-e` or `not e`.
    Unary(UnaryOp, Box<Expr>),
    /// `a op b`.
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    /// `f(a, b, …)`.
    Call(Ident, Vec<Expr>),
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
