//! Executing a `solve` statement: domains, circuit, SAT calls, decoding.

use std::time::Duration;

use web_time::Instant;

use num_bigint::BigInt;
use num_traits::One;
use redsat_circuit::{Aig, CircuitSolver, Edge, IntVec, Node, int};
use redsat_sat::{SolveResult, Solver};

use crate::ast::{Objective, SolveMode};
use crate::blast::{self, Bits, Blasted};
use crate::config::{CompilerOptions, UnboundedInts};
use crate::diagnostic::Diagnostic;
use crate::domain::{self, Ext, Interval};
use crate::ir::{Constant, Sort, Term, TermId, TermStore};
use crate::source::{SourceFile, SourceMap, Span};
use crate::{Answer, AskFn, SolverFactory, TraceFn};

/// A requirement accumulated by the program.
#[derive(Clone, Debug)]
pub(crate) struct Constraint {
    pub term: TermId,
    pub span: Span,
    /// Where `--verbose` reports its cost: `span`, or for a constraint made inside the standard
    /// library, the call in the program that led to it.
    pub site: Span,
    /// Whether a `require` statement made it; otherwise it is implicit (a divisor that must not
    /// be 0, a value that must fit its type, …).
    pub explicit: bool,
}

/// What a `solve` statement asks for, after its expressions have been evaluated.
pub(crate) struct SolveRequest<'a> {
    pub constraints: &'a [Constraint],
    /// The symbolic solve targets: solutions are distinct when these differ.
    pub projection: Vec<TermId>,
    pub mode: SolveMode,
    pub limit: Option<u64>,
    pub objective: Option<(Objective, TermId)>,
    pub span: Span,
    /// Whether to explain a proven "no solution".
    pub explain: bool,
    /// Asked after each solution in interactive mode.
    pub ask: Option<&'a AskFn>,
    /// Told about progress, for `--verbose`.
    pub trace: Option<&'a TraceFn>,
    /// The program's files, to show source lines in `--verbose` reports.
    pub sources: &'a SourceMap,
    /// Asked whether to stop.
    pub cancel: Option<&'a crate::CancelFn>,
    /// Lets `solve all` enumerate on several threads.
    pub parallel: Option<&'a crate::Parallel>,
}

/// How a `solve` ended.
pub(crate) enum SolveOutcome {
    /// At least one solution was found and reported.
    Solved,
    /// There is no solution. `searched` is set when the search was limited to a magnitude
    /// (because some unknowns had no inferable bounds), so the claim is not a proof.
    NoSolution {
        searched: Option<(u32, Vec<String>)>,
        /// The source locations of requirements that cannot hold together, when explained.
        conflict: Option<Vec<Span>>,
    },
    /// `solve unique` found two different solutions.
    NotUnique {
        first: Vec<Constant>,
        second: Vec<Constant>,
    },
}

/// Facts about one `solve`, printed with `--stats`.
#[derive(Clone, Debug, Default)]
pub struct SolveStats {
    /// The `solve` statement.
    pub span: Span,
    /// Each unknown in the circuit with its final domain and bit width.
    pub unknowns: Vec<(String, String, usize)>,
    /// AND gates in the circuit.
    pub and_gates: usize,
    /// CNF variables.
    pub cnf_vars: u32,
    /// CNF clauses.
    pub cnf_clauses: usize,
    /// SAT solver calls.
    pub sat_calls: u32,
    /// Solutions reported.
    pub solutions: u64,
    /// The magnitude limit used for unbounded unknowns, if any.
    pub magnitude_bits: Option<u32>,
    /// The solver's name.
    pub solver: String,
    /// Integer unknowns the solve depends on.
    pub int_unknowns: usize,
    /// Boolean unknowns the solve depends on.
    pub bool_unknowns: usize,
    /// Unknowns whose value domain inference fixed outright (they cost no search).
    pub fixed_unknowns: usize,
    /// Bits of the unknowns the solver has to decide: the search space has `2^decision_bits`
    /// points.
    pub decision_bits: usize,
    /// Circuit inputs (decision bits plus helpers such as division quotients).
    pub aig_inputs: usize,
    /// Time spent inferring domains.
    pub domains: Duration,
    /// Time spent building the circuit and CNF.
    pub build: Duration,
    /// Time spent in SAT calls and decoding models.
    pub search: Duration,
    /// Total time.
    pub elapsed: Duration,
}

impl SolveStats {
    /// One line per fact, prefixed with the statement's location.
    #[must_use]
    pub fn render(&self, source: &SourceFile) -> String {
        let (line, _) = source.line_col(self.span.start);
        let mut out = format!("stats: solve at {}:{line}\n", source.name());
        for (name, domain, width) in &self.unknowns {
            out += &format!(
                "  {name}: {domain} · {width} bit{}\n",
                if *width == 1 { "" } else { "s" }
            );
        }
        if let Some(bits) = self.magnitude_bits {
            out += &format!("  unbounded unknowns searched with |x| < 2^{bits}\n");
        }
        out += &format!(
            "  circuit: {} AND gates · CNF: {} variables, {} clauses\n",
            self.and_gates, self.cnf_vars, self.cnf_clauses
        );
        out += &format!(
            "  {} SAT call{} · {} solution{} · {:.2?} (build {:.2?}, search {:.2?})\n",
            self.sat_calls,
            if self.sat_calls == 1 { "" } else { "s" },
            self.solutions,
            if self.solutions == 1 { "" } else { "s" },
            self.elapsed,
            self.build,
            self.search
        );
        out
    }
}

