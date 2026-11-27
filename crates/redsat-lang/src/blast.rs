//! Bit-blasting: lowering word-level terms to an And-Inverter Graph.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use num_bigint::BigInt;
use num_traits::{One, Signed, Zero};

use redsat_circuit::{Aig, BitOp, Edge, IntVec, Layout, int};

use crate::domain::{self, Interval};
use crate::ir::{Constant, Sort, Term, TermId, TermStore};

/// The circuit for one term.
#[derive(Clone, Debug)]
pub(crate) enum Bits {
    Bool(Edge),
    Int(IntVec),
}

impl Bits {
    pub(crate) fn edge(&self) -> Edge {
        match self {
            Self::Bool(edge) => *edge,
            Self::Int(_) => panic!("expected a boolean circuit"),
        }
    }

    pub(crate) fn int(&self) -> &IntVec {
        match self {
            Self::Int(int) => int,
            Self::Bool(_) => panic!("expected an integer circuit"),
        }
    }

    /// Every edge of the circuit's output.
    pub(crate) fn edges(&self) -> Vec<Edge> {
        match self {
            Self::Bool(edge) => vec![*edge],
            Self::Int(int) => int.bits().to_vec(),
        }
    }
}

/// The circuits for all terms reachable from some roots.
pub(crate) struct Blasted {
    bits: Vec<Option<Bits>>,
    /// Constraints that must always hold for the circuit to mean what the terms mean: domain
    /// constraints keeping each unknown inside its domain (when its bit layout can represent
    /// more values, e.g. `int[0..=9]` in 4 bits), and the definitions of division results. Each
    /// comes with the term whose circuit needs it (the unknown, or the division).
    pub(crate) side_constraints: Vec<(TermId, Edge)>,
}

impl Blasted {
    pub(crate) fn of(&self, id: TermId) -> &Bits {
        self.bits[id.index()].as_ref().expect("term was blasted")
    }

    pub(crate) fn get(&self, id: TermId) -> Option<&Bits> {
        self.bits[id.index()].as_ref()
    }
}

/// The most gates a generalized totalizer may take for one constraint; beyond it, the sum stays
/// binary (large weights or bounds make the binary encoding the better one anyway).
const PSEUDO_BOOLEAN_BUDGET: usize = 400_000;

/// How many times the binary encoding's size a weighted generalized totalizer may take.
const WEIGHTED_SIZE_FACTOR: usize = 8;

/// The largest weight a generalized totalizer accepts.
const MAX_WEIGHT: u64 = 1 << 32;

/// The value of a comparison that the operands' ranges decide on their own (e.g. `x >= 0` when
/// `x` lies in `0..=35`): such requirements then cost no clauses, and comparisons inside
/// larger expressions fold away.
fn decided_by_ranges(term: &Term, a: &Interval, b: &Interval) -> Option<bool> {
    let ((a_lo, a_hi), (b_lo, b_hi)) = (a.bounds()?, b.bounds()?);
    match term {
        Term::Lt(..) if a_hi < b_lo => Some(true),
        Term::Lt(..) if a_lo >= b_hi => Some(false),
        Term::Le(..) if a_hi <= b_lo => Some(true),
        Term::Le(..) if a_lo > b_hi => Some(false),
        Term::Eq(..) if a_hi < b_lo || b_hi < a_lo => Some(false),
        Term::Eq(..) if a_lo == a_hi && b_lo == b_hi && a_lo == b_lo => Some(true),
        _ => None,
    }
}

/// The smallest layout for a bounded range.
pub(crate) fn layout_of(range: &Interval) -> Layout {
    let (lo, hi) = range.bounds().expect("sizes of blasted terms are bounded");
    Layout::for_range(lo, hi)
}

