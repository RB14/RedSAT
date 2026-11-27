//! A built-in CDCL (conflict-driven clause learning) SAT solver, in pure Rust.
//!
//! It is much simpler than CaDiCaL, and much slower on hard instances, but it has no native
//! dependencies and shows how a modern solver works. It implements the same incremental
//! [`Solver`] interface, so every part of RedSAT can run on it (`redsat --solver builtin`).
//!
//! The ingredients are the classic ones (as in MiniSat and Glucose):
//! - **two watched literals** per clause, with a *blocker* literal to skip satisfied clauses;
//! - **VSIDS** branching: variables involved in recent conflicts are tried first;
//! - **phase saving**: a variable is re-assigned the value it last had;
//! - **first-UIP conflict analysis** with recursive clause minimisation;
//! - **LBD**-based clause database reduction (keeping "glue" clauses);
//! - **Luby restarts**;
//! - **assumptions** with failed-assumption analysis, for incremental use.

use std::collections::HashSet;

use crate::{ClauseSink, Lit, SolveResult, Solver, Var};

/// The truth value of a literal or variable under the current assignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    True,
    False,
    Unassigned,
}

/// An index into the clause arena.
type ClauseRef = usize;

#[derive(Clone, Debug)]
struct Clause {
    lits: Vec<Lit>,
    learnt: bool,
    /// Literal block distance: the number of decision levels among the literals when learnt.
    lbd: u32,
    deleted: bool,
}

#[derive(Clone, Copy, Debug)]
struct Watch {
    clause: ClauseRef,
    /// Another literal of the clause: if it is true, the clause is satisfied.
    blocker: Lit,
}

/// A max-heap of variables ordered by activity, with positions for decrease/increase-key.
#[derive(Debug, Default)]
struct VarHeap {
    heap: Vec<u32>,
    position: Vec<Option<usize>>,
}

impl VarHeap {
    fn grow(&mut self, num_vars: usize) {
        self.position.resize(num_vars, None);
    }

    fn contains(&self, var: u32) -> bool {
        self.position[var as usize].is_some()
    }

    fn insert(&mut self, var: u32, activity: &[f64]) {
        if self.contains(var) {
            return;
        }
        self.position[var as usize] = Some(self.heap.len());
        self.heap.push(var);
        self.up(self.heap.len() - 1, activity);
    }

    fn pop(&mut self, activity: &[f64]) -> Option<u32> {
        let top = *self.heap.first()?;
        let last = self.heap.pop().expect("non-empty");
        self.position[top as usize] = None;
        if !self.heap.is_empty() {
            self.heap[0] = last;
            self.position[last as usize] = Some(0);
            self.down(0, activity);
        }
        Some(top)
    }

    /// Restores the heap after `var`'s activity increased.
    fn increased(&mut self, var: u32, activity: &[f64]) {
        if let Some(index) = self.position[var as usize] {
            self.up(index, activity);
        }
    }

    fn up(&mut self, mut index: usize, activity: &[f64]) {
        let var = self.heap[index];
        while index > 0 {
            let parent = (index - 1) / 2;
            if activity[self.heap[parent] as usize] >= activity[var as usize] {
                break;
            }
            self.heap[index] = self.heap[parent];
            self.position[self.heap[index] as usize] = Some(index);
            index = parent;
        }
        self.heap[index] = var;
        self.position[var as usize] = Some(index);
    }

    fn down(&mut self, mut index: usize, activity: &[f64]) {
        let var = self.heap[index];
        loop {
            let left = 2 * index + 1;
            if left >= self.heap.len() {
                break;
            }
            let right = left + 1;
            let child = if right < self.heap.len()
                && activity[self.heap[right] as usize] > activity[self.heap[left] as usize]
            {
                right
            } else {
                left
            };
            if activity[self.heap[child] as usize] <= activity[var as usize] {
                break;
            }
            self.heap[index] = self.heap[child];
            self.position[self.heap[index] as usize] = Some(index);
            index = child;
        }
        self.heap[index] = var;
        self.position[var as usize] = Some(index);
    }
}