/// Records the unknowns the solve depends on, with their final domains and widths.
fn measure_unknowns(
    stats: &mut SolveStats,
    store: &TermStore,
    relevant: &[usize],
    domains: &[Interval],
) {
    let fixed = |index: usize| {
        store.unknowns()[index].sort == Sort::Int
            && domains[index].bounds().is_some_and(|(lo, hi)| lo == hi)
    };
    stats.unknowns = relevant
        .iter()
        .map(|&index| {
            let unknown = &store.unknowns()[index];
            let (domain, width) = match unknown.sort {
                Sort::Bool => ("bool".to_string(), 1),
                Sort::Int => {
                    let domain = &domains[index];
                    (domain.to_string(), blast::layout_of(domain).width)
                }
            };
            (unknown.name.clone(), domain, width)
        })
        .collect();
    stats.int_unknowns = relevant
        .iter()
        .filter(|&&index| store.unknowns()[index].sort == Sort::Int)
        .count();
    stats.bool_unknowns = relevant.len() - stats.int_unknowns;
    stats.fixed_unknowns = relevant.iter().filter(|&&index| fixed(index)).count();
    stats.decision_bits = stats
        .unknowns
        .iter()
        .zip(relevant)
        .filter(|(_, index)| !fixed(**index))
        .map(|((_, _, width), _)| width)
        .sum();
}

/// How many unknowns `--verbose` lists one by one.
const TRACED_UNKNOWNS: usize = 8;

fn trace_unknowns(
    trace: &dyn Fn(String),
    stats: &SolveStats,
    unbounded: &[String],
    magnitude: u32,
) {
    let unknowns = stats.int_unknowns + stats.bool_unknowns;
    trace(format!(
        "  unknowns   {unknowns} ({} integer, {} boolean), {} fixed by domain inference · domains in {:.2?}",
        stats.int_unknowns, stats.bool_unknowns, stats.fixed_unknowns, stats.domains
    ));
    for (name, domain, width) in stats.unknowns.iter().take(TRACED_UNKNOWNS) {
        trace(format!(
            "             {name}: {domain} · {width} bit{}",
            plural(*width)
        ));
    }
    if unknowns > TRACED_UNKNOWNS {
        trace(format!(
            "             … and {} more",
            unknowns - TRACED_UNKNOWNS
        ));
    }
    if !unbounded.is_empty() {
        trace(format!(
            "             unbounded ({}) searched with |x| < 2^{magnitude}",
            unbounded.join(", ")
        ));
    }
    trace(format!(
        "  space      {} decision bit{} → 2^{} candidate assignments",
        stats.decision_bits,
        plural(stats.decision_bits),
        stats.decision_bits
    ));
}

/// The most source lines the cost report lists; the others are summed up in one row.
const TRACED_COST_LINES: usize = 24;

/// One row of the cost report: a source line, or what has none.
struct CostRow {
    /// Sorts rows in program order (the line's position); implicit rows go last.
    position: usize,
    /// `12`, or `<prelude>:40` for a line of another file.
    line: String,
    source: String,
    cost: Cost,
}

fn add_cost(total: &mut Cost, cost: &Cost) {
    total.unknowns += cost.unknowns;
    total.requirements += cost.requirements;
    total.free += cost.free;
    total.inputs += cost.inputs;
    total.gates += cost.gates;
    total.clauses += cost.clauses;
}

/// Reports, for `--verbose`, what each source line added to the circuit, like a database's
/// query plan with costs: unknowns, requirements, circuit inputs (decision bits), AND gates and
/// clauses, and each line's share of the clauses. A line run several times (in a loop, or a
/// function called from several places) shows how many times. Circuitry shared by several
/// lines counts at the first one that needs it.
fn trace_costs(trace: &dyn Fn(String), sources: &SourceMap, costs: &[Cost]) {
    let main = sources.file_at(0).name();
    let mut rows: std::collections::BTreeMap<usize, CostRow> = std::collections::BTreeMap::new();
    for cost in costs {
        let row = match cost.span {
            Some(span) => {
                let file = sources.file_at(span.start);
                let (line, _) = file.line_col(span.start);
                let position = file.line_end(line);
                rows.entry(position).or_insert_with(|| CostRow {
                    position,
                    line: if file.name() == main {
                        line.to_string()
                    } else {
                        format!("{}:{line}", file.name())
                    },
                    source: file
                        .line_text(line)
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" "),
                    cost: Cost::default(),
                })
            }
            None => rows.entry(usize::MAX).or_insert_with(|| CostRow {
                position: usize::MAX,
                line: String::new(),
                source: "(implicit: divisions and targets only the search needs)".to_string(),
                cost: Cost::default(),
            }),
        };
        add_cost(&mut row.cost, cost);
    }
    let mut rows: Vec<CostRow> = rows
        .into_values()
        .filter(|row| row.cost != Cost::default())
        .collect();
    let mut total = Cost::default();
    for row in &rows {
        add_cost(&mut total, &row.cost);
    }
    if rows.len() > TRACED_COST_LINES {
        // Keep the costliest lines, in program order, and sum up the rest.
        rows.sort_by_key(|row| std::cmp::Reverse(row.cost.clauses + row.cost.gates));
        let rest = rows.split_off(TRACED_COST_LINES - 1);
        let mut summary = CostRow {
            position: usize::MAX - 1,
            line: String::new(),
            source: format!("… {} more lines", rest.len()),
            cost: Cost::default(),
        };
        for row in &rest {
            add_cost(&mut summary.cost, &row.cost);
        }
        rows.push(summary);
        rows.sort_by_key(|row| row.position);
    }

    trace(
        "  cost       by source line (shared circuitry counts at the first line needing it)"
            .to_string(),
    );
    trace(format!(
        "             {:>6}  {:>8}  {:>6}  {:>9}  {:>10}  {:>10}  {:>5}  source",
        "line", "unknowns", "times", "inputs", "AND gates", "clauses", "share"
    ));
    let count = |n: usize| if n == 0 { String::new() } else { grouped(n) };
    let mut any_free = false;
    for row in &rows {
        let free = row.cost.requirements > 0
            && row.cost.free == row.cost.requirements
            && row.cost.clauses == 0;
        any_free |= free;
        let share = if free {
            "free".to_string()
        } else if total.clauses == 0 {
            String::new()
        } else {
            #[allow(clippy::cast_precision_loss)]
            let percent = row.cost.clauses as f64 * 100.0 / total.clauses as f64;
            format!("{percent:.0}%")
        };
        let times = if row.cost.requirements > 1 {
            format!("×{}", row.cost.requirements)
        } else {
            String::new()
        };
        trace(format!(
            "             {:>6}  {:>8}  {:>6}  {:>9}  {:>10}  {:>10}  {:>5}  {}",
            row.line,
            count(row.cost.unknowns),
            times,
            grouped(row.cost.inputs),
            grouped(row.cost.gates),
            grouped(row.cost.clauses),
            share,
            truncated(&row.source, 60)
        ));
    }
    if any_free {
        trace(
            "             free: folded into the inferred ranges (it may narrow the unknowns), so it needs no circuit".to_string(),
        );
    }
}

