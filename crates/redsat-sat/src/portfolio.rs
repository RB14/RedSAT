//! A portfolio of solvers racing on hard calls, one thread each.
//!
//! CDCL solvers are sequential, and their run times on hard instances vary wildly with
//! heuristics, seeds and phases. A portfolio keeps several differently configured solvers on the
//! same formula and takes the first answer, which cuts that variance and uses every core (as in
//! ManySAT and plingeling, though without clause sharing).
//!
//! Most calls are easy, though (enumerating solutions makes hundreds of them), and racing them
//! would only add overhead. So the first member solves alone, on the calling thread; only when a
//! call is still running after a short grace period do the other members (the *helpers*) join.
//!
//! The helpers are long-lived threads, each owning its solver and processing commands in order:
//! new variables, clauses, and solves. They are created on the first race, from a log of the
//! formula, so a portfolio that never races costs no more than a single solver. A watchdog
//! thread times each call, starts the race, collects the helpers' reports, and stops whoever
//! lost. A helper that wins reports its whole model and core, so reading them never crosses
//! threads. Every member receives every clause, so the portfolio stays incremental.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{ClauseSink, Cnf, Interrupter, Lit, SolveResult, Solver, Var};

/// Creates the portfolio's members: member 0 is the main solver, the others its helpers.
pub type MemberFactory = Arc<dyn Fn(u32) -> Box<dyn Solver + Send> + Send + Sync>;

/// How long the first member searches alone before the helpers join. Most calls (e.g. while
/// enumerating solutions) finish well within it, and then cost no more than a single solver.
const GRACE: Duration = Duration::from_millis(25);

/// What a helper thread is told to do, in order.
enum Command {
    /// Grow to this many variables.
    Vars(u32),
    /// Add these clauses.
    Clauses(Vec<Vec<Lit>>),
    /// Solve call `call` under these assumptions, and report.
    Solve { call: u64, assumptions: Arc<[Lit]> },
}

/// A helper's answer to one call.
struct Report {
    call: u64,
    member: usize,
    result: SolveResult,
    /// After SAT: the value of every variable.
    model: Vec<bool>,
    /// After UNSAT: the failed assumptions.
    failed: Vec<Lit>,
}

/// What the watchdog hears about.
enum Event {
    /// The first member starts call `call` under these assumptions.
    Start { call: u64, assumptions: Arc<[Lit]> },
    /// The first member finished call `call`.
    Done(u64),
    /// A helper's report.
    Report(Report),
    /// The portfolio is being dropped.
    Shutdown,
}

/// A helper thread, as seen by the others.
struct Helper {
    commands: Sender<Command>,
    interrupt: Option<Interrupter>,
    thread: Option<JoinHandle<()>>,
}

/// State shared by the caller's thread and the watchdog.
struct Shared {
    /// Created by the watchdog on the first race.
    helpers: Mutex<Vec<Helper>>,
    /// The formula until the helpers exist (they start from it).
    log: Mutex<Option<Cnf>>,
    /// A helper's winning report for the current call.
    answer: Mutex<Option<Report>>,
}

/// Several solvers on one formula; a hard call races them.
pub struct Portfolio {
    size: usize,
    first: Box<dyn Solver + Send>,
    shared: Arc<Shared>,
    events: Sender<Event>,
    watchdog: Option<JoinHandle<()>>,
    /// Clauses and variables not yet sent to the helpers (once they exist).
    pending_clauses: Vec<Vec<Lit>>,
    helpers_exist: bool,
    call: u64,
    /// The winner of the last call: `None` for the first member, else a helper's report.
    winner: Option<Report>,
    num_vars: u32,
    num_clauses: usize,
    name: &'static str,
}