/// The built-in CDCL solver.
#[derive(Debug)]
pub struct Cdcl {
    num_vars: u32,
    num_clauses: usize,
    clauses: Vec<Clause>,
    /// For each literal code, the clauses watching that literal (visited when it becomes false).
    watches: Vec<Vec<Watch>>,
    values: Vec<Value>,
    level: Vec<u32>,
    reason: Vec<Option<ClauseRef>>,
    trail: Vec<Lit>,
    trail_limits: Vec<usize>,
    propagated: usize,
    activity: Vec<f64>,
    activity_increment: f64,
    heap: VarHeap,
    saved_phase: Vec<bool>,
    seen: Vec<bool>,
    /// False once the clauses are unsatisfiable regardless of assumptions.
    consistent: bool,
    model: Vec<bool>,
    has_model: bool,
    failed: HashSet<Lit>,
    learnt_limit: usize,
    /// The number of learnt clauses currently kept.
    learnts: usize,
    conflicts: u64,
    /// Set by an [`Interrupter`](crate::Interrupter) to stop the running search.
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The phase new variables start with.
    initial_phase: bool,
}

impl Default for Cdcl {
    fn default() -> Self {
        Self::new()
    }
}

const ACTIVITY_DECAY: f64 = 0.95;
const RESTART_UNIT: u64 = 100;

impl Cdcl {
    /// Creates a solver with an empty formula.
    #[must_use]
    pub fn new() -> Self {
        Self {
            num_vars: 0,
            num_clauses: 0,
            clauses: Vec::new(),
            watches: Vec::new(),
            values: Vec::new(),
            level: Vec::new(),
            reason: Vec::new(),
            trail: Vec::new(),
            trail_limits: Vec::new(),
            propagated: 0,
            activity: Vec::new(),
            activity_increment: 1.0,
            heap: VarHeap::default(),
            saved_phase: Vec::new(),
            seen: Vec::new(),
            consistent: true,
            model: Vec::new(),
            has_model: false,
            failed: HashSet::new(),
            learnt_limit: 2000,
            learnts: 0,
            conflicts: 0,
            stop: std::sync::Arc::default(),
            initial_phase: false,
        }
    }

    /// A solver that searches differently from [`Cdcl::new`], for portfolios: variable `seed`
    /// picks the initial phase (odd seeds start from true).
    #[must_use]
    pub fn with_seed(seed: u64) -> Self {
        Self {
            initial_phase: seed % 2 == 1,
            ..Self::new()
        }
    }

    /// The number of conflicts encountered so far (a measure of work done).
    #[must_use]
    pub fn conflicts(&self) -> u64 {
        self.conflicts
    }

    fn current(&self, lit: Lit) -> Value {
        match self.values[lit.var().index() as usize] {
            Value::Unassigned => Value::Unassigned,
            Value::True if lit.is_negated() => Value::False,
            Value::False if lit.is_negated() => Value::True,
            value => value,
        }
    }

    fn decision_level(&self) -> u32 {
        u32::try_from(self.trail_limits.len()).expect("few levels")
    }

    fn assign(&mut self, lit: Lit, reason: Option<ClauseRef>) {
        let var = lit.var().index() as usize;
        self.values[var] = if lit.is_negated() {
            Value::False
        } else {
            Value::True
        };
        self.level[var] = self.decision_level();
        self.reason[var] = reason;
        self.trail.push(lit);
    }

    fn backtrack(&mut self, level: u32) {
        if self.decision_level() <= level {
            return;
        }
        let limit = self.trail_limits[level as usize];
        for &lit in &self.trail[limit..] {
            let var = lit.var().index() as usize;
            self.saved_phase[var] = !lit.is_negated();
            self.values[var] = Value::Unassigned;
            self.reason[var] = None;
            self.heap.insert(lit.var().index(), &self.activity);
        }
        self.trail.truncate(limit);
        self.trail_limits.truncate(level as usize);
        self.propagated = self.propagated.min(limit);
    }