/// `1234567` as `1,234,567`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// `text`, cut to `width` characters with an ellipsis.
fn truncated(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(width - 1).collect::<String>())
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn describe_mode(request: &SolveRequest<'_>) -> String {
    let mut mode = match request.mode {
        SolveMode::First => "first",
        SolveMode::All => "all",
        SolveMode::Unique => "unique",
    }
    .to_string();
    if let Some(limit) = request.limit {
        mode += &format!(", limit {limit}");
    }
    match request.objective {
        Some((Objective::Minimize, _)) => mode += ", minimize",
        Some((Objective::Maximize, _)) => mode += ", maximize",
        None => {}
    }
    mode
}

type Callback<'c> = dyn FnMut(&[Constant]) -> Result<(), Diagnostic> + 'c;

/// Runs a `solve`: calls `on_solution` with the value of every term, once per reported solution.
pub(crate) fn solve(
    store: &TermStore,
    request: &SolveRequest<'_>,
    options: &CompilerOptions,
    factory: &SolverFactory,
    on_solution: &mut Callback<'_>,
) -> Result<(SolveOutcome, SolveStats, Vec<Diagnostic>), Diagnostic> {
    let start = Instant::now();
    let mut stats = SolveStats {
        span: request.span,
        ..SolveStats::default()
    };
    let trace = |line: String| {
        if let Some(trace) = request.trace {
            trace(&line);
        }
    };
    trace(format!(
        "  model      {} · {} requirement{} · {} IR terms",
        describe_mode(request),
        request.constraints.len(),
        plural(request.constraints.len()),
        store.len()
    ));
    let mut warnings = Vec::new();
    let required: Vec<TermId> = request.constraints.iter().map(|c| c.term).collect();

    let inferring = Instant::now();
    let propagated = domain::propagate(store, &required);
    stats.domains = inferring.elapsed();
    let Some(ranges) = propagated else {
        trace(format!(
            "  domains    contradictory after {:.2?}: no solution",
            stats.domains
        ));
        let conflict = explain_if_cheap(store, request, options, factory, start);
        stats.elapsed = start.elapsed();
        return Ok((
            SolveOutcome::NoSolution {
                searched: None,
                conflict,
            },
            stats,
            warnings,
        ));
    };

    // Solve targets that no requirement (or objective) mentions can take any value of their
    // domain.
    let mut mentioned = required.clone();
    mentioned.extend(request.objective.map(|(_, term)| term));
    let constrained: std::collections::HashSet<usize> =
        relevant_unknowns(store, &mentioned).into_iter().collect();
    for &target in &request.projection {
        let unknowns = relevant_unknowns(store, &[target]);
        if let Some(&first) = unknowns.first() {
            if unknowns
                .iter()
                .all(|unknown| !constrained.contains(unknown))
            {
                let unknown = &store.unknowns()[first];
                warnings.push(
                    Diagnostic::warning(
                        unknown.span,
                        format!(
                            "`{}` is unconstrained: no requirement mentions it",
                            unknown.name
                        ),
                    )
                    .with_note(
                        "any value in its domain is a solution, so the value shown is arbitrary",
                    ),
                );
            }
        }
    }

    let mut roots = required.clone();
    roots.extend(&request.projection);
    roots.extend(request.objective.map(|(_, term)| term));
    let relevant = relevant_unknowns(store, &roots);

    let mut magnitude = options.fallback_int_bits;
    loop {
        let (domains, unbounded) = final_domains(store, &ranges, &relevant, magnitude);
        if !unbounded.is_empty() && options.unbounded_ints == UnboundedInts::Error {
            return Err(unbounded_error(store, unbounded[0]));
        }
        let names: Vec<String> = unbounded
            .iter()
            .map(|&u| store.unknowns()[u].name.clone())
            .collect();
        stats.magnitude_bits = (!unbounded.is_empty()).then_some(magnitude);
        measure_unknowns(&mut stats, store, &relevant, &domains);
        if request.trace.is_some() {
            trace_unknowns(&trace, &stats, &names, magnitude);
        }

        let building = Instant::now();
        let mut attempt = Attempt::build(
            store,
            request.constraints,
            &required,
            &roots,
            &domains,
            factory,
        );
        let objective_range = request
            .objective
            .map_or_else(Interval::full, |(_, term)| ranges[term.index()].clone());
        stats.build += building.elapsed();
        trace(format!(
            "  circuit    {} AND gates over {} inputs → CNF {} variables ({} auxiliary), {} clauses · built in {:.2?}",
            attempt.cs.aig().num_ands(),
            attempt.cs.aig().inputs().len(),
            attempt.cs.solver().num_vars(),
            (attempt.cs.solver().num_vars() as usize).saturating_sub(stats.decision_bits),
            attempt.cs.solver().num_clauses(),
            stats.build
        ));
        if request.trace.is_some() {
            trace_costs(&trace, request.sources, &attempt.costs);
        }
        let parallel = parallel_enumeration(request, &attempt);
        match parallel {
            Some((parallel, cubes)) => trace(format!(
                "  search     on {} threads, enumerating {cubes} cubes of the solve targets in parallel…",
                parallel.workers
            )),
            None => trace(format!("  search     with {}…", attempt.cs.solver().name())),
        }
        let searching = Instant::now();
        let calls_before = stats.sat_calls;
        let outcome = match parallel {
            Some((parallel, cubes)) => {
                let context = Workers {
                    store,
                    request,
                    required: &required,
                    roots: &roots,
                    domains: &domains,
                    parallel,
                    cubes,
                };
                enumerate_in_parallel(&context, on_solution).map(|(outcome, calls, solutions)| {
                    attempt.sat_calls += calls;
                    attempt.solutions += solutions;
                    outcome
                })
            }
            None => attempt.run(request, &objective_range, on_solution),
        };
        stats.search += searching.elapsed();
        attempt.record(&mut stats);
        // A solver stopped on request gives up; say why.
        let outcome = outcome.map_err(|error| {
            if request.cancel.is_some_and(|cancel| cancel()) {
                Diagnostic::error(error.span, crate::STOPPED)
            } else {
                error
            }
        })?;
        trace(format!(
            "  searched   {} SAT call{} · {} solution{} · {:.2?} (CNF grew to {} variables, {} clauses)",
            stats.sat_calls - calls_before,
            plural((stats.sat_calls - calls_before) as usize),
            attempt.solutions,
            plural(attempt.solutions as usize),
            searching.elapsed(),
            stats.cnf_vars,
            stats.cnf_clauses
        ));
        let can_deepen =
            !unbounded.is_empty() && magnitude.saturating_mul(2) <= options.max_int_bits;
        match outcome {
            SolveOutcome::NoSolution { .. } if can_deepen => {
                trace(format!(
                    "  deepening  no solution with |x| < 2^{magnitude} for the unbounded unknowns; retrying with 2^{}",
                    magnitude * 2
                ));
                magnitude *= 2;
            }
            outcome => {
                if !unbounded.is_empty() {
                    warnings.push(
                        Diagnostic::warning(
                            store.unknowns()[unbounded[0]].span,
                            format!(
                                "could not infer bounds for {}; searched |value| < 2^{magnitude}",
                                names
                                    .iter()
                                    .map(|n| format!("`{n}`"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        )
                        .with_help("add bounds (e.g. `int[0..=1000]`) for a complete search"),
                    );
                }
                let outcome = match outcome {
                    SolveOutcome::NoSolution { .. } if !unbounded.is_empty() => {
                        SolveOutcome::NoSolution {
                            searched: Some((magnitude, names)),
                            conflict: None,
                        }
                    }
                    SolveOutcome::NoSolution { searched: None, .. } => {
                        let conflict = explain_if_cheap(store, request, options, factory, start);
                        SolveOutcome::NoSolution {
                            searched: None,
                            conflict,
                        }
                    }
                    other => other,
                };
                stats.elapsed = start.elapsed();
                return Ok((outcome, stats, warnings));
            }
        }
    }
}

/// The error for an unbounded integer under `unbounded-ints = "error"`.
fn unbounded_error(store: &TermStore, index: usize) -> Diagnostic {
    let unknown = &store.unknowns()[index];
    Diagnostic::error(
        unknown.span,
        format!("cannot infer a finite domain for `{}`", unknown.name),
    )
    .with_label("unbounded")
    .with_note("required by the compiler option unbounded-ints = \"error\"")
    .with_help(format!(
        "give it bounds, e.g. `var {}: int[0..=1000]`, or a constraint that limits it",
        unknown.name
    ))
}

/// The domains a `solve` would use, without solving: for editor tooling. Returns the domain of
/// every relevant unknown (by index) and diagnostics about unbounded unknowns.
pub(crate) fn infer_domains(
    store: &TermStore,
    constraints: &[Constraint],
    projection: &[TermId],
    objective: Option<TermId>,
    options: &CompilerOptions,
) -> (Vec<(usize, Interval)>, Vec<Diagnostic>) {
    let required: Vec<TermId> = constraints.iter().map(|c| c.term).collect();
    let Some(ranges) = domain::propagate(store, &required) else {
        return (Vec::new(), Vec::new());
    };
    let mut roots = required;
    roots.extend(projection);
    roots.extend(objective);
    let relevant = relevant_unknowns(store, &roots);
    let (domains, unbounded) = final_domains(store, &ranges, &relevant, options.fallback_int_bits);
    let mut diagnostics = Vec::new();
    if let Some(&first) = unbounded.first() {
        diagnostics.push(if options.unbounded_ints == UnboundedInts::Error {
            unbounded_error(store, first)
        } else {
            let names: Vec<String> = unbounded
                .iter()
                .map(|&u| format!("`{}`", store.unknowns()[u].name))
                .collect();
            Diagnostic::warning(
                store.unknowns()[first].span,
                format!("could not infer bounds for {}", names.join(", ")),
            )
            .with_help("add bounds (e.g. `int[0..=1000]`) for a complete search")
        });
    }
    let inferred = relevant
        .into_iter()
        .map(|index| (index, domains[index].clone()))
        .collect();
    (inferred, diagnostics)
}

/// The unknowns (by index) that the roots depend on.
fn relevant_unknowns(store: &TermStore, roots: &[TermId]) -> Vec<usize> {
    let mut seen = vec![false; store.len()];
    let mut pending = roots.to_vec();
    let mut unknowns = Vec::new();
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut seen[id.index()], true) {
            continue;
        }
        if let Term::Unknown(unknown) = store.term(id) {
            unknowns.push(unknown.index());
        }
        pending.extend(store.term(id).operands());
    }
    unknowns.sort_unstable();
    unknowns
}

/// Each unknown's domain for this attempt: the propagated one, with infinite ends of relevant
/// integer unknowns capped at `±2^magnitude`. Also returns which unknowns needed the cap.
fn final_domains(
    store: &TermStore,
    ranges: &[Interval],
    relevant: &[usize],
    magnitude: u32,
) -> (Vec<Interval>, Vec<usize>) {
    let limit = BigInt::one() << magnitude;
    let cap = Interval::new(-limit.clone(), limit - 1);
    let mut unbounded = Vec::new();
    let domains = store
        .unknowns()
        .iter()
        .enumerate()
        .map(|(index, unknown)| {
            let range = ranges[unknown.term.index()].clone();
            if unknown.sort == Sort::Int
                && range.bounds().is_none()
                && relevant.binary_search(&index).is_ok()
            {
                unbounded.push(index);
                range.intersect(&cap)
            } else {
                range
            }
        })
        .collect();
    (domains, unbounded)
}

/// One circuit and solver for a fixed choice of domains.
struct Attempt<'a> {
    store: &'a TermStore,
    domains: &'a [Interval],
    constraints: &'a [Constraint],
    cs: CircuitSolver<Box<dyn Solver>>,
    blasted: Blasted,
    sat_calls: u32,
    solutions: u64,
    /// What each part of the program added to the circuit, for `--verbose`.
    costs: Vec<Cost>,
}

