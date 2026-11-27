//! RedSAT in the browser: the compiler and the built-in CDCL solver, compiled to WebAssembly
//! with `wasm-bindgen`. The web playground calls these functions from a Web Worker.
//!
//! Results are returned as JSON strings, which keeps the boundary small and explicit. Positions
//! are given both as one-based lines and columns and as UTF-16 offsets (what JavaScript strings
//! and editors index by).

mod js_solver;

use std::cell::RefCell;
use std::io::{self, Write};

use js_sys::Function;
use redsat_lang::check::SymbolKind;
use redsat_lang::config::parse_config;
use redsat_lang::syntax::{
    BUILTIN_FUNCTIONS, BUILTIN_TYPES, CONTEXTUAL_KEYWORDS, KEYWORDS, PRELUDE_FUNCTIONS,
    builtin_doc, keyword_doc, type_doc,
};
use redsat_lang::{
    CompilerOptions, Diagnostic, Hooks, OptionLayer, Severity, SolveStats, SourceFile, SourceMap,
    Span,
};
use redsat_sat::{Cdcl, Solver};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

thread_local! {
    /// Receives program output, one line at a time (WebAssembly runs on one thread).
    static OUTPUT: RefCell<Option<Function>> = const { RefCell::new(None) };
    /// Receives `--verbose` progress lines.
    static TRACE: RefCell<Option<Function>> = const { RefCell::new(None) };
    /// Creates instances of the JavaScript solver chosen for this run, with its name.
    static SOLVER: RefCell<Option<(Function, String)>> = const { RefCell::new(None) };
    /// The page's stop flag for this run: an `Int32Array` over a `SharedArrayBuffer`, nonzero
    /// when the run should stop (only available when the page is cross-origin isolated).
    static STOP: RefCell<Option<js_sys::Int32Array>> = const { RefCell::new(None) };
}

/// Whether the page asked the current run to stop.
fn stop_requested() -> bool {
    STOP.with(|stop| {
        stop.borrow()
            .as_ref()
            .is_some_and(|flag| js_sys::Atomics::load(flag, 0).unwrap_or(0) != 0)
    })
}

/// Reports Rust panics on the JavaScript console instead of an opaque "unreachable" trap.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// The compiler's version.
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The language's vocabulary as JSON, so editors highlight and complete exactly what the
/// compiler knows: `{keywords, contextual, types, builtins, prelude}`, where keywords carry
/// `{name, doc}`, as do types and functions (a prelude doc starts with the signature line).
/// Docs are light Markdown: inline code, `**bold**`, paragraphs and `- ` lists.
#[wasm_bindgen]
#[must_use]
pub fn vocabulary() -> String {
    let prelude: Vec<Value> = PRELUDE_FUNCTIONS
        .iter()
        .map(|name| json!({ "name": name, "doc": prelude_doc(name) }))
        .collect();
    let named = |words: &[&str], doc: fn(&str) -> Option<&'static str>| -> Vec<Value> {
        words
            .iter()
            .map(|name| json!({ "name": name, "doc": doc(name).unwrap_or_default() }))
            .collect()
    };
    json!({
        "keywords": named(KEYWORDS, keyword_doc),
        "contextual": named(CONTEXTUAL_KEYWORDS, keyword_doc),
        "types": named(BUILTIN_TYPES, type_doc),
        "builtins": named(BUILTIN_FUNCTIONS, builtin_doc),
        "prelude": prelude,
    })
    .to_string()
}