    fn attach(&mut self, clause: Clause) -> ClauseRef {
        let reference = self.clauses.len();
        self.watches[(!clause.lits[0]).code() as usize].push(Watch {
            clause: reference,
            blocker: clause.lits[1],
        });
        self.watches[(!clause.lits[1]).code() as usize].push(Watch {
            clause: reference,
            blocker: clause.lits[0],
        });
        self.clauses.push(clause);
        reference
    }

    /// Unit propagation with two watched literals. Returns a conflicting clause, if any.
    fn propagate(&mut self) -> Option<ClauseRef> {
        while self.propagated < self.trail.len() {
            let lit = self.trail[self.propagated];
            self.propagated += 1;
            // Clauses watching `¬lit` are stored under `lit`'s code: `lit` became true, so `¬lit`
            // became false.
            let mut watches = std::mem::take(&mut self.watches[lit.code() as usize]);
            let false_lit = !lit;
            let mut keep = 0;
            let mut conflict = None;
            let mut index = 0;
            while index < watches.len() {
                let watch = watches[index];
                index += 1;
                if self.clauses[watch.clause].deleted {
                    continue;
                }
                if self.current(watch.blocker) == Value::True {
                    watches[keep] = watch;
                    keep += 1;
                    continue;
                }
                let clause = &mut self.clauses[watch.clause];
                if clause.lits[0] == false_lit {
                    clause.lits.swap(0, 1);
                }
                let first = clause.lits[0];
                if first != watch.blocker && self.current(first) == Value::True {
                    watches[keep] = Watch {
                        clause: watch.clause,
                        blocker: first,
                    };
                    keep += 1;
                    continue;
                }
                // Look for a new literal to watch.
                let replacement = (2..self.clauses[watch.clause].lits.len())
                    .find(|&k| self.current(self.clauses[watch.clause].lits[k]) != Value::False);
                if let Some(k) = replacement {
                    let clause = &mut self.clauses[watch.clause];
                    clause.lits.swap(1, k);
                    let new_watch = clause.lits[1];
                    self.watches[(!new_watch).code() as usize].push(Watch {
                        clause: watch.clause,
                        blocker: first,
                    });
                    continue;
                }
                // The clause is unit or conflicting.
                watches[keep] = Watch {
                    clause: watch.clause,
                    blocker: first,
                };
                keep += 1;
                if self.current(first) == Value::False {
                    conflict = Some(watch.clause);
                    while index < watches.len() {
                        watches[keep] = watches[index];
                        keep += 1;
                        index += 1;
                    }
                } else {
                    self.assign(first, Some(watch.clause));
                }
            }
            watches.truncate(keep);
            let pending = std::mem::take(&mut self.watches[lit.code() as usize]);
            watches.extend(pending);
            self.watches[lit.code() as usize] = watches;
            if conflict.is_some() {
                return conflict;
            }
        }
        None
    }

    fn bump(&mut self, var: u32) {
        let index = var as usize;
        self.activity[index] += self.activity_increment;
        if self.activity[index] > 1e100 {
            for activity in &mut self.activity {
                *activity *= 1e-100;
            }
            self.activity_increment *= 1e-100;
        }
        self.heap.increased(var, &self.activity);
    }