/// What one part of the program added to the circuit and its CNF: an unknown (its bits), a
/// requirement, or the solver's implicit side conditions. Circuitry shared by several parts
/// counts for the first one that needs it, in program order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cost {
    /// Where it comes from; `None` for implicit side conditions (e.g. of divisions) and for
    /// circuitry only the solve targets or objective need.
    pub span: Option<Span>,
    pub unknowns: usize,
    pub requirements: usize,
    /// Requirements the inferred ranges decide (always true): they cost nothing.
    pub free: usize,
    /// New circuit inputs: an unknown's bits, or auxiliary bits such as a division's quotient.
    pub inputs: usize,
    pub gates: usize,
    pub clauses: usize,
}

/// How many cubes each worker should get, on average, so that the work stays balanced.
const CUBES_PER_WORKER: usize = 8;

/// The parallel enumeration settings and the number of cubes, when `request` asks for
/// `solve all` and may enumerate in parallel: no objective (enumeration follows the
/// optimization), no `limit` (reporting a cube means finishing it, proving it has no more
/// solutions, which a limited search avoids: that proof can be far harder than the solutions,
/// as with Pell's equation), not interactive and not cancellable (both are about one solver),
/// at least two workers, and solve targets with bits to split on.
fn parallel_enumeration<'a>(
    request: &SolveRequest<'a>,
    attempt: &Attempt<'_>,
) -> Option<(&'a crate::Parallel, usize)> {
    let parallel = request.parallel?;
    let eligible = parallel.workers >= 2
        && request.mode == SolveMode::All
        && request.limit.is_none()
        && request.objective.is_none()
        && request.ask.is_none()
        && request.cancel.is_none();
    if !eligible {
        return None;
    }
    let free = cube_edges(attempt, &request.projection).len();
    let wanted = (parallel.workers * CUBES_PER_WORKER)
        .next_power_of_two()
        .trailing_zeros();
    let bits = free.min(usize::try_from(wanted).unwrap_or(0));
    (bits > 0).then_some((parallel, 1 << bits))
}

