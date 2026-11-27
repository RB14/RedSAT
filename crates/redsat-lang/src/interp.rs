//! Elaboration: running a program's concrete parts and recording its constraints.
//!
//! The interpreter executes statements in order. Values that are known are computed directly
//! (arbitrary-precision integers, booleans, characters, strings, arrays, structs). Values that
//! depend on unknowns become terms in the [`TermStore`]. `require` records constraints, `solve`
//! hands them to the solver, and a `solve` body then runs once per solution with every unknown
//! replaced by its value.
//!
//! Control flow whose condition depends on unknowns runs *every* path:
//! - an `if` runs both branches; requirements inside are guarded by the branch condition and
//!   variables assigned in either branch are merged into `cond ? then_value : else_value`;
//! - `a and b` evaluates `b` under the condition `a` (so `i < n and xs[i] > 0` is safe);
//! - a `return` under an unknown condition records the result for that condition, and masks
//!   every later effect of the function on the paths that already returned;
//! - `while cond bound K` unrolls at most `K` iterations and requires the loop to finish.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;
use web_time::Instant;

use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::ast::{
    Access, Arg, BinaryOp, Block, Clause, Expr, ExprKind, FnDecl, Ident, Iterable, Place, Program,
    Range, Solve, SolveMode, Stmt, StmtKind, StrPart, TypeExpr, TypeKind, UnaryOp, VarDecl,
};
use crate::check::{Type, TypeInfo};
use crate::config::CompilerOptions;
use crate::diagnostic::Diagnostic;
use crate::domain::Interval;
use crate::ir::{BitOp, Constant, Sort, TermId, TermStore, euclid};
use crate::solve::{self, Constraint, SolveOutcome, SolveRequest, SolveStats};
use crate::source::{SourceMap, Span};
use crate::{AskFn, SolverFactory};

type Result<T> = std::result::Result<T, Diagnostic>;

/// Loops longer than this are almost certainly mistakes (and would build huge formulas).
const MAX_LOOP_ITERATIONS: u64 = 10_000_000;
/// The largest exponent allowed on an unknown base (`x ^ k` multiplies `x` by itself).
const MAX_SYMBOLIC_EXPONENT: u32 = 64;
/// Functions are inlined; deeper recursion means it does not stop on known values.
const MAX_CALL_DEPTH: usize = 200;
/// The capacity of an unknown `string` declared without length bounds.
const DEFAULT_STRING_CAPACITY: usize = 32;
/// The largest Unicode scalar value.
const MAX_CHAR: u32 = 0x10_FFFF;

/// How to show and combine a scalar that is stored as an integer or boolean term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Bool,
    Int,
    Char,
    Enum(usize),
}

/// A runtime value: known, or (partly) made of terms that depend on unknowns.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Unit,
    Bool(bool),
    Int(BigInt),
    Char(u32),
    /// An enum value: (enum index, variant index).
    Enum(usize, usize),
    Str(Rc<str>),
    Sym(TermId, Tag),
    Array(Rc<Vec<Value>>),
    Struct(usize, Rc<Vec<Value>>),
    SymStr(Rc<SymStr>),
}

/// A string with unknown characters and/or length. Positions at and beyond `len` hold `'\0'`,
/// and positions before it never do, so every string has exactly one representation.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SymStr {
    chars: Vec<Value>,
    len: Value,
}

/// A value together with the condition under which it is present (for filtered comprehensions
/// and strings of unknown length).
struct Item {
    value: Value,
    cond: Option<TermId>,
}

/// The evaluated form of a declared type: domains and lengths are known numbers.
#[derive(Clone, Debug)]
enum Shape {
    Bool,
    Int(Option<Interval>),
    Char,
    Str {
        min_len: usize,
        capacity: usize,
        defaulted: bool,
    },
    Array(Box<Shape>, Option<usize>),
    Enum(usize),
    Struct(usize, Vec<Shape>),
}

#[derive(Clone, Debug)]
struct Binding {
    value: Value,
    /// The declared type, enforced on every assignment.
    shape: Option<Shape>,
}

#[derive(Clone, Debug, Default)]
struct Scope {
    order: Vec<String>,
    bindings: HashMap<String, Binding>,
}

/// The state of one function call.
struct Frame {
    /// The result so far (merged over the paths that returned).
    result: Option<Value>,
    /// Whether the function has returned: `Bool(false)`, or a condition.
    done: Value,
    /// The number of guards active when the call started.
    guard_depth: usize,
}

/// Whether execution continues or unwinds out of a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Next,
    Return,
}

/// One evaluated step of an assignment target.
enum Accessor {
    Index(Value, Span),
    Field(String),
}

pub(crate) struct Interpreter<'a> {
    sources: &'a SourceMap,
    options: &'a CompilerOptions,
    types: &'a TypeInfo,
    factory: &'a SolverFactory,
    out: &'a mut (dyn Write + Send),
    ask: Option<&'a AskFn>,
    trace: Option<&'a crate::TraceFn>,
    cancel: Option<&'a crate::CancelFn>,
    store: TermStore,
    constraints: Vec<Constraint>,
    scopes: Vec<Scope>,
    functions: HashMap<String, Rc<FnDecl>>,
    frames: Vec<Frame>,
    /// Conditions of the enclosing symbolic branches.
    guards: Vec<TermId>,
    /// The value of every term while a `solve` body runs.
    model: Option<Rc<Vec<Constant>>>,
    struct_depth: usize,
    /// Set when the program is analysed for editor tooling instead of run.
    pub(crate) analysis: Option<AnalysisState>,
    pub(crate) warnings: Vec<Diagnostic>,
    pub(crate) stats: Vec<SolveStats>,
    pub(crate) steps: crate::ExecSteps,
}

impl<'a> Interpreter<'a> {
    pub(crate) fn new(
        sources: &'a SourceMap,
        options: &'a CompilerOptions,
        types: &'a TypeInfo,
        factory: &'a SolverFactory,
        out: &'a mut (dyn Write + Send),
        hooks: crate::Hooks<'a>,
    ) -> Self {
        Self {
            sources,
            options,
            types,
            factory,
            out,
            ask: hooks.ask,
            trace: hooks.trace,
            cancel: hooks.cancel,
            store: TermStore::default(),
            constraints: Vec::new(),
            scopes: vec![Scope::default()],
            functions: HashMap::new(),
            frames: Vec::new(),
            guards: Vec::new(),
            model: None,
            struct_depth: 0,
            analysis: None,
            warnings: Vec::new(),
            stats: Vec::new(),
            steps: crate::ExecSteps::default(),
        }
    }

    pub(crate) fn run(&mut self, program: &Program) -> Result<()> {
        for stmt in &program.stmts {
            if let StmtKind::Fn(decl) = &stmt.kind {
                self.functions
                    .insert(decl.name.name.clone(), Rc::clone(decl));
            }
        }
        for stmt in &program.stmts {
            self.exec(stmt)?;
        }
        Ok(())
    }

    // --- statements ----------------------------------------------------------------------

    fn exec_block(&mut self, block: &Block) -> Result<Flow> {
        self.scopes.push(Scope::default());
        let mut result = Ok(Flow::Next);
        for stmt in &block.stmts {
            match self.exec(stmt) {
                Ok(Flow::Next) => {}
                other => {
                    result = other;
                    break;
                }
            }
        }
        self.scopes.pop();
        result
    }

    fn exec(&mut self, stmt: &Stmt) -> Result<Flow> {
        if let Some(analysis) = &self.analysis {
            if Instant::now() > analysis.deadline {
                return Err(Diagnostic::error(stmt.span, ANALYSIS_TIMEOUT));
            }
        }
        self.steps.statements += 1;
        if self.steps.statements % 256 == 0 && self.cancel.is_some_and(|cancel| cancel()) {
            return Err(Diagnostic::error(stmt.span, crate::STOPPED));
        }
        match &stmt.kind {
            StmtKind::Var(decl) => self.exec_var(decl)?,
            StmtKind::Assign { target, value } => self.exec_assign(target, value)?,
            StmtKind::Require(cond) => {
                self.forbid_in_solve_body("`require`", stmt.span)?;
                let value = self.eval(cond)?;
                self.require(&value, cond.span);
            }
            StmtKind::Print(args) => self.exec_print(args, stmt.span)?,
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                return self.exec_if(cond, then_block, else_block.as_ref());
            }
            StmtKind::For { var, iter, body } => return self.exec_for(var, iter, body),
            StmtKind::While { cond, bound, body } => {
                return self.exec_while(cond, bound.as_ref(), body);
            }
            StmtKind::Solve(solve) => self.exec_solve(solve, stmt.span)?,
            StmtKind::Return(value) => return self.exec_return(value.as_ref(), stmt.span),
            StmtKind::Expr(expr) => {
                self.eval(expr)?;
            }
            StmtKind::Fn(_) | StmtKind::Enum(_) | StmtKind::Struct(_) | StmtKind::Import(_) => {}
        }
        Ok(Flow::Next)
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

    /// Whether execution currently depends on unknowns (a symbolic branch, or a function that
    /// may already have returned).
    fn under_condition(&self) -> bool {
        !self.guards.is_empty()
            || self
                .frames
                .iter()
                .any(|frame| matches!(frame.done, Value::Sym(..)))
    }

    /// The condition under which the current statement executes.
    fn path_guard(&mut self) -> TermId {
        let mut guard = self.store.bool(true);
        for condition in self.guards.clone() {
            guard = self.store.and(guard, condition);
        }
        let done: Vec<TermId> = self
            .frames
            .iter()
            .filter_map(|frame| match frame.done {
                Value::Sym(term, _) => Some(term),
                _ => None,
            })
            .collect();
        for returned in done {
            let still_running = self.store.not(returned);
            guard = self.store.and(guard, still_running);
        }
        guard
    }

    /// Records a constraint, guarded by the current path condition.
    fn add_constraint(&mut self, term: TermId, span: Span) {
        let guard = self.path_guard();
        let term = self.store.implies(guard, term);
        self.constraints.push(Constraint { term, span });
    }

    fn require(&mut self, value: &Value, span: Span) {
        match value {
            Value::Bool(true) => {}
            Value::Bool(false) => {
                if !self.under_condition() {
                    self.warnings.push(Diagnostic::warning(
                        span,
                        "this requirement is always false, so the problem has no solution",
                    ));
                }
                let never = self.store.bool(false);
                self.add_constraint(never, span);
            }
            Value::Sym(term, _) => self.add_constraint(*term, span),
            other => unreachable!("type checked as bool, got {other:?}"),
        }
    }