/// Runs a program.
///
/// `config` is the text of a `redsat.toml` (may be empty). Program output goes to `on_output`
/// line by line; when `on_trace` is given it receives the progress lines `redsat --verbose`
/// prints. The SAT solver is RedSAT's built-in CDCL, unless `create_solver` is given: then it
/// is called for each solver instance and returns a JavaScript solver (see `js_solver`) called
/// `solver_name`. When `stop` is given (an `Int32Array` over a `SharedArrayBuffer`), the run
/// checks it as it goes and ends with the error "stopped" once it holds a nonzero value.
/// Returns `{ok, diagnostics, rendered, solves, phases, steps}` as JSON.
///
/// # Errors
/// When `config` is not a valid configuration.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)] // a flat function is the simplest JavaScript API
pub fn run(
    name: &str,
    source: &str,
    config: &str,
    on_output: Function,
    on_trace: Option<Function>,
    create_solver: Option<Function>,
    solver_name: Option<String>,
    stop: Option<js_sys::Int32Array>,
) -> Result<String, JsError> {
    let options = options(config)?;
    let file = SourceFile::new(name, source);
    OUTPUT.with(|output| *output.borrow_mut() = Some(on_output));
    let verbose = on_trace.is_some();
    TRACE.with(|trace| *trace.borrow_mut() = on_trace);
    let external = create_solver.is_some();
    let stoppable = stop.is_some();
    STOP.with(|slot| *slot.borrow_mut() = stop);
    SOLVER.with(|solver| {
        *solver.borrow_mut() = create_solver.map(|create| {
            (
                create,
                solver_name.unwrap_or_else(|| "JavaScript solver".into()),
            )
        });
    });

    let factory = move || -> Box<dyn Solver> {
        if !external {
            let mut solver = Cdcl::new();
            if stoppable {
                solver.set_terminator(std::sync::Arc::new(stop_requested));
            }
            return Box::new(solver);
        }
        SOLVER.with(|solver| {
            let solver = solver.borrow();
            let (create, name) = solver.as_ref().expect("set for this run");
            let instance = create
                .call0(&JsValue::NULL)
                .expect("the solver module failed to create an instance");
            Box::new(js_solver::JsSolver::new(instance.unchecked_into(), name)) as Box<dyn Solver>
        })
    };
    let trace = |line: &str| call(&TRACE, line);
    let hooks = Hooks {
        ask: None,
        trace: verbose.then_some(&trace as &redsat_lang::TraceFn),
        cancel: stoppable.then_some(&stop_requested as &redsat_lang::CancelFn),
        // The compiler runs in one Web Worker; parallel races happen across workers instead.
        parallel: None,
    };
    let mut out = LineWriter::default();
    let outcome = redsat_lang::run_with(&file, &options, &factory, &mut out, hooks);
    let _ = out.flush();
    OUTPUT.with(|output| output.borrow_mut().take());
    TRACE.with(|trace| trace.borrow_mut().take());
    SOLVER.with(|solver| solver.borrow_mut().take());
    STOP.with(|stop| stop.borrow_mut().take());

    let result = json!({
        "ok": !outcome.has_errors(),
        "diagnostics": diagnostics_json(&outcome.diagnostics, &outcome.sources),
        "rendered": outcome.render_diagnostics(),
        "solves": outcome.stats.iter().map(|stats| solve_json(stats, &outcome.sources)).collect::<Vec<_>>(),
        "phases": outcome.phases.iter().map(|(phase, time)| json!({ "phase": phase, "ms": millis(*time) })).collect::<Vec<_>>(),
        "steps": {
            "statements": outcome.steps.statements,
            "calls": outcome.steps.calls,
            "maxDepth": outcome.steps.max_depth,
        },
    });
    Ok(result.to_string())
}

/// Analyses a program for the editor without solving it: diagnostics, the inferred domain of
/// each unknown declaration, and (when the program parses) its symbols, structs and enums, or
/// `null` for those. Elaboration stops after `budget_ms` milliseconds.
///
/// # Errors
/// When `config` is not a valid configuration.
#[wasm_bindgen]
pub fn analyze(name: &str, source: &str, config: &str, budget_ms: u32) -> Result<String, JsError> {
    let options = options(config)?;
    let file = SourceFile::new(name, source);
    let budget = std::time::Duration::from_millis(u64::from(budget_ms));
    let analysis = redsat_lang::analyze(&file, &options, budget);
    let main = analysis.sources.main();
    let domains: Vec<Value> = analysis
        .domains
        .iter()
        .filter(|domain| main.contains(domain.span.end))
        .map(|domain| {
            json!({
                "name": domain.name,
                "domain": domain.domain,
                "width": domain.width,
                "unknowns": domain.unknowns,
                "from": utf16_offset(main.text(), domain.span.start),
                "to": utf16_offset(main.text(), domain.span.end),
            })
        })
        .collect();
    let (symbols, structs, enums) = analysis.types.as_ref().map_or((Value::Null, Value::Null, Value::Null), |types| {
        let symbols: Vec<Value> = types
            .symbols
            .iter()
            .filter(|symbol| main.contains(symbol.span.start) && main.contains(symbol.span.end))
            .map(|symbol| {
                json!({
                    "name": symbol.name,
                    "kind": symbol_kind(symbol.kind),
                    "detail": symbol.detail,
                    "from": utf16_offset(main.text(), symbol.span.start),
                    "to": utf16_offset(main.text(), symbol.span.end),
                    "scopeFrom": utf16_offset(main.text(), symbol.scope.start),
                    "scopeTo": utf16_offset(main.text(), symbol.scope.end),
                })
            })
            .collect();
        let structs: Vec<Value> = types
            .structs
            .iter()
            .map(|info| {
                let fields: Vec<Value> = info
                    .fields
                    .iter()
                    .map(|(name, ty)| json!({ "name": name, "type": analysis.sources.slice(ty.span) }))
                    .collect();
                json!({ "name": info.name, "fields": fields })
            })
            .collect();
        let enums: Vec<Value> =
            types.enums.iter().map(|info| json!({ "name": info.name, "variants": info.variants })).collect();
        (Value::from(symbols), Value::from(structs), Value::from(enums))
    });
    // Your functions, with their `///` docs (the standard library's come with `vocabulary()`).
    let functions: Vec<Value> = analysis
        .functions
        .iter()
        .filter(|function| function.origin == redsat_lang::signatures::Origin::Program)
        .map(|function| json!({ "name": function.name, "label": function.label(), "doc": function.doc }))
        .collect();
    Ok(json!({
        "diagnostics": diagnostics_json(&analysis.diagnostics, &analysis.sources),
        "domains": domains,
        "functions": functions,
        "symbols": symbols,
        "structs": structs,
        "enums": enums,
    })
    .to_string())
}