/// The bits of the solve targets that cubes fix, in order (the constant ones excluded).
fn cube_edges(attempt: &Attempt<'_>, projection: &[TermId]) -> Vec<Edge> {
    projection
        .iter()
        .flat_map(|&term| attempt.blasted.of(term).edges())
        .filter(|edge| edge.as_constant().is_none())
        .collect()
}

/// What the workers of a parallel enumeration share.
struct Workers<'a, 'r> {
    store: &'a TermStore,
    request: &'a SolveRequest<'r>,
    required: &'a [TermId],
    roots: &'a [TermId],
    domains: &'a [Interval],
    parallel: &'a crate::Parallel,
    /// A power of two: cube `c` fixes the first `log2(cubes)` target bits to the bits of `c`.
    cubes: usize,
}

/// What a worker tells the thread that reports solutions.
enum Found {
    Solution(usize, Vec<Constant>),
    CubeDone(usize),
    Failed(Diagnostic),
    Finished { sat_calls: u32 },
}

/// `solve all` by cube-and-conquer. Each worker builds its own circuit and solver, then takes
/// cubes from a shared counter and enumerates each cube's solutions under assumptions (so one
/// solver serves all its cubes; blocking clauses only ever exclude solutions it reported).
/// Solutions are reported cube by cube, in cube order, each cube's in order of the targets'
/// values: which worker takes a cube, and so the order its solver finds them in, varies from
/// run to run, but the output does not. Returns the outcome, the SAT calls made and the
/// solutions reported.
fn enumerate_in_parallel(
    context: &Workers<'_, '_>,
    on_solution: &mut Callback<'_>,
) -> Result<(SolveOutcome, u32, u64), Diagnostic> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Mutex, mpsc};

    let request = context.request;
    let bits = context.cubes.trailing_zeros() as usize;
    let stop = AtomicBool::new(false);
    let next_cube = AtomicUsize::new(0);
    // One slot per worker, holding its solver's interrupter while the solver exists: a worker
    // empties its slot (under the lock) before dropping its solver, so stopping never calls an
    // interrupter of a dropped solver.
    let interrupters: Mutex<Vec<Option<redsat_sat::Interrupter>>> =
        Mutex::new((0..context.parallel.workers).map(|_| None).collect());
    let (sender, receiver) = mpsc::channel::<Found>();

    std::thread::scope(|scope| {
        for worker in 0..context.parallel.workers {
            let sender = sender.clone();
            let (stop, next_cube, interrupters) = (&stop, &next_cube, &interrupters);
            scope.spawn(move || {
                let mut attempt = Attempt::build(
                    context.store,
                    request.constraints,
                    context.required,
                    context.roots,
                    context.domains,
                    &*context.parallel.solver,
                );
                interrupters.lock().expect("not poisoned")[worker] =
                    attempt.cs.solver_mut().interrupter();
                let edges = cube_edges(&attempt, &request.projection);
                'cubes: while !stop.load(Ordering::Relaxed) {
                    let cube = next_cube.fetch_add(1, Ordering::Relaxed);
                    if cube >= context.cubes {
                        break;
                    }
                    let assumptions: Vec<Edge> = (0..bits)
                        .map(|bit| edges[bit].negate_if(cube >> bit & 1 == 0))
                        .collect();
                    while !stop.load(Ordering::Relaxed) {
                        match attempt.next_model(&assumptions) {
                            Ok(Some(model)) => {
                                attempt.block(&request.projection);
                                if sender.send(Found::Solution(cube, model)).is_err() {
                                    break 'cubes;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                if !stop.load(Ordering::Relaxed) {
                                    let _ = sender.send(Found::Failed(error));
                                }
                                break 'cubes;
                            }
                        }
                    }
                    if !stop.load(Ordering::Relaxed) {
                        let _ = sender.send(Found::CubeDone(cube));
                    }
                }
                interrupters.lock().expect("not poisoned")[worker] = None;
                let _ = sender.send(Found::Finished {
                    sat_calls: attempt.sat_calls,
                });
                drop(attempt);
            });
        }
        drop(sender);

        // Report in cube order. `pending[c]` holds cube c's solutions not reported yet.
        let mut pending: Vec<std::collections::VecDeque<Vec<Constant>>> =
            vec![std::collections::VecDeque::new(); context.cubes];
        let mut done = vec![false; context.cubes];
        let (mut current, mut reported, mut sat_calls) = (0, 0u64, 0u32);
        let mut failure = None;
        let halt = |stop: &AtomicBool| {
            stop.store(true, Ordering::Relaxed);
            for interrupt in interrupters.lock().expect("not poisoned").iter().flatten() {
                interrupt();
            }
        };
        for found in &receiver {
            match found {
                Found::Solution(cube, model) => pending[cube].push_back(model),
                Found::CubeDone(cube) => done[cube] = true,
                Found::Failed(error) => {
                    failure.get_or_insert(error);
                    halt(&stop);
                }
                Found::Finished { sat_calls: calls } => sat_calls += calls,
            }
            while failure.is_none() && current < context.cubes && done[current] {
                let mut solutions: Vec<Vec<Constant>> = pending[current].drain(..).collect();
                solutions.sort_by_cached_key(|model| {
                    request
                        .projection
                        .iter()
                        .map(|term| model[term.index()].clone())
                        .collect::<Vec<_>>()
                });
                for model in solutions {
                    if request.limit.is_some_and(|limit| reported >= limit) {
                        break;
                    }
                    reported += 1;
                    if let Err(error) = on_solution(&model) {
                        failure = Some(error);
                        halt(&stop);
                        break;
                    }
                }
                current += 1;
            }
            let finished =
                current == context.cubes || request.limit.is_some_and(|limit| reported >= limit);
            if finished && !stop.load(Ordering::Relaxed) {
                halt(&stop);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        let outcome = if reported == 0 {
            SolveOutcome::NoSolution {
                searched: None,
                conflict: None,
            }
        } else {
            SolveOutcome::Solved
        };
        Ok((outcome, sat_calls, reported))
    })
}