    /// First-UIP conflict analysis. Returns the learnt clause (asserting literal first) and the
    /// level to backtrack to.
    fn analyze(&mut self, mut conflict: ClauseRef) -> (Vec<Lit>, u32) {
        let mut learnt = vec![Lit::new(Var::new(0), false)]; // placeholder for the UIP
        let mut pending = 0;
        let mut index = self.trail.len();
        let mut uip = None;
        loop {
            let start = usize::from(uip.is_some());
            let lits = self.clauses[conflict].lits.clone();
            for &lit in &lits[start..] {
                let var = lit.var().index() as usize;
                if self.seen[var] || self.level[var] == 0 {
                    continue;
                }
                self.seen[var] = true;
                self.bump(lit.var().index());
                if self.level[var] == self.decision_level() {
                    pending += 1;
                } else {
                    learnt.push(lit);
                }
            }
            // The next literal of the current level on the trail.
            loop {
                index -= 1;
                if self.seen[self.trail[index].var().index() as usize] {
                    break;
                }
            }
            let lit = self.trail[index];
            self.seen[lit.var().index() as usize] = false;
            pending -= 1;
            if pending == 0 {
                learnt[0] = !lit;
                break;
            }
            uip = Some(lit);
            conflict =
                self.reason[lit.var().index() as usize].expect("implied literals have reasons");
            // Make the implied literal the clause's first literal, so it is skipped above.
            let position = self.clauses[conflict]
                .lits
                .iter()
                .position(|&l| l == lit)
                .expect("reason contains it");
            self.clauses[conflict].lits.swap(0, position);
        }

        // Recursive minimisation: drop literals implied by the rest of the clause.
        let marked: Vec<usize> = learnt.iter().map(|l| l.var().index() as usize).collect();
        for &var in &marked {
            self.seen[var] = true;
        }
        let mut kept = vec![learnt[0]];
        for &lit in &learnt[1..] {
            if !self.redundant(lit) {
                kept.push(lit);
            }
        }
        for &var in &marked {
            self.seen[var] = false;
        }
        let mut learnt = kept;

        // Backtrack to the second-highest level in the clause, and watch that literal.
        let backtrack_level = if learnt.len() == 1 {
            0
        } else {
            let (best, _) = learnt
                .iter()
                .enumerate()
                .skip(1)
                .max_by_key(|(_, l)| self.level[l.var().index() as usize])
                .expect("two literals");
            learnt.swap(1, best);
            self.level[learnt[1].var().index() as usize]
        };
        (learnt, backtrack_level)
    }

    /// Whether `lit` (in the learnt clause) is implied by the other marked literals.
    fn redundant(&mut self, lit: Lit) -> bool {
        let Some(reason) = self.reason[lit.var().index() as usize] else {
            return false;
        };
        let mut stack = vec![reason];
        let mut visited = Vec::new();
        while let Some(clause) = stack.pop() {
            for &other in &self.clauses[clause].lits[1..] {
                let var = other.var().index() as usize;
                if self.seen[var] || self.level[var] == 0 {
                    continue;
                }
                match self.reason[var] {
                    Some(next) if visited.len() < 1000 => {
                        self.seen[var] = true;
                        visited.push(var);
                        stack.push(next);
                    }
                    _ => {
                        for &v in &visited {
                            self.seen[v] = false;
                        }
                        return false;
                    }
                }
            }
        }
        for &v in &visited {
            self.seen[v] = false;
        }
        true
    }

    fn lbd(&self, lits: &[Lit]) -> u32 {
        let levels: HashSet<u32> = lits
            .iter()
            .map(|l| self.level[l.var().index() as usize])
            .collect();
        u32::try_from(levels.len()).unwrap_or(u32::MAX)
    }

    /// Which assumptions forced `¬lit`: fills [`Self::failed`] with the assumptions involved.
    fn analyze_final(&mut self, lit: Lit) {
        self.failed.clear();
        self.failed.insert(!lit);
        if self.decision_level() == 0 {
            return;
        }
        self.seen[lit.var().index() as usize] = true;
        for index in (self.trail_limits[0]..self.trail.len()).rev() {
            let trail_lit = self.trail[index];
            let var = trail_lit.var().index() as usize;
            if !self.seen[var] {
                continue;
            }
            match self.reason[var] {
                None => {
                    if self.level[var] > 0 {
                        self.failed.insert(trail_lit);
                    }
                }
                Some(reason) => {
                    for &other in &self.clauses[reason].lits[1..] {
                        if self.level[other.var().index() as usize] > 0 {
                            self.seen[other.var().index() as usize] = true;
                        }
                    }
                }
            }
            self.seen[var] = false;
        }
        self.seen[lit.var().index() as usize] = false;
    }

