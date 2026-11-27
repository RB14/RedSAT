//! Elaboration: running a program's concrete parts and recording its constraints.
//!
//! The interpreter executes statements in order. Values that are known are computed directly
//! (arbitrary-precision integers, booleans, strings). Values that depend on unknowns become
//! terms in the [`TermStore`]. `require` records constraints, `solve` hands them to the solver,
//! and a `solve` body then runs once per solution with every unknown replaced by its value.
//!
//! An `if` whose condition depends on unknowns runs *both* branches: the requirements inside a
//! branch are guarded by its condition, and every variable assigned in either branch is merged
//! into `cond ? then_value : else_value`.

use std::collections::HashMap;
use std::io::Write;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::SolverFactory;
use crate::ast::{
    BinaryOp, Block, Expr, ExprKind, Ident, Program, Range, Solve, SolveMode, Stmt, StmtKind,
    StrPart, TypeExpr, TypeKind, UnaryOp, VarDecl,
};
use crate::check::{Type, TypeInfo};
use crate::config::CompilerOptions;
use crate::diagnostic::Diagnostic;
use crate::domain::Interval;
use crate::ir::{Constant, Sort, TermId, TermStore};
use crate::solve::{self, Constraint, SolveOutcome, SolveRequest, SolveStats};
use crate::source::{SourceFile, Span};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Loops longer than this are almost certainly mistakes (and would build huge formulas).
const MAX_LOOP_ITERATIONS: u64 = 10_000_000;
/// The largest exponent allowed on an unknown base (`x ^ k` multiplies `x` by itself).
const MAX_SYMBOLIC_EXPONENT: u32 = 64;

/// A runtime value: known, or a term that depends on unknowns.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Bool(bool),
    Int(BigInt),
    Str(String),
    Sym(TermId),
}

#[derive(Clone, Debug)]
struct Binding {
    value: Value,
    /// The declared integer domain, enforced on every assignment.
    domain: Option<Interval>,
}

#[derive(Clone, Debug, Default)]
struct Scope {
    order: Vec<String>,
    bindings: HashMap<String, Binding>,
}

pub(crate) struct Interpreter<'a> {
    source: &'a SourceFile,
    options: &'a CompilerOptions,
    types: &'a TypeInfo,
    factory: &'a SolverFactory,
    out: &'a mut dyn Write,
    store: TermStore,
    constraints: Vec<Constraint>,
    scopes: Vec<Scope>,
    /// Conditions of the enclosing symbolic `if` branches.
    guards: Vec<TermId>,
    /// The value of every term while a `solve` body runs.
    model: Option<Vec<Constant>>,
    pub(crate) warnings: Vec<Diagnostic>,
    pub(crate) stats: Vec<SolveStats>,
}

impl<'a> Interpreter<'a> {
    pub(crate) fn new(
        source: &'a SourceFile,
        options: &'a CompilerOptions,
        types: &'a TypeInfo,
        factory: &'a SolverFactory,
        out: &'a mut dyn Write,
    ) -> Self {
        Self {
            source,
            options,
            types,
            factory,
            out,
            store: TermStore::default(),
            constraints: Vec::new(),
            scopes: vec![Scope::default()],
            guards: Vec::new(),
            model: None,
            warnings: Vec::new(),
            stats: Vec::new(),
        }
    }

    pub(crate) fn run(&mut self, program: &Program) -> Result<()> {
        program.stmts.iter().try_for_each(|stmt| self.exec(stmt))
    }

    // --- statements ----------------------------------------------------------------------

    fn exec_block(&mut self, block: &Block) -> Result<()> {
        self.scopes.push(Scope::default());
        let result = block.stmts.iter().try_for_each(|stmt| self.exec(stmt));
        self.scopes.pop();
        result
    }