/// Marks the not yet `claimed` nodes of the cones of `edges` as claimed and counts them:
/// (AND gates, inputs).
fn claim(aig: &Aig, edges: &[Edge], claimed: &mut [bool]) -> (usize, usize) {
    let (mut gates, mut inputs) = (0, 0);
    let mut pending: Vec<_> = edges.iter().map(|edge| edge.node()).collect();
    while let Some(node) = pending.pop() {
        if std::mem::replace(&mut claimed[node.index()], true) {
            continue;
        }
        match aig.node(node) {
            Node::And(a, b) => {
                gates += 1;
                pending.extend([a.node(), b.node()]);
            }
            Node::Input(_) => inputs += 1,
            Node::False => {}
        }
    }
    (gates, inputs)
}

/// The edges of the circuits of `terms` (the bits of integers).
fn edges_of(blasted: &Blasted, terms: &[TermId]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for bits in terms.iter().filter_map(|&term| blasted.get(term)) {
        match bits {
            Bits::Bool(edge) => edges.push(*edge),
            Bits::Int(int) => edges.extend_from_slice(int.bits()),
        }
    }
    edges
}

/// Encodes the requirements, then the side constraints, into `cs` (always in this order: the
/// order of clauses steers the solver), measuring what each part of the program adds. Each
/// part is charged the circuitry that no earlier part needed: first come, first charged.
///
/// - An unknown is charged its bits, and the side constraints keeping it in its domain.
/// - A requirement is charged its expression's circuitry, and the definitions of the divisions
///   in that expression (`x % m` is defined by a quotient times `m`).
/// - What only the solve targets or the objective need is left over, in a row of its own (it
///   is encoded later, as the search asks about it).
///
/// The encoding is lazy (only gates not encoded yet produce clauses), so measuring around each
/// `require` gives each part's clauses.
fn encode_by_part(
    store: &TermStore,
    constraints: &[Constraint],
    required: &[TermId],
    roots: &[TermId],
    blasted: &Blasted,
    cs: &mut CircuitSolver<Box<dyn Solver>>,
) -> Vec<Cost> {
    debug_assert_eq!(required.len(), constraints.len());
    let mut claimed = vec![false; cs.aig().num_nodes()];
    // Claims `edges`, requires them and returns what that added: (inputs, gates, clauses).
    let encode = |cs: &mut CircuitSolver<Box<dyn Solver>>, edges: &[Edge], claimed: &mut [bool]| {
        let (gates, inputs) = claim(cs.aig(), edges, claimed);
        let before = cs.solver().num_clauses();
        for &edge in edges {
            cs.require(edge);
        }
        (inputs, gates, cs.solver().num_clauses() - before)
    };
    let charge = |cost: &mut Cost, (inputs, gates, clauses): (usize, usize, usize)| {
        cost.inputs += inputs;
        cost.gates += gates;
        cost.clauses += clauses;
    };

    // Unknowns: their bits.
    let mut costs = Vec::new();
    let mut row_of_unknown = vec![None; store.unknowns().len()];
    for (index, unknown) in store.unknowns().iter().enumerate() {
        let bits = match blasted.get(unknown.term) {
            Some(Bits::Bool(edge)) => std::slice::from_ref(edge),
            Some(Bits::Int(int)) => int.bits(),
            None => continue,
        };
        let (_, inputs) = claim(cs.aig(), bits, &mut claimed);
        row_of_unknown[index] = Some(costs.len());
        costs.push(Cost {
            span: Some(unknown.span),
            unknowns: 1,
            inputs,
            ..Cost::default()
        });
    }

    // Requirements, in program order. Each term belongs to the first requirement needing it.
    let mut owner: Vec<Option<usize>> = vec![None; store.len()];
    let mut row_of_requirement = Vec::with_capacity(required.len());
    for (index, (&term, constraint)) in required.iter().zip(constraints).enumerate() {
        let mut pending = vec![term];
        while let Some(id) = pending.pop() {
            if owner[id.index()].is_none() {
                owner[id.index()] = Some(index);
                pending.extend(store.term(id).operands());
            }
        }
        let edge = blasted.of(term).edge();
        let explicit = usize::from(constraint.explicit);
        let mut cost = Cost {
            span: Some(constraint.site),
            requirements: explicit,
            free: explicit * usize::from(edge == Edge::TRUE),
            ..Cost::default()
        };
        charge(&mut cost, encode(cs, &[edge], &mut claimed));
        row_of_requirement.push(costs.len());
        costs.push(cost);
    }

    // Side constraints, charged to the unknown or requirement that needs them.
    let leftover = costs.len();
    costs.push(Cost::default());
    for &(origin, edge) in &blasted.side_constraints {
        let row = match store.term(origin) {
            Term::Unknown(unknown) => row_of_unknown[unknown.index()],
            _ => owner[origin.index()].map(|index| row_of_requirement[index]),
        };
        let added = encode(cs, &[edge], &mut claimed);
        charge(&mut costs[row.unwrap_or(leftover)], added);
    }
    // Whatever only the targets or the objective need.
    let (gates, inputs) = claim(cs.aig(), &edges_of(blasted, roots), &mut claimed);
    charge(&mut costs[leftover], (inputs, gates, 0));
    costs
}

