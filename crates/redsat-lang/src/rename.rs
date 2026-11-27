//! Renaming the functions that calls refer to, everywhere in a piece of syntax.
//!
//! The loader uses it to keep shadowing lexical: when your program defines a function with a
//! standard-library name, the library's own functions must keep calling the library's version.

use std::collections::HashMap;

use crate::ast::{
    Access, Block, Clause, Expr, ExprKind, Iterable, Range, Stmt, StmtKind, StrPart, TypeExpr,
    TypeKind,
};

/// Renames the callees in `block` according to `renames` (old name → new name).
pub(crate) fn calls_in_block(block: &mut Block, renames: &HashMap<String, String>) {
    for stmt in &mut block.stmts {
        stmt_calls(stmt, renames);
    }
}

fn stmt_calls(stmt: &mut Stmt, renames: &HashMap<String, String>) {
    let rename = |expr: &mut Expr| expr_calls(expr, renames);
    match &mut stmt.kind {
        StmtKind::Var(decl) => {
            if let Some(ty) = &mut decl.ty {
                type_calls(ty, renames);
            }
            if let Some(init) = &mut decl.init {
                rename(init);
            }
        }
        StmtKind::Assign { target, value } => {
            for access in &mut target.path {
                if let Access::Index(index) = access {
                    rename(index);
                }
            }
            rename(value);
        }
        StmtKind::Require(expr) | StmtKind::Expr(expr) | StmtKind::Return(Some(expr)) => {
            rename(expr)
        }
        StmtKind::Print(exprs) => exprs.iter_mut().for_each(rename),
        StmtKind::If {
            cond,
            then_block,
            else_block,
        } => {
            rename(cond);
            calls_in_block(then_block, renames);
            if let Some(block) = else_block {
                calls_in_block(block, renames);
            }
        }
        StmtKind::For { iter, body, .. } => {
            iterable_calls(iter, renames);
            calls_in_block(body, renames);
        }
        StmtKind::While { cond, bound, body } => {
            rename(cond);
            if let Some(bound) = bound {
                rename(bound);
            }
            calls_in_block(body, renames);
        }
        StmtKind::Solve(solve) => {
            solve.targets.iter_mut().for_each(rename);
            if let Some(limit) = &mut solve.limit {
                rename(limit);
            }
            if let Some((_, objective)) = &mut solve.objective {
                rename(objective);
            }
            for block in solve.body.iter_mut().chain(solve.else_block.iter_mut()) {
                calls_in_block(block, renames);
            }
        }
        StmtKind::Struct(decl) => {
            for (_, ty) in &mut decl.fields {
                type_calls(ty, renames);
            }
        }
        StmtKind::Fn(_) | StmtKind::Return(None) | StmtKind::Enum(_) | StmtKind::Import(_) => {}
    }
}

fn expr_calls(expr: &mut Expr, renames: &HashMap<String, String>) {
    let rename = |expr: &mut Expr| expr_calls(expr, renames);
    match &mut expr.kind {
        ExprKind::Call(callee, args) => {
            if let Some(new) = renames.get(&callee.name) {
                callee.name.clone_from(new);
            }
            args.iter_mut().for_each(|arg| rename(&mut arg.value));
        }
        ExprKind::Comprehension {
            func,
            body,
            clauses,
        } => {
            if let Some(func) = func {
                if let Some(new) = renames.get(&func.name) {
                    func.name.clone_from(new);
                }
            }
            rename(body);
            for clause in clauses {
                match clause {
                    Clause::For(_, iterable) => iterable_calls(iterable, renames),
                    Clause::If(cond) => rename(cond),
                }
            }
        }
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(expr) = part {
                    rename(expr);
                }
            }
        }
        ExprKind::Unary(_, operand) | ExprKind::Field(operand, _) => rename(operand),
        ExprKind::Binary(_, left, right) | ExprKind::Index(left, right) => {
            rename(left);
            rename(right);
        }
        ExprKind::Array(items) => items.iter_mut().for_each(rename),
        ExprKind::In(value, iterable) => {
            rename(value);
            iterable_calls(iterable, renames);
        }
        ExprKind::If(cond, then, otherwise) => {
            rename(cond);
            rename(then);
            rename(otherwise);
        }
        ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Char(_)
        | ExprKind::Var(_)
        | ExprKind::Hole => {}
    }
}

fn iterable_calls(iterable: &mut Iterable, renames: &HashMap<String, String>) {
    match iterable {
        Iterable::Range(range) => range_calls(range, renames),
        Iterable::Expr(expr) => expr_calls(expr, renames),
    }
}

fn range_calls(range: &mut Range, renames: &HashMap<String, String>) {
    expr_calls(&mut range.lo, renames);
    expr_calls(&mut range.hi, renames);
}

fn type_calls(ty: &mut TypeExpr, renames: &HashMap<String, String>) {
    match &mut ty.kind {
        TypeKind::Int(Some(range)) | TypeKind::Str(Some(range)) => range_calls(range, renames),
        TypeKind::IntBits { bits, .. } => expr_calls(bits, renames),
        TypeKind::Array(element, length) => {
            type_calls(element, renames);
            if let Some(length) = length {
                expr_calls(length, renames);
            }
        }
        TypeKind::Bool
        | TypeKind::Int(None)
        | TypeKind::Char
        | TypeKind::Str(None)
        | TypeKind::Named(_) => {}
    }
}