    fn exec_var(&mut self, decl: &VarDecl) -> Result<()> {
        let shape = match &decl.ty {
            Some(ty) => Some(self.shape_of(ty)?),
            None => None,
        };
        for (name, id) in &decl.names {
            let value = match &decl.init {
                Some(init) => {
                    let value = self.eval(init)?;
                    if let Some(shape) = &shape {
                        self.enforce_shape(&value, shape, init.span)?;
                    }
                    value
                }
                None => {
                    self.forbid_in_solve_body("declaring an unknown", name.span)?;
                    let shape = match &shape {
                        Some(shape) => shape.clone(),
                        None => {
                            self.shape_of_type(&self.types.decl_type(*id).clone(), name.span)?
                        }
                    };
                    self.new_unknown(&name.name, &shape, name.span)?
                }
            };
            self.declare(
                &name.name,
                Binding {
                    value,
                    shape: shape.clone(),
                },
            );
        }
        Ok(())
    }

    fn exec_assign(&mut self, place: &Place, value: &Expr) -> Result<()> {
        let new_value = self.eval(value)?;
        let binding = self.binding(&place.root.name).clone();
        let mut accessors = Vec::with_capacity(place.path.len());
        let mut shape = binding.shape.clone();
        for access in &place.path {
            match access {
                Access::Index(index) => {
                    let index_value = self.eval(index)?;
                    accessors.push(Accessor::Index(index_value, index.span));
                    shape = match shape {
                        Some(Shape::Array(element, _)) => Some(*element),
                        _ => None,
                    };
                }
                Access::Field(field) => {
                    accessors.push(Accessor::Field(field.name.clone()));
                    shape = match shape {
                        Some(Shape::Struct(id, fields)) => {
                            fields.get(self.field_position(id, &field.name)).cloned()
                        }
                        _ => None,
                    };
                }
            }
        }
        if let Some(shape) = &shape {
            self.enforce_shape(&new_value, shape, value.span)?;
        }
        let mut updated = self.update(binding.value.clone(), &accessors, new_value, place.span)?;
        if let Some(Value::Sym(done, _)) = self.frames.last().map(|frame| frame.done.clone()) {
            // On paths where the function already returned, the variable keeps its value.
            updated = self.merge(done, binding.value, updated, place.span)?;
        }
        self.binding_mut(&place.root.name).value = updated;
        Ok(())
    }

    /// `current` with the element at `accessors` replaced by `new`.
    fn update(
        &mut self,
        current: Value,
        accessors: &[Accessor],
        new: Value,
        span: Span,
    ) -> Result<Value> {
        let Some((first, rest)) = accessors.split_first() else {
            return Ok(new);
        };
        match (first, current) {
            (Accessor::Index(index, index_span), Value::Array(mut items)) => match index {
                Value::Int(i) => {
                    let k = Self::check_index(i, items.len(), *index_span)?;
                    let element = items[k].clone();
                    let updated = self.update(element, rest, new, span)?;
                    Rc::make_mut(&mut items)[k] = updated;
                    Ok(Value::Array(items))
                }
                Value::Sym(index, _) => {
                    self.require_index_in_range(*index, items.len(), *index_span);
                    let mut result = Vec::with_capacity(items.len());
                    for (k, element) in items.iter().enumerate() {
                        let updated = self.update(element.clone(), rest, new.clone(), span)?;
                        let position = self.store.int(BigInt::from(k));
                        let here = self.store.eq(*index, position);
                        result.push(self.merge(here, updated, element.clone(), span)?);
                    }
                    Ok(Value::Array(Rc::new(result)))
                }
                other => unreachable!("type checked as int, got {other:?}"),
            },
            (Accessor::Field(name), Value::Struct(id, mut fields)) => {
                let position = self.field_position(id, name);
                let field = fields[position].clone();
                let updated = self.update(field, rest, new, span)?;
                Rc::make_mut(&mut fields)[position] = updated;
                Ok(Value::Struct(id, fields))
            }
            (Accessor::Index(..), Value::Str(_) | Value::SymStr(_)) => Err(Diagnostic::error(
                span,
                "strings cannot be changed in place",
            )
            .with_help("build a new string instead, e.g. with `+`")),
            (_, other) => unreachable!("type checked place, got {other:?}"),
        }
    }

    fn exec_print(&mut self, args: &[Expr], span: Span) -> Result<()> {
        if self.under_condition() {
            return Err(Diagnostic::error(
                span,
                "cannot print under a condition that depends on unknowns",
            )
            .with_help("print inside a `solve` body instead"));
        }
        let mut parts = Vec::with_capacity(args.len());
        for arg in args {
            let value = self.eval(arg)?;
            parts.push(self.display(&value, arg.span, false)?);
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
    ) -> Result<Flow> {
        match self.eval(cond)? {
            Value::Bool(true) => self.exec_block(then_block),
            Value::Bool(false) => else_block.map_or(Ok(Flow::Next), |block| self.exec_block(block)),
            Value::Sym(condition, _) => {
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
    ) -> Result<Flow> {
        let before = self.scopes.clone();
        self.guards.push(condition);
        let then_result = self.exec_block(then_block);
        self.guards.pop();
        then_result?;

        let after_then = std::mem::replace(&mut self.scopes, before);
        let negated = self.store.not(condition);
        self.guards.push(negated);
        let else_result = else_block.map_or(Ok(Flow::Next), |block| self.exec_block(block));
        self.guards.pop();
        else_result?;

        // Both branches leave the same variables in scope; merge the ones that differ.
        for (level, scope) in after_then.into_iter().enumerate() {
            for (name, then_binding) in scope.bindings {
                let else_value = self.scopes[level].bindings[&name].value.clone();
                if then_binding.value != else_value {
                    let merged = self.merge(condition, then_binding.value, else_value, span)?;
                    self.scopes[level]
                        .bindings
                        .get_mut(&name)
                        .expect("present")
                        .value = merged;
                }
            }
        }
        Ok(Flow::Next)
    }

    fn exec_for(&mut self, var: &Ident, iter: &Iterable, body: &Block) -> Result<Flow> {
        for item in self.iterate(iter)? {
            self.scopes.push(Scope::default());
            self.declare(
                &var.name,
                Binding {
                    value: item.value,
                    shape: None,
                },
            );
            let flow = match item.cond {
                None => self.exec_block(body),
                Some(cond) => self.exec_symbolic_if(cond, body.span, body, None),
            };
            self.scopes.pop();
            if flow? == Flow::Return {
                return Ok(Flow::Return);
            }
        }
        Ok(Flow::Next)
    }

    fn exec_while(&mut self, cond: &Expr, bound: Option<&Expr>, body: &Block) -> Result<Flow> {
        let bound = match bound {
            Some(expr) => {
                let value = self.concrete_int(expr, "a loop bound")?;
                Some(value.to_u64().ok_or_else(|| {
                    Diagnostic::error(expr.span, "a loop bound must be a non-negative number")
                })?)
            }
            None => None,
        };
        let limit = bound.unwrap_or(MAX_LOOP_ITERATIONS);
        let mut iterations = 0u64;
        loop {
            let condition = self.eval(cond)?;
            match condition {
                Value::Bool(false) => return Ok(Flow::Next),
                Value::Bool(true) => {
                    if iterations >= limit {
                        return Err(Diagnostic::error(
                            cond.span,
                            format!("the loop did not finish within {limit} iterations"),
                        ));
                    }
                    iterations += 1;
                    if self.exec_block(body)? == Flow::Return {
                        return Ok(Flow::Return);
                    }
                }
                Value::Sym(term, _) => {
                    if bound.is_none() {
                        return Err(Diagnostic::error(
                            cond.span,
                            "this loop's condition depends on unknowns",
                        )
                        .with_help(
                            "add a maximum number of iterations: `while … bound 100 { … }`",
                        ));
                    }
                    if iterations >= limit {
                        let finished = self.store.not(term);
                        self.add_constraint(finished, cond.span);
                        return Ok(Flow::Next);
                    }
                    iterations += 1;
                    self.exec_symbolic_if(term, cond.span, body, None)?;
                }
                other => unreachable!("type checked as bool, got {other:?}"),
            }
        }
    }

    fn exec_return(&mut self, value: Option<&Expr>, span: Span) -> Result<Flow> {
        let value = match value {
            Some(expr) => self.eval(expr)?,
            None => Value::Unit,
        };
        let index = self.frames.len() - 1;
        let depth = self.frames[index].guard_depth;
        let mut local = self.store.bool(true);
        for condition in self.guards[depth..].iter().copied() {
            local = self.store.and(local, condition);
        }
        let done = self.frames[index].done.clone();
        let previous = self.frames[index].result.take();
        let unconditional = self.store.constant_of(local) == Some(&Constant::Bool(true));
        let (result, done) = match (unconditional, done) {
            (true, Value::Sym(returned, _)) => {
                let previous = previous.expect("a symbolic return happened earlier");
                (
                    self.merge(returned, previous, value, span)?,
                    Value::Bool(true),
                )
            }
            (true, _) => (value, Value::Bool(true)),
            (false, Value::Sym(returned, _)) => {
                let still_running = self.store.not(returned);
                let now = self.store.and(local, still_running);
                let previous = previous.expect("a symbolic return happened earlier");
                let result = self.merge(now, value, previous, span)?;
                let done = self.store.or(returned, local);
                (result, self.bool_value(done))
            }
            (false, _) => (value, Value::Sym(local, Tag::Bool)),
        };
        let finished = done == Value::Bool(true);
        let frame = &mut self.frames[index];
        frame.result = Some(result);
        frame.done = done;
        Ok(if finished { Flow::Return } else { Flow::Next })
    }

    fn exec_solve(&mut self, solve: &Solve, span: Span) -> Result<()> {
        if self.model.is_some() {
            return Err(Diagnostic::error(
                span,
                "`solve` cannot be nested inside another `solve` body",
            ));
        }
        if self.under_condition() || !self.frames.is_empty() {
            return Err(
                Diagnostic::error(span, "`solve` must be a top-level statement").with_note(
                    "it cannot run inside a function or under a condition that depends on unknowns",
                ),
            );
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
                Value::Sym(term, _) => Some((*direction, term)),
                _ => None, // a known objective leaves nothing to optimize
            },
            None => None,
        };
        let mut projection = Vec::new();
        for (_, value) in &targets {
            leaves(value, &mut projection);
        }

        if self.analysis.is_some() {
            // Editor analysis: infer the domains, but never call the SAT solver.
            let objective = objective.map(|(_, term)| term);
            let (domains, diagnostics) = solve::infer_domains(
                &self.store,
                &self.constraints,
                &projection,
                objective,
                self.options,
            );
            self.warnings.extend(diagnostics);
            if let Some(analysis) = self.analysis.as_mut() {
                for (index, domain) in domains {
                    analysis.record(&self.store, index, domain);
                }
            }
            return Ok(());
        }

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
            explain: solve.else_block.is_none(),
            ask: self.ask,
            trace: self.trace,
            cancel: self.cancel,
        };
        if let Some(trace) = self.trace {
            let file = self.sources.file_at(span.start);
            let (line, _) = file.line_col(span.start);
            trace(&format!(
                "solve #{} at {}:{line}",
                self.stats.len() + 1,
                file.name()
            ));
        }
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
            SolveOutcome::NoSolution { searched, conflict } => match &solve.else_block {
                Some(block) => self.exec_block(block).map(|_| ()),
                None => {
                    let note = searched.map_or_else(String::new, |(bits, names)| {
                        format!(" with |{}| < 2^{bits}", names.join("|, |"))
                    });
                    self.write_line(&format!("no solution{note}"), span)?;
                    if let Some(conflict) = conflict {
                        self.warnings.push(explanation(span, &conflict));
                    }
                    Ok(())
                }
            },
            SolveOutcome::NotUnique { first, second } => Err(Diagnostic::error(
                span,
                "`solve unique` found more than one solution",
            )
            .with_note(format!("one solution: {}", self.describe(&targets, &first)))
            .with_note(format!(
                "another one:  {}",
                self.describe(&targets, &second)
            ))),
        }
    }