impl<'a> Attempt<'a> {
    fn build(
        store: &'a TermStore,
        constraints: &'a [Constraint],
        required: &[TermId],
        roots: &[TermId],
        domains: &'a [Interval],
        factory: &SolverFactory,
    ) -> Self {
        let sizes = domain::forward_sizes(store, domains);
        let mut cs = CircuitSolver::new(factory());
        let blasted = blast::blast(store, &sizes, domains, roots, cs.aig_mut());

        let costs = encode_by_part(store, constraints, required, roots, &blasted, &mut cs);
        Self {
            store,
            domains,
            constraints,
            cs,
            blasted,
            sat_calls: 0,
            solutions: 0,
            costs,
        }
    }

    fn record(&self, stats: &mut SolveStats) {
        stats.aig_inputs = self.cs.aig().inputs().len();
        stats.solver = self.cs.solver().name().to_string();
        stats.and_gates = self.cs.aig().num_ands();
        stats.cnf_vars = self.cs.solver().num_vars();
        stats.cnf_clauses = self.cs.solver().num_clauses();
        stats.sat_calls += self.sat_calls;
        stats.solutions += self.solutions;
    }

    /// Solves (under `assumptions`) and decodes the model into a value for every term.
    fn next_model(&mut self, assumptions: &[Edge]) -> Result<Option<Vec<Constant>>, Diagnostic> {
        self.sat_calls += 1;
        match self.cs.solve_assuming(assumptions) {
            SolveResult::Sat => Ok(Some(self.decode()?)),
            SolveResult::Unsat => Ok(None),
            SolveResult::Unknown => Err(Diagnostic::error(
                self.constraints
                    .first()
                    .map_or_else(Span::default, |c| c.span),
                "the SAT solver gave up",
            )),
        }
    }

    fn decode(&self) -> Result<Vec<Constant>, Diagnostic> {
        let unknown_values: Vec<Constant> = self
            .store
            .unknowns()
            .iter()
            .enumerate()
            .map(|(index, unknown)| match self.blasted.get(unknown.term) {
                Some(bits) => blast::decode(bits, |edge| self.cs.value(edge)),
                None => blast::default_value(unknown.sort, &self.domains[index]),
            })
            .collect();
        let values = self.store.evaluate(&unknown_values);
        // Self-check: the decoded answer must satisfy every constraint under the language's own
        // semantics. A failure here is a compiler bug, never a property of the program.
        if let Some(violated) = self
            .constraints
            .iter()
            .find(|c| !values[c.term.index()].as_bool())
        {
            return Err(Diagnostic::error(
                violated.span,
                "internal error: the solver's answer violates this constraint",
            )
            .with_note("this is a bug in the RedSAT compiler; please report it"));
        }
        Ok(values)
    }

    fn block(&mut self, projection: &[TermId]) {
        let edges: Vec<Edge> = projection
            .iter()
            .flat_map(|&term| self.blasted.of(term).edges())
            .collect();
        self.cs.block(&edges);
    }

    fn report(
        &mut self,
        model: &[Constant],
        on_solution: &mut Callback<'_>,
    ) -> Result<(), Diagnostic> {
        self.solutions += 1;
        on_solution(model)
    }

    fn run(
        &mut self,
        request: &SolveRequest<'_>,
        objective_range: &Interval,
        on_solution: &mut Callback<'_>,
    ) -> Result<SolveOutcome, Diagnostic> {
        let enumerate =
            request.mode == SolveMode::All || request.limit.is_some() || request.ask.is_some();
        if let Some((direction, objective)) = request.objective {
            let Some(optimal) = self.optimize(direction, objective, objective_range)? else {
                return Ok(SolveOutcome::NoSolution {
                    searched: None,
                    conflict: None,
                });
            };
            if request.mode == SolveMode::First && !enumerate {
                self.report(&optimal, on_solution)?;
                return Ok(SolveOutcome::Solved);
            }
            // Otherwise the objective is now fixed to its optimum, and the mode below picks among
            // the optimal solutions.
        }

        if request.mode == SolveMode::Unique {
            let Some(first) = self.next_model(&[])? else {
                return Ok(SolveOutcome::NoSolution {
                    searched: None,
                    conflict: None,
                });
            };
            self.block(&request.projection);
            if let Some(second) = self.next_model(&[])? {
                return Ok(SolveOutcome::NotUnique { first, second });
            }
            self.report(&first, on_solution)?;
            return Ok(SolveOutcome::Solved);
        }
        if !enumerate {
            let Some(model) = self.next_model(&[])? else {
                return Ok(SolveOutcome::NoSolution {
                    searched: None,
                    conflict: None,
                });
            };
            self.report(&model, on_solution)?;
            return Ok(SolveOutcome::Solved);
        }

        let mut ask = request.ask;
        let mut count = 0u64;
        while request.limit.is_none_or(|limit| count < limit) {
            let Some(model) = self.next_model(&[])? else {
                break;
            };
            self.report(&model, on_solution)?;
            count += 1;
            self.block(&request.projection);
            if let Some(asker) = ask {
                match asker(count) {
                    Answer::Next => {}
                    Answer::All => ask = None,
                    Answer::Quit => break,
                }
            }
        }
        Ok(if count == 0 {
            SolveOutcome::NoSolution {
                searched: None,
                conflict: None,
            }
        } else {
            SolveOutcome::Solved
        })
    }