    /// Deletes about half of the learnt clauses, keeping glue clauses (LBD ≤ 2) and reasons.
    fn reduce(&mut self) {
        let locked: HashSet<ClauseRef> = self
            .trail
            .iter()
            .filter_map(|l| self.reason[l.var().index() as usize])
            .collect();
        let mut candidates: Vec<ClauseRef> = (0..self.clauses.len())
            .filter(|&c| {
                let clause = &self.clauses[c];
                clause.learnt && !clause.deleted && clause.lbd > 2 && !locked.contains(&c)
            })
            .collect();
        candidates.sort_by_key(|&c| std::cmp::Reverse(self.clauses[c].lbd));
        for &c in &candidates[..candidates.len() / 2] {
            self.clauses[c].deleted = true;
            self.clauses[c].lits = Vec::new();
            self.learnts -= 1;
        }
        for watches in &mut self.watches {
            let clauses = &self.clauses;
            watches.retain(|w| !clauses[w.clause].deleted);
        }
    }

    /// The `index`-th element of the Luby sequence 1, 1, 2, 1, 1, 2, 4, …
    fn luby(mut index: u64) -> u64 {
        let mut size = 1;
        let mut sequence = 0;
        while size < index + 1 {
            sequence += 1;
            size = 2 * size + 1;
        }
        while size - 1 != index {
            size = (size - 1) / 2;
            sequence -= 1;
            index %= size;
        }
        1 << sequence
    }

    /// One restart's worth of search. `None` means "restart"; otherwise the answer.
    fn search(&mut self, assumptions: &[Lit], budget: u64) -> Option<SolveResult> {
        let mut conflicts = 0;
        loop {
            if let Some(conflict) = self.propagate() {
                self.conflicts += 1;
                conflicts += 1;
                if self.stop.load(std::sync::atomic::Ordering::Relaxed) {
                    self.backtrack(0);
                    return Some(SolveResult::Unknown);
                }
                if self.decision_level() == 0 {
                    self.consistent = false;
                    return Some(SolveResult::Unsat);
                }
                let (learnt, level) = self.analyze(conflict);
                self.backtrack(level);
                if learnt.len() == 1 {
                    self.assign(learnt[0], None);
                } else {
                    let lbd = self.lbd(&learnt);
                    let asserting = learnt[0];
                    let reference = self.attach(Clause {
                        lits: learnt,
                        learnt: true,
                        lbd,
                        deleted: false,
                    });
                    self.learnts += 1;
                    self.assign(asserting, Some(reference));
                }
                self.activity_increment /= ACTIVITY_DECAY;
                continue;
            }
            if conflicts >= budget {
                self.backtrack(0);
                return None;
            }
            if self.learnts >= self.learnt_limit + self.trail.len() {
                self.reduce();
                self.learnt_limit += self.learnt_limit / 10;
            }
            // Assumptions first, each on its own decision level.
            let mut decision = None;
            while (self.decision_level() as usize) < assumptions.len() {
                let assumption = assumptions[self.decision_level() as usize];
                match self.current(assumption) {
                    Value::True => self.trail_limits.push(self.trail.len()),
                    Value::False => {
                        self.analyze_final(!assumption);
                        return Some(SolveResult::Unsat);
                    }
                    Value::Unassigned => {
                        decision = Some(assumption);
                        break;
                    }
                }
            }
            if decision.is_none() {
                loop {
                    match self.heap.pop(&self.activity) {
                        None => {
                            self.model = self.values.iter().map(|&v| v == Value::True).collect();
                            return Some(SolveResult::Sat);
                        }
                        Some(var) if self.values[var as usize] == Value::Unassigned => {
                            decision =
                                Some(Lit::new(Var::new(var), !self.saved_phase[var as usize]));
                            break;
                        }
                        Some(_) => {}
                    }
                }
            }
            self.trail_limits.push(self.trail.len());
            self.assign(decision.expect("chosen above"), None);
        }
    }
}

impl ClauseSink for Cdcl {
    fn new_var(&mut self) -> Var {
        let var = Var::new(self.num_vars);
        self.num_vars += 1;
        self.values.push(Value::Unassigned);
        self.level.push(0);
        self.reason.push(None);
        self.activity.push(0.0);
        self.saved_phase.push(self.initial_phase);
        self.seen.push(false);
        self.watches.push(Vec::new());
        self.watches.push(Vec::new());
        self.heap.grow(self.num_vars as usize);
        self.heap.insert(var.index(), &self.activity);
        var
    }