    fn run_solution(
        &mut self,
        model: &[Constant],
        targets: &[(String, Value)],
        body: Option<&Block>,
    ) -> Result<()> {
        self.model = Some(Rc::new(model.to_vec()));
        let result = match body {
            Some(body) => self.exec_block(body).map(|_| ()),
            None => {
                let line = self.describe(targets, model);
                self.write_line(&line, Span::default())
            }
        };
        self.model = None;
        result
    }

    /// `x = 3, y = 4` for the targets under `model`.
    fn describe(&self, targets: &[(String, Value)], model: &[Constant]) -> String {
        targets
            .iter()
            .map(|(label, value)| {
                let shown = self
                    .render(&concretize(model, value), true)
                    .unwrap_or_else(|| "?".into());
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
            .filter(|(_, binding)| {
                let mut terms = Vec::new();
                leaves(&binding.value, &mut terms);
                !terms.is_empty()
            })
            .map(|(name, binding)| (name.clone(), binding.value.clone()))
            .collect()
    }

    fn label(&self, expr: &Expr) -> String {
        match &expr.kind {
            ExprKind::Var(ident) => ident.name.clone(),
            _ => self.sources.slice(expr.span).to_string(),
        }
    }

    // --- types and unknowns --------------------------------------------------------------

    fn shape_of(&mut self, ty: &TypeExpr) -> Result<Shape> {
        Ok(match &ty.kind {
            TypeKind::Bool => Shape::Bool,
            TypeKind::Char => Shape::Char,
            TypeKind::Int(None) => Shape::Int(None),
            TypeKind::Int(Some(range)) => {
                let (lo, hi) = self.eval_range(range)?;
                if lo > hi {
                    return Err(Diagnostic::error(
                        range.span,
                        format!(
                            "the range `{}` contains no values",
                            self.sources.slice(range.span)
                        ),
                    )
                    .with_note("`lo..hi` excludes `hi`, while `lo..=hi` includes it"));
                }
                Shape::Int(Some(Interval::new(lo, hi)))
            }
            TypeKind::IntBits { signed, bits } => {
                let width = self.concrete_int(bits, "a bit width")?;
                let width = width
                    .to_u32()
                    .filter(|w| (1..=65_536).contains(w))
                    .ok_or_else(|| {
                        Diagnostic::error(bits.span, "the bit width must be between 1 and 65536")
                    })?;
                Shape::Int(Some(if *signed {
                    let half = BigInt::one() << (width - 1);
                    Interval::new(-half.clone(), half - 1)
                } else {
                    Interval::new(BigInt::zero(), (BigInt::one() << width) - 1)
                }))
            }
            TypeKind::Str(None) => Shape::Str {
                min_len: 0,
                capacity: DEFAULT_STRING_CAPACITY,
                defaulted: true,
            },
            TypeKind::Str(Some(range)) => {
                let (lo, hi) = self.eval_range(range)?;
                let bounds = lo
                    .to_usize()
                    .zip(hi.to_usize())
                    .filter(|(lo, hi)| lo <= hi && *hi <= 100_000);
                let Some((min_len, capacity)) = bounds else {
                    return Err(Diagnostic::error(
                        range.span,
                        "string lengths must form a non-empty range within 0..=100000",
                    ));
                };
                Shape::Str {
                    min_len,
                    capacity,
                    defaulted: false,
                }
            }
            TypeKind::Array(element, length) => {
                let element = self.shape_of(element)?;
                let length = match length {
                    Some(expr) => {
                        let n = self.concrete_int(expr, "an array length")?;
                        Some(n.to_usize().filter(|&n| n <= 10_000_000).ok_or_else(|| {
                            Diagnostic::error(
                                expr.span,
                                "an array length must be a non-negative number",
                            )
                        })?)
                    }
                    None => None,
                };
                Shape::Array(Box::new(element), length)
            }
            TypeKind::Named(name) => match self.types.enum_named(&name.name) {
                Some(id) => Shape::Enum(id),
                None => {
                    let id = self.types.struct_named(&name.name).expect("type checked");
                    self.struct_shape(id, name.span)?
                }
            },
        })
    }

    fn struct_shape(&mut self, id: usize, span: Span) -> Result<Shape> {
        if self.struct_depth > 32 {
            return Err(Diagnostic::error(
                span,
                format!(
                    "the struct `{}` contains itself",
                    self.types.structs[id].name
                ),
            ));
        }
        self.struct_depth += 1;
        // Field types are evaluated in an empty scope: their bounds are constants.
        let saved = std::mem::replace(&mut self.scopes, vec![Scope::default()]);
        let fields = self.types.structs[id].fields.clone();
        let shapes: Result<Vec<Shape>> = fields.iter().map(|(_, ty)| self.shape_of(ty)).collect();
        self.scopes = saved;
        self.struct_depth -= 1;
        Ok(Shape::Struct(id, shapes?))
    }

    /// The shape of an inferred (unannotated) type.
    fn shape_of_type(&mut self, ty: &Type, span: Span) -> Result<Shape> {
        Ok(match ty {
            Type::Bool => Shape::Bool,
            Type::Int => Shape::Int(None),
            Type::Char => Shape::Char,
            Type::Enum(id) => Shape::Enum(*id),
            Type::Struct(id) => self.struct_shape(*id, span)?,
            Type::Unit | Type::Str | Type::Array(_) => unreachable!("rejected by the checker"),
        })
    }

    fn new_unknown(&mut self, name: &str, shape: &Shape, span: Span) -> Result<Value> {
        Ok(match shape {
            Shape::Bool => Value::Sym(
                self.store
                    .new_unknown(name.to_string(), Sort::Bool, Interval::full(), span),
                Tag::Bool,
            ),
            Shape::Int(domain) => {
                let domain = domain.clone().unwrap_or_else(Interval::full);
                Value::Sym(
                    self.store
                        .new_unknown(name.to_string(), Sort::Int, domain, span),
                    Tag::Int,
                )
            }
            Shape::Char => {
                let domain = Interval::new(BigInt::zero(), BigInt::from(MAX_CHAR));
                Value::Sym(
                    self.store
                        .new_unknown(name.to_string(), Sort::Int, domain, span),
                    Tag::Char,
                )
            }
            Shape::Enum(id) => {
                let last = self.types.enums[*id].variants.len() - 1;
                let domain = Interval::new(BigInt::zero(), BigInt::from(last));
                Value::Sym(
                    self.store
                        .new_unknown(name.to_string(), Sort::Int, domain, span),
                    Tag::Enum(*id),
                )
            }
            Shape::Array(element, Some(length)) => {
                let mut items = Vec::with_capacity(*length);
                for index in 0..*length {
                    items.push(self.new_unknown(&format!("{name}[{index}]"), element, span)?);
                }
                Value::Array(Rc::new(items))
            }
            Shape::Array(_, None) => {
                return Err(Diagnostic::error(span, "an unknown array needs a length"));
            }
            Shape::Struct(id, fields) => {
                let names: Vec<String> = self.types.structs[*id]
                    .fields
                    .iter()
                    .map(|(field, _)| field.clone())
                    .collect();
                let mut values = Vec::with_capacity(fields.len());
                for (field, field_shape) in names.iter().zip(fields) {
                    values.push(self.new_unknown(&format!("{name}.{field}"), field_shape, span)?);
                }
                Value::Struct(*id, Rc::new(values))
            }
            Shape::Str {
                min_len,
                capacity,
                defaulted,
            } => {
                if *defaulted {
                    self.warnings.push(
                        Diagnostic::warning(span, format!("`{name}` has no length bounds; lengths up to {capacity} are considered"))
                            .with_help(format!("declare its lengths, e.g. `var {name}: string[1..=10]`")),
                    );
                }
                let len = if min_len == capacity {
                    self.store.int(BigInt::from(*capacity))
                } else {
                    let len_domain = Interval::new(BigInt::from(*min_len), BigInt::from(*capacity));
                    self.store
                        .new_unknown(format!("len({name})"), Sort::Int, len_domain, span)
                };
                let zero = self.store.int(BigInt::zero());
                let mut chars = Vec::with_capacity(*capacity);
                for index in 0..*capacity {
                    let domain = Interval::new(BigInt::zero(), BigInt::from(MAX_CHAR));
                    let c =
                        self.store
                            .new_unknown(format!("{name}[{index}]"), Sort::Int, domain, span);
                    // A character is present (non-zero) exactly at the positions before `len`.
                    let position = self.store.int(BigInt::from(index));
                    let inside = self.store.lt(position, len);
                    let is_zero = self.store.eq(c, zero);
                    let present = self.store.not(is_zero);
                    let canonical = self.store.iff(inside, present);
                    self.add_constraint(canonical, span);
                    chars.push(Value::Sym(c, Tag::Char));
                }
                let len = self.int_value(len);
                Value::SymStr(Rc::new(SymStr { chars, len }))
            }
        })
    }

    /// Keeps `value` inside `shape`: checked now for known values, required for unknown ones.
    fn enforce_shape(&mut self, value: &Value, shape: &Shape, span: Span) -> Result<()> {
        match (shape, value) {
            (Shape::Int(Some(domain)), value) => self.enforce_domain(value, domain, span),
            (Shape::Array(element, length), Value::Array(items)) => {
                if let Some(length) = length {
                    if items.len() != *length {
                        return Err(Diagnostic::error(
                            span,
                            format!("expected {length} elements, found {}", items.len()),
                        ));
                    }
                }
                for item in items.iter() {
                    self.enforce_shape(item, element, span)?;
                }
                Ok(())
            }
            (Shape::Struct(_, fields), Value::Struct(_, values)) => {
                for (field, value) in fields.iter().zip(values.iter()) {
                    self.enforce_shape(value, field, span)?;
                }
                Ok(())
            }
            (
                Shape::Str {
                    min_len, capacity, ..
                },
                value,
            ) => {
                let range = Interval::new(BigInt::from(*min_len), BigInt::from(*capacity));
                let length = self.length_of(value);
                match &length {
                    Value::Int(n) if !range.contains(n) => Err(Diagnostic::error(
                        span,
                        format!("a string of length {n} does not fit the declared lengths {range}"),
                    )),
                    _ => self.enforce_domain(&length, &range, span),
                }
            }
            _ => Ok(()),
        }
    }

    fn enforce_domain(&mut self, value: &Value, domain: &Interval, span: Span) -> Result<()> {
        match value {
            Value::Int(known) if !domain.contains(known) => Err(Diagnostic::error(
                span,
                format!("{known} is outside the declared range {domain}"),
            )),
            Value::Sym(term, _) => {
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

    fn eval_range(&mut self, range: &Range) -> Result<(BigInt, BigInt)> {
        let lo = self.concrete_int(&range.lo, "a range bound")?;
        let hi = self.concrete_int(&range.hi, "a range bound")?;
        Ok((lo, if range.inclusive { hi } else { hi - 1 }))
    }

    fn concrete_int(&mut self, expr: &Expr, what: &str) -> Result<BigInt> {
        match self.eval(expr)? {
            Value::Int(value) => Ok(value),
            Value::Char(c) => Ok(BigInt::from(c)),
            Value::Sym(..) => Err(Diagnostic::error(
                expr.span,
                format!("{what} must be known at compile time"),
            )
            .with_label("this depends on unknowns")),
            other => unreachable!("type checked as int, got {other:?}"),
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

    fn is_variable(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope.bindings.contains_key(name))
    }

    fn field_position(&self, id: usize, field: &str) -> usize {
        self.types.structs[id]
            .fields
            .iter()
            .position(|(name, _)| name == field)
            .expect("type checked field")
    }

    // --- expressions ---------------------------------------------------------------------

    fn eval(&mut self, expr: &Expr) -> Result<Value> {
        match &expr.kind {
            ExprKind::Int(value) => Ok(Value::Int(value.clone())),
            ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            ExprKind::Char(value) => Ok(Value::Char(u32::from(*value))),
            ExprKind::Str(parts) => {
                let mut text = String::new();
                for part in parts {
                    match part {
                        StrPart::Text(literal) => text.push_str(literal),
                        StrPart::Expr(inner) => {
                            let value = self.eval(inner)?;
                            text.push_str(&self.display(&value, inner.span, false)?);
                        }
                    }
                }
                Ok(Value::Str(text.into()))
            }
            ExprKind::Var(ident) => {
                let value = self.binding(&ident.name).value.clone();
                Ok(match &self.model {
                    Some(model) => concretize(model, &value),
                    None => value,
                })
            }
            ExprKind::Hole => {
                self.forbid_in_solve_body("`?`", expr.span)?;
                let ty = self.types.hole_type(expr.span).clone();
                let shape = self.shape_of_type(&ty, expr.span)?;
                let (line, column) = self
                    .sources
                    .file_at(expr.span.start)
                    .line_col(expr.span.start);
                self.new_unknown(&format!("?{line}:{column}"), &shape, expr.span)
            }
            ExprKind::Unary(op, operand) => {
                let value = self.eval(operand)?;
                Ok(self.unary(*op, value))
            }
            ExprKind::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs, expr.span),
            ExprKind::Call(name, args) => self.call(name, args, expr.span),
            ExprKind::Index(base, index) => {
                let base = self.eval(base)?;
                let index_value = self.eval(index)?;
                self.index(base, index_value, index.span)
            }
            ExprKind::Field(base, field) => {
                if let ExprKind::Var(name) = &base.kind {
                    if !self.is_variable(&name.name) {
                        let id = self
                            .types
                            .enum_named(&name.name)
                            .expect("type checked enum");
                        let variant = self.types.enums[id]
                            .variants
                            .iter()
                            .position(|v| *v == field.name)
                            .expect("type checked");
                        return Ok(Value::Enum(id, variant));
                    }
                }
                match self.eval(base)? {
                    Value::Struct(id, fields) => {
                        Ok(fields[self.field_position(id, &field.name)].clone())
                    }
                    other => unreachable!("type checked as a struct, got {other:?}"),
                }
            }
            ExprKind::Array(elements) => {
                let mut items = Vec::with_capacity(elements.len());
                for element in elements {
                    items.push(self.eval(element)?);
                }
                Ok(Value::Array(Rc::new(items)))
            }
            ExprKind::Comprehension {
                func,
                body,
                clauses,
            } => self.comprehension(func.as_ref(), body, clauses, expr.span),
            ExprKind::In(value, iterable) => {
                let value = self.eval(value)?;
                self.membership(value, iterable, expr.span)
            }
            ExprKind::If(cond, then_value, else_value) => match self.eval(cond)? {
                Value::Bool(true) => self.eval(then_value),
                Value::Bool(false) => self.eval(else_value),
                Value::Sym(condition, _) => {
                    let then_result = self.guarded(condition, |this| this.eval(then_value))?;
                    let negated = self.store.not(condition);
                    let else_result = self.guarded(negated, |this| this.eval(else_value))?;
                    self.merge(condition, then_result, else_result, expr.span)
                }
                other => unreachable!("type checked as bool, got {other:?}"),
            },
        }
    }

    /// Runs `f` with `condition` added to the path condition.
    fn guarded<T>(
        &mut self,
        condition: TermId,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.guards.push(condition);
        let result = f(self);
        self.guards.pop();
        result
    }

    fn unary(&mut self, op: UnaryOp, value: Value) -> Value {
        match (op, value) {
            (UnaryOp::Neg, Value::Int(v)) => Value::Int(-v),
            (UnaryOp::Not, Value::Bool(b)) => Value::Bool(!b),
            (UnaryOp::Neg, Value::Sym(t, _)) => Value::Sym(self.store.neg(t), Tag::Int),
            (UnaryOp::Not, Value::Sym(t, _)) => Value::Sym(self.store.not(t), Tag::Bool),
            (op, value) => unreachable!("type checked: {op:?} {value:?}"),
        }
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr, span: Span) -> Result<Value> {
        if matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Implies) {
            // The right side runs only when it matters, and under the left side's condition.
            let left = self.eval(lhs)?;
            let condition = match (op, &left) {
                (BinaryOp::And, Value::Bool(false)) => return Ok(Value::Bool(false)),
                (BinaryOp::Or, Value::Bool(true)) | (BinaryOp::Implies, Value::Bool(false)) => {
                    return Ok(Value::Bool(true));
                }
                (_, Value::Bool(_)) => return self.eval(rhs),
                (BinaryOp::Or, Value::Sym(l, _)) => self.store.not(*l),
                (_, Value::Sym(l, _)) => *l,
                (_, other) => unreachable!("type checked as bool, got {other:?}"),
            };
            let right = self.guarded(condition, |this| this.eval(rhs))?;
            let (l, r) = (self.bool_term(&left), self.bool_term(&right));
            let term = match op {
                BinaryOp::And => self.store.and(l, r),
                BinaryOp::Or => self.store.or(l, r),
                _ => self.store.implies(l, r),
            };
            return Ok(self.bool_value(term));
        }
        let left = self.eval(lhs)?;
        let right = self.eval(rhs)?;
        self.apply_binary(op, left, right, span, rhs.span)
    }

    fn apply_binary(
        &mut self,
        op: BinaryOp,
        left: Value,
        right: Value,
        span: Span,
        rhs_span: Span,
    ) -> Result<Value> {
        match op {
            BinaryOp::Eq => self.equal(&left, &right),
            BinaryOp::Ne => {
                let equal = self.equal(&left, &right)?;
                Ok(self.not_value(&equal))
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                Ok(self.compare(op, &left, &right))
            }
            BinaryOp::Iff => {
                let (a, b) = (self.bool_term(&left), self.bool_term(&right));
                let term = self.store.iff(a, b);
                Ok(self.bool_value(term))
            }
            BinaryOp::Xor if matches!(left, Value::Bool(_) | Value::Sym(_, Tag::Bool)) => {
                let (a, b) = (self.bool_term(&left), self.bool_term(&right));
                let term = self.store.xor(a, b);
                Ok(self.bool_value(term))
            }
            BinaryOp::Add if matches!(left, Value::Array(_)) => {
                let (Value::Array(a), Value::Array(b)) = (&left, &right) else {
                    unreachable!("type checked as arrays")
                };
                Ok(Value::Array(Rc::new(
                    a.iter().chain(b.iter()).cloned().collect(),
                )))
            }
            BinaryOp::Add if matches!(left, Value::Str(_) | Value::SymStr(_)) => {
                Ok(self.concat(&left, &right))
            }
            _ => self.arithmetic(op, left, right, span, rhs_span),
        }
    }

    fn arithmetic(
        &mut self,
        op: BinaryOp,
        left: Value,
        right: Value,
        span: Span,
        rhs_span: Span,
    ) -> Result<Value> {
        if let (Value::Int(a), Value::Int(b)) = (&left, &right) {
            return concrete_arithmetic(op, a, b, rhs_span).map(Value::Int);
        }
        let exponent_or_shift = |value: &Value, what: &str| -> Result<u32> {
            match value {
                Value::Int(n) => n.to_u32().ok_or_else(|| {
                    Diagnostic::error(
                        rhs_span,
                        format!("{what} must be a non-negative number below 2^32"),
                    )
                }),
                _ => Err(Diagnostic::error(
                    rhs_span,
                    format!("{what} must be known at compile time"),
                )),
            }
        };
        let a = self.int_term(&left);
        let term = match op {
            BinaryOp::Pow => {
                let exponent = exponent_or_shift(&right, "the exponent")?;
                if exponent > MAX_SYMBOLIC_EXPONENT {
                    return Err(Diagnostic::error(
                        rhs_span,
                        format!(
                            "exponents above {MAX_SYMBOLIC_EXPONENT} are not supported for unknown bases"
                        ),
                    ));
                }
                self.power(a, exponent)
            }
            BinaryOp::Shl | BinaryOp::Shr => {
                let shift = exponent_or_shift(&right, "the shift amount")?;
                let factor = self.store.int(BigInt::one() << shift);
                if op == BinaryOp::Shl {
                    self.store.mul(a, factor)
                } else {
                    self.store.div(a, factor)
                }
            }
            _ => {
                let b = self.int_term(&right);
                match op {
                    BinaryOp::Add => self.store.add(a, b),
                    BinaryOp::Sub => self.store.sub(a, b),
                    BinaryOp::Mul => self.store.mul(a, b),
                    BinaryOp::Div | BinaryOp::Mod => {
                        // Division by zero is not a solution.
                        let zero = self.store.int(BigInt::zero());
                        let is_zero = self.store.eq(b, zero);
                        let nonzero = self.store.not(is_zero);
                        self.add_constraint(nonzero, rhs_span);
                        if op == BinaryOp::Div {
                            self.store.div(a, b)
                        } else {
                            self.store.rem(a, b)
                        }
                    }
                    BinaryOp::BitAnd => self.store.bitwise(BitOp::And, a, b),
                    BinaryOp::BitOr => self.store.bitwise(BitOp::Or, a, b),
                    BinaryOp::Xor => self.store.bitwise(BitOp::Xor, a, b),
                    other => unreachable!("not arithmetic: {}", other.symbol()),
                }
            }
        };
        let _ = span;
        Ok(self.int_value(term))
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

    // --- scalar helpers ------------------------------------------------------------------

    fn bool_term(&mut self, value: &Value) -> TermId {
        match value {
            Value::Bool(b) => self.store.bool(*b),
            Value::Sym(t, _) => *t,
            other => unreachable!("expected a bool, got {other:?}"),
        }
    }

    fn int_term(&mut self, value: &Value) -> TermId {
        match value {
            Value::Int(v) => self.store.int(v.clone()),
            Value::Char(c) => self.store.int(BigInt::from(*c)),
            Value::Enum(_, k) => self.store.int(BigInt::from(*k)),
            Value::Sym(t, _) => *t,
            other => unreachable!("expected an integer-like value, got {other:?}"),
        }
    }

    /// The term and tag of a scalar value.
    fn scalar(&mut self, value: &Value) -> (TermId, Tag) {
        match value {
            Value::Bool(b) => (self.store.bool(*b), Tag::Bool),
            Value::Int(v) => (self.store.int(v.clone()), Tag::Int),
            Value::Char(c) => (self.store.int(BigInt::from(*c)), Tag::Char),
            Value::Enum(id, k) => (self.store.int(BigInt::from(*k)), Tag::Enum(*id)),
            Value::Sym(t, tag) => (*t, *tag),
            other => unreachable!("expected a scalar, got {other:?}"),
        }
    }

    /// A boolean term as a value (concrete when the term is a constant).
    fn bool_value(&self, term: TermId) -> Value {
        match self.store.constant_of(term) {
            Some(Constant::Bool(b)) => Value::Bool(*b),
            _ => Value::Sym(term, Tag::Bool),
        }
    }

    fn int_value(&self, term: TermId) -> Value {
        match self.store.constant_of(term) {
            Some(Constant::Int(v)) => Value::Int(v.clone()),
            _ => Value::Sym(term, Tag::Int),
        }
    }

    fn tagged(&self, term: TermId, tag: Tag) -> Value {
        match (self.store.constant_of(term), tag) {
            (Some(Constant::Bool(b)), _) => Value::Bool(*b),
            (Some(Constant::Int(v)), Tag::Int) => Value::Int(v.clone()),
            (Some(Constant::Int(v)), Tag::Char) => {
                Value::Char(v.to_u32().expect("valid character"))
            }
            (Some(Constant::Int(v)), Tag::Enum(id)) => {
                Value::Enum(id, v.to_usize().expect("valid variant"))
            }
            _ => Value::Sym(term, tag),
        }
    }

    fn not_value(&mut self, value: &Value) -> Value {
        match value {
            Value::Bool(b) => Value::Bool(!b),
            other => {
                let t = self.bool_term(other);
                let negated = self.store.not(t);
                self.bool_value(negated)
            }
        }
    }

    fn and_values(&mut self, a: &Value, b: &Value) -> Value {
        match (a, b) {
            (Value::Bool(x), Value::Bool(y)) => Value::Bool(*x && *y),
            _ => {
                let (x, y) = (self.bool_term(a), self.bool_term(b));
                let term = self.store.and(x, y);
                self.bool_value(term)
            }
        }
    }

    fn or_values(&mut self, a: &Value, b: &Value) -> Value {
        match (a, b) {
            (Value::Bool(x), Value::Bool(y)) => Value::Bool(*x || *y),
            _ => {
                let (x, y) = (self.bool_term(a), self.bool_term(b));
                let term = self.store.or(x, y);
                self.bool_value(term)
            }
        }
    }

    /// `cond ? then_value : else_value` for values of any (matching) type.
    fn merge(
        &mut self,
        condition: TermId,
        then_value: Value,
        else_value: Value,
        span: Span,
    ) -> Result<Value> {
        if then_value == else_value {
            return Ok(then_value);
        }
        match (&then_value, &else_value) {
            (Value::Array(a), Value::Array(b)) => {
                if a.len() != b.len() {
                    return Err(Diagnostic::error(
                        span,
                        "arrays of different lengths cannot be merged under a condition that depends on unknowns",
                    ));
                }
                let mut items = Vec::with_capacity(a.len());
                for (x, y) in a.iter().zip(b.iter()) {
                    items.push(self.merge(condition, x.clone(), y.clone(), span)?);
                }
                Ok(Value::Array(Rc::new(items)))
            }
            (Value::Struct(id, a), Value::Struct(_, b)) => {
                let mut fields = Vec::with_capacity(a.len());
                for (x, y) in a.iter().zip(b.iter()) {
                    fields.push(self.merge(condition, x.clone(), y.clone(), span)?);
                }
                Ok(Value::Struct(*id, Rc::new(fields)))
            }
            (Value::Str(_) | Value::SymStr(_), Value::Str(_) | Value::SymStr(_)) => {
                let (a, b) = (self.sym_str(&then_value), self.sym_str(&else_value));
                let capacity = a.chars.len().max(b.chars.len());
                let mut chars = Vec::with_capacity(capacity);
                for index in 0..capacity {
                    let x = a.chars.get(index).cloned().unwrap_or(Value::Char(0));
                    let y = b.chars.get(index).cloned().unwrap_or(Value::Char(0));
                    chars.push(self.merge(condition, x, y, span)?);
                }
                let len = self.merge(condition, a.len.clone(), b.len.clone(), span)?;
                Ok(Value::SymStr(Rc::new(SymStr { chars, len })))
            }
            (Value::Unit, Value::Unit) => Ok(Value::Unit),
            _ => {
                let (a, tag) = self.scalar(&then_value);
                let (b, _) = self.scalar(&else_value);
                let term = self.store.ite(condition, a, b);
                Ok(self.tagged(term, tag))
            }
        }
    }

    /// `a == b` for values of any (matching) type.
    fn equal(&mut self, a: &Value, b: &Value) -> Result<Value> {
        match (a, b) {
            (Value::Array(x), Value::Array(y)) => {
                if x.len() != y.len() {
                    return Ok(Value::Bool(false));
                }
                let mut all = Value::Bool(true);
                for (p, q) in x.iter().zip(y.iter()) {
                    let same = self.equal(p, q)?;
                    all = self.and_values(&all, &same);
                }
                Ok(all)
            }
            (Value::Struct(_, x), Value::Struct(_, y)) => {
                let mut all = Value::Bool(true);
                for (p, q) in x.iter().zip(y.iter()) {
                    let same = self.equal(p, q)?;
                    all = self.and_values(&all, &same);
                }
                Ok(all)
            }
            (Value::Str(x), Value::Str(y)) => Ok(Value::Bool(x == y)),
            (Value::Str(_) | Value::SymStr(_), Value::Str(_) | Value::SymStr(_)) => {
                // Thanks to the canonical padding, equal strings agree on every position.
                let (x, y) = (self.sym_str(a), self.sym_str(b));
                let mut all = self.equal(&x.len, &y.len)?;
                for index in 0..x.chars.len().max(y.chars.len()) {
                    let p = x.chars.get(index).cloned().unwrap_or(Value::Char(0));
                    let q = y.chars.get(index).cloned().unwrap_or(Value::Char(0));
                    let same = self.equal(&p, &q)?;
                    all = self.and_values(&all, &same);
                }
                Ok(all)
            }
            (Value::Unit, Value::Unit) => Ok(Value::Bool(true)),
            _ if !matches!(a, Value::Sym(..)) && !matches!(b, Value::Sym(..)) => {
                Ok(Value::Bool(a == b))
            }
            _ => {
                let (x, tag) = self.scalar(a);
                let (y, _) = self.scalar(b);
                let term = if tag == Tag::Bool {
                    self.store.iff(x, y)
                } else {
                    self.store.eq(x, y)
                };
                Ok(self.bool_value(term))
            }
        }
    }

    fn compare(&mut self, op: BinaryOp, a: &Value, b: &Value) -> Value {
        let (a, b) = if matches!(op, BinaryOp::Gt | BinaryOp::Ge) {
            (b, a)
        } else {
            (a, b)
        };
        let strict = matches!(op, BinaryOp::Lt | BinaryOp::Gt);
        let (x, y) = (self.int_term(a), self.int_term(b));
        let term = if strict {
            self.store.lt(x, y)
        } else {
            self.store.le(x, y)
        };
        self.bool_value(term)
    }

    fn pick(&mut self, pick_min: bool, a: Value, b: Value) -> Value {
        if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
            return if pick_min == (x <= y) { a } else { b };
        }
        let (x, y) = (self.int_term(&a), self.int_term(&b));
        let x_le_y = self.store.le(x, y);
        let term = if pick_min {
            self.store.ite(x_le_y, x, y)
        } else {
            self.store.ite(x_le_y, y, x)
        };
        self.int_value(term)
    }

    /// The sum of integer values, as a balanced tree of additions (which keeps the circuit's
    /// intermediate results narrow).
    fn sum(&mut self, values: Vec<Value>) -> Value {
        let mut constant = BigInt::zero();
        let mut terms = Vec::new();
        for value in values {
            match value {
                Value::Int(v) => constant += v,
                other => terms.push(self.int_term(&other)),
            }
        }
        while terms.len() > 1 {
            let mut next = Vec::with_capacity(terms.len().div_ceil(2));
            for pair in terms.chunks(2) {
                next.push(if pair.len() == 2 {
                    self.store.add(pair[0], pair[1])
                } else {
                    pair[0]
                });
            }
            terms = next;
        }
        match terms.pop() {
            None => Value::Int(constant),
            Some(term) => {
                let c = self.store.int(constant);
                let total = self.store.add(term, c);
                self.int_value(total)
            }
        }
    }

    // --- collections ---------------------------------------------------------------------

    fn check_index(index: &BigInt, len: usize, span: Span) -> Result<usize> {
        index.to_usize().filter(|&k| k < len).ok_or_else(|| {
            Diagnostic::error(
                span,
                format!("index {index} is out of range for length {len}"),
            )
        })
    }

    fn require_index_in_range(&mut self, index: TermId, len: usize, span: Span) {
        let zero = self.store.int(BigInt::zero());
        let n = self.store.int(BigInt::from(len));
        let above = self.store.le(zero, index);
        let below = self.store.lt(index, n);
        let inside = self.store.and(above, below);
        self.add_constraint(inside, span);
    }

    /// Selects `items[index]` for an unknown index (without range checks).
    fn select(&mut self, index: TermId, items: &[Value], span: Span) -> Result<Value> {
        let Some(last) = items.last() else {
            return Ok(Value::Unit);
        };
        let mut result = last.clone();
        for (k, item) in items.iter().enumerate().rev().skip(1) {
            let position = self.store.int(BigInt::from(k));
            let here = self.store.eq(index, position);
            result = self.merge(here, item.clone(), result, span)?;
        }
        Ok(result)
    }

    fn index(&mut self, base: Value, index: Value, span: Span) -> Result<Value> {
        match (base, index) {
            (Value::Array(items), Value::Int(i)) => {
                Ok(items[Self::check_index(&i, items.len(), span)?].clone())
            }
            (Value::Array(items), Value::Sym(i, _)) => {
                self.require_index_in_range(i, items.len(), span);
                self.select(i, &items, span)
            }
            (Value::Str(text), Value::Int(i)) => {
                let chars: Vec<char> = text.chars().collect();
                Ok(Value::Char(u32::from(
                    chars[Self::check_index(&i, chars.len(), span)?],
                )))
            }
            (string @ (Value::Str(_) | Value::SymStr(_)), index) => {
                let s = self.sym_str(&string);
                let i = self.int_term(&index);
                let zero = self.store.int(BigInt::zero());
                let len = self.int_term(&s.len);
                let above = self.store.le(zero, i);
                let below = self.store.lt(i, len);
                let inside = self.store.and(above, below);
                self.add_constraint(inside, span);
                match &index {
                    Value::Int(k) => match k.to_usize().and_then(|k| s.chars.get(k)) {
                        Some(c) => Ok(c.clone()),
                        None => Err(Diagnostic::error(
                            span,
                            format!(
                                "index {k} is beyond the string's capacity {}",
                                s.chars.len()
                            ),
                        )),
                    },
                    _ => self.select(i, &s.chars, span),
                }
            }
            (base, index) => unreachable!("type checked index: {base:?}[{index:?}]"),
        }
    }

    /// A string as a symbolic string (known strings become one with known characters).
    fn sym_str(&self, value: &Value) -> SymStr {
        match value {
            Value::Str(text) => SymStr {
                chars: text.chars().map(|c| Value::Char(u32::from(c))).collect(),
                len: Value::Int(BigInt::from(text.chars().count())),
            },
            Value::SymStr(s) => (**s).clone(),
            other => unreachable!("expected a string, got {other:?}"),
        }
    }

    fn length_of(&self, value: &Value) -> Value {
        match value {
            Value::Str(text) => Value::Int(BigInt::from(text.chars().count())),
            Value::SymStr(s) => s.len.clone(),
            Value::Array(items) => Value::Int(BigInt::from(items.len())),
            other => unreachable!("expected a collection, got {other:?}"),
        }
    }

    fn concat(&mut self, left: &Value, right: &Value) -> Value {
        if let (Value::Str(a), Value::Str(b)) = (left, right) {
            return Value::Str(format!("{a}{b}").into());
        }
        let (a, b) = (self.sym_str(left), self.sym_str(right));
        let capacity = a.chars.len() + b.chars.len();
        let a_len = self.int_term(&a.len);
        let b_len = self.int_term(&b.len);
        let total = self.store.add(a_len, b_len);
        let mut chars = Vec::with_capacity(capacity);
        for index in 0..capacity {
            let from_a = a.chars.get(index).cloned().unwrap_or(Value::Char(0));
            // Position `index` of `b` lands at `index + len(a)`: select b[index - len(a)].
            let position = self.store.int(BigInt::from(index));
            let offset = self.store.sub(position, a_len);
            let from_b = match self.store.constant_of(offset) {
                Some(Constant::Int(k)) => k
                    .to_usize()
                    .and_then(|k| b.chars.get(k))
                    .cloned()
                    .unwrap_or(Value::Char(0)),
                _ => {
                    let candidates: Vec<Value> = b.chars.clone();
                    let mut selected = Value::Char(0);
                    for (k, c) in candidates.iter().enumerate().rev() {
                        let target = self.store.int(BigInt::from(k));
                        let here = self.store.eq(offset, target);
                        selected = self
                            .merge(here, c.clone(), selected, Span::default())
                            .expect("chars merge");
                    }
                    selected
                }
            };
            let in_a = self.store.lt(position, a_len);
            chars.push(
                self.merge(in_a, from_a, from_b, Span::default())
                    .expect("chars merge"),
            );
        }
        let len = self.int_value(total);
        Value::SymStr(Rc::new(SymStr { chars, len }))
    }

    fn reverse_string(&mut self, value: &Value) -> Value {
        if let Value::Str(text) = value {
            return Value::Str(text.chars().rev().collect::<String>().into());
        }
        let s = self.sym_str(value);
        if let Value::Int(n) = &s.len {
            let n = n.to_usize().expect("small length");
            let mut chars: Vec<Value> = s.chars[..n].iter().rev().cloned().collect();
            chars.resize(s.chars.len(), Value::Char(0));
            return Value::SymStr(Rc::new(SymStr {
                chars,
                len: s.len.clone(),
            }));
        }
        // reversed[i] = s[len - 1 - i] for i < len, and '\0' after.
        let len = self.int_term(&s.len);
        let mut chars = Vec::with_capacity(s.chars.len());
        for index in 0..s.chars.len() {
            let position = self.store.int(BigInt::from(index));
            let one = self.store.int(BigInt::one());
            let last = self.store.sub(len, one);
            let source = self.store.sub(last, position);
            let mut selected = Value::Char(0);
            for (k, c) in s.chars.iter().enumerate().rev() {
                let target = self.store.int(BigInt::from(k));
                let here = self.store.eq(source, target);
                selected = self
                    .merge(here, c.clone(), selected, Span::default())
                    .expect("chars merge");
            }
            let inside = self.store.lt(position, len);
            chars.push(
                self.merge(inside, selected, Value::Char(0), Span::default())
                    .expect("chars merge"),
            );
        }
        Value::SymStr(Rc::new(SymStr {
            chars,
            len: s.len.clone(),
        }))
    }

    /// The elements of an array or string, each with the condition under which it exists.
    fn items_of(&mut self, value: Value) -> Vec<Item> {
        match value {
            Value::Array(items) => items
                .iter()
                .map(|value| Item {
                    value: value.clone(),
                    cond: None,
                })
                .collect(),
            Value::Str(text) => text
                .chars()
                .map(|c| Item {
                    value: Value::Char(u32::from(c)),
                    cond: None,
                })
                .collect(),
            Value::SymStr(s) => match &s.len {
                Value::Int(n) => {
                    let n = n.to_usize().expect("small length");
                    s.chars[..n]
                        .iter()
                        .map(|c| Item {
                            value: c.clone(),
                            cond: None,
                        })
                        .collect()
                }
                len => {
                    let len = self.int_term(len);
                    let mut items = Vec::with_capacity(s.chars.len());
                    for (index, c) in s.chars.iter().enumerate() {
                        let position = self.store.int(BigInt::from(index));
                        let inside = self.store.lt(position, len);
                        items.push(Item {
                            value: c.clone(),
                            cond: Some(inside),
                        });
                    }
                    items
                }
            },
            other => unreachable!("expected a collection, got {other:?}"),
        }
    }

    fn iterate(&mut self, iterable: &Iterable) -> Result<Vec<Item>> {
        match iterable {
            Iterable::Range(range) => {
                let lo = self.eval(&range.lo)?;
                let is_char = matches!(lo, Value::Char(_));
                let (lo, hi) = self.eval_range(range)?;
                if lo > hi {
                    return Ok(Vec::new());
                }
                let count = (&hi - &lo + 1u32)
                    .to_u64()
                    .filter(|&n| n <= MAX_LOOP_ITERATIONS)
                    .ok_or_else(|| {
                        Diagnostic::error(
                            range.span,
                            format!("this range has more than {MAX_LOOP_ITERATIONS} values"),
                        )
                    })?;
                let mut items = Vec::with_capacity(usize::try_from(count).unwrap_or(0));
                let mut value = lo;
                for _ in 0..count {
                    let item = if is_char {
                        Value::Char(value.to_u32().unwrap_or(0))
                    } else {
                        Value::Int(value.clone())
                    };
                    items.push(Item {
                        value: item,
                        cond: None,
                    });
                    value += 1;
                }
                Ok(items)
            }
            Iterable::Expr(expr) => {
                let value = self.eval(expr)?;
                Ok(self.items_of(value))
            }
        }
    }

    fn membership(&mut self, value: Value, iterable: &Iterable, span: Span) -> Result<Value> {
        match iterable {
            Iterable::Range(range) => {
                let (lo, hi) = self.eval_range(range)?;
                let x = self.int_term(&value);
                let (lo, hi) = (self.store.int(lo), self.store.int(hi));
                let above = self.store.le(lo, x);
                let below = self.store.le(x, hi);
                let inside = self.store.and(above, below);
                Ok(self.bool_value(inside))
            }
            Iterable::Expr(expr) => {
                let collection = self.eval(expr)?;
                let mut found = Value::Bool(false);
                for item in self.items_of(collection) {
                    let mut here = self.equal(&value, &item.value)?;
                    if let Some(cond) = item.cond {
                        let present = self.bool_value(cond);
                        here = self.and_values(&present, &here);
                    }
                    found = self.or_values(&found, &here);
                }
                let _ = span;
                Ok(found)
            }
        }
    }

    fn comprehension(
        &mut self,
        func: Option<&Ident>,
        body: &Expr,
        clauses: &[Clause],
        span: Span,
    ) -> Result<Value> {
        let mut items = Vec::new();
        self.scopes.push(Scope::default());
        let result = self.collect_items(body, clauses, None, &mut items);
        self.scopes.pop();
        result?;
        match func {
            None => Ok(Value::Array(Rc::new(Self::unconditional(
                items,
                span,
                "an array comprehension",
            )?))),
            Some(func) => match self.functions.get(&func.name).cloned() {
                Some(decl) => {
                    let values = Self::unconditional(items, span, "a function argument")?;
                    self.call_function(&decl, vec![Value::Array(Rc::new(values))], span)
                }
                None => self.aggregate(&func.name, items, span),
            },
        }
    }

    fn unconditional(items: Vec<Item>, span: Span, what: &str) -> Result<Vec<Value>> {
        if items.iter().any(|item| item.cond.is_some()) {
            return Err(Diagnostic::error(span, format!("{what} cannot filter on unknowns: its length would depend on them"))
                .with_help("aggregate with `sum`, `count`, `all`, `any`, `len` or `all_different`, or move the condition into the element"));
        }
        Ok(items.into_iter().map(|item| item.value).collect())
    }

    fn collect_items(
        &mut self,
        body: &Expr,
        clauses: &[Clause],
        cond: Option<TermId>,
        out: &mut Vec<Item>,
    ) -> Result<()> {
        let Some((clause, rest)) = clauses.split_first() else {
            let value = match cond {
                Some(condition) => self.guarded(condition, |this| this.eval(body))?,
                None => self.eval(body)?,
            };
            out.push(Item { value, cond });
            return Ok(());
        };
        match clause {
            Clause::For(var, iterable) => {
                let items = match cond {
                    Some(condition) => self.guarded(condition, |this| this.iterate(iterable))?,
                    None => self.iterate(iterable)?,
                };
                for item in items {
                    let combined = self.combine(cond, item.cond);
                    self.scopes.push(Scope::default());
                    self.declare(
                        &var.name,
                        Binding {
                            value: item.value,
                            shape: None,
                        },
                    );
                    let result = self.collect_items(body, rest, combined, out);
                    self.scopes.pop();
                    result?;
                }
                Ok(())
            }
            Clause::If(filter) => {
                let value = match cond {
                    Some(condition) => self.guarded(condition, |this| this.eval(filter))?,
                    None => self.eval(filter)?,
                };
                match value {
                    Value::Bool(false) => Ok(()),
                    Value::Bool(true) => self.collect_items(body, rest, cond, out),
                    Value::Sym(t, _) => {
                        let combined = self.combine(cond, Some(t));
                        self.collect_items(body, rest, combined, out)
                    }
                    other => unreachable!("type checked as bool, got {other:?}"),
                }
            }
        }
    }

    fn combine(&mut self, a: Option<TermId>, b: Option<TermId>) -> Option<TermId> {
        match (a, b) {
            (Some(x), Some(y)) => Some(self.store.and(x, y)),
            (x, None) => x,
            (None, y) => y,
        }
    }

    fn aggregate(&mut self, name: &str, items: Vec<Item>, span: Span) -> Result<Value> {
        let gated =
            |this: &mut Self, item: &Item, value: Value, otherwise: Value| -> Result<Value> {
                match item.cond {
                    None => Ok(value),
                    Some(cond) => this.merge(cond, value, otherwise, span),
                }
            };
        match name {
            "len" => {
                let mut ones = Vec::with_capacity(items.len());
                for item in &items {
                    ones.push(gated(
                        self,
                        item,
                        Value::Int(BigInt::one()),
                        Value::Int(BigInt::zero()),
                    )?);
                }
                Ok(self.sum(ones))
            }
            "sum" => {
                let mut values = Vec::with_capacity(items.len());
                for item in &items {
                    values.push(gated(
                        self,
                        item,
                        item.value.clone(),
                        Value::Int(BigInt::zero()),
                    )?);
                }
                Ok(self.sum(values))
            }
            "count" => {
                let mut ones = Vec::with_capacity(items.len());
                for item in &items {
                    let one = match &item.value {
                        Value::Bool(true) => Value::Int(BigInt::one()),
                        Value::Bool(false) => Value::Int(BigInt::zero()),
                        other => {
                            let t = self.bool_term(other);
                            let (one, zero) = (
                                self.store.int(BigInt::one()),
                                self.store.int(BigInt::zero()),
                            );
                            let term = self.store.ite(t, one, zero);
                            self.int_value(term)
                        }
                    };
                    ones.push(gated(self, item, one, Value::Int(BigInt::zero()))?);
                }
                Ok(self.sum(ones))
            }
            "all" => {
                let mut all = Value::Bool(true);
                for item in &items {
                    let holds = gated(self, item, item.value.clone(), Value::Bool(true))?;
                    all = self.and_values(&all, &holds);
                }
                Ok(all)
            }
            "any" => {
                let mut any = Value::Bool(false);
                for item in &items {
                    let holds = gated(self, item, item.value.clone(), Value::Bool(false))?;
                    any = self.or_values(&any, &holds);
                }
                Ok(any)
            }
            "min" | "max" => {
                let values = Self::unconditional(items, span, &format!("`{name}`"))?;
                let mut values = values.into_iter();
                let first = values.next().ok_or_else(|| {
                    Diagnostic::error(span, format!("`{name}` of an empty collection"))
                })?;
                Ok(values.fold(first, |best, next| self.pick(name == "min", best, next)))
            }
            "all_different" => {
                let integers = items.iter().all(|item| {
                    item.cond.is_none()
                        && matches!(
                            item.value,
                            Value::Int(_)
                                | Value::Char(_)
                                | Value::Enum(..)
                                | Value::Sym(_, Tag::Int | Tag::Char | Tag::Enum(_))
                        )
                });
                if integers {
                    let terms: Vec<TermId> = items
                        .iter()
                        .map(|item| self.int_term(&item.value))
                        .collect();
                    let term = self.store.all_different(terms);
                    return Ok(self.bool_value(term));
                }
                let mut all = Value::Bool(true);
                for (i, a) in items.iter().enumerate() {
                    for b in &items[i + 1..] {
                        let same = self.equal(&a.value, &b.value)?;
                        let mut different = self.not_value(&same);
                        let both = self.combine(a.cond, b.cond);
                        if let Some(both) = both {
                            let present = self.bool_value(both);
                            let absent = self.not_value(&present);
                            different = self.or_values(&absent, &different);
                        }
                        all = self.and_values(&all, &different);
                    }
                }
                Ok(all)
            }
            "reverse" => {
                let mut values = Self::unconditional(items, span, "`reverse`")?;
                values.reverse();
                Ok(Value::Array(Rc::new(values)))
            }
            other => unreachable!("type checked aggregate `{other}`"),
        }
    }

    fn call(&mut self, name: &Ident, args: &[Arg], span: Span) -> Result<Value> {
        if let Some(id) = self.types.struct_named(&name.name) {
            return self.construct(id, args, span);
        }
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(&arg.value)?);
        }
        if let Some(decl) = self.functions.get(&name.name).cloned() {
            return self.call_function(&decl, values, span);
        }
        self.builtin(&name.name, values, span)
    }

    fn construct(&mut self, id: usize, args: &[Arg], span: Span) -> Result<Value> {
        let Shape::Struct(_, shapes) = self.struct_shape(id, span)? else {
            unreachable!("a struct shape")
        };
        let mut fields = vec![Value::Unit; shapes.len()];
        for (position, arg) in args.iter().enumerate() {
            let index = match &arg.name {
                Some(field) => self.field_position(id, &field.name),
                None => position,
            };
            let value = self.eval(&arg.value)?;
            self.enforce_shape(&value, &shapes[index], arg.value.span)?;
            fields[index] = value;
        }
        Ok(Value::Struct(id, Rc::new(fields)))
    }

    fn builtin(&mut self, name: &str, mut values: Vec<Value>, span: Span) -> Result<Value> {
        Ok(match (name, values.len()) {
            ("abs", _) => match values.pop().expect("one argument") {
                Value::Int(v) => Value::Int(v.abs()),
                other => {
                    let t = self.int_term(&other);
                    let zero = self.store.int(BigInt::zero());
                    let negative = self.store.lt(t, zero);
                    let negated = self.store.neg(t);
                    let term = self.store.ite(negative, negated, t);
                    self.int_value(term)
                }
            },
            ("ord", _) => match values.pop().expect("one argument") {
                Value::Char(c) => Value::Int(BigInt::from(c)),
                Value::Sym(t, _) => Value::Sym(t, Tag::Int),
                other => unreachable!("type checked as char, got {other:?}"),
            },
            ("chr", _) => match values.pop().expect("one argument") {
                Value::Int(v) => match v
                    .to_u32()
                    .filter(|&c| c <= MAX_CHAR && char::from_u32(c).is_some())
                {
                    Some(c) => Value::Char(c),
                    None => {
                        return Err(Diagnostic::error(
                            span,
                            format!("{v} is not a valid character code"),
                        ));
                    }
                },
                Value::Sym(t, _) => {
                    let (zero, max) = (
                        self.store.int(BigInt::zero()),
                        self.store.int(BigInt::from(MAX_CHAR)),
                    );
                    let above = self.store.le(zero, t);
                    let below = self.store.le(t, max);
                    let valid = self.store.and(above, below);
                    self.add_constraint(valid, span);
                    Value::Sym(t, Tag::Char)
                }
                other => unreachable!("type checked as int, got {other:?}"),
            },
            ("min" | "max", n) if n >= 2 => {
                let pick_min = name == "min";
                let mut values = values.into_iter();
                let first = values.next().expect("two arguments");
                values.fold(first, |best, next| self.pick(pick_min, best, next))
            }
            ("len", _) => self.length_of(&values[0]),
            ("reverse", _) if matches!(values[0], Value::Str(_) | Value::SymStr(_)) => {
                self.reverse_string(&values[0])
            }
            (aggregate, _) => {
                let items = self.items_of(values.pop().expect("one argument"));
                return self.aggregate(aggregate, items, span);
            }
        })
    }

    fn call_function(&mut self, decl: &Rc<FnDecl>, args: Vec<Value>, span: Span) -> Result<Value> {
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(Diagnostic::error(span, format!("calls nested more than {MAX_CALL_DEPTH} deep"))
                .with_note("functions are inlined, so recursion must stop based on values known at compile time"));
        }
        self.steps.calls += 1;
        self.steps.max_depth = self.steps.max_depth.max(self.frames.len() + 1);
        let saved = std::mem::replace(&mut self.scopes, vec![Scope::default()]);
        let result = self.run_function(decl, args, span);
        self.scopes = saved;
        // Point from errors inside the function back to the call (a short call stack).
        result.map_err(|error| {
            let repeated =
                error.span == span || error.related.iter().any(|(related, _)| *related == span);
            if error.message == ANALYSIS_TIMEOUT || error.related.len() >= 4 || repeated {
                error
            } else {
                error.with_related(span, format!("in this call to `{}`", decl.name.name))
            }
        })
    }

    fn run_function(&mut self, decl: &Rc<FnDecl>, args: Vec<Value>, span: Span) -> Result<Value> {
        for (param, arg) in decl.params.iter().zip(args) {
            let shape = self.shape_of(&param.ty)?;
            self.enforce_shape(&arg, &shape, span)?;
            self.declare(
                &param.name.name,
                Binding {
                    value: arg,
                    shape: Some(shape),
                },
            );
        }
        self.frames.push(Frame {
            result: None,
            done: Value::Bool(false),
            guard_depth: self.guards.len(),
        });
        let flow = self.exec_block(&decl.body);
        let frame = self.frames.pop().expect("pushed above");
        flow?;
        Ok(frame.result.unwrap_or(Value::Unit))
    }

    // --- output --------------------------------------------------------------------------

    fn display(&self, value: &Value, span: Span, nested: bool) -> Result<String> {
        let value = match &self.model {
            Some(model) => concretize(model, value),
            None => value.clone(),
        };
        self.render(&value, nested).ok_or_else(|| {
            Diagnostic::error(
                span,
                "this value depends on unknowns, so it cannot be shown here",
            )
            .with_help("print it inside a `solve` body, where every unknown has a value")
        })
    }

    /// Renders a known value; `None` if it still depends on unknowns. Nested strings and
    /// characters are quoted.
    fn render(&self, value: &Value, nested: bool) -> Option<String> {
        Some(match value {
            Value::Unit => "()".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(v) => v.to_string(),
            Value::Char(c) => {
                let c = char::from_u32(*c).unwrap_or('\u{FFFD}');
                if nested {
                    format!("{c:?}")
                } else {
                    c.to_string()
                }
            }
            Value::Enum(id, k) => self.types.enums[*id].variants[*k].clone(),
            Value::Str(s) => {
                if nested {
                    format!("{s:?}")
                } else {
                    s.to_string()
                }
            }
            Value::Array(items) => {
                let parts: Option<Vec<String>> =
                    items.iter().map(|item| self.render(item, true)).collect();
                format!("[{}]", parts?.join(", "))
            }
            Value::Struct(id, fields) => {
                let info = &self.types.structs[*id];
                let parts: Option<Vec<String>> = info
                    .fields
                    .iter()
                    .zip(fields.iter())
                    .map(|((name, _), value)| {
                        self.render(value, true).map(|v| format!("{name}: {v}"))
                    })
                    .collect();
                format!("{}({})", info.name, parts?.join(", "))
            }
            Value::Sym(..) | Value::SymStr(_) => return None,
        })
    }
}

/// Replaces every term in `value` by its value in `model`.
fn concretize(model: &[Constant], value: &Value) -> Value {
    match value {
        Value::Sym(term, tag) => match (&model[term.index()], tag) {
            (Constant::Bool(b), _) => Value::Bool(*b),
            (Constant::Int(v), Tag::Char) => Value::Char(v.to_u32().unwrap_or(0)),
            (Constant::Int(v), Tag::Enum(id)) => Value::Enum(*id, v.to_usize().unwrap_or(0)),
            (Constant::Int(v), _) => Value::Int(v.clone()),
        },
        Value::Array(items) => Value::Array(Rc::new(
            items.iter().map(|item| concretize(model, item)).collect(),
        )),
        Value::Struct(id, fields) => Value::Struct(
            *id,
            Rc::new(fields.iter().map(|f| concretize(model, f)).collect()),
        ),
        Value::SymStr(s) => {
            let len = match concretize(model, &s.len) {
                Value::Int(n) => n.to_usize().unwrap_or(0),
                _ => 0,
            };
            let text: String = s.chars[..len.min(s.chars.len())]
                .iter()
                .map(|c| match concretize(model, c) {
                    Value::Char(c) => char::from_u32(c).unwrap_or('\u{FFFD}'),
                    _ => '?',
                })
                .collect();
            Value::Str(text.into())
        }
        other => other.clone(),
    }
}

/// Collects the terms a value depends on.
fn leaves(value: &Value, out: &mut Vec<TermId>) {
    match value {
        Value::Sym(term, _) => out.push(*term),
        Value::Array(items) | Value::Struct(_, items) => {
            items.iter().for_each(|item| leaves(item, out))
        }
        Value::SymStr(s) => {
            s.chars.iter().for_each(|c| leaves(c, out));
            leaves(&s.len, out);
        }
        _ => {}
    }
}

/// Arithmetic on known integers. Division is Euclidean: the remainder lies in `0..|divisor|`.
fn concrete_arithmetic(op: BinaryOp, a: &BigInt, b: &BigInt, rhs_span: Span) -> Result<BigInt> {
    let small = |n: &BigInt, what: &str| {
        n.to_u32().ok_or_else(|| {
            Diagnostic::error(
                rhs_span,
                format!("{what} must be a non-negative number below 2^32"),
            )
        })
    };
    Ok(match op {
        BinaryOp::Add => a + b,
        BinaryOp::Sub => a - b,
        BinaryOp::Mul => a * b,
        BinaryOp::Div | BinaryOp::Mod => {
            if b.is_zero() {
                return Err(Diagnostic::error(rhs_span, "division by zero"));
            }
            let (q, r) = euclid(a, b);
            if op == BinaryOp::Div { q } else { r }
        }
        BinaryOp::Pow => num_traits::pow(a.clone(), small(b, "the exponent")? as usize),
        BinaryOp::BitAnd => a & b,
        BinaryOp::BitOr => a | b,
        BinaryOp::Xor => a ^ b,
        BinaryOp::Shl => a << small(b, "the shift amount")?,
        BinaryOp::Shr => a >> small(b, "the shift amount")?,
        other => unreachable!("not arithmetic: {}", other.symbol()),
    })
}

/// A note naming the requirements that cannot hold together.
fn explanation(solve: Span, conflict: &[Span]) -> Diagnostic {
    match conflict.split_first() {
        None => Diagnostic::note(
            solve,
            "no solution: the declared types alone cannot be satisfied",
        ),
        Some((first, rest)) => {
            let message = if rest.is_empty() {
                "no solution: this requirement can never hold"
            } else {
                "no solution: these requirements cannot all hold together"
            };
            rest.iter()
                .fold(Diagnostic::note(*first, message), |note, span| {
                    note.with_related(*span, "")
                })
        }
    }
}

/// The message of the error that stops an analysis that ran out of time.
pub(crate) const ANALYSIS_TIMEOUT: &str = "analysis time limit reached";

/// What editor analysis collects: the inferred domain of every unknown, grouped by the
/// declaration that created it.
pub(crate) struct AnalysisState {
    pub(crate) deadline: Instant,
    seen: HashSet<usize>,
    groups: Vec<(Span, Sort, Interval, usize)>,
}

impl AnalysisState {
    pub(crate) fn new(budget: std::time::Duration) -> Self {
        Self {
            deadline: Instant::now() + budget,
            seen: HashSet::new(),
            groups: Vec::new(),
        }
    }

    fn record(&mut self, store: &TermStore, index: usize, domain: Interval) {
        if !self.seen.insert(index) {
            return;
        }
        let unknown = &store.unknowns()[index];
        match self
            .groups
            .iter_mut()
            .find(|(span, ..)| *span == unknown.span)
        {
            Some((_, _, hull, count)) => {
                *hull = hull.hull(&domain);
                *count += 1;
            }
            None => self.groups.push((unknown.span, unknown.sort, domain, 1)),
        }
    }

    /// The inferred domains, one per declaration.
    pub(crate) fn finish(self, sources: &SourceMap) -> Vec<crate::InferredDomain> {
        self.groups
            .into_iter()
            .map(|(span, sort, domain, unknowns)| {
                let (domain, width) = match sort {
                    Sort::Bool => ("bool".to_string(), 1),
                    Sort::Int => (domain.to_string(), crate::blast::layout_of(&domain).width),
                };
                crate::InferredDomain {
                    span,
                    name: sources.slice(span).to_string(),
                    domain,
                    width,
                    unknowns,
                }
            })
            .collect()
    }
}