    /// Finds an optimal model by binary search between the best value found so far and the
    /// bound on the objective, then fixes the objective to its optimum so that the requested
    /// mode (first/all/unique) applies among optimal solutions only.
    fn optimize(
        &mut self,
        direction: Objective,
        objective: TermId,
        range: &Interval,
    ) -> Result<Option<Vec<Constant>>, Diagnostic> {
        let Some(mut best) = self.next_model(&[])? else {
            return Ok(None);
        };
        let target = self.blasted.of(objective).int().clone();
        let layout = target.layout();
        let mut lo = layout.min();
        let mut hi = layout.max();
        if let Ext::Fin(bound) = &range.lo {
            lo = lo.max(bound.clone());
        }
        if let Ext::Fin(bound) = &range.hi {
            hi = hi.min(bound.clone());
        }
        loop {
            let value = best[objective.index()].as_int().clone();
            let (mid, probe) = match direction {
                Objective::Minimize => {
                    if lo >= value {
                        break;
                    }
                    let mid: BigInt = &lo + (&value - 1 - &lo) / 2;
                    let probe = int::le(self.cs.aig_mut(), &target, &IntVec::constant(&mid));
                    (mid, probe)
                }
                Objective::Maximize => {
                    if hi <= value {
                        break;
                    }
                    let mid: BigInt = &value + 1 + (&hi - &value - 1) / 2;
                    let probe = int::le(self.cs.aig_mut(), &IntVec::constant(&mid), &target);
                    (mid, probe)
                }
            };
            match self.next_model(&[probe])? {
                Some(model) => best = model,
                None => match direction {
                    Objective::Minimize => lo = mid + 1,
                    Objective::Maximize => hi = mid - 1,
                },
            }
        }
        let value = best[objective.index()].as_int().clone();
        let optimal = int::eq(self.cs.aig_mut(), &target, &IntVec::constant(&value));
        self.cs.require(optimal);
        Ok(Some(best))
    }
}

/// Explains a proven "no solution" when that is likely to be quick: only if it was asked for and
/// the original proof took less than [`EXPLAIN_IF_FASTER_THAN`].
fn explain_if_cheap(
    store: &TermStore,
    request: &SolveRequest<'_>,
    options: &CompilerOptions,
    factory: &SolverFactory,
    start: Instant,
) -> Option<Vec<Span>> {
    if !request.explain || start.elapsed() > EXPLAIN_IF_FASTER_THAN {
        return None;
    }
    explain(store, request.constraints, options, factory)
}

/// Explaining stops shrinking the explanation after this long.
const EXPLAIN_BUDGET: Duration = Duration::from_secs(5);
/// Proofs slower than this are not explained (explaining repeats the proof several times).
const EXPLAIN_IF_FASTER_THAN: Duration = Duration::from_secs(10);

/// Finds a small set of requirements that cannot hold together. Each source location gets a
/// selector that is assumed true; the solver's unsat core names the locations involved, and
/// deletion-based minimization drops the ones that are not needed.
///
/// Unknowns use their *declared* domains here: the propagated domains already encode the
/// requirements, so dropping a requirement would not relax them.
fn explain(
    store: &TermStore,
    constraints: &[Constraint],
    options: &CompilerOptions,
    factory: &SolverFactory,
) -> Option<Vec<Span>> {
    let started = Instant::now();
    let mut groups: Vec<(Span, Vec<TermId>)> = Vec::new();
    for constraint in constraints {
        match groups.iter_mut().find(|(span, _)| *span == constraint.span) {
            Some((_, terms)) => terms.push(constraint.term),
            None => groups.push((constraint.span, vec![constraint.term])),
        }
    }
    let roots: Vec<TermId> = constraints.iter().map(|c| c.term).collect();
    let relevant = relevant_unknowns(store, &roots);
    let mut declared = vec![Interval::full(); store.len()];
    for unknown in store.unknowns() {
        declared[unknown.term.index()] = unknown.domain.clone();
    }
    let (domains, _) = final_domains(store, &declared, &relevant, options.fallback_int_bits);
    let sizes = domain::forward_sizes(store, &domains);

    let mut cs = CircuitSolver::new(factory());
    let blasted = blast::blast(store, &sizes, &domains, &roots, cs.aig_mut());
    for &(_, edge) in &blasted.side_constraints {
        cs.require(edge);
    }
    let mut selectors = Vec::with_capacity(groups.len());
    for (_, terms) in &groups {
        let selector = cs.aig_mut().new_input();
        for &term in terms {
            let edge = blasted.of(term).edge();
            let guarded = cs.aig_mut().implies(selector, edge);
            cs.require(guarded);
        }
        selectors.push(selector);
    }
    if cs.solve_assuming(&selectors) != SolveResult::Unsat {
        return None;
    }
    let mut core: Vec<usize> = (0..groups.len())
        .filter(|&g| cs.failed(selectors[g]))
        .collect();
    let mut index = 0;
    while index < core.len() && started.elapsed() < EXPLAIN_BUDGET {
        let candidate: Vec<usize> = core
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != index)
            .map(|(_, &g)| g)
            .collect();
        let assumptions: Vec<_> = candidate.iter().map(|&g| selectors[g]).collect();
        if cs.solve_assuming(&assumptions) == SolveResult::Unsat {
            core = candidate
                .into_iter()
                .filter(|&g| cs.failed(selectors[g]))
                .collect();
        } else {
            index += 1;
        }
    }
    Some(core.into_iter().map(|g| groups[g].0).collect())
}