    fn exec(&mut self, stmt: &Stmt) -> Result<()> {
        match &stmt.kind {
            StmtKind::Var(decl) => self.exec_var(decl),
            StmtKind::Assign { target, value } => self.exec_assign(target, value),
            StmtKind::Require(cond) => {
                self.forbid_in_solve_body("`require`", stmt.span)?;
                let value = self.eval(cond)?;
                self.require(value, cond.span);
                Ok(())
            }
            StmtKind::Print(args) => self.exec_print(args, stmt.span),
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => self.exec_if(cond, then_block, else_block.as_ref()),
            StmtKind::For { var, range, body } => self.exec_for(var, range, body),
            StmtKind::Solve(solve) => self.exec_solve(solve, stmt.span),
        }
    }

    fn forbid_in_solve_body(&self, what: &str, span: Span) -> Result<()> {
        if self.model.is_some() {
            return Err(Diagnostic::error(
                span,
                format!("{what} is not allowed inside a `solve` body"),
            )
            .with_note("a solve body runs once per solution, after every unknown got its value"));
        }
        Ok(())
    }

    fn exec_var(&mut self, decl: &VarDecl) -> Result<()> {
        let domain = match &decl.ty {
            Some(ty) => self.eval_domain(ty)?,
            None => None,
        };
        for (name, id) in &decl.names {
            let value = match &decl.init {
                Some(init) => {
                    let value = self.eval(init)?;
                    if let Some(domain) = &domain {
                        self.enforce_domain(&value, domain, init.span)?;
                    }
                    value
                }
                None => {
                    self.forbid_in_solve_body("declaring an unknown", name.span)?;
                    let sort = match self.types.decl_type(*id) {
                        Type::Bool => Sort::Bool,
                        Type::Int => Sort::Int,
                        Type::Str => unreachable!("string unknowns are rejected by the checker"),
                    };
                    let domain = domain.clone().unwrap_or_else(Interval::full);
                    Value::Sym(
                        self.store
                            .new_unknown(name.name.clone(), sort, domain, name.span),
                    )
                }
            };
            self.declare(
                &name.name,
                Binding {
                    value,
                    domain: domain.clone(),
                },
            );
        }
        Ok(())
    }

    fn eval_domain(&mut self, ty: &TypeExpr) -> Result<Option<Interval>> {
        match &ty.kind {
            TypeKind::Bool | TypeKind::Int(None) => Ok(None),
            TypeKind::Int(Some(range)) => {
                let (lo, hi) = self.eval_range(range)?;
                if lo > hi {
                    return Err(Diagnostic::error(
                        range.span,
                        format!(
                            "the range `{}` contains no values",
                            self.source.slice(range.span)
                        ),
                    )
                    .with_note("`lo..hi` excludes `hi`, while `lo..=hi` includes it"));
                }
                Ok(Some(Interval::new(lo, hi)))
            }
            TypeKind::IntBits { signed, bits } => {
                let width = self.concrete_int(bits, "a bit width")?;
                let width = width
                    .to_u32()
                    .filter(|w| (1..=65_536).contains(w))
                    .ok_or_else(|| {
                        Diagnostic::error(bits.span, "the bit width must be between 1 and 65536")
                    })?;
                Ok(Some(if *signed {
                    let half = BigInt::one() << (width - 1);
                    Interval::new(-half.clone(), half - 1)
                } else {
                    Interval::new(BigInt::zero(), (BigInt::one() << width) - 1)
                }))
            }
        }
    }

    /// The first and last value of a range (the end is excluded for `..`).
    fn eval_range(&mut self, range: &Range) -> Result<(BigInt, BigInt)> {
        let lo = self.concrete_int(&range.lo, "a range bound")?;
        let hi = self.concrete_int(&range.hi, "a range bound")?;
        Ok((lo, if range.inclusive { hi } else { hi - 1 }))
    }

    fn concrete_int(&mut self, expr: &Expr, what: &str) -> Result<BigInt> {
        match self.eval(expr)? {
            Value::Int(value) => Ok(value),
            Value::Sym(_) => Err(Diagnostic::error(
                expr.span,
                format!("{what} must be known at compile time"),
            )
            .with_label("this depends on unknowns")),
            other => unreachable!("type checked as int, got {other:?}"),
        }
    }

