//! Name resolution and type inference.
//!
//! Unannotated unknowns (`var x, y`) get a type variable that the operators they appear in
//! resolve, e.g. `x * y == n` makes both `int`. Anything still unresolved at the end is an error
//! asking for an annotation. The `explicit-types` option (see [`crate::config`]) can instead
//! require annotations up front.
//!
//! Enums, structs and functions are registered before anything else is checked, so they may be
//! used before their declaration. Functions only see their own parameters and locals.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::ast::{
    Access, Arg, BinaryOp, Block, Clause, DeclId, Expr, ExprKind, FnDecl, Ident, Iterable, Place,
    Program, Solve, Stmt, StmtKind, StrPart, TypeExpr, TypeKind, UnaryOp, VarDecl,
};
use crate::config::{CompilerOptions, ExplicitTypes};
use crate::diagnostic::Diagnostic;
use crate::source::Span;
use crate::syntax::BUILTIN_FUNCTIONS;

/// The types a value can have.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    /// No value (the result of a function without `-> T`).
    Unit,
    /// A truth value.
    Bool,
    /// A mathematical integer.
    Int,
    /// A Unicode character.
    Char,
    /// A string.
    Str,
    /// An array of the given element type (its length is a run-time property).
    Array(Box<Type>),
    /// The enum with the given index in [`TypeInfo::enums`].
    Enum(usize),
    /// The struct with the given index in [`TypeInfo::structs`].
    Struct(usize),
}

/// A declared enum.
#[derive(Clone, Debug)]
pub struct EnumInfo {
    /// Its name.
    pub name: String,
    /// Its variants, in order.
    pub variants: Vec<String>,
}

/// A declared struct.
#[derive(Clone, Debug)]
pub struct StructInfo {
    /// Its name.
    pub name: String,
    /// Its fields with their declared types, in order.
    pub fields: Vec<(String, TypeExpr)>,
}

/// What kind of name a [`Symbol`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
    /// A `var`.
    Variable,
    /// A function parameter.
    Parameter,
    /// A `for` or comprehension variable.
    LoopVariable,
    /// A function.
    Function,
    /// An enum type.
    Enum,
    /// A struct type.
    Struct,
}

/// A declared name, for editor tooling.
#[derive(Clone, Debug)]
pub struct Symbol {
    /// The name.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// Where it is declared.
    pub span: Span,
    /// Its type, rendered (e.g. `int`, `[bool]`, `fn(int, int) -> int`).
    pub detail: String,
    /// The region of the source where it can be used.
    pub scope: Span,
}

/// The result of type checking.
#[derive(Clone, Debug, Default)]
pub struct TypeInfo {
    decl_types: Vec<Option<Type>>,
    /// The type of each `?`, by the offset of its span.
    hole_types: HashMap<usize, Type>,
    /// Declared enums.
    pub enums: Vec<EnumInfo>,
    /// Declared structs.
    pub structs: Vec<StructInfo>,
    /// Every declared name, for editor tooling.
    pub symbols: Vec<Symbol>,
}

impl TypeInfo {
    /// The type of the declared variable `decl`.
    ///
    /// # Panics
    ///
    /// If checking failed for this declaration (callers only proceed after a clean check).
    #[must_use]
    pub fn decl_type(&self, decl: DeclId) -> &Type {
        self.decl_types[decl.0]
            .as_ref()
            .expect("type checking succeeded")
    }

    /// The type of the `?` at `span`.
    ///
    /// # Panics
    ///
    /// If checking failed for it.
    #[must_use]
    pub fn hole_type(&self, span: Span) -> &Type {
        &self.hole_types[&span.start]
    }

    /// The enum called `name`.
    #[must_use]
    pub fn enum_named(&self, name: &str) -> Option<usize> {
        self.enums.iter().position(|e| e.name == name)
    }

    /// The struct called `name`.
    #[must_use]
    pub fn struct_named(&self, name: &str) -> Option<usize> {
        self.structs.iter().position(|s| s.name == name)
    }

    /// Renders a type the way it is written in source.
    #[must_use]
    pub fn display(&self, ty: &Type) -> String {
        match ty {
            Type::Unit => "()".into(),
            Type::Bool => "bool".into(),
            Type::Int => "int".into(),
            Type::Char => "char".into(),
            Type::Str => "string".into(),
            Type::Array(element) => format!("[{}]", self.display(element)),
            Type::Enum(id) => self.enums[*id].name.clone(),
            Type::Struct(id) => self.structs[*id].name.clone(),
        }
    }
}

/// Type-checks `program`. Returns the inferred types and every diagnostic found.
#[must_use]
pub fn check(program: &Program, options: &CompilerOptions) -> (TypeInfo, Vec<Diagnostic>) {
    let mut checker = Checker {
        library: program.library,
        options,
        vars: Vec::new(),
        decls: vec![None; program.num_decls],
        scopes: vec![Scope::new(Span::new(0, usize::MAX))],
        outer_scopes: Vec::new(),
        enums: Vec::new(),
        structs: Vec::new(),
        struct_fields: Vec::new(),
        functions: HashMap::new(),
        return_type: None,
        diagnostics: Vec::new(),
        symbols: Vec::new(),
        holes: Vec::new(),
    };
    checker.register_declarations(&program.stmts);
    for stmt in &program.stmts {
        checker.stmt(stmt, true);
    }
    checker.finish()
}

/// A type during inference.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ty {
    Unit,
    Bool,
    Int,
    Char,
    Str,
    Array(Box<Ty>),
    Enum(usize),
    Struct(usize),
    Var(usize),
    Error,
}

#[derive(Clone)]
struct Binding {
    ty: Ty,
    is_loop_var: bool,
}

struct Scope {
    bindings: HashMap<String, Binding>,
    span: Span,
}

impl Scope {
    fn new(span: Span) -> Self {
        Self {
            bindings: HashMap::new(),
            span,
        }
    }
}

struct Signature {
    params: Vec<Ty>,
    ret: Ty,
}