impl Portfolio {
    /// A portfolio of `size` members made by `make(0)`, `make(1)`, … (only member 0 is made
    /// right away). Members should differ, e.g. in configuration or seed.
    #[must_use]
    pub fn new(
        size: usize,
        make: impl Fn(u32) -> Box<dyn Solver + Send> + Send + Sync + 'static,
    ) -> Self {
        let make: MemberFactory = Arc::new(make);
        let mut first = make(0);
        let size = size.max(1);
        let name = if size == 1 {
            first.name().to_string()
        } else {
            format!("portfolio of {} ×{size}", first.name())
        };
        let shared = Arc::new(Shared {
            helpers: Mutex::new(Vec::new()),
            log: Mutex::new((size > 1).then(Cnf::new)),
            answer: Mutex::new(None),
        });
        let (events, inbox) = mpsc::channel();
        let watchdog = (size > 1).then(|| {
            let (shared, events, first_interrupt) =
                (Arc::clone(&shared), events.clone(), first.interrupter());
            std::thread::Builder::new()
                .name("redsat-portfolio".into())
                .spawn(move || watch(&inbox, &events, &shared, &make, size, first_interrupt))
                .expect("cannot start the portfolio's watchdog")
        });
        Self {
            size,
            first,
            shared,
            events,
            watchdog,
            pending_clauses: Vec::new(),
            helpers_exist: false,
            call: 0,
            winner: None,
            num_vars: 0,
            num_clauses: 0,
            name: Box::leak(name.into_boxed_str()),
        }
    }