/// Builds circuits for every term reachable from `roots`. `sizes` gives each term's guaranteed
/// range (see [`crate::domain::forward_sizes`]) and `domains` each unknown's final domain.
pub(crate) fn blast(
    store: &TermStore,
    sizes: &[Interval],
    domains: &[Interval],
    roots: &[TermId],
    aig: &mut Aig,
) -> Blasted {
    let mut reachable = vec![false; store.len()];
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        if !std::mem::replace(&mut reachable[id.index()], true) {
            pending.extend(store.term(id).operands());
        }
    }

    let mut blaster = Blaster {
        store,
        sizes,
        aig,
        bits: vec![None; store.len()],
        side: Vec::new(),
        divisions: HashMap::new(),
    };
    let mut origins = Vec::new();
    for (id, term) in store.terms() {
        if reachable[id.index()] {
            let result = blaster.term(id, term, domains);
            blaster.bits[id.index()] = Some(result);
            origins.resize(blaster.side.len(), id);
        }
    }
    Blasted {
        bits: blaster.bits,
        side_constraints: origins.into_iter().zip(blaster.side).collect(),
    }
}

struct Blaster<'a> {
    store: &'a TermStore,
    sizes: &'a [Interval],
    aig: &'a mut Aig,
    bits: Vec<Option<Bits>>,
    side: Vec<Edge>,
    /// Quotient and remainder vectors per (dividend, divisor) pair.
    divisions: HashMap<(TermId, TermId), (IntVec, IntVec)>,
}