    fn add_clause(&mut self, clause: &[Lit]) {
        self.num_clauses += 1;
        self.has_model = false;
        if !self.consistent {
            return;
        }
        self.backtrack(0);
        let mut lits: Vec<Lit> = Vec::with_capacity(clause.len());
        for &lit in clause {
            match self.current(lit) {
                Value::True => return, // already satisfied
                Value::False => {}     // false at level 0: drop the literal
                Value::Unassigned => {
                    if lits.contains(&!lit) {
                        return; // tautology
                    }
                    if !lits.contains(&lit) {
                        lits.push(lit);
                    }
                }
            }
        }
        match lits.len() {
            0 => self.consistent = false,
            1 => {
                self.assign(lits[0], None);
                if self.propagate().is_some() {
                    self.consistent = false;
                }
            }
            _ => {
                self.attach(Clause {
                    lits,
                    learnt: false,
                    lbd: 0,
                    deleted: false,
                });
            }
        }
    }

    fn num_vars(&self) -> u32 {
        self.num_vars
    }

    fn num_clauses(&self) -> usize {
        self.num_clauses
    }
}

impl Solver for Cdcl {
    fn name(&self) -> &'static str {
        "built-in CDCL"
    }

    fn interrupter(&mut self) -> Option<crate::Interrupter> {
        let stop = std::sync::Arc::clone(&self.stop);
        Some(Box::new(move || {
            stop.store(true, std::sync::atomic::Ordering::Relaxed)
        }))
    }

    fn solve_with(&mut self, assumptions: &[Lit]) -> SolveResult {
        self.has_model = false;
        self.failed.clear();
        if !self.consistent {
            return SolveResult::Unsat;
        }
        self.backtrack(0);
        if self.propagate().is_some() {
            self.consistent = false;
            return SolveResult::Unsat;
        }
        self.stop.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut restarts = 0;
        let result = loop {
            if let Some(result) = self.search(assumptions, Self::luby(restarts) * RESTART_UNIT) {
                break result;
            }
            restarts += 1;
        };
        self.has_model = result == SolveResult::Sat;
        self.backtrack(0);
        result
    }

    fn value(&self, lit: Lit) -> bool {
        assert!(
            self.has_model,
            "no model available: the last solve did not return SAT"
        );
        self.model
            .get(lit.var().index() as usize)
            .is_some_and(|&value| value != lit.is_negated())
            || (self.model.len() <= lit.var().index() as usize && lit.is_negated())
    }

    fn failed(&self, lit: Lit) -> bool {
        self.failed.contains(&lit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cnf;

    /// A small deterministic random generator.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn random_3sat(seed: u64, vars: u32, clauses: usize) -> Cnf {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut cnf = Cnf::with_vars(vars);
        for _ in 0..clauses {
            let clause: Vec<Lit> = (0..3)
                .map(|_| {
                    Lit::new(
                        Var::new(u32::try_from(rng.next() % u64::from(vars)).unwrap()),
                        rng.next() & 1 == 1,
                    )
                })
                .collect();
            cnf.add_clause(&clause);
        }
        cnf
    }

    fn pigeonhole(pigeons: usize, holes: usize) -> Cnf {
        let mut cnf = Cnf::new();
        let p: Vec<Vec<Var>> = (0..pigeons)
            .map(|_| (0..holes).map(|_| cnf.new_var()).collect())
            .collect();
        for pigeon in &p {
            cnf.add_clause(&pigeon.iter().map(|v| v.pos()).collect::<Vec<_>>());
        }
        for hole in 0..holes {
            for i in 0..pigeons {
                for j in i + 1..pigeons {
                    cnf.add_clause(&[p[i][hole].neg(), p[j][hole].neg()]);
                }
            }
        }
        cnf
    }

    #[test]
    fn agrees_with_brute_force_on_random_formulas() {
        // Around the 3-SAT threshold (4.26 clauses per variable), with both outcomes.
        let (mut sat, mut unsat) = (0, 0);
        for seed in 0..300 {
            let vars = 12;
            let cnf = random_3sat(seed, vars, 52);
            let brute =
                (0..1u32 << vars).any(|bits| cnf.is_satisfied_by(|v| bits >> v.index() & 1 == 1));
            let mut solver = Cdcl::new();
            let table = cnf.load_into(&mut solver);
            match solver.solve() {
                SolveResult::Sat => {
                    assert!(brute, "seed {seed}: claimed SAT");
                    assert!(
                        cnf.is_satisfied_by(|v| solver.value(table[v.index() as usize].pos())),
                        "seed {seed}: bad model"
                    );
                    sat += 1;
                }
                SolveResult::Unsat => {
                    assert!(!brute, "seed {seed}: claimed UNSAT");
                    unsat += 1;
                }
                SolveResult::Unknown => unreachable!(),
            }
        }
        assert!(
            sat > 20 && unsat > 20,
            "both outcomes covered: {sat} SAT, {unsat} UNSAT"
        );
    }

    #[test]
    fn solves_larger_random_formulas_and_validates_models() {
        for seed in 0..20 {
            let cnf = random_3sat(seed + 1000, 150, 600);
            let mut solver = Cdcl::new();
            let table = cnf.load_into(&mut solver);
            if solver.solve() == SolveResult::Sat {
                assert!(cnf.is_satisfied_by(|v| solver.value(table[v.index() as usize].pos())));
            }
        }
    }

    #[test]
    fn proves_the_pigeonhole_principle() {
        let cnf = pigeonhole(7, 6);
        let mut solver = Cdcl::new();
        cnf.load_into(&mut solver);
        assert_eq!(solver.solve(), SolveResult::Unsat);
        assert!(solver.conflicts() > 0);
    }

    #[test]
    fn supports_incremental_use_with_assumptions() {
        let mut solver = Cdcl::new();
        let (a, b, c) = (solver.new_var(), solver.new_var(), solver.new_var());
        solver.add_clause(&[a.neg(), b.neg()]);
        assert_eq!(
            solver.solve_with(&[a.pos(), b.pos(), c.pos()]),
            SolveResult::Unsat
        );
        assert!(solver.failed(a.pos()) || solver.failed(b.pos()));
        assert!(!solver.failed(c.pos()));
        assert_eq!(solver.solve_with(&[a.pos()]), SolveResult::Sat);
        assert!(!solver.value(b.pos()));
        solver.add_clause(&[a.pos()]);
        solver.add_clause(&[b.pos(), c.pos()]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        assert!(solver.value(a.pos()) && solver.value(c.pos()));
        solver.add_clause(&[c.neg()]);
        assert_eq!(solver.solve(), SolveResult::Unsat);
        assert_eq!(solver.solve(), SolveResult::Unsat, "stays unsatisfiable");
    }

    #[test]
    fn reports_cores_through_implications() {
        // a → b, b → c, and the assumptions a and ¬c: both are needed, d is not.
        let mut solver = Cdcl::new();
        let (a, b, c, d) = (
            solver.new_var(),
            solver.new_var(),
            solver.new_var(),
            solver.new_var(),
        );
        solver.add_clause(&[a.neg(), b.pos()]);
        solver.add_clause(&[b.neg(), c.pos()]);
        assert_eq!(
            solver.solve_with(&[d.pos(), a.pos(), c.neg()]),
            SolveResult::Unsat
        );
        assert!(solver.failed(a.pos()) && solver.failed(c.neg()));
        assert!(!solver.failed(d.pos()));
    }

    #[test]
    fn handles_empty_and_trivial_formulas() {
        let mut solver = Cdcl::new();
        assert_eq!(solver.solve(), SolveResult::Sat);
        let a = solver.new_var();
        solver.add_clause(&[a.pos(), a.neg()]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        solver.add_clause(&[]);
        assert_eq!(solver.solve(), SolveResult::Unsat);
    }

    #[test]
    fn luby_sequence() {
        let values: Vec<u64> = (0..15).map(Cdcl::luby).collect();
        assert_eq!(values, [1, 1, 2, 1, 1, 2, 4, 1, 1, 2, 1, 1, 2, 4, 8]);
    }
}
