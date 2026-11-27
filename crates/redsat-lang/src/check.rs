//! Name resolution and type inference.
//!
//! Unannotated unknowns (`var x, y`) get a type variable that the operators they appear in
//! resolve, e.g. `x * y == n` makes both `int`. Anything still unresolved at the end is an error
//! asking for an annotation. The `explicit-types` option (see [`crate::config`]) can instead
//! require annotations up front.

use std::collections::HashMap;

use crate::ast::{
    BinaryOp, Block, DeclId, Expr, ExprKind, Program, Solve, Stmt, StmtKind, StrPart, TypeExpr,
    TypeKind, UnaryOp, VarDecl,
};
use crate::config::{CompilerOptions, ExplicitTypes};
use crate::diagnostic::Diagnostic;
use crate::source::Span;

/// The types a value can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    /// A truth value.
    Bool,
    /// A mathematical integer.
    Int,
    /// A (concrete) string, e.g. for printing.
    Str,
}

impl Type {
    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Str => "string",
        }
    }
}

/// The result of type checking: the type of every declared name.
#[derive(Clone, Debug, Default)]
pub struct TypeInfo {
    decl_types: Vec<Option<Type>>,
}

impl TypeInfo {
    /// The type of the declared name `decl`.
    ///
    /// # Panics
    ///
    /// If checking failed for this declaration (callers only proceed after a clean check).
    #[must_use]
    pub fn decl_type(&self, decl: DeclId) -> Type {
        self.decl_types[decl.0].expect("type checking succeeded")
    }
}

/// Type-checks `program`. Returns the inferred types and every diagnostic found.
#[must_use]
pub fn check(program: &Program, options: &CompilerOptions) -> (TypeInfo, Vec<Diagnostic>) {
    let mut checker = Checker {
        options,
        vars: Vec::new(),
        decls: vec![None; program.num_decls],
        scopes: vec![HashMap::new()],
        diagnostics: Vec::new(),
    };
    for stmt in &program.stmts {
        checker.stmt(stmt);
    }
    let decl_types = checker
        .decls
        .clone()
        .into_iter()
        .map(|entry| {
            let (ty, name, span) = entry?;
            match checker.resolve(ty) {
                Ty::Known(ty) => Some(ty),
                Ty::Error => None,
                Ty::Var(_) => {
                    checker.diagnostics.push(
                        Diagnostic::error(span, format!("cannot infer the type of `{name}`"))
                            .with_label("declared here")
                            .with_help(format!(
                                "use it in a way that determines its type, or annotate it: \
                                 `var {name}: int` or `var {name}: bool`"
                            )),
                    );
                    None
                }
            }
        })
        .collect();
    (TypeInfo { decl_types }, checker.diagnostics)
}

/// A type during inference: known, an inference variable, or the result of an earlier error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Known(Type),
    Var(usize),
    Error,
}

const INT: Ty = Ty::Known(Type::Int);
const BOOL: Ty = Ty::Known(Type::Bool);

#[derive(Clone)]
struct Binding {
    ty: Ty,
    is_loop_var: bool,
}

struct Checker<'a> {
    options: &'a CompilerOptions,
    /// Inference variables: `Some(ty)` once bound.
    vars: Vec<Option<Ty>>,
    /// Per declaration: its type, name and span.
    decls: Vec<Option<(Ty, String, Span)>>,
    scopes: Vec<HashMap<String, Binding>>,
    diagnostics: Vec<Diagnostic>,
}