impl Blaster<'_> {
    fn of(&self, id: TermId) -> &Bits {
        self.bits[id.index()].as_ref().expect("operands come first")
    }

    fn int_of(&self, id: TermId) -> IntVec {
        self.of(id).int().clone()
    }

    fn layout(&self, id: TermId) -> Layout {
        layout_of(&self.sizes[id.index()])
    }

    fn term(&mut self, id: TermId, term: &Term, domains: &[Interval]) -> Bits {
        match term {
            Term::Const(Constant::Bool(value)) => Bits::Bool(Edge::constant(*value)),
            Term::Const(Constant::Int(value)) => Bits::Int(IntVec::constant(value)),
            Term::Unknown(unknown) => match self.store.unknown(*unknown).sort {
                Sort::Bool => Bits::Bool(self.aig.new_input()),
                Sort::Int => {
                    let (lo, hi) = domains[unknown.index()]
                        .bounds()
                        .expect("blasted unknowns are bounded");
                    if lo == hi {
                        // Domain inference fixed the value: no search needed.
                        return Bits::Int(IntVec::constant(lo));
                    }
                    let layout = Layout::for_range(lo, hi);
                    let vector = IntVec::new_input(self.aig, layout);
                    if layout.min() < *lo {
                        let above = int::le(self.aig, &IntVec::constant(lo), &vector);
                        self.side.push(above);
                    }
                    if layout.max() > *hi {
                        let below = int::le(self.aig, &vector, &IntVec::constant(hi));
                        self.side.push(below);
                    }
                    Bits::Int(vector)
                }
            },
            Term::Not(a) => Bits::Bool(!self.of(*a).edge()),
            Term::And(a, b) | Term::Or(a, b) | Term::Xor(a, b) => {
                let (x, y) = (self.of(*a).edge(), self.of(*b).edge());
                Bits::Bool(match term {
                    Term::And(..) => self.aig.and(x, y),
                    Term::Or(..) => self.aig.or(x, y),
                    _ => self.aig.xor(x, y),
                })
            }
            Term::Ite(c, a, b) => {
                let cond = self.of(*c).edge();
                match (self.of(*a).clone(), self.of(*b).clone()) {
                    (Bits::Bool(x), Bits::Bool(y)) => Bits::Bool(self.aig.ite(cond, x, y)),
                    (x, y) => {
                        let layout = self.layout(id);
                        Bits::Int(int::ite_in(self.aig, cond, x.int(), y.int(), layout))
                    }
                }
            }
            Term::Neg(a) => {
                let (x, layout) = (self.int_of(*a), self.layout(id));
                Bits::Int(int::neg_in(self.aig, &x, layout))
            }
            Term::Add(a, b) | Term::Sub(a, b) | Term::Mul(a, b) => {
                let (x, y, layout) = (self.int_of(*a), self.int_of(*b), self.layout(id));
                Bits::Int(match term {
                    Term::Add(..) => int::add_in(self.aig, &x, &y, layout),
                    Term::Sub(..) => int::sub_in(self.aig, &x, &y, layout),
                    _ => int::mul_in(self.aig, &x, &y, layout),
                })
            }
            Term::BitAnd(a, b) | Term::BitOr(a, b) | Term::BitXor(a, b) => {
                let (x, y, layout) = (self.int_of(*a), self.int_of(*b), self.layout(id));
                let op = match term {
                    Term::BitAnd(..) => BitOp::And,
                    Term::BitOr(..) => BitOp::Or,
                    _ => BitOp::Xor,
                };
                Bits::Int(int::bitwise_in(self.aig, op, &x, &y, layout))
            }
            Term::Div(a, b) | Term::Mod(a, b) => {
                let (quotient, remainder) = self.division(*a, *b);
                Bits::Int(if matches!(term, Term::Div(..)) {
                    quotient
                } else {
                    remainder
                })
            }
            Term::AllDifferent(items) => Bits::Bool(self.all_different(items)),
            Term::Eq(a, b) | Term::Lt(a, b) | Term::Le(a, b) => {
                if let Some(decided) =
                    decided_by_ranges(term, &self.sizes[a.index()], &self.sizes[b.index()])
                {
                    return Bits::Bool(if decided { Edge::TRUE } else { Edge::FALSE });
                }
                if let Some(edge) = self.pseudo_boolean(term) {
                    return Bits::Bool(edge);
                }
                let (x, y) = (self.int_of(*a), self.int_of(*b));
                Bits::Bool(match term {
                    Term::Eq(..) => int::eq(self.aig, &x, &y),
                    Term::Lt(..) => int::lt(self.aig, &x, &y),
                    _ => int::le(self.aig, &x, &y),
                })
            }
        }
    }

    /// A sum of weighted booleans compared with a constant: `count(…) <= k`, `exactly`,
    /// `at_least`, `at_most`, or a weighted `sum(w[i] for i … if b[i]) <= k`. Encoded with a
    /// generalized totalizer (GTE: Joshi, Martins and Manquinho, CP 2015; with unit weights it is
    /// the totalizer of Bailleux and Boufkhad, 2003) instead of an adder and a comparator: unit
    /// propagation then enforces the bound by itself (arc consistency). Its size grows with the
    /// number of distinct partial sums up to the bound, so sums with large weights or bounds
    /// keep the binary encoding.
    fn pseudo_boolean(&mut self, term: &Term) -> Option<Edge> {
        let (Term::Eq(a, b) | Term::Lt(a, b) | Term::Le(a, b)) = term else {
            return None;
        };
        let constant = |id: TermId| match self.store.term(id) {
            Term::Const(Constant::Int(value)) => Some(value.clone()),
            _ => None,
        };
        let (terms, offset, k, sum_on_left) = match (self.as_weighted_sum(*a), constant(*b)) {
            (Some((terms, offset)), Some(k)) => (terms, offset, k, true),
            _ => match (constant(*a), self.as_weighted_sum(*b)) {
                (Some(k), Some((terms, offset))) => (terms, offset, k, false),
                _ => return None,
            },
        };
        let total: u64 = terms.iter().map(|&(_, weight)| weight).sum();
        let n = i64::try_from(total).ok()?;
        // The bound on the weighted sum (clamped: anything beyond 0..=total is decided).
        let m = i64::try_from((k - offset).clamp(BigInt::from(-1), BigInt::from(n + 1))).ok()?;
        let (lo, hi) = match (term, sum_on_left) {
            (Term::Eq(..), _) => (m, m),
            (Term::Le(..), true) => (0, m),
            (Term::Le(..), false) => (m, n),
            (Term::Lt(..), true) => (0, m - 1),
            _ => (m + 1, n),
        };
        let (lo, hi) = (lo.max(0), hi.min(n));
        if lo > hi {
            return Some(Edge::FALSE);
        }
        if lo == 0 && hi == n {
            return Some(Edge::TRUE);
        }
        if hi == 0 {
            return Some(self.aig.and_all(terms.iter().map(|&(b, _)| !b)));
        }
        if lo == n {
            return Some(self.aig.and_all(terms.iter().map(|&(b, _)| b)));
        }
        // The thresholds needed are lo and hi + 1; partial sums are capped at the larger one.
        let limit = u64::try_from(if hi < n { hi + 1 } else { lo }).ok()?;
        // Counts (unit weights) always use the totalizer. Weighted sums use it only when it stays
        // close to the binary encoding's size (about n·log2(total) gates): with large weights or
        // bounds its many distinct partial sums make it far bigger and slower (measured: 27×
        // slower on a 50-item knapsack), as encoding studies also find.
        let unit = terms.iter().all(|&(_, weight)| weight == 1);
        let bits = usize::try_from(u64::BITS - total.leading_zeros()).unwrap_or(64);
        let mut budget = if unit {
            PSEUDO_BOOLEAN_BUDGET
        } else {
            (WEIGHTED_SIZE_FACTOR * terms.len() * bits).max(2_000)
        };
        let at_least = self.generalized_totalizer(&terms, limit, &mut budget)?;
        // `sum ≥ t` holds exactly when the smallest achievable sum ≥ t is reached.
        let reaches = |threshold: i64| -> Edge {
            let threshold = u64::try_from(threshold).unwrap_or(0);
            if threshold == 0 {
                return Edge::TRUE;
            }
            at_least
                .range(threshold..)
                .next()
                .map_or(Edge::FALSE, |(_, &edge)| edge)
        };
        let mut bounds = Vec::new();
        if lo > 0 {
            bounds.push(reaches(lo));
        }
        if hi < n {
            bounds.push(!reaches(hi + 1));
        }
        Some(self.aig.and_all(bounds))
    }

    /// The weighted booleans of a sum and its constant part, if `id` is one: any tree of `+`
    /// over constants and `b ? x : y` terms with constant `x` and `y` (as `count(…)` and
    /// filtered `sum(…)` produce), normalized to positive weights on `b` or `¬b`.
    fn as_weighted_sum(&self, id: TermId) -> Option<(Vec<(Edge, u64)>, BigInt)> {
        let mut terms = Vec::new();
        let mut offset = BigInt::zero();
        let mut pending = vec![id];
        while let Some(term) = pending.pop() {
            match self.store.term(term) {
                Term::Add(a, b) => pending.extend([*a, *b]),
                Term::Const(Constant::Int(value)) => offset += value,
                Term::Ite(cond, then, otherwise) => {
                    let value = |id: TermId| match self.store.term(id) {
                        Term::Const(Constant::Int(value)) => Some(value.clone()),
                        _ => None,
                    };
                    let (x, y) = (value(*then)?, value(*otherwise)?);
                    let edge = self.of(*cond).edge();
                    // b ? x : y  =  y + (x - y)·b  =  x + (y - x)·¬b
                    match x.cmp(&y) {
                        std::cmp::Ordering::Equal => offset += x,
                        std::cmp::Ordering::Greater => {
                            terms.push((
                                edge,
                                u64::try_from(&x - &y).ok().filter(|&w| w <= MAX_WEIGHT)?,
                            ));
                            offset += y;
                        }
                        std::cmp::Ordering::Less => {
                            terms.push((
                                !edge,
                                u64::try_from(&y - &x).ok().filter(|&w| w <= MAX_WEIGHT)?,
                            ));
                            offset += x;
                        }
                    }
                }
                _ => return None,
            }
        }
        (terms.len() >= 2).then_some((terms, offset))
    }

    /// A generalized totalizer over `terms`: for every achievable partial sum `s` (capped at
    /// `limit`), an edge that holds exactly when the sum is at least `s`. `None` when it would
    /// take more than `budget` gates.
    fn generalized_totalizer(
        &mut self,
        terms: &[(Edge, u64)],
        limit: u64,
        budget: &mut usize,
    ) -> Option<BTreeMap<u64, Edge>> {
        if let [(edge, weight)] = terms {
            return Some(BTreeMap::from([((*weight).min(limit), *edge)]));
        }
        let (left, right) = terms.split_at(terms.len() / 2);
        let left = self.generalized_totalizer(left, limit, budget)?;
        let right = self.generalized_totalizer(right, limit, budget)?;
        let sums: BTreeSet<u64> = std::iter::once(0)
            .chain(left.keys().copied())
            .flat_map(|a| {
                std::iter::once(0)
                    .chain(right.keys().copied())
                    .map(move |b| (a + b).min(limit))
            })
            .filter(|&sum| sum > 0)
            .collect();
        let cost = sums.len().saturating_mul(left.len() + 1);
        *budget = budget.checked_sub(cost)?;
        let mut outputs = BTreeMap::new();
        for sum in sums {
            // sum_left + sum_right ≥ sum  ⇔  some achievable a has sum_left ≥ a and
            // sum_right ≥ sum - a (the smallest achievable right sum that large).
            // Left sums below `sum`, and the smallest one reaching it (the larger ones imply it).
            let below = left.range(..sum).map(|(&a, &edge)| (a, edge));
            let reaching = left.range(sum..).next().map(|(&a, &edge)| (a, edge));
            let ways: Vec<Edge> = std::iter::once((0, Edge::TRUE))
                .chain(below)
                .chain(reaching)
                .filter_map(|(a, left_edge)| {
                    let right_edge = if a >= sum {
                        Edge::TRUE
                    } else {
                        *right.range(sum - a..).next()?.1
                    };
                    Some((left_edge, right_edge))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(x, y)| self.aig.and(x, y))
                .collect();
            outputs.insert(sum, self.aig.or_all(ways));
        }
        Some(outputs)
    }

    /// `all_different(items)`: the conjunction of pairwise inequalities.
    ///
    /// One-hot channeling (value indicators with at-most-one / at-least-one constraints) was
    /// measured and rejected: it made quasigroup completion and Sudoku 2–4× slower with CaDiCaL,
    /// and its pigeonhole-only variant was neutral (see design §5.5).
    fn all_different(&mut self, items: &[TermId]) -> Edge {
        let vectors: Vec<IntVec> = items.iter().map(|&item| self.int_of(item)).collect();
        let mut pairwise = Vec::new();
        for (i, a) in vectors.iter().enumerate() {
            for b in &vectors[i + 1..] {
                pairwise.push(!int::eq(self.aig, a, b));
            }
        }
        self.aig.and_all(pairwise)
    }

    /// The quotient and remainder of the Euclidean division `a / b`, shared by `a / b` and
    /// `a % b`. Powers of two need no gates; other divisors get fresh vectors constrained by
    /// `b == 0 ? (q == 0 and r == a) : (q·b + r == a and 0 <= r < |b|)`.
    fn division(&mut self, a: TermId, b: TermId) -> (IntVec, IntVec) {
        if let Some(pair) = self.divisions.get(&(a, b)) {
            return pair.clone();
        }
        let (ranges_a, ranges_b) = (&self.sizes[a.index()], &self.sizes[b.index()]);
        let quotient_layout = layout_of(&domain::div_range(ranges_a, ranges_b));
        let remainder_layout = layout_of(&domain::mod_range(ranges_a, ranges_b));
        let (x, y) = (self.int_of(a), self.int_of(b));

        let pair = match y
            .as_constant()
            .filter(|d| d.is_positive() && (d & (d - BigInt::one())).is_zero())
        {
            Some(power) => {
                let k = usize::try_from(power.bits() - 1).expect("small shift");
                (int::shr_floor(&x, k, quotient_layout), int::low_bits(&x, k))
            }
            None => {
                let aig = &mut *self.aig;
                let q = IntVec::new_input(aig, quotient_layout);
                let r = IntVec::new_input(aig, remainder_layout);
                let zero = IntVec::constant(&BigInt::zero());
                let divisor_is_zero = int::eq(aig, &y, &zero);
                let product = int::mul(aig, &q, &y);
                let recombined = int::add(aig, &product, &r);
                let exact = int::eq(aig, &recombined, &x);
                let non_negative = int::le(aig, &zero, &r);
                let negative = int::lt(aig, &y, &zero);
                let negated = int::neg(aig, &y);
                let magnitude = int::ite(aig, negative, &negated, &y);
                let below = int::lt(aig, &r, &magnitude);
                let defined = aig.and_all([exact, non_negative, below]);
                let q_zero = int::eq(aig, &q, &zero);
                let r_is_a = int::eq(aig, &r, &x);
                let by_zero = aig.and(q_zero, r_is_a);
                let definition = aig.ite(divisor_is_zero, by_zero, defined);
                self.side.push(definition);
                (q, r)
            }
        };
        self.divisions.insert((a, b), pair.clone());
        pair
    }
}