struct Checker<'a> {
    /// The standard library's source range (exempt from `explicit-types`).
    library: Option<Span>,
    options: &'a CompilerOptions,
    /// Inference variables: `Some(ty)` once bound.
    vars: Vec<Option<Ty>>,
    /// Per declaration: its type, name, span and whether it had an initializer or annotation.
    decls: Vec<Option<(Ty, String, Span, bool)>>,
    scopes: Vec<Scope>,
    /// The caller's scopes while a function body is checked (only for better messages).
    outer_scopes: Vec<Scope>,
    enums: Vec<EnumInfo>,
    structs: Vec<StructInfo>,
    struct_fields: Vec<Vec<(String, Ty)>>,
    functions: HashMap<String, Signature>,
    /// The declared result type of the function being checked.
    return_type: Option<Ty>,
    diagnostics: Vec<Diagnostic>,
    /// Declared names, with their inference type when the detail is rendered at the end.
    symbols: Vec<(Symbol, Option<Ty>)>,
    /// Every `?` with its inference type.
    holes: Vec<(Span, Ty)>,
}

impl Checker<'_> {
    fn error(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    // --- types ---------------------------------------------------------------------------

    fn resolve(&self, ty: &Ty) -> Ty {
        let mut ty = ty.clone();
        while let Ty::Var(var) = ty {
            match &self.vars[var] {
                Some(bound) => ty = bound.clone(),
                None => return ty,
            }
        }
        ty
    }

    fn resolve_deep(&self, ty: &Ty) -> Ty {
        match self.resolve(ty) {
            Ty::Array(element) => Ty::Array(Box::new(self.resolve_deep(&element))),
            other => other,
        }
    }

    fn occurs(&self, var: usize, ty: &Ty) -> bool {
        match self.resolve(ty) {
            Ty::Var(other) => other == var,
            Ty::Array(element) => self.occurs(var, &element),
            _ => false,
        }
    }

    fn name_of(&self, ty: &Ty) -> String {
        match self.resolve(ty) {
            Ty::Unit => "()".into(),
            Ty::Bool => "bool".into(),
            Ty::Int => "int".into(),
            Ty::Char => "char".into(),
            Ty::Str => "string".into(),
            Ty::Array(element) => format!("[{}]", self.name_of(&element)),
            Ty::Enum(id) => self.enums[id].name.clone(),
            Ty::Struct(id) => self.structs[id].name.clone(),
            Ty::Var(_) | Ty::Error => "_".into(),
        }
    }

    /// Makes `actual` equal to `expected`, reporting a mismatch at `span`.
    fn unify(&mut self, expected: &Ty, actual: &Ty, span: Span) -> Ty {
        match (self.resolve(expected), self.resolve(actual)) {
            (Ty::Error, other) | (other, Ty::Error) => other,
            (Ty::Var(a), Ty::Var(b)) if a == b => Ty::Var(a),
            (Ty::Var(var), other) | (other, Ty::Var(var)) if !self.occurs(var, &other) => {
                self.vars[var] = Some(other.clone());
                other
            }
            (Ty::Array(a), Ty::Array(b)) => {
                let element = self.unify(&a, &b, span);
                Ty::Array(Box::new(element))
            }
            (a, b) if a == b => a,
            (a, b) => {
                let (expected, found) = (self.name_of(&a), self.name_of(&b));
                self.error(
                    Diagnostic::error(span, format!("expected `{expected}`, found `{found}`"))
                        .with_label(format!("this is `{found}`")),
                );
                Ty::Error
            }
        }
    }

    fn fresh(&mut self) -> Ty {
        self.vars.push(None);
        Ty::Var(self.vars.len() - 1)
    }

    fn to_type(&self, ty: &Ty) -> Option<Type> {
        Some(match self.resolve(ty) {
            Ty::Unit => Type::Unit,
            Ty::Bool => Type::Bool,
            Ty::Int => Type::Int,
            Ty::Char => Type::Char,
            Ty::Str => Type::Str,
            Ty::Array(element) => Type::Array(Box::new(self.to_type(&element)?)),
            Ty::Enum(id) => Type::Enum(id),
            Ty::Struct(id) => Type::Struct(id),
            Ty::Var(_) | Ty::Error => return None,
        })
    }

    /// The type an annotation denotes (bounds are checked to be integers).
    fn type_expr(&mut self, ty: &TypeExpr) -> Ty {
        match &ty.kind {
            TypeKind::Bool => Ty::Bool,
            TypeKind::Char => Ty::Char,
            TypeKind::Int(range) | TypeKind::Str(range) => {
                if let Some(range) = range {
                    for bound in [&range.lo, &range.hi] {
                        let bound_ty = self.expr(bound);
                        self.unify(&Ty::Int, &bound_ty, bound.span);
                    }
                }
                if matches!(ty.kind, TypeKind::Str(_)) {
                    Ty::Str
                } else {
                    Ty::Int
                }
            }
            TypeKind::IntBits { bits, .. } => {
                let bits_ty = self.expr(bits);
                self.unify(&Ty::Int, &bits_ty, bits.span);
                Ty::Int
            }
            TypeKind::Array(element, length) => {
                if let Some(length) = length {
                    let length_ty = self.expr(length);
                    self.unify(&Ty::Int, &length_ty, length.span);
                }
                Ty::Array(Box::new(self.type_expr(element)))
            }
            TypeKind::Named(name) => {
                if let Some(id) = self.enums.iter().position(|e| e.name == name.name) {
                    Ty::Enum(id)
                } else if let Some(id) = self.structs.iter().position(|s| s.name == name.name) {
                    Ty::Struct(id)
                } else {
                    self.error(
                        Diagnostic::error(name.span, format!("unknown type `{}`", name.name)).with_help(
                            "the built-in types are `bool`, `int`, `uint<N>`, `char`, `string` and arrays \
                             `[T; N]`; declare others with `enum` or `struct`",
                        ),
                    );
                    Ty::Error
                }
            }
        }
    }

    // --- declarations --------------------------------------------------------------------

    /// Registers top-level enums, structs and function signatures, so that they can be used
    /// anywhere in the program.
    fn register_declarations(&mut self, stmts: &[Stmt]) {
        let mut names: HashMap<String, Span> = HashMap::new();
        let mut claim = |this: &mut Self, ident: &Ident, what: &str| {
            if BUILTIN_FUNCTIONS.contains(&ident.name.as_str())
                || crate::syntax::BUILTIN_TYPES.contains(&ident.name.as_str())
            {
                this.error(
                    Diagnostic::error(ident.span, format!("`{}` is a built-in name", ident.name))
                        .with_help(format!("give the {what} another name")),
                );
            } else if names.insert(ident.name.clone(), ident.span).is_some() {
                this.error(
                    Diagnostic::error(ident.span, format!("`{}` is declared twice", ident.name))
                        .with_help(format!("give the {what} another name")),
                );
            }
        };
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Enum(decl) => {
                    claim(self, &decl.name, "enum");
                    let variants: Vec<String> =
                        decl.variants.iter().map(|v| v.name.clone()).collect();
                    for (index, variant) in decl.variants.iter().enumerate() {
                        if variants[..index].contains(&variant.name) {
                            self.error(Diagnostic::error(
                                variant.span,
                                format!("variant `{}` is listed twice", variant.name),
                            ));
                        }
                    }
                    if variants.is_empty() {
                        self.error(Diagnostic::error(
                            decl.name.span,
                            "an enum needs at least one variant",
                        ));
                    }
                    self.symbol(
                        &decl.name,
                        SymbolKind::Enum,
                        format!("enum {}", decl.name.name),
                        Span::new(0, usize::MAX),
                    );
                    self.enums.push(EnumInfo {
                        name: decl.name.name.clone(),
                        variants,
                    });
                }
                StmtKind::Struct(decl) => {
                    claim(self, &decl.name, "struct");
                    self.symbol(
                        &decl.name,
                        SymbolKind::Struct,
                        format!("struct {}", decl.name.name),
                        Span::new(0, usize::MAX),
                    );
                    self.structs.push(StructInfo {
                        name: decl.name.name.clone(),
                        fields: decl
                            .fields
                            .iter()
                            .map(|(name, ty)| (name.name.clone(), ty.clone()))
                            .collect(),
                    });
                }
                StmtKind::Fn(decl) => claim(self, &decl.name, "function"),
                _ => {}
            }
        }
        // Field types and signatures may mention any enum or struct, so resolve them last.
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Struct(decl) => {
                    let mut fields: Vec<(String, Ty)> = Vec::new();
                    for (name, ty) in &decl.fields {
                        if fields.iter().any(|(existing, _)| *existing == name.name) {
                            self.error(Diagnostic::error(
                                name.span,
                                format!("field `{}` is listed twice", name.name),
                            ));
                        }
                        let field_ty = self.type_expr(ty);
                        fields.push((name.name.clone(), field_ty));
                    }
                    self.struct_fields.push(fields);
                }
                StmtKind::Fn(decl) => {
                    let params: Vec<Ty> =
                        decl.params.iter().map(|p| self.type_expr(&p.ty)).collect();
                    let ret = decl.ret.as_ref().map_or(Ty::Unit, |ty| self.type_expr(ty));
                    let detail = format!(
                        "fn({}){}",
                        params
                            .iter()
                            .map(|p| self.name_of(p))
                            .collect::<Vec<_>>()
                            .join(", "),
                        if ret == Ty::Unit {
                            String::new()
                        } else {
                            format!(" -> {}", self.name_of(&ret))
                        }
                    );
                    self.symbol(
                        &decl.name,
                        SymbolKind::Function,
                        detail,
                        Span::new(0, usize::MAX),
                    );
                    self.functions
                        .insert(decl.name.name.clone(), Signature { params, ret });
                }
                _ => {}
            }
        }
    }

    fn symbol(&mut self, ident: &Ident, kind: SymbolKind, detail: String, scope: Span) {
        self.symbols.push((
            Symbol {
                name: ident.name.clone(),
                kind,
                span: ident.span,
                detail,
                scope,
            },
            None,
        ));
    }

    fn finish(mut self) -> (TypeInfo, Vec<Diagnostic>) {
        let decls = std::mem::take(&mut self.decls);
        let decl_types = decls
            .into_iter()
            .map(|entry| {
                let (ty, name, span, typed) = entry?;
                let resolved = self.resolve_deep(&ty);
                match resolved {
                    Ty::Var(_) => {
                        self.error(
                            Diagnostic::error(span, format!("cannot infer the type of `{name}`"))
                                .with_label("declared here")
                                .with_help(format!(
                                    "use it in a way that determines its type, or annotate it: \
                                     `var {name}: int` or `var {name}: bool`"
                                )),
                        );
                        None
                    }
                    Ty::Array(_) | Ty::Str if !typed => {
                        self.error(
                            Diagnostic::error(span, format!("`{name}` needs a type with a length"))
                                .with_help(format!(
                                    "e.g. `var {name}: [int; 5]` or `var {name}: string[1..=10]`"
                                )),
                        );
                        None
                    }
                    other => self.to_type(&other),
                }
            })
            .collect();
        let symbols = std::mem::take(&mut self.symbols)
            .into_iter()
            .map(|(mut symbol, ty)| {
                if let Some(ty) = ty {
                    symbol.detail = self.name_of(&self.resolve_deep(&ty));
                }
                symbol
            })
            .collect();
        let mut hole_types = HashMap::new();
        for (span, ty) in std::mem::take(&mut self.holes) {
            match self.resolve_deep(&ty) {
                Ty::Var(_) => self.error(
                    Diagnostic::error(span, "cannot infer the type of this `?`").with_help(
                        "use it where the type is known, e.g. in `var x: int = ?` or `x == ?`",
                    ),
                ),
                Ty::Bool | Ty::Int | Ty::Char | Ty::Enum(_) | Ty::Struct(_) => {
                    hole_types.insert(span.start, self.to_type(&ty).expect("resolved"));
                }
                Ty::Error => {}
                other => self.error(Diagnostic::error(
                    span,
                    format!(
                        "`?` stands for a single unknown value, not a `{}`",
                        self.name_of(&other)
                    ),
                )),
            }
        }
        let info = TypeInfo {
            decl_types,
            hole_types,
            enums: self.enums,
            structs: self.structs,
            symbols,
        };
        (info, self.diagnostics)
    }

    // --- scopes --------------------------------------------------------------------------

    fn lookup(&mut self, name: &str, span: Span) -> Option<Binding> {
        let found = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.bindings.get(name))
            .cloned();
        if found.is_none() {
            let mut diagnostic = Diagnostic::error(span, format!("unknown variable `{name}`"))
                .with_label("not declared");
            if self
                .outer_scopes
                .iter()
                .any(|scope| scope.bindings.contains_key(name))
            {
                diagnostic = diagnostic
                    .with_note("functions can only use their own parameters and variables")
                    .with_help(format!("pass `{name}` to the function as a parameter"));
            } else if self.enums.iter().any(|e| e.name == name) {
                diagnostic = Diagnostic::error(span, format!("`{name}` is an enum, not a value"))
                    .with_help(format!(
                        "use one of its variants, e.g. `{name}.{}`",
                        self.enums
                            .iter()
                            .find(|e| e.name == name)
                            .map_or("", |e| e.variants[0].as_str())
                    ));
            } else {
                diagnostic =
                    diagnostic.with_help(format!("declare it first, e.g. `var {name}: int`"));
            }
            self.error(diagnostic);
        }
        found
    }

    fn declare(&mut self, ident: &Ident, binding: Binding, kind: SymbolKind) {
        let scope = self.scopes.last_mut().expect("there is always a scope");
        if scope.bindings.contains_key(&ident.name) {
            self.error(
                Diagnostic::error(
                    ident.span,
                    format!("`{}` is already declared in this scope", ident.name),
                )
                .with_help("assign to it with `=` instead, or pick another name"),
            );
        }
        let scope = self.scopes.last().expect("scope").span;
        let symbol = Symbol {
            name: ident.name.clone(),
            kind,
            span: ident.span,
            detail: String::new(),
            scope,
        };
        self.symbols.push((symbol, Some(binding.ty.clone())));
        self.scopes
            .last_mut()
            .expect("scope")
            .bindings
            .insert(ident.name.clone(), binding);
    }

    fn in_scope(&mut self, span: Span, f: impl FnOnce(&mut Self)) {
        self.scopes.push(Scope::new(span));
        f(self);
        self.scopes.pop();
    }

    fn block(&mut self, block: &Block) {
        self.in_scope(block.span, |this| {
            for stmt in &block.stmts {
                this.stmt(stmt, false);
            }
        });
    }

    // --- statements ----------------------------------------------------------------------

    fn stmt(&mut self, stmt: &Stmt, top_level: bool) {
        match &stmt.kind {
            StmtKind::Var(decl) => self.var_decl(decl),
            StmtKind::Assign { target, value } => {
                let value_ty = self.expr(value);
                if let Some(place_ty) = self.place(target) {
                    self.unify(&place_ty, &value_ty, value.span);
                }
            }
            StmtKind::Require(cond) => {
                let ty = self.expr(cond);
                self.unify(&Ty::Bool, &ty, cond.span);
            }
            StmtKind::Print(args) => {
                for arg in args {
                    let ty = self.expr(arg);
                    self.no_unit(&ty, arg.span);
                }
            }
            StmtKind::If {
                cond,
                then_block,
                else_block,
            } => {
                let ty = self.expr(cond);
                self.unify(&Ty::Bool, &ty, cond.span);
                self.block(then_block);
                if let Some(block) = else_block {
                    self.block(block);
                }
            }
            StmtKind::For { var, iter, body } => {
                let element = self.iterable(iter);
                self.in_scope(body.span, |this| {
                    this.declare(
                        var,
                        Binding {
                            ty: element,
                            is_loop_var: true,
                        },
                        SymbolKind::LoopVariable,
                    );
                    this.block(body);
                });
            }
            StmtKind::While { cond, bound, body } => {
                let ty = self.expr(cond);
                self.unify(&Ty::Bool, &ty, cond.span);
                if let Some(bound) = bound {
                    let bound_ty = self.expr(bound);
                    self.unify(&Ty::Int, &bound_ty, bound.span);
                }
                self.block(body);
            }
            StmtKind::Solve(solve) => self.solve(solve),
            StmtKind::Fn(decl) => {
                if top_level {
                    self.function(decl);
                } else {
                    self.error(Diagnostic::error(
                        decl.name.span,
                        "functions must be declared at the top level",
                    ));
                }
            }
            StmtKind::Return(value) => self.return_stmt(value.as_ref(), stmt.span),
            StmtKind::Expr(expr) => {
                self.expr(expr);
            }
            StmtKind::Enum(decl) if !top_level => {
                self.error(Diagnostic::error(
                    decl.name.span,
                    "enums must be declared at the top level",
                ));
            }
            StmtKind::Struct(decl) if !top_level => {
                self.error(Diagnostic::error(
                    decl.name.span,
                    "structs must be declared at the top level",
                ));
            }
            StmtKind::Import(_) if !top_level => {
                self.error(Diagnostic::error(
                    stmt.span,
                    "imports must be at the top level",
                ));
            }
            StmtKind::Enum(_) | StmtKind::Struct(_) | StmtKind::Import(_) => {}
        }
    }

    fn no_unit(&mut self, ty: &Ty, span: Span) {
        if self.resolve(ty) == Ty::Unit {
            self.error(Diagnostic::error(span, "this function call has no value"));
        }
    }

    fn var_decl(&mut self, decl: &VarDecl) {
        let annotated = decl.ty.as_ref().map(|ty| self.type_expr(ty));
        let init_ty = decl.init.as_ref().map(|init| self.expr(init));
        if let (
            Some(TypeExpr {
                kind: TypeKind::Array(_, None),
                span,
            }),
            None,
        ) = (&decl.ty, &decl.init)
        {
            self.error(
                Diagnostic::error(*span, "an unknown array needs a length")
                    .with_help("e.g. `[int; 5]`; lengths may be any value known at compile time"),
            );
        }
        for (name, id) in &decl.names {
            let needs_annotation = match self.options.explicit_types {
                ExplicitTypes::Off => false,
                ExplicitTypes::Unknowns => decl.init.is_none(),
                ExplicitTypes::All => true,
            };
            let in_library = self.library.is_some_and(|library| {
                library.start <= name.span.start && name.span.end <= library.end
            });
            if needs_annotation && annotated.is_none() && !in_library {
                let example = if decl.init.is_some() {
                    "var n: int = 42"
                } else {
                    "var x: int"
                };
                let option = if self.options.explicit_types == ExplicitTypes::All {
                    "all"
                } else {
                    "unknowns"
                };
                self.error(
                    Diagnostic::error(
                        name.span,
                        format!("`{}` needs a type annotation", name.name),
                    )
                    .with_label("no type given")
                    .with_note(format!(
                        "required by the compiler option explicit-types = \"{option}\""
                    ))
                    .with_help(format!("annotate it, e.g. `{example}`")),
                );
            }
            let ty = match (&annotated, &init_ty) {
                (Some(annotated), Some(init)) => {
                    let span = decl.init.as_ref().expect("has init").span;
                    self.unify(annotated, init, span);
                    annotated.clone()
                }
                (Some(annotated), None) => annotated.clone(),
                (None, Some(init)) => {
                    self.no_unit(init, decl.init.as_ref().expect("has init").span);
                    init.clone()
                }
                (None, None) => self.fresh(),
            };
            let typed = decl.ty.is_some() || decl.init.is_some();
            self.decls[id.0] = Some((ty.clone(), name.name.clone(), name.span, typed));
            self.declare(
                name,
                Binding {
                    ty,
                    is_loop_var: false,
                },
                SymbolKind::Variable,
            );
        }
    }

    /// The type of an assignable place, or `None` after reporting an error.
    fn place(&mut self, place: &Place) -> Option<Ty> {
        let binding = self.lookup(&place.root.name, place.root.span)?;
        if binding.is_loop_var {
            self.error(Diagnostic::error(
                place.root.span,
                format!("cannot assign to the loop variable `{}`", place.root.name),
            ));
        }
        let mut ty = binding.ty;
        for access in &place.path {
            ty = match access {
                Access::Index(index) => self.index_type(&ty, index, place.span),
                Access::Field(field) => self.field_type(&ty, field),
            };
        }
        Some(ty)
    }

    fn function(&mut self, decl: &Rc<FnDecl>) {
        let signature = &self.functions[&decl.name.name];
        let (params, ret) = (signature.params.clone(), signature.ret.clone());
        let outer = std::mem::replace(&mut self.scopes, vec![Scope::new(decl.body.span)]);
        self.outer_scopes = outer;
        let previous_return = self.return_type.replace(ret.clone());
        for (param, ty) in decl.params.iter().zip(params) {
            self.declare(
                &param.name,
                Binding {
                    ty,
                    is_loop_var: false,
                },
                SymbolKind::Parameter,
            );
        }
        self.block(&decl.body);
        if ret != Ty::Unit && !always_returns(&decl.body) {
            self.error(
                Diagnostic::error(
                    decl.name.span,
                    format!("`{}` might end without returning a value", decl.name.name),
                )
                .with_help("make sure every path through the function ends with `return …`"),
            );
        }
        self.return_type = previous_return;
        self.scopes = std::mem::take(&mut self.outer_scopes);
    }

    fn return_stmt(&mut self, value: Option<&Expr>, span: Span) {
        let Some(expected) = self.return_type.clone() else {
            self.error(Diagnostic::error(
                span,
                "`return` is only allowed inside a function",
            ));
            return;
        };
        match (value, expected == Ty::Unit) {
            (Some(value), true) => {
                self.expr(value);
                self.error(
                    Diagnostic::error(value.span, "this function does not return a value")
                        .with_help("declare its result type, e.g. `fn name(…) -> int`"),
                );
            }
            (Some(value), false) => {
                let ty = self.expr(value);
                self.unify(&expected, &ty, value.span);
            }
            (None, false) => {
                self.error(Diagnostic::error(span, "this function must return a value"))
            }
            (None, true) => {}
        }
    }

    fn solve(&mut self, solve: &Solve) {
        for target in &solve.targets {
            let ty = self.expr(target);
            self.no_unit(&ty, target.span);
        }
        if let Some(limit) = &solve.limit {
            let ty = self.expr(limit);
            self.unify(&Ty::Int, &ty, limit.span);
        }
        if let Some((_, objective)) = &solve.objective {
            let ty = self.expr(objective);
            self.unify(&Ty::Int, &ty, objective.span);
        }
        if let Some(body) = &solve.body {
            self.block(body);
        }
        if let Some(block) = &solve.else_block {
            self.block(block);
        }
    }

    // --- expressions ---------------------------------------------------------------------

    /// The element type of an iterable.
    fn iterable(&mut self, iterable: &Iterable) -> Ty {
        match iterable {
            Iterable::Range(range) => {
                let lo = self.expr(&range.lo);
                let hi = self.expr(&range.hi);
                let ty = self.unify(&lo, &hi, range.hi.span);
                match self.resolve(&ty) {
                    Ty::Char => Ty::Char,
                    _ => self.unify(&Ty::Int, &ty, range.span),
                }
            }
            Iterable::Expr(expr) => {
                let ty = self.expr(expr);
                match self.resolve(&ty) {
                    Ty::Str => Ty::Char,
                    Ty::Array(element) => *element,
                    Ty::Error => Ty::Error,
                    _ => {
                        let element = self.fresh();
                        self.unify(&Ty::Array(Box::new(element.clone())), &ty, expr.span);
                        element
                    }
                }
            }
        }
    }

    fn index_type(&mut self, base: &Ty, index: &Expr, span: Span) -> Ty {
        let index_ty = self.expr(index);
        self.unify(&Ty::Int, &index_ty, index.span);
        match self.resolve(base) {
            Ty::Str => Ty::Char,
            Ty::Array(element) => *element,
            Ty::Error => Ty::Error,
            _ => {
                let element = self.fresh();
                self.unify(&Ty::Array(Box::new(element.clone())), base, span);
                element
            }
        }
    }

    fn field_type(&mut self, base: &Ty, field: &Ident) -> Ty {
        match self.resolve(base) {
            Ty::Struct(id) => {
                if let Some((_, ty)) = self.struct_fields[id]
                    .iter()
                    .find(|(name, _)| *name == field.name)
                {
                    return ty.clone();
                }
                let fields: Vec<&str> = self.struct_fields[id]
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect();
                self.error(
                    Diagnostic::error(
                        field.span,
                        format!("`{}` has no field `{}`", self.structs[id].name, field.name),
                    )
                    .with_help(format!("its fields are {}", fields.join(", "))),
                );
                Ty::Error
            }
            Ty::Error => Ty::Error,
            other => {
                let name = self.name_of(&other);
                self.error(Diagnostic::error(field.span, format!("`{name}` has no fields"))
                    .with_note("only structs have fields; if the type is still unknown, annotate the variable"));
                Ty::Error
            }
        }
    }

    fn expr(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Char(_) => Ty::Char,
            ExprKind::Str(parts) => {
                for part in parts {
                    if let StrPart::Expr(inner) = part {
                        let ty = self.expr(inner);
                        self.no_unit(&ty, inner.span);
                    }
                }
                Ty::Str
            }
            ExprKind::Var(ident) => self
                .lookup(&ident.name, ident.span)
                .map_or(Ty::Error, |b| b.ty),
            ExprKind::Hole => {
                let ty = self.fresh();
                self.holes.push((expr.span, ty.clone()));
                ty
            }
            ExprKind::Unary(op, operand) => {
                let ty = self.expr(operand);
                let expected = match op {
                    UnaryOp::Neg => Ty::Int,
                    UnaryOp::Not => Ty::Bool,
                };
                self.unify(&expected, &ty, operand.span)
            }
            ExprKind::Binary(op, lhs, rhs) => self.binary(*op, lhs, rhs),
            ExprKind::Call(name, args) => self.call(name, args, expr.span),
            ExprKind::Index(base, index) => {
                let base_ty = self.expr(base);
                self.index_type(&base_ty, index, base.span)
            }
            ExprKind::Field(base, field) => {
                if let ExprKind::Var(name) = &base.kind {
                    let is_variable = self
                        .scopes
                        .iter()
                        .any(|scope| scope.bindings.contains_key(&name.name));
                    if let (false, Some(id)) = (
                        is_variable,
                        self.enums.iter().position(|e| e.name == name.name),
                    ) {
                        if !self.enums[id].variants.contains(&field.name) {
                            let variants = self.enums[id].variants.join(", ");
                            self.error(
                                Diagnostic::error(
                                    field.span,
                                    format!("`{}` has no variant `{}`", name.name, field.name),
                                )
                                .with_help(format!("its variants are {variants}")),
                            );
                        }
                        return Ty::Enum(id);
                    }
                }
                let base_ty = self.expr(base);
                self.field_type(&base_ty, field)
            }
            ExprKind::Array(elements) => {
                let element = self.fresh();
                for item in elements {
                    let ty = self.expr(item);
                    self.unify(&element, &ty, item.span);
                }
                Ty::Array(Box::new(element))
            }
            ExprKind::Comprehension {
                func,
                body,
                clauses,
            } => {
                let element = self.comprehension(body, clauses);
                match func {
                    None => Ty::Array(Box::new(element)),
                    Some(func) => self.aggregate(func, &Ty::Array(Box::new(element)), expr.span),
                }
            }
            ExprKind::In(value, iterable) => {
                let value_ty = self.expr(value);
                let element = self.iterable(iterable);
                self.unify(&element, &value_ty, value.span);
                Ty::Bool
            }
            ExprKind::If(cond, then_value, else_value) => {
                let cond_ty = self.expr(cond);
                self.unify(&Ty::Bool, &cond_ty, cond.span);
                let then_ty = self.expr(then_value);
                let else_ty = self.expr(else_value);
                self.unify(&then_ty, &else_ty, else_value.span)
            }
        }
    }

    fn comprehension(&mut self, body: &Expr, clauses: &[Clause]) -> Ty {
        let mut element = Ty::Error;
        self.in_scope(body.span, |this| {
            for clause in clauses {
                match clause {
                    Clause::For(var, iterable) => {
                        let ty = this.iterable(iterable);
                        this.scopes.push(Scope::new(body.span));
                        this.declare(
                            var,
                            Binding {
                                ty,
                                is_loop_var: true,
                            },
                            SymbolKind::LoopVariable,
                        );
                    }
                    Clause::If(cond) => {
                        let ty = this.expr(cond);
                        this.unify(&Ty::Bool, &ty, cond.span);
                    }
                }
            }
            element = this.expr(body);
            for clause in clauses {
                if matches!(clause, Clause::For(..)) {
                    this.scopes.pop();
                }
            }
        });
        element
    }

    fn binary(&mut self, op: BinaryOp, lhs: &Expr, rhs: &Expr) -> Ty {
        let (lhs_ty, rhs_ty) = (self.expr(lhs), self.expr(rhs));
        match op {
            BinaryOp::Add
                if matches!(self.resolve(&lhs_ty), Ty::Array(_))
                    || matches!(self.resolve(&rhs_ty), Ty::Array(_)) =>
            {
                // `+` joins two arrays.
                self.unify(&lhs_ty, &rhs_ty, rhs.span)
            }
            BinaryOp::Add => {
                // `+` concatenates when either side is known to be a string, and adds otherwise.
                if self.resolve(&lhs_ty) == Ty::Str || self.resolve(&rhs_ty) == Ty::Str {
                    self.unify(&Ty::Str, &lhs_ty, lhs.span);
                    self.unify(&Ty::Str, &rhs_ty, rhs.span);
                    return Ty::Str;
                }
                self.unify(&Ty::Int, &lhs_ty, lhs.span);
                self.unify(&Ty::Int, &rhs_ty, rhs.span);
                Ty::Int
            }
            BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow
            | BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                self.unify(&Ty::Int, &lhs_ty, lhs.span);
                self.unify(&Ty::Int, &rhs_ty, rhs.span);
                Ty::Int
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let ty = self.unify(&lhs_ty, &rhs_ty, rhs.span);
                if !matches!(self.resolve(&ty), Ty::Char | Ty::Error) {
                    self.unify(&Ty::Int, &ty, lhs.span.to(rhs.span));
                }
                Ty::Bool
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                let ty = self.unify(&lhs_ty, &rhs_ty, rhs.span);
                self.no_unit(&ty, lhs.span);
                Ty::Bool
            }
            BinaryOp::Xor => {
                let ty = self.unify(&lhs_ty, &rhs_ty, rhs.span);
                match self.resolve(&ty) {
                    Ty::Int => Ty::Int,
                    Ty::Error => Ty::Error,
                    _ => self.unify(&Ty::Bool, &ty, lhs.span.to(rhs.span)),
                }
            }
            BinaryOp::And | BinaryOp::Or | BinaryOp::Implies | BinaryOp::Iff => {
                self.unify(&Ty::Bool, &lhs_ty, lhs.span);
                self.unify(&Ty::Bool, &rhs_ty, rhs.span);
                Ty::Bool
            }
        }
    }

    fn call(&mut self, name: &Ident, args: &[Arg], span: Span) -> Ty {
        if let Some(id) = self.structs.iter().position(|s| s.name == name.name) {
            return self.constructor(id, args, span);
        }
        for arg in args {
            if let Some(arg_name) = &arg.name {
                self.error(Diagnostic::error(
                    arg_name.span,
                    "named arguments are only for struct constructors",
                ));
            }
        }
        let arg_tys: Vec<Ty> = args.iter().map(|arg| self.expr(&arg.value)).collect();
        if let Some(signature) = self.functions.get(&name.name) {
            let (params, ret) = (signature.params.clone(), signature.ret.clone());
            if params.len() != args.len() {
                self.error(Diagnostic::error(
                    span,
                    format!(
                        "`{}` takes {} argument{}, but {} were given",
                        name.name,
                        params.len(),
                        if params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                ));
            }
            for ((param, arg_ty), arg) in params.iter().zip(&arg_tys).zip(args) {
                self.unify(param, arg_ty, arg.value.span);
            }
            return ret;
        }
        match name.name.as_str() {
            "abs" if args.len() == 1 => self.unify(&Ty::Int, &arg_tys[0], args[0].value.span),
            "ord" if args.len() == 1 => {
                self.unify(&Ty::Char, &arg_tys[0], args[0].value.span);
                Ty::Int
            }
            "chr" if args.len() == 1 => {
                self.unify(&Ty::Int, &arg_tys[0], args[0].value.span);
                Ty::Char
            }
            "min" | "max" if args.len() >= 2 => {
                for (ty, arg) in arg_tys.iter().zip(args) {
                    self.unify(&Ty::Int, ty, arg.value.span);
                }
                Ty::Int
            }
            "min" | "max" | "len" | "sum" | "count" | "all" | "any" | "all_different"
            | "reverse"
                if args.len() == 1 =>
            {
                self.aggregate(name, &arg_tys[0], args[0].value.span)
            }
            builtin if BUILTIN_FUNCTIONS.contains(&builtin) => {
                self.error(Diagnostic::error(
                    span,
                    format!("wrong number of arguments for `{builtin}`"),
                ));
                Ty::Error
            }
            other => {
                self.error(Diagnostic::error(name.span, format!("unknown function `{other}`"))
                    .with_help("declare it with `fn`, or use a built-in such as `sum`, `count`, `all`, `min`"));
                Ty::Error
            }
        }
    }

    /// The result type of a built-in (or user function) applied to one array or string.
    fn aggregate(&mut self, func: &Ident, argument: &Ty, span: Span) -> Ty {
        if let Some(signature) = self.functions.get(&func.name) {
            let (params, ret) = (signature.params.clone(), signature.ret.clone());
            if params.len() != 1 {
                self.error(Diagnostic::error(
                    span,
                    format!(
                        "`{}` must take exactly one array to be used with `for`",
                        func.name
                    ),
                ));
                return ret;
            }
            self.unify(&params[0], argument, span);
            return ret;
        }
        let element = match self.resolve(argument) {
            Ty::Str if matches!(func.name.as_str(), "len" | "reverse") => {
                return if func.name == "len" { Ty::Int } else { Ty::Str };
            }
            Ty::Array(element) => *element,
            Ty::Error => return Ty::Error,
            _ => {
                let element = self.fresh();
                self.unify(&Ty::Array(Box::new(element.clone())), argument, span);
                element
            }
        };
        match func.name.as_str() {
            "sum" | "min" | "max" => {
                self.unify(&Ty::Int, &element, span);
                Ty::Int
            }
            "count" => {
                self.unify(&Ty::Bool, &element, span);
                Ty::Int
            }
            "all" | "any" => self.unify(&Ty::Bool, &element, span),
            "len" => Ty::Int,
            "all_different" => {
                self.no_unit(&element, span);
                Ty::Bool
            }
            "reverse" => Ty::Array(Box::new(element)),
            other => {
                self.error(Diagnostic::error(func.span, format!("`{other}` cannot aggregate a comprehension"))
                    .with_help("use `sum`, `count`, `all`, `any`, `min`, `max`, `len`, `all_different`, `reverse` or one of your functions"));
                Ty::Error
            }
        }
    }

    fn constructor(&mut self, id: usize, args: &[Arg], span: Span) -> Ty {
        let fields = self.struct_fields[id].clone();
        let name = self.structs[id].name.clone();
        let named = args.iter().any(|arg| arg.name.is_some());
        if named && args.iter().any(|arg| arg.name.is_none()) {
            self.error(Diagnostic::error(
                span,
                "use either all named or all positional arguments",
            ));
        }
        if args.len() != fields.len() {
            self.error(Diagnostic::error(
                span,
                format!(
                    "`{name}` has {} fields, but {} values were given",
                    fields.len(),
                    args.len()
                ),
            ));
        }
        for (position, arg) in args.iter().enumerate() {
            let ty = self.expr(&arg.value);
            let field = match &arg.name {
                Some(arg_name) => match fields.iter().find(|(field, _)| *field == arg_name.name) {
                    Some(field) => field,
                    None => {
                        self.error(Diagnostic::error(
                            arg_name.span,
                            format!("`{name}` has no field `{}`", arg_name.name),
                        ));
                        continue;
                    }
                },
                None => match fields.get(position) {
                    Some(field) => field,
                    None => continue,
                },
            };
            self.unify(&field.1, &ty, arg.value.span);
        }
        Ty::Struct(id)
    }
}

