//! Executing a `solve` statement: domains, circuit, SAT calls, decoding.

use std::time::{Duration, Instant};

use num_bigint::BigInt;
use num_traits::One;
use redsat_circuit::{CircuitSolver, Edge, IntVec, int};
use redsat_sat::{SolveResult, Solver};

use crate::ast::{Objective, SolveMode};
use crate::blast::{self, Blasted};
use crate::config::{CompilerOptions, UnboundedInts};
use crate::diagnostic::Diagnostic;
use crate::domain::{self, Ext, Interval};
use crate::ir::{Constant, Sort, Term, TermId, TermStore};
use crate::source::{SourceFile, Span};
use crate::{Answer, AskFn, SolverFactory};

/// A requirement accumulated by the program.
#[derive(Clone, Debug)]
pub(crate) struct Constraint {
    pub term: TermId,
    pub span: Span,
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
            "  circuit: {} AND gates · CNF: {} variables, {} clauses\n  {} SAT call{} · {} \
             solution{} · {:.2?}\n",
            self.and_gates,
            self.cnf_vars,
            self.cnf_clauses,
            self.sat_calls,
            if self.sat_calls == 1 { "" } else { "s" },
            self.solutions,
            if self.solutions == 1 { "" } else { "s" },
            self.elapsed
        );
        out
    }
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
    let mut warnings = Vec::new();
    let required: Vec<TermId> = request.constraints.iter().map(|c| c.term).collect();

    let Some(ranges) = domain::propagate(store, &required) else {
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
        let outcome = attempt.run(request, &objective_range, on_solution)?;
        attempt.record(&mut stats, store, &relevant, &domains);
        let can_deepen =
            !unbounded.is_empty() && magnitude.saturating_mul(2) <= options.max_int_bits;
        match outcome {
            SolveOutcome::NoSolution { .. } if can_deepen => {
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
        for &term in required {
            cs.require(blasted.of(term).edge());
        }
        for &edge in &blasted.side_constraints {
            cs.require(edge);
        }
        Self {
            store,
            domains,
            constraints,
            cs,
            blasted,
            sat_calls: 0,
            solutions: 0,
        }
    }

    fn record(
        &self,
        stats: &mut SolveStats,
        store: &TermStore,
        relevant: &[usize],
        domains: &[Interval],
    ) {
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
    for &edge in &blasted.side_constraints {
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