/// Reads an unknown's value from its circuit and the solver's model.
pub(crate) fn decode(bits: &Bits, value_of: impl Fn(Edge) -> bool) -> Constant {
    match bits {
        Bits::Bool(edge) => Constant::Bool(value_of(*edge)),
        Bits::Int(int) => Constant::Int(int.evaluate(value_of)),
    }
}

/// A value for an unknown the circuit never needed: the value closest to zero in its domain.
pub(crate) fn default_value(sort: Sort, domain: &Interval) -> Constant {
    match sort {
        Sort::Bool => Constant::Bool(false),
        Sort::Int => {
            let zero = BigInt::default();
            let value = match (&domain.lo, &domain.hi) {
                (crate::domain::Ext::Fin(lo), _) if *lo > zero => lo.clone(),
                (_, crate::domain::Ext::Fin(hi)) if *hi < zero => hi.clone(),
                _ => zero,
            };
            Constant::Int(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Span;

    /// Counting constraints, exhaustively: `count op k` and `k op count` for every operator,
    /// every bound from -1 to n + 1 and every assignment of n booleans, n up to 6, give the
    /// same answer as arithmetic. (Bounds may be decided outright, or need a totalizer.)
    #[test]
    fn counting_constraints_match_arithmetic() {
        for n in 2..=6_usize {
            for k in -1..=i64::try_from(n).expect("small") + 1 {
                for op in 0..6 {
                    let mut store = TermStore::default();
                    let bools: Vec<TermId> = (0..n)
                        .map(|i| {
                            store.new_unknown(
                                format!("b{i}"),
                                Sort::Bool,
                                Interval::full(),
                                Span::default(),
                            )
                        })
                        .collect();
                    let (one, zero) = (store.int(BigInt::one()), store.int(BigInt::zero()));
                    let indicators: Vec<TermId> =
                        bools.iter().map(|&b| store.ite(b, one, zero)).collect();
                    let count = indicators[1..]
                        .iter()
                        .fold(indicators[0], |sum, &term| store.add(sum, term));
                    let bound = store.int(BigInt::from(k));
                    let comparison = match op {
                        0 => store.eq(count, bound),
                        1 => store.le(count, bound),
                        2 => store.lt(count, bound),
                        3 => store.le(bound, count),
                        4 => store.lt(bound, count),
                        _ => store.eq(bound, count),
                    };
                    let domains = vec![Interval::full(); store.unknowns().len()];
                    let sizes = domain::forward_sizes(&store, &domains);
                    let mut aig = Aig::new();
                    let blasted = blast(&store, &sizes, &domains, &[comparison], &mut aig);
                    let edge = blasted.of(comparison).edge();
                    for mask in 0..1_u32 << n {
                        let inputs: Vec<bool> = (0..n).map(|i| mask >> i & 1 == 1).collect();
                        let ones = i64::from(mask.count_ones());
                        let expected = match op {
                            0 | 5 => ones == k,
                            1 => ones <= k,
                            2 => ones < k,
                            3 => k <= ones,
                            _ => k < ones,
                        };
                        assert_eq!(
                            aig.simulate(&inputs).value(edge),
                            expected,
                            "n = {n}, k = {k}, op {op}, mask {mask:b}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod weighted_tests {
    use super::*;
    use crate::source::Span;

    /// Weighted sums of `b ? x : y` terms (positive, zero, negative and equal arms) compared with
    /// every bound, in both orientations, for every assignment: the generalized totalizer agrees
    /// with arithmetic.
    #[test]
    fn weighted_sums_match_arithmetic() {
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = |range: i64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            i64::try_from(state % u64::try_from(range).expect("positive")).expect("small")
        };
        for _ in 0..60 {
            let n = 2 + usize::try_from(next(4)).expect("small");
            let arms: Vec<(i64, i64)> = (0..n).map(|_| (next(9) - 2, next(5) - 2)).collect();
            let (lowest, highest): (i64, i64) = arms
                .iter()
                .fold((0, 0), |(lo, hi), &(x, y)| (lo + x.min(y), hi + x.max(y)));
            for k in lowest - 1..=highest + 1 {
                for op in 0..5 {
                    let mut store = TermStore::default();
                    let bools: Vec<TermId> = (0..n)
                        .map(|i| {
                            store.new_unknown(
                                format!("b{i}"),
                                Sort::Bool,
                                Interval::full(),
                                Span::default(),
                            )
                        })
                        .collect();
                    let terms: Vec<TermId> = bools
                        .iter()
                        .zip(&arms)
                        .map(|(&b, &(x, y))| {
                            let (x, y) = (store.int(BigInt::from(x)), store.int(BigInt::from(y)));
                            store.ite(b, x, y)
                        })
                        .collect();
                    let sum = terms[1..]
                        .iter()
                        .fold(terms[0], |sum, &term| store.add(sum, term));
                    let bound = store.int(BigInt::from(k));
                    let comparison = match op {
                        0 => store.eq(sum, bound),
                        1 => store.le(sum, bound),
                        2 => store.lt(sum, bound),
                        3 => store.le(bound, sum),
                        _ => store.lt(bound, sum),
                    };
                    let domains = vec![Interval::full(); store.unknowns().len()];
                    let sizes = domain::forward_sizes(&store, &domains);
                    let mut aig = Aig::new();
                    let blasted = blast(&store, &sizes, &domains, &[comparison], &mut aig);
                    let edge = blasted.of(comparison).edge();
                    // A boolean whose arms are equal folds away and gets no circuit input.
                    let used: Vec<usize> = (0..n)
                        .filter(|&i| blasted.get(bools[i]).is_some())
                        .collect();
                    for mask in 0..1_u32 << n {
                        let values: Vec<bool> = (0..n).map(|i| mask >> i & 1 == 1).collect();
                        let inputs: Vec<bool> = used.iter().map(|&i| values[i]).collect();
                        let value: i64 = arms
                            .iter()
                            .zip(&values)
                            .map(|(&(x, y), &b)| if b { x } else { y })
                            .sum();
                        let expected = match op {
                            0 => value == k,
                            1 => value <= k,
                            2 => value < k,
                            3 => k <= value,
                            _ => k < value,
                        };
                        assert_eq!(
                            aig.simulate(&inputs).value(edge),
                            expected,
                            "arms {arms:?}, k = {k}, op {op}, mask {mask:b}"
                        );
                    }
                }
            }
        }
    }
}