/// Whether every path through `block` ends in a `return`.
fn always_returns(block: &Block) -> bool {
    block.stmts.iter().any(|stmt| match &stmt.kind {
        StmtKind::Return(_) => true,
        StmtKind::If {
            then_block,
            else_block: Some(else_block),
            ..
        } => always_returns(then_block) && always_returns(else_block),
        _ => false,
    })
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Unit => write!(f, "()"),
            Type::Bool => write!(f, "bool"),
            Type::Int => write!(f, "int"),
            Type::Char => write!(f, "char"),
            Type::Str => write!(f, "string"),
            Type::Array(element) => write!(f, "[{element}]"),
            Type::Enum(id) => write!(f, "enum#{id}"),
            Type::Struct(id) => write!(f, "struct#{id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use crate::source::SourceFile;

    fn check_with(text: &str, options: &CompilerOptions) -> (TypeInfo, Vec<Diagnostic>) {
        let program =
            parse(&SourceFile::new("t.rsat", text)).unwrap_or_else(|d| panic!("{}", d.message));
        check(&program, options)
    }

    fn errors(text: &str) -> Vec<String> {
        check_with(text, &CompilerOptions::default())
            .1
            .into_iter()
            .map(|d| d.message)
            .collect()
    }

    fn ok(text: &str) -> TypeInfo {
        let (info, diagnostics) = check_with(text, &CompilerOptions::default());
        assert!(
            diagnostics.is_empty(),
            "{:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        info
    }

    #[test]
    fn infers_types_from_usage() {
        let types = ok("var x, y\nvar b\nrequire x * y == 6 and b");
        assert_eq!(types.decl_type(DeclId(0)), &Type::Int);
        assert_eq!(types.decl_type(DeclId(1)), &Type::Int);
        assert_eq!(types.decl_type(DeclId(2)), &Type::Bool);
    }

    #[test]
    fn infers_through_equality_between_unknowns() {
        let types = ok("var p, q\nrequire p == q\nrequire q");
        assert_eq!(types.decl_type(DeclId(0)), &Type::Bool);
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
            errors("for i in 0..3 { i = 2 }"),
            ["cannot assign to the loop variable `i`"]
        );
        assert_eq!(
            errors("require len(5) > 1"),
            ["expected `[_]`, found `int`"]
        );
        assert_eq!(errors("require foo(1)"), ["unknown function `foo`"]);
    }

    #[test]
    fn types_arrays_comprehensions_and_builtins() {
        let types = ok(
            "var grid: [[int[1..=9]; 3]; 3]\nvar take: [bool; 4]\nvar w = [3, 1, 4, 1]\n\
             require all_different(grid[0])\nrequire sum(w[i] for i in 0..4 if take[i]) <= 5\n\
             require count(take) == 2 and all(x > 0 for x in w)\nvar squares = [i * i for i in 1..=3]\n\
             require 4 in squares and max(squares) == 9 and len(reverse(w)) == 4",
        );
        assert_eq!(
            types.decl_type(DeclId(0)),
            &Type::Array(Box::new(Type::Array(Box::new(Type::Int))))
        );
        assert_eq!(
            types.decl_type(DeclId(3)),
            &Type::Array(Box::new(Type::Int))
        );
    }

    #[test]
    fn types_functions_and_checks_returns() {
        ok(
            "fn clamp(v: int, lo: int, hi: int) -> int {\n if v < lo { return lo }\n return min(v, hi)\n}\n\
            fn sorted(xs: [int]) { for i in 1..len(xs) { require xs[i - 1] <= xs[i] } }\n\
            var a: [int[0..=9]; 3]\nsorted(a)\nrequire clamp(a[0], 2, 5) == 3",
        );
        assert_eq!(
            errors("fn f(x: int) -> int { if x > 0 { return 1 } }"),
            ["`f` might end without returning a value"]
        );
        assert_eq!(
            errors("var n = 3\nfn f() -> int { return n }"),
            ["unknown variable `n`"]
        );
        assert_eq!(
            errors("fn f(x: int) -> int { return x }\nrequire f(true) == 1"),
            ["expected `int`, found `bool`"]
        );
        assert_eq!(
            errors("return 1"),
            ["`return` is only allowed inside a function"]
        );
        assert_eq!(
            errors("fn sum(x: int) -> int { return x }"),
            ["`sum` is a built-in name"]
        );
    }

    #[test]
    fn types_enums_structs_chars_and_strings() {
        let types = ok(
            "enum Color { Red, Green, Blue }\nstruct Point { x: int, y: int }\n\
             var paint: Color\nvar p: Point\nvar word: string[1..=5]\nvar c: char\n\
             require paint != Color.Red and p.x < p.y and p == Point(x: 1, y: 2)\n\
             require c in 'a'..='z' and word[0] == c and len(word) == 3 and word + \"!\" == \"abc!\"\n\
             require ord(c) < 120 and chr(97) == 'a' and (3 xor 5) == 6",
        );
        assert_eq!(types.decl_type(DeclId(0)), &Type::Enum(0));
        assert_eq!(types.decl_type(DeclId(1)), &Type::Struct(0));
        assert_eq!(
            errors("enum E { A }\nrequire E.B == E.A"),
            ["`E` has no variant `B`"]
        );
        assert_eq!(
            errors("struct P { x: int }\nvar p: P\nrequire p.z > 0"),
            ["`P` has no field `z`"]
        );
    }

    #[test]
    fn unknown_collections_need_lengths() {
        assert_eq!(errors("var a: [int]"), ["an unknown array needs a length"]);
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

    #[test]
    fn records_symbols_for_tooling() {
        let types = ok(
            "fn double(v: int) -> int { return v * 2 }\nvar x: int[0..=9]\nrequire double(x) == 4",
        );
        let names: Vec<_> = types
            .symbols
            .iter()
            .map(|s| (s.name.as_str(), s.kind, s.detail.as_str()))
            .collect();
        assert!(names.contains(&("double", SymbolKind::Function, "fn(int) -> int")));
        assert!(names.contains(&("v", SymbolKind::Parameter, "int")));
        assert!(names.contains(&("x", SymbolKind::Variable, "int")));
    }
}