impl Checker<'_> {
    fn error(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn resolve(&self, mut ty: Ty) -> Ty {
        while let Ty::Var(var) = ty {
            match self.vars[var] {
                Some(bound) => ty = bound,
                None => return ty,
            }
        }
        ty
    }

    /// Makes `actual` equal to `expected`, reporting a mismatch at `span`.
    fn unify(&mut self, expected: Ty, actual: Ty, span: Span) -> Ty {
        match (self.resolve(expected), self.resolve(actual)) {
            (Ty::Error, other) | (other, Ty::Error) => other,
            (Ty::Var(a), Ty::Var(b)) if a == b => Ty::Var(a),
            (Ty::Var(var), other) | (other, Ty::Var(var)) => {
                self.vars[var] = Some(other);
                other
            }
            (Ty::Known(a), Ty::Known(b)) if a == b => Ty::Known(a),
            (Ty::Known(a), Ty::Known(b)) => {
                self.error(
                    Diagnostic::error(
                        span,
                        format!("expected `{}`, found `{}`", a.name(), b.name()),
                    )
                    .with_label(format!("this is `{}`", b.name())),
                );
                Ty::Error
            }
        }
    }

    fn fresh(&mut self) -> Ty {
        self.vars.push(None);
        Ty::Var(self.vars.len() - 1)
    }

    fn lookup(&mut self, name: &str, span: Span) -> Option<Binding> {
        let found = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned();
        if found.is_none() {
            self.error(
                Diagnostic::error(span, format!("unknown variable `{name}`"))
                    .with_label("not declared")
                    .with_help(format!("declare it first, e.g. `var {name}: int`")),
            );
        }
        found
    }

    fn declare(&mut self, name: &str, span: Span, binding: Binding) {
        let scope = self.scopes.last_mut().expect("there is always a scope");
        if scope.contains_key(name) {
            self.error(
                Diagnostic::error(span, format!("`{name}` is already declared in this scope"))
                    .with_help("assign to it with `=` instead, or pick another name"),
            );
        }
        self.scopes
            .last_mut()
            .expect("there is always a scope")
            .insert(name.to_string(), binding);
    }

    fn in_scope(&mut self, f: impl FnOnce(&mut Self)) {
        self.scopes.push(HashMap::new());
        f(self);
        self.scopes.pop();
    }

    fn block(&mut self, block: &Block) {
        self.in_scope(|this| {
            for stmt in &block.stmts {
                this.stmt(stmt);
            }
        });
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Var(decl) => self.var_decl(decl),
            StmtKind::Assign { target, value } => {
                let value_ty = self.expr(value);
                if let Some(binding) = self.lookup(&target.name, target.span) {
                    if binding.is_loop_var {
                        self.error(Diagnostic::error(
                            target.span,
                            format!("cannot assign to the loop variable `{}`", target.name),
                        ));
                    }
                    self.unify(binding.ty, value_ty, value.span);
                }
            }
            StmtKind::Require(cond) => {
                let ty = self.expr(cond);
                self.unify(BOOL, ty, cond.span);
            }
            StmtKind::Print(args) => {
                for arg in args {
                    self.expr(arg);
                }
            }
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let ty = self.expr(cond);
                self.unify(BOOL, ty, cond.span);
                self.block(then_block);
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            StmtKind::For { var, range, body } => {
                for bound in [&range.lo, &range.hi] {
                    let ty = self.expr(bound);
                    self.unify(INT, ty, bound.span);
                }
                self.in_scope(|this| {
                    this.declare(
                        &var.name,
                        var.span,
                        Binding {
                            ty: INT,
                            is_loop_var: true,
                        },
                    );
                    this.block(body);
                });
            }
            StmtKind::Solve(solve) => self.solve(solve),
        }
    }

    fn var_decl(&mut self, decl: &VarDecl) {
        let annotated = decl.ty.as_ref().map(|ty| self.type_expr(ty));
        let init_ty = decl.init.as_ref().map(|init| self.expr(init));
        for (name, id) in &decl.names {
            let needs_annotation = match self.options.explicit_types {
                ExplicitTypes::Off => false,
                ExplicitTypes::Unknowns => decl.init.is_none(),
                ExplicitTypes::All => true,
            };
            if needs_annotation && annotated.is_none() {
                let example = if decl.init.is_some() {
                    "var n: int = 42"
                } else {
                    "var x: int"
                };
                self.error(
                    Diagnostic::error(
                        name.span,
                        format!("`{}` needs a type annotation", name.name),
                    )
                    .with_label("no type given")
                    .with_note(format!(
                        "required by the compiler option explicit-types = \"{}\"",
                        if self.options.explicit_types == ExplicitTypes::All {
                            "all"
                        } else {
                            "unknowns"
                        }
                    ))
                    .with_help(format!("annotate it, e.g. `{example}`")),
                );
            }
            let ty = match (annotated, init_ty) {
                (Some(annotated), Some(init)) => {
                    let span = decl.init.as_ref().expect("has init").span;
                    self.unify(annotated, init, span);
                    annotated
                }
                (Some(annotated), None) => annotated,
                (None, Some(init)) => init,
                (None, None) => self.fresh(),
            };
            if let Ty::Known(Type::Str) = self.resolve(ty) {
                if decl.init.is_none() {
                    self.error(Diagnostic::error(
                        name.span,
                        "string unknowns are not available yet",
                    ));
                }
            }
            self.decls[id.0] = Some((ty, name.name.clone(), name.span));
            self.declare(
                &name.name,
                name.span,
                Binding {
                    ty,
                    is_loop_var: false,
                },
            );
        }
    }

    fn type_expr(&mut self, ty: &TypeExpr) -> Ty {
        match &ty.kind {
            TypeKind::Bool => BOOL,
            TypeKind::Int(range) => {
                if let Some(range) = range {
                    for bound in [&range.lo, &range.hi] {
                        let bound_ty = self.expr(bound);
                        self.unify(INT, bound_ty, bound.span);
                    }
                }
                INT
            }
            TypeKind::IntBits { bits, .. } => {
                let bits_ty = self.expr(bits);
                self.unify(INT, bits_ty, bits.span);
                INT
            }
        }
    }

    fn solve(&mut self, solve: &Solve) {
        for target in &solve.targets {
            self.expr(target);
        }
        if let Some(limit) = &solve.limit {
            let ty = self.expr(limit);
            self.unify(INT, ty, limit.span);
        }
        if let Some((_, objective)) = &solve.objective {
            let ty = self.expr(objective);
            self.unify(INT, ty, objective.span);
        }
        if let Some(body) = &solve.body {
            self.block(body);
        }
        if let Some(block) = &solve.else_block {
            self.block(block);
        }
    }

    fn expr(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Int(_) => INT,
            ExprKind::Bool(_) => BOOL,
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Expr(inner) = part {
                        self.expr(inner);
                    }
                }
                Ty::Known(Type::Str)
            }
            ExprKind::Var(ident) => self
                .lookup(&ident.name, ident.span)
                .map_or(Ty::Error, |b| b.ty),
            ExprKind::Unary(op, operand) => {
                let ty = self.expr(operand);
                let expected = match op {
                    UnaryOp::Neg => INT,
                    UnaryOp::Not => BOOL,
                };
                self.unify(expected, ty, operand.span)
            }
            ExprKind::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs),
            ExprKind::Call(name, args) => self.call(&name.name, name.span, args),
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Ty {
        let (lhs_ty, rhs_ty) = (self.expr(lhs), self.expr(rhs));
        match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow => {
                self.unify(INT, lhs_ty, lhs.span);
                self.unify(INT, rhs_ty, rhs.span);
                INT
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                self.unify(INT, lhs_ty, lhs.span);
                self.unify(INT, rhs_ty, rhs.span);
                BOOL
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                self.unify(lhs_ty, rhs_ty, rhs.span);
                BOOL
            }
            BinaryOp::And | BinaryOp::Or | BinaryOp::Xor | BinaryOp::Implies | BinaryOp::Iff => {
                self.unify(BOOL, lhs_ty, lhs.span);
                self.unify(BOOL, rhs_ty, rhs.span);
                BOOL
            }
        }
    }

    fn call(&mut self, name: &str, span: Span, args: &[Expr]) -> Ty {
        let arg_tys: Vec<Ty> = args.iter().map(|arg| self.expr(arg)).collect();
        let arity_ok = match name {
            "abs" => args.len() == 1,
            "min" | "max" => !args.is_empty(),
            _ => {
                let message = if crate::syntax::BUILTIN_FUNCTIONS.contains(&name) {
                    Diagnostic::error(span, format!("`{name}` is not available yet"))
                        .with_note("it arrives together with arrays in Phase 3 of the roadmap")
                } else {
                    Diagnostic::error(span, format!("unknown function `{name}`"))
                        .with_help("the available functions are `abs`, `min` and `max`")
                };
                self.error(message);
                return Ty::Error;
            }
        };
        if !arity_ok {
            self.error(Diagnostic::error(
                span,
                format!("wrong number of arguments for `{name}`"),
            ));
        }
        for (ty, arg) in arg_tys.into_iter().zip(args) {
            self.unify(INT, ty, arg.span);
        }
        INT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::source::SourceFile;

    fn check_with(text: &str, options: &CompilerOptions) -> (TypeInfo, Vec<Diagnostic>) {
        let program = parse(&SourceFile::new("t.rsat", text)).unwrap();
        check(&program, options)
    }

    fn errors(text: &str) -> Vec<String> {
        check_with(text, &CompilerOptions::default())
            .1
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn infers_types_from_usage() {
        let (types, diagnostics) = check_with(
            "var x, y\nvar b\nrequire x * y == 6 and b",
            &CompilerOptions::default(),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(types.decl_type(DeclId(0)), Type::Int);
        assert_eq!(types.decl_type(DeclId(1)), Type::Int);
        assert_eq!(types.decl_type(DeclId(2)), Type::Bool);
    }

    #[test]
    fn infers_through_equality_between_unknowns() {
        let (types, diagnostics) = check_with(
            "var p, q\nrequire p == q\nrequire q",
            &CompilerOptions::default(),
        );
        assert!(diagnostics.is_empty());
        assert_eq!(types.decl_type(DeclId(0)), Type::Bool);
    }

    #[test]
    fn reports_uninferable_and_mismatched_types() {
        assert_eq!(errors("var x\nsolve x"), ["cannot infer the type of `x`"]);
        assert_eq!(
            errors("var x: bool\nrequire x + 1 > 2"),
            ["expected `int`, found `bool`"]
        );
        assert_eq!(errors("require z"), ["unknown variable `z`"]);
        assert_eq!(
            errors("var a\nvar a"),
            [
                "`a` is already declared in this scope",
                "cannot infer the type of `a`",
                "cannot infer the type of `a`"
            ]
        );
        assert_eq!(
            errors("for i in 0..3 { i = 2 }"),
            ["cannot assign to the loop variable `i`"]
        );
        assert_eq!(errors("require len(5) > 1"), ["`len` is not available yet"]);
    }

    #[test]
    fn explicit_types_unknowns_requires_annotations_on_unknowns_only() {
        let options = CompilerOptions {
            explicit_types: ExplicitTypes::Unknowns,
            ..CompilerOptions::default()
        };
        let (_, diagnostics) = check_with(
            "var x\nvar y: int\nvar n = 42\nrequire x == y + n",
            &options,
        );
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["`x` needs a type annotation"]);
    }

    #[test]
    fn explicit_types_all_requires_annotations_everywhere() {
        let options = CompilerOptions {
            explicit_types: ExplicitTypes::All,
            ..CompilerOptions::default()
        };
        let (_, diagnostics) = check_with(
            "var y: int\nvar n = 42\nvar m: int = 1\nrequire y == n + m",
            &options,
        );
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["`n` needs a type annotation"]);
    }
}