    /// Keeps `value` inside `domain`: checked now for known values, required for unknown ones.
    fn enforce_domain(&mut self, value: &Value, domain: &Interval, span: Span) -> Result<()> {
        match value {
            Value::Int(known) if !domain.contains(known) => Err(Diagnostic::error(
                span,
                format!("{known} is outside the declared range {domain}"),
            )),
            Value::Sym(term) => {
                let (lo, hi) = domain.bounds().expect("declared domains are bounded");
                let (lo, hi) = (self.store.int(lo.clone()), self.store.int(hi.clone()));
                let above = self.store.le(lo, *term);
                let below = self.store.le(*term, hi);
                let inside = self.store.and(above, below);
                self.add_constraint(inside, span);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Records a constraint, guarded by the conditions of enclosing symbolic branches.
    fn add_constraint(&mut self, term: TermId, span: Span) {
        let mut guard = self.store.bool(true);
        for condition in self.guards.clone() {
            guard = self.store.and(guard, condition);
        }
        let term = self.store.implies(guard, term);
        self.constraints.push(Constraint { term, span });
    }

    fn require(&mut self, value: Value, span: Span) {
        match value {
            Value::Bool(true) => {}
            Value::Bool(false) => {
                if self.guards.is_empty() {
                    self.warnings.push(Diagnostic::warning(
                        span,
                        "this requirement is always false, so the problem has no solution",
                    ));
                }
                let never = self.store.bool(false);
                self.add_constraint(never, span);
            }
            Value::Sym(term) => self.add_constraint(term, span),
            Value::Int(_) | Value::Str(_) => unreachable!("type checked as bool"),
        }
    }

    fn exec_assign(&mut self, target: &Ident, value: &Expr) -> Result<()> {
        let new_value = self.eval(value)?;
        if let Some(domain) = self.binding(&target.name).domain.clone() {
            self.enforce_domain(&new_value, &domain, value.span)?;
        }
        self.binding_mut(&target.name).value = new_value;
        Ok(())
    }

    fn exec_print(&mut self, args: &[Expr], span: Span) -> Result<()> {
        if !self.guards.is_empty() {
            return Err(Diagnostic::error(
                span,
                "cannot print under a condition that depends on unknowns",
            )
            .with_help("print inside a `solve` body instead"));
        }
        let mut parts = Vec::with_capacity(args.len());
        for arg in args {
            let value = self.eval(arg)?;
            parts.push(Self::display(&value, arg.span)?);
        }
        self.write_line(&parts.join(" "), span)
    }

    fn write_line(&mut self, line: &str, span: Span) -> Result<()> {
        writeln!(self.out, "{line}")
            .map_err(|error| Diagnostic::error(span, format!("cannot write output: {error}")))
    }

    fn exec_if(
        &mut self,
        cond: &Expr,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<()> {
        match self.eval(cond)? {
            Value::Bool(true) => self.exec_block(then_block),
            Value::Bool(false) => else_block.map_or(Ok(()), |block| self.exec_block(block)),
            Value::Sym(condition) => {
                self.exec_symbolic_if(condition, cond.span, then_block, else_block)
            }
            other => unreachable!("type checked as bool, got {other:?}"),
        }
    }

    fn exec_symbolic_if(
        &mut self,
        condition: TermId,
        span: Span,
        then_block: &Block,
        else_block: Option<&Block>,
    ) -> Result<()> {
        let before = self.scopes.clone();
        self.guards.push(condition);
        let then_result = self.exec_block(then_block);
        self.guards.pop();
        then_result?;

        let after_then = std::mem::replace(&mut self.scopes, before);
        let negated = self.store.not(condition);
        self.guards.push(negated);
        let else_result = else_block.map_or(Ok(()), |block| self.exec_block(block));
        self.guards.pop();
        else_result?;

        // Both branches leave the same variables in scope; merge the ones that differ.
        for (level, scope) in after_then.into_iter().enumerate() {
            for (name, then_binding) in scope.bindings {
                let else_value = self.scopes[level].bindings[&name].value.clone();
                if then_binding.value != else_value {
                    let merged = self.merge(condition, &then_binding.value, &else_value, span)?;
                    self.scopes[level]
                        .bindings
                        .get_mut(&name)
                        .expect("present")
                        .value = merged;
                }
            }
        }
        Ok(())
    }

    fn merge(
        &mut self,
        condition: TermId,
        then_value: &Value,
        else_value: &Value,
        span: Span,
    ) -> Result<Value> {
        if matches!(then_value, Value::Str(_)) || matches!(else_value, Value::Str(_)) {
            return Err(Diagnostic::error(
                span,
                "a string cannot be assigned under a condition that depends on unknowns",
            ));
        }
        let then_term = self.term_of(then_value);
        let else_term = self.term_of(else_value);
        Ok(Value::Sym(self.store.ite(condition, then_term, else_term)))
    }

    fn exec_for(&mut self, var: &Ident, range: &Range, body: &Block) -> Result<()> {
        let (lo, hi) = self.eval_range(range)?;
        if lo > hi {
            return Ok(());
        }
        let iterations = (&hi - &lo + 1u32)
            .to_u64()
            .filter(|&n| n <= MAX_LOOP_ITERATIONS)
            .ok_or_else(|| {
                Diagnostic::error(
                    range.span,
                    format!("this loop would run more than {MAX_LOOP_ITERATIONS} times"),
                )
            })?;
        let mut index = lo;
        for _ in 0..iterations {
            self.scopes.push(Scope::default());
            self.declare(
                &var.name,
                Binding {
                    value: Value::Int(index.clone()),
                    domain: None,
                },
            );
            let result = self.exec_block(body);
            self.scopes.pop();
            result?;
            index += 1;
        }
        Ok(())
    }

    fn exec_solve(&mut self, solve: &Solve, span: Span) -> Result<()> {
        if self.model.is_some() {
            return Err(Diagnostic::error(
                span,
                "`solve` cannot be nested inside another `solve` body",
            ));
        }
        if !self.guards.is_empty() {
            return Err(Diagnostic::error(
                span,
                "`solve` cannot run under a condition that depends on unknowns",
            ));
        }

        let targets: Vec<(String, Value)> = if solve.targets.is_empty() {
            self.symbolic_variables()
        } else {
            let mut targets = Vec::with_capacity(solve.targets.len());
            for target in &solve.targets {
                targets.push((self.label(target), self.eval(target)?));
            }
            targets
        };
        let limit = match &solve.limit {
            Some(expr) => {
                let limit = self.concrete_int(expr, "`limit`")?;
                Some(limit.to_u64().filter(|&n| n >= 1).ok_or_else(|| {
                    Diagnostic::error(expr.span, "`limit` must be a positive number")
                })?)
            }
            None => None,
        };
        if solve.mode == SolveMode::Unique && limit.is_some() {
            return Err(Diagnostic::error(
                span,
                "`limit` cannot be combined with `unique`",
            ));
        }
        let objective = match &solve.objective {
            Some((direction, expr)) => match self.eval(expr)? {
                Value::Sym(term) => Some((*direction, term)),
                _ => None, // a known objective leaves nothing to optimize
            },
            None => None,
        };
        let projection = targets
            .iter()
            .filter_map(|(_, value)| match value {
                Value::Sym(t) => Some(*t),
                _ => None,
            })
            .collect();

        // The store and constraints are lent to the solver while solve bodies run, which only
        // ever see known values.
        let store = std::mem::take(&mut self.store);
        let constraints = std::mem::take(&mut self.constraints);
        let (options, factory) = (self.options, self.factory);
        let request = SolveRequest {
            constraints: &constraints,
            projection,
            mode: solve.mode,
            limit,
            objective,
            span,
        };
        let body = solve.body.as_ref();
        let result = solve::solve(&store, &request, options, factory, &mut |model| {
            self.run_solution(model, &targets, body)
        });
        self.store = store;
        self.constraints = constraints;

        let (outcome, stats, warnings) = result?;
        self.warnings.extend(warnings);
        self.stats.push(stats);
        match outcome {
            SolveOutcome::Solved => Ok(()),
            SolveOutcome::NoSolution { searched } => match &solve.else_block {
                Some(block) => self.exec_block(block),
                None => {
                    let note = searched.map_or_else(String::new, |(bits, names)| {
                        format!(" with |{}| < 2^{bits}", names.join("|, |"))
                    });
                    self.write_line(&format!("no solution{note}"), span)
                }
            },
            SolveOutcome::NotUnique { first, second } => Err(Diagnostic::error(
                span,
                "`solve unique` found more than one solution",
            )
            .with_note(format!(
                "one solution: {}",
                Self::describe(&targets, &first)
            ))
            .with_note(format!(
                "another one:  {}",
                Self::describe(&targets, &second)
            ))),
        }
    }

    fn run_solution(
        &mut self,
        model: &[Constant],
        targets: &[(String, Value)],
        body: Option<&Block>,
    ) -> Result<()> {
        self.model = Some(model.to_vec());
        let result = match body {
            Some(body) => self.exec_block(body),
            None => {
                let line = Self::describe(targets, model);
                self.write_line(&line, Span::default())
            }
        };
        self.model = None;
        result
    }

    /// `x = 3, y = 4` for the targets under `model`.
    fn describe(targets: &[(String, Value)], model: &[Constant]) -> String {
        targets
            .iter()
            .map(|(label, value)| {
                let shown = match value {
                    Value::Sym(term) => model[term.index()].to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Int(v) => v.to_string(),
                    Value::Str(s) => s.clone(),
                };
                format!("{label} = {shown}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Every variable in scope whose value depends on unknowns (the targets of a bare `solve`).
    fn symbolic_variables(&self) -> Vec<(String, Value)> {
        self.scopes
            .iter()
            .flat_map(|scope| {
                scope
                    .order
                    .iter()
                    .map(move |name| (name, &scope.bindings[name]))
            })
            .filter(|(_, binding)| matches!(binding.value, Value::Sym(_)))
            .map(|(name, binding)| (name.clone(), binding.value.clone()))
            .collect()
    }

    fn label(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Var(ident) => ident.name.clone(),
            _ => self.source.slice(expr.span).to_string(),
        }
    }

    // --- variables -----------------------------------------------------------------------

    fn declare(&mut self, name: &str, binding: Binding) {
        let scope = self.scopes.last_mut().expect("there is always a scope");
        if scope.bindings.insert(name.to_string(), binding).is_none() {
            scope.order.push(name.to_string());
        }
    }

    fn binding(&self, name: &str) -> &Binding {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.bindings.get(name))
            .expect("names are resolved by the checker")
    }

    fn binding_mut(&mut self, name: &str) -> &mut Binding {
        self.scopes
            .iter_mut()
            .rev()
            .find_map(|scope| scope.bindings.get_mut(name))
            .expect("names are resolved by the checker")
    }

    // --- expressions ---------------------------------------------------------------------

    fn eval(&mut self, expr: &Expr) -> Result<Value> {
        match &expr.kind {
            ExprKind::Int(value) => Ok(Value::Int(value.clone())),
            ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            ExprKind::Str(parts) => {
                let mut text = String::new();
                for part in parts {
                    match part {
                        StrPart::Text(literal) => text.push_str(literal),
                        StrPart::Expr(inner) => {
                            let value = self.eval(inner)?;
                            text.push_str(&Self::display(&value, inner.span)?);
                        }
                    }
                }
                Ok(Value::Str(text))
            }
            ExprKind::Var(ident) => Ok(self.concretize(self.binding(&ident.name).value.clone())),
            ExprKind::Unary(op, operand) => {
                let value = self.eval(operand)?;
                Ok(self.unary(*op, value))
            }
            ExprKind::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs),
            ExprKind::Call(name, args) => self.call(&name.name, args),
        }
    }

    /// Inside a `solve` body, replaces terms by their value in the current solution.
    fn concretize(&self, value: Value) -> Value {
        match (&value, &self.model) {
            (Value::Sym(term), Some(model)) => match &model[term.index()] {
                Constant::Bool(b) => Value::Bool(*b),
                Constant::Int(v) => Value::Int(v.clone()),
            },
            _ => value,
        }
    }

    fn display(value: &Value, span: Span) -> Result<String> {
        match value {
            Value::Bool(b) => Ok(b.to_string()),
            Value::Int(v) => Ok(v.to_string()),
            Value::Str(s) => Ok(s.clone()),
            Value::Sym(_) => Err(Diagnostic::error(
                span,
                "this value depends on unknowns, so it cannot be shown here",
            )
            .with_help("print it inside a `solve` body, where every unknown has a value")),
        }
    }

    fn term_of(&mut self, value: &Value) -> TermId {
        match value {
            Value::Bool(b) => self.store.bool(*b),
            Value::Int(v) => self.store.int(v.clone()),
            Value::Sym(term) => *term,
            Value::Str(_) => unreachable!("strings never become terms"),
        }
    }

    fn unary(&mut self, op: UnaryOp, value: Value) -> Value {
        match (op, value) {
            (UnaryOp::Neg, Value::Int(v)) => Value::Int(-v),
            (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
            (UnaryOp::Neg, Value::Sym(t)) => Value::Sym(self.store.neg(t)),
            (UnaryOp::Not, Value::Sym(t)) => Value::Sym(self.store.not(t)),
            (op, value) => unreachable!("type checked: {op:?} {value:?}"),
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Result<Value> {
        let left = self.eval(lhs)?;
        // Short-circuit on a known left operand.
        match (op, &left) {
            (BinaryOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
            (BinaryOp::Or, Value::Bool(true)) | (BinaryOp::Implies, Value::Bool(false)) => {
                return Ok(Value::Bool(true));
            }
            _ => {}
        }
        let right = self.eval(rhs)?;
        if matches!(left, Value::Sym(_)) || matches!(right, Value::Sym(_)) {
            self.symbolic_binary(op, &left, &right, lhs.span.to(rhs.span), rhs.span)
        } else {
            concrete_binary(op, left, right, rhs.span)
        }
    }

    fn symbolic_binary(
        &mut self,
        op: BinaryOp,
        left: &Value,
        right: &Value,
        span: Span,
        rhs_span: Span,
    ) -> Result<Value> {
        match op {
            BinaryOp::Pow => {
                let Value::Int(exponent) = right else {
                    return Err(Diagnostic::error(
                        rhs_span,
                        "the exponent must be known at compile time",
                    ));
                };
                let exponent = exponent_of(exponent, rhs_span)?;
                if exponent > MAX_SYMBOLIC_EXPONENT {
                    return Err(Diagnostic::error(
                        rhs_span,
                        format!(
                            "exponents above {MAX_SYMBOLIC_EXPONENT} are not supported for unknown bases"
                        ),
                    ));
                }
                let base = self.term_of(left);
                return Ok(Value::Sym(self.power(base, exponent)));
            }
            BinaryOp::Div | BinaryOp::Mod => {
                return Err(Diagnostic::error(
                    span,
                    "`/` and `%` on unknown values are not available yet",
                )
                .with_note("they arrive in Phase 4 of the roadmap"));
            }
            _ => {}
        }
        let (a, b) = (self.term_of(left), self.term_of(right));
        let store = &mut self.store;
        let booleans = store.sort(a) == Sort::Bool;
        let term = match op {
            BinaryOp::Add => store.add(a, b),
            BinaryOp::Sub => store.sub(a, b),
            BinaryOp::Mul => store.mul(a, b),
            BinaryOp::Eq if booleans => store.iff(a, b),
            BinaryOp::Eq => store.eq(a, b),
            BinaryOp::Ne => {
                let equal = if booleans {
                    store.iff(a, b)
                } else {
                    store.eq(a, b)
                };
                store.not(equal)
            }
            BinaryOp::Lt => store.lt(a, b),
            BinaryOp::Le => store.le(a, b),
            BinaryOp::Gt => store.lt(b, a),
            BinaryOp::Ge => store.le(b, a),
            BinaryOp::And => store.and(a, b),
            BinaryOp::Or => store.or(a, b),
            BinaryOp::Xor => store.xor(a, b),
            BinaryOp::Implies => store.implies(a, b),
            BinaryOp::Iff => store.iff(a, b),
            BinaryOp::Pow | BinaryOp::Div | BinaryOp::Mod => unreachable!("handled above"),
        };
        Ok(Value::Sym(term))
    }

    /// `base ^ exponent` by repeated squaring.
    fn power(&mut self, base: TermId, exponent: u32) -> TermId {
        let mut result = self.store.int(BigInt::one());
        for bit in (0..u32::BITS - exponent.leading_zeros()).rev() {
            result = self.store.mul(result, result);
            if (exponent >> bit) & 1 == 1 {
                result = self.store.mul(result, base);
            }
        }
        result
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg)?);
        }
        Ok(match name {
            "abs" => match values.pop().expect("one argument") {
                Value::Int(v) => Value::Int(v.abs()),
                Value::Sym(t) => {
                    let zero = self.store.int(BigInt::zero());
                    let negative = self.store.lt(t, zero);
                    let negated = self.store.neg(t);
                    Value::Sym(self.store.ite(negative, negated, t))
                }
                other => unreachable!("type checked as int, got {other:?}"),
            },
            "min" | "max" => {
                let pick_min = name == "min";
                let mut values = values.into_iter();
                let first = values.next().expect("at least one argument");
                values.fold(first, |best, next| self.pick(pick_min, best, next))
            }
            other => unreachable!("unknown functions are rejected by the checker: {other}"),
        })
    }

    fn pick(&mut self, pick_min: bool, a: Value, b: Value) -> Value {
        if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
            return if pick_min == (x <= y) { a } else { b };
        }
        let (x, y) = (self.term_of(&a), self.term_of(&b));
        let x_le_y = self.store.le(x, y);
        Value::Sym(if pick_min {
            self.store.ite(x_le_y, x, y)
        } else {
            self.store.ite(x_le_y, y, x)
        })
    }
}

fn exponent_of(exponent: &BigInt, span: Span) -> Result<u32> {
    exponent.to_u32().ok_or_else(|| {
        Diagnostic::error(
            span,
            "the exponent must be a non-negative number below 2^32",
        )
    })
}

/// Arithmetic, comparison and logic on known values. Division is Euclidean: the remainder
/// always lies in `0..|divisor|`.
fn concrete_binary(op: BinaryOp, left: Value, right: Value, rhs_span: Span) -> Result<Value> {
    use Value::{Bool, Int};
    let division_by_zero = || Diagnostic::error(rhs_span, "division by zero");
    Ok(match (op, left, right) {
        (BinaryOp::Add, Int(a), Int(b)) => Int(a + b),
        (BinaryOp::Sub, Int(a), Int(b)) => Int(a - b),
        (BinaryOp::Mul, Int(a), Int(b)) => Int(a * b),
        (BinaryOp::Div, Int(a), Int(b)) => {
            if b.is_zero() {
                return Err(division_by_zero());
            }
            let remainder = a.mod_floor(&b.abs());
            Int((a - remainder) / b)
        }
        (BinaryOp::Mod, Int(a), Int(b)) => {
            if b.is_zero() {
                return Err(division_by_zero());
            }
            Int(a.mod_floor(&b.abs()))
        }
        (BinaryOp::Pow, Int(a), Int(b)) => {
            let exponent = exponent_of(&b, rhs_span)?;
            Int(num_traits::pow(a, exponent as usize))
        }
        (BinaryOp::Eq, a, b) => Bool(a == b),
        (BinaryOp::Ne, a, b) => Bool(a != b),
        (BinaryOp::Lt, Int(a), Int(b)) => Bool(a < b),
        (BinaryOp::Le, Int(a), Int(b)) => Bool(a <= b),
        (BinaryOp::Gt, Int(a), Int(b)) => Bool(a > b),
        (BinaryOp::Ge, Int(a), Int(b)) => Bool(a >= b),
        (BinaryOp::And, Bool(a), Bool(b)) => Bool(a && b),
        (BinaryOp::Or, Bool(a), Bool(b)) => Bool(a || b),
        (BinaryOp::Xor, Bool(a), Bool(b)) => Bool(a != b),
        (BinaryOp::Implies, Bool(a), Bool(b)) => Bool(!a || b),
        (BinaryOp::Iff, Bool(a), Bool(b)) => Bool(a == b),
        (op, a, b) => unreachable!("type checked: {a:?} {} {b:?}", op.symbol()),
    })
}