fn symbol_kind(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Variable => "variable",
        SymbolKind::Parameter => "parameter",
        SymbolKind::LoopVariable => "loop variable",
        SymbolKind::Function => "function",
        SymbolKind::Enum => "enum",
        SymbolKind::Struct => "struct",
    }
}

fn options(config: &str) -> Result<CompilerOptions, JsError> {
    let layer =
        parse_config(config).map_err(|error| JsError::new(&format!("redsat.toml: {error}")))?;
    CompilerOptions::resolve(&layer, &OptionLayer::default()).map_err(|error| JsError::new(&error))
}

fn call(slot: &'static std::thread::LocalKey<RefCell<Option<Function>>>, text: &str) {
    slot.with(|function| {
        if let Some(function) = function.borrow().as_ref() {
            let _ = function.call1(&JsValue::NULL, &JsValue::from_str(text));
        }
    });
}

/// Hands program output to JavaScript one complete line at a time, so multi-byte characters are
/// never split.
#[derive(Default)]
struct LineWriter {
    pending: Vec<u8>,
}

impl Write for LineWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            call(&OUTPUT, &String::from_utf8_lossy(&line));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            call(&OUTPUT, &String::from_utf8_lossy(&rest));
        }
        Ok(())
    }
}

/// Diagnostics with positions in the main file. One that lies in another file (the prelude) is
/// placed at its first related location in the main file, like the language server does.
fn diagnostics_json(diagnostics: &[Diagnostic], sources: &SourceMap) -> Vec<Value> {
    let main = sources.main();
    let in_main = |span: &Span| main.contains(span.start) && main.contains(span.end);
    diagnostics
        .iter()
        .map(|diagnostic| {
            let mut message = diagnostic.message.clone();
            let span = if in_main(&diagnostic.span) {
                diagnostic.span
            } else {
                let file = sources.file_at(diagnostic.span.start);
                let (line, column) = file.line_col(diagnostic.span.start);
                message = format!("{message} (in {}:{line}:{column})", file.name());
                diagnostic
                    .related
                    .iter()
                    .map(|(span, _)| *span)
                    .find(in_main)
                    .unwrap_or_default()
            };
            let (line, column) = main.line_col(span.start);
            json!({
                "severity": match diagnostic.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Note => "info",
                },
                "message": message,
                "label": diagnostic.label,
                "notes": diagnostic.notes,
                "line": line,
                "column": column,
                "from": utf16_offset(main.text(), span.start),
                "to": utf16_offset(main.text(), span.end),
                "rendered": diagnostic.render(sources),
            })
        })
        .collect()
}

fn solve_json(stats: &SolveStats, sources: &SourceMap) -> Value {
    let file = sources.file_at(stats.span.start);
    let (line, _) = file.line_col(stats.span.start);
    json!({
        "line": line,
        "solver": stats.solver,
        "unknowns": stats.unknowns.iter().map(|(name, domain, width)| json!({ "name": name, "domain": domain, "width": width })).collect::<Vec<_>>(),
        "decisionBits": stats.decision_bits,
        "andGates": stats.and_gates,
        "cnfVars": stats.cnf_vars,
        "cnfClauses": stats.cnf_clauses,
        "satCalls": stats.sat_calls,
        "solutions": stats.solutions,
        "ms": millis(stats.elapsed),
    })
}

fn millis(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The UTF-16 offset of the byte `offset` of `text` (clamped to the text).
fn utf16_offset(text: &str, offset: usize) -> usize {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].encode_utf16().count()
}

/// The doc comment (`/// …` lines) of a standard library function.
fn prelude_doc(name: &str) -> String {
    let lines: Vec<&str> = redsat_lang::load::PRELUDE.lines().collect();
    let Some(declaration) = lines
        .iter()
        .position(|line| line.starts_with(&format!("fn {name}(")))
    else {
        return String::new();
    };
    let signature = lines[declaration].trim_end_matches('{').trim();
    let doc: Vec<&str> = lines[..declaration]
        .iter()
        .rev()
        .take_while(|line| line.starts_with("///"))
        .map(|line| line.trim_start_matches('/').trim())
        .collect();
    let doc = redsat_lang::signatures::join_doc_lines(doc.into_iter().rev());
    format!("{signature}\n{doc}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_byte_offsets_to_utf16() {
        let text = "a × b 😀 c";
        assert_eq!(utf16_offset(text, 1), 1);
        assert_eq!(utf16_offset(text, text.find('b').unwrap()), 4);
        assert_eq!(
            utf16_offset(text, text.find('c').unwrap()),
            9,
            "an emoji is two UTF-16 units"
        );
    }

    #[test]
    fn documents_every_prelude_function() {
        for name in PRELUDE_FUNCTIONS {
            let doc = prelude_doc(name);
            assert!(doc.starts_with(&format!("fn {name}(")), "{name}: {doc}");
            assert!(
                doc.lines().nth(1).is_some_and(|line| !line.is_empty()),
                "{name} has no doc"
            );
        }
    }
}