    /// The number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.size
    }

    /// Whether the portfolio has no members (never: it always has a first one).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Which member answered the last call (0 is the first).
    #[must_use]
    pub fn winner(&self) -> usize {
        self.winner.as_ref().map_or(0, |report| report.member)
    }

    /// Sends the clauses added since the last call to the helpers, if they exist.
    fn flush(&mut self) {
        if !self.helpers_exist {
            self.helpers_exist = !lock(&self.shared.helpers).is_empty();
            if !self.helpers_exist {
                return;
            }
        }
        let clauses = std::mem::take(&mut self.pending_clauses);
        for helper in lock(&self.shared.helpers).iter() {
            let _ = helper.commands.send(Command::Vars(self.num_vars));
            if !clauses.is_empty() {
                let _ = helper.commands.send(Command::Clauses(clauses.clone()));
            }
        }
    }

    /// One call: the first member on this thread; the watchdog starts the helpers if it takes
    /// long, and stops the first member if a helper answers first.
    fn race(&mut self, assumptions: &[Lit]) -> SolveResult {
        self.flush();
        self.call += 1;
        let call = self.call;
        let _ = self.events.send(Event::Start {
            call,
            assumptions: assumptions.into(),
        });
        let result = self.first.solve_with(assumptions);
        let _ = self.events.send(Event::Done(call));
        let answer = lock(&self.shared.answer)
            .take()
            .filter(|report| report.call == call);
        // Helpers created during this call already have every clause.
        self.helpers_exist = self.helpers_exist || !lock(&self.shared.helpers).is_empty();
        match (result, answer) {
            (SolveResult::Unknown, Some(report)) => {
                let result = report.result;
                self.winner = Some(report);
                result
            }
            (result, _) => {
                self.winner = None;
                result
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How often helpers still busy with a finished call are asked again to stop: a stop that
/// reaches a solver before its search begins may be ignored.
const RESTOP: Duration = Duration::from_millis(2);

/// The watchdog: times each call of the first member, races the helpers on slow ones, and
/// stops the losers (again and again, until they report back).
fn watch(
    inbox: &Receiver<Event>,
    events: &Sender<Event>,
    shared: &Shared,
    make: &MemberFactory,
    size: usize,
    first_interrupt: Option<Interrupter>,
) {
    let stop = |busy: &[bool]| {
        for (helper, _) in lock(&shared.helpers)
            .iter()
            .zip(busy)
            .filter(|(_, busy)| **busy)
        {
            if let Some(interrupt) = &helper.interrupt {
                interrupt();
            }
        }
    };
    // The call in progress: its number, its assumptions, when helpers join, whether they did.
    let mut current: Option<(u64, Arc<[Lit]>, Instant, bool)> = None;
    // Which helpers are still searching a call that is over (or, during a race, any call).
    let mut busy = vec![false; size.saturating_sub(1)];
    loop {
        let waiting_for_grace = matches!(current, Some((.., false)));
        let stopping = current.is_none() && busy.iter().any(|busy| *busy);
        let timeout = match &current {
            Some((_, _, deadline, false)) => {
                Some(deadline.saturating_duration_since(Instant::now()))
            }
            _ if stopping => Some(RESTOP),
            _ => None,
        };
        let event = match timeout {
            Some(timeout) => match inbox.recv_timeout(timeout) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match inbox.recv() {
                Ok(event) => Some(event),
                Err(_) => return,
            },
        };
        match event {
            Some(Event::Shutdown) => return,
            Some(Event::Start { call, assumptions }) => {
                current = Some((call, assumptions, Instant::now() + GRACE, false));
            }
            Some(Event::Done(call)) => {
                if current
                    .as_ref()
                    .is_some_and(|(current_call, ..)| *current_call == call)
                {
                    current = None;
                    stop(&busy);
                }
            }
            Some(Event::Report(report)) => {
                if let Some(busy) = busy.get_mut(report.member - 1) {
                    *busy = false;
                }
                let live = current
                    .as_ref()
                    .is_some_and(|(call, ..)| *call == report.call);
                if live && report.result != SolveResult::Unknown {
                    let mut answer = lock(&shared.answer);
                    if answer.is_none() {
                        *answer = Some(report);
                        drop(answer);
                        if let Some(interrupt) = &first_interrupt {
                            interrupt();
                        }
                        stop(&busy);
                    }
                }
            }
            None if waiting_for_grace => {
                // The grace period is over: race.
                let Some((call, assumptions, _, raced)) = &mut current else {
                    continue;
                };
                *raced = true;
                let mut helpers = lock(&shared.helpers);
                if helpers.is_empty() {
                    let formula = lock(&shared.log).take().unwrap_or_default();
                    for member in 1..size {
                        helpers.push(spawn_helper(member, make, &formula, events.clone()));
                    }
                }
                for helper in helpers.iter() {
                    let _ = helper.commands.send(Command::Solve {
                        call: *call,
                        assumptions: Arc::clone(assumptions),
                    });
                }
                busy.iter_mut().for_each(|busy| *busy = true);
            }
            None => stop(&busy),
        }
    }
}

/// Starts helper `member` on `formula`: a thread that runs commands and reports solves.
fn spawn_helper(
    member: usize,
    make: &MemberFactory,
    formula: &Cnf,
    reports: Sender<Event>,
) -> Helper {
    let mut solver = make(u32::try_from(member).unwrap_or(u32::MAX));
    formula.load_into(&mut solver);
    let interrupt = solver.interrupter();
    let (commands, inbox) = mpsc::channel::<Command>();
    let thread = std::thread::Builder::new()
        .name(format!("redsat-portfolio-{member}"))
        .spawn(move || {
            for command in inbox {
                match command {
                    Command::Vars(count) => {
                        while solver.num_vars() < count {
                            solver.new_var();
                        }
                    }
                    Command::Clauses(clauses) => {
                        clauses.iter().for_each(|clause| solver.add_clause(clause))
                    }
                    Command::Solve { call, assumptions } => {
                        let result = solver.solve_with(&assumptions);
                        let model = if result == SolveResult::Sat {
                            (0..solver.num_vars())
                                .map(|var| solver.value(Lit::new(Var::new(var), false)))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let failed = if result == SolveResult::Unsat {
                            assumptions
                                .iter()
                                .copied()
                                .filter(|&lit| solver.failed(lit))
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let report = Report {
                            call,
                            member,
                            result,
                            model,
                            failed,
                        };
                        if reports.send(Event::Report(report)).is_err() {
                            return;
                        }
                    }
                }
            }
        })
        .expect("cannot start a portfolio helper");
    Helper {
        commands,
        interrupt,
        thread: Some(thread),
    }
}

impl Drop for Portfolio {
    fn drop(&mut self) {
        // End the watchdog first (it may be starting helpers), then the helpers: closing their
        // command channels ends their loops, once their current solve has stopped. A stop can
        // be missed by a search that is just starting, so it is repeated until the thread ends.
        let _ = self.events.send(Event::Shutdown);
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        let helpers = std::mem::take(&mut *lock(&self.shared.helpers));
        for mut helper in helpers {
            drop(helper.commands);
            if let Some(thread) = helper.thread.take() {
                while !thread.is_finished() {
                    if let Some(interrupt) = &helper.interrupt {
                        interrupt();
                    }
                    std::thread::sleep(RESTOP);
                }
                let _ = thread.join();
            }
        }
    }
}

impl ClauseSink for Portfolio {
    fn new_var(&mut self) -> Var {
        self.first.new_var();
        if !self.helpers_exist {
            if let Some(log) = lock(&self.shared.log).as_mut() {
                log.new_var();
            }
        }
        let var = Var::new(self.num_vars);
        self.num_vars += 1;
        var
    }

    fn add_clause(&mut self, clause: &[Lit]) {
        self.first.add_clause(clause);
        if self.helpers_exist {
            self.pending_clauses.push(clause.to_vec());
        } else if let Some(log) = lock(&self.shared.log).as_mut() {
            log.add_clause(clause);
        }
        self.num_clauses += 1;
    }

    fn num_vars(&self) -> u32 {
        self.num_vars
    }

    fn num_clauses(&self) -> usize {
        self.num_clauses
    }
}

impl Solver for Portfolio {
    fn name(&self) -> &'static str {
        self.name
    }

    fn solve_with(&mut self, assumptions: &[Lit]) -> SolveResult {
        if self.size == 1 {
            self.winner = None;
            return self.first.solve_with(assumptions);
        }
        // A stop that reaches a member just as it finishes stays pending and makes that member's
        // next call return at once; should that happen, race again.
        for _ in 0..3 {
            let result = self.race(assumptions);
            if result != SolveResult::Unknown {
                return result;
            }
        }
        SolveResult::Unknown
    }

    fn value(&self, lit: Lit) -> bool {
        match &self.winner {
            None => self.first.value(lit),
            Some(report) => {
                let positive = report
                    .model
                    .get(lit.var().index() as usize)
                    .copied()
                    .unwrap_or(false);
                positive != lit.is_negated()
            }
        }
    }

    fn failed(&self, lit: Lit) -> bool {
        match &self.winner {
            None => self.first.failed(lit),
            Some(report) => report.failed.contains(&lit),
        }
    }

    fn interrupter(&mut self) -> Option<Interrupter> {
        let first = self.first.interrupter();
        let shared = Arc::clone(&self.shared);
        Some(Box::new(move || {
            if let Some(interrupt) = &first {
                interrupt();
            }
            for helper in lock(&shared.helpers).iter() {
                if let Some(interrupt) = &helper.interrupt {
                    interrupt();
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cdcl;

    fn lit(dimacs: i32) -> Lit {
        Lit::from_dimacs(dimacs)
    }

    fn portfolio(size: usize) -> Portfolio {
        Portfolio::new(size, |member| Box::new(Cdcl::with_seed(u64::from(member))))
    }

    #[test]
    fn answers_like_a_single_solver_and_stays_incremental() {
        let mut solver = portfolio(4);
        let [a, b, c] = [solver.new_var(), solver.new_var(), solver.new_var()];
        let _ = (a, b, c);
        solver.add_clause(&[lit(1), lit(2)]);
        solver.add_clause(&[lit(-1), lit(3)]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        let (x, y, z) = (
            solver.value(lit(1)),
            solver.value(lit(2)),
            solver.value(lit(3)),
        );
        assert!(
            (x || y) && (!x || z),
            "the winner's model satisfies the formula"
        );

        assert_eq!(solver.solve_with(&[lit(1), lit(-3)]), SolveResult::Unsat);
        assert!(solver.failed(lit(1)) || solver.failed(lit(-3)));

        solver.add_clause(&[lit(-2)]);
        solver.add_clause(&[lit(-3)]);
        assert_eq!(solver.solve(), SolveResult::Unsat);
        assert_eq!(solver.num_clauses(), 4);
    }

    /// A member that never answers until it is stopped.
    struct Stalling {
        cnf: crate::Cnf,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl ClauseSink for Stalling {
        fn new_var(&mut self) -> Var {
            self.cnf.new_var()
        }
        fn add_clause(&mut self, clause: &[Lit]) {
            self.cnf.add_clause(clause);
        }
        fn num_vars(&self) -> u32 {
            self.cnf.num_vars()
        }
        fn num_clauses(&self) -> usize {
            self.cnf.num_clauses()
        }
    }

    impl Solver for Stalling {
        fn name(&self) -> &'static str {
            "stalling"
        }
        fn solve_with(&mut self, _: &[Lit]) -> SolveResult {
            while !self.stop.swap(false, std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(1));
            }
            SolveResult::Unknown
        }
        fn value(&self, _: Lit) -> bool {
            unreachable!("never wins")
        }
        fn failed(&self, _: Lit) -> bool {
            unreachable!("never wins")
        }
        fn interrupter(&mut self) -> Option<Interrupter> {
            let stop = std::sync::Arc::clone(&self.stop);
            Some(Box::new(move || {
                stop.store(true, std::sync::atomic::Ordering::SeqCst)
            }))
        }
    }

    #[test]
    fn a_helper_answers_when_the_first_member_is_slow() {
        let mut solver = Portfolio::new(3, |member| -> Box<dyn Solver + Send> {
            if member == 0 {
                Box::new(Stalling {
                    cnf: crate::Cnf::new(),
                    stop: std::sync::Arc::default(),
                })
            } else {
                Box::new(Cdcl::with_seed(u64::from(member)))
            }
        });
        for _ in 0..3 {
            solver.new_var();
        }
        solver.add_clause(&[lit(1), lit(2)]);
        solver.add_clause(&[lit(-1)]);
        assert_eq!(solver.solve(), SolveResult::Sat);
        assert_ne!(solver.winner(), 0, "a helper answered");
        assert!(
            !solver.value(lit(1)) && solver.value(lit(2)),
            "the helper's model"
        );
        // Clauses added after the helpers exist reach them too.
        solver.add_clause(&[lit(-2), lit(3)]);
        assert_eq!(solver.solve_with(&[lit(-3)]), SolveResult::Unsat);
        assert!(solver.failed(lit(-3)), "the helper's core");
    }

    #[test]
    fn a_hard_instance_finishes_and_losers_are_stopped() {
        // Pigeonhole: 8 pigeons in 7 holes is unsatisfiable and takes CDCL a while; the
        // portfolio must return once one member proves it, not wait for all to finish.
        let mut solver = portfolio(3);
        let (pigeons, holes) = (8, 7);
        let var = |p: i32, h: i32| p * holes + h + 1;
        for _ in 0..pigeons * holes {
            solver.new_var();
        }
        for p in 0..pigeons {
            solver.add_clause(&(0..holes).map(|h| lit(var(p, h))).collect::<Vec<_>>());
        }
        for h in 0..holes {
            for p in 0..pigeons {
                for q in p + 1..pigeons {
                    solver.add_clause(&[lit(-var(p, h)), lit(-var(q, h))]);
                }
            }
        }
        assert_eq!(solver.solve(), SolveResult::Unsat);
        // Still usable afterwards.
        assert_eq!(solver.solve(), SolveResult::Unsat);
    }
}

#[cfg(all(test, feature = "cadical"))]
mod cadical_tests {
    use super::*;
    use crate::Cadical;

    /// Enumerating models makes many quick calls in a row: no call may come back unknown, even
    /// though losing members are stopped after every call.
    #[test]
    fn enumerates_with_many_quick_calls() {
        let mut solver = Portfolio::new(4, |member| Box::new(Cadical::diversified(member)));
        let vars: Vec<Var> = (0..8).map(|_| solver.new_var()).collect();
        let mut models = 0;
        loop {
            match solver.solve() {
                SolveResult::Sat => {
                    models += 1;
                    let block: Vec<Lit> = vars
                        .iter()
                        .map(|&var| Lit::new(var, solver.value(Lit::new(var, false))))
                        .collect();
                    solver.add_clause(&block);
                }
                SolveResult::Unsat => break,
                SolveResult::Unknown => panic!("a portfolio call gave up after {models} models"),
            }
            assert!(models <= 256);
        }
        assert_eq!(models, 256, "every assignment of 8 free variables");
    }
}
