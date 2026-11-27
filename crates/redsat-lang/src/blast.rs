//! Bit-blasting: lowering word-level terms to an And-Inverter Graph.

use std::collections::HashMap;

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
    /// more values, e.g. `int[0..=9]` in 4 bits), and the definitions of division results.
    pub(crate) side_constraints: Vec<Edge>,
}

impl Blasted {
    pub(crate) fn of(&self, id: TermId) -> &Bits {
        self.bits[id.index()].as_ref().expect("term was blasted")
    }

    pub(crate) fn get(&self, id: TermId) -> Option<&Bits> {
        self.bits[id.index()].as_ref()
    }
}

/// The largest totalizer (booleans × outputs) built for a counting constraint; beyond it, the
/// count stays a binary sum.
const TOTALIZER_BUDGET: usize = 200_000;

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
    for (id, term) in store.terms() {
        if reachable[id.index()] {
            let result = blaster.term(id, term, domains);
            blaster.bits[id.index()] = Some(result);
        }
    }
    Blasted {
        bits: blaster.bits,
        side_constraints: blaster.side,
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
                if let Some(edge) = self.cardinality(term) {
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

    /// A count compared with a constant (`count(…) <= k`, `exactly`, `at_least`, `at_most`),
    /// encoded with a totalizer instead of an adder and a comparator: unit propagation then
    /// enforces the bound by itself (arc consistency), which the binary encoding does not.
    fn cardinality(&mut self, term: &Term) -> Option<Edge> {
        let (Term::Eq(a, b) | Term::Lt(a, b) | Term::Le(a, b)) = term else {
            return None;
        };
        let constant = |id: TermId| match self.store.term(id) {
            Term::Const(Constant::Int(value)) => Some(value.clone()),
            _ => None,
        };
        let (bools, offset, k, count_on_left) = match (self.as_count(*a), constant(*b)) {
            (Some((bools, offset)), Some(k)) => (bools, offset, k, true),
            _ => match (constant(*a), self.as_count(*b)) {
                (Some(k), Some((bools, offset))) => (bools, offset, k, false),
                _ => return None,
            },
        };
        let n = i64::try_from(bools.len()).ok()?;
        // The bound on the number of true booleans (clamped: anything beyond 0..=n is decided).
        let m = i64::try_from((k - offset).clamp(BigInt::from(-1), BigInt::from(n + 1))).ok()?;
        let (lo, hi) = match (term, count_on_left) {
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
            return Some(self.aig.and_all(bools.iter().map(|&b| !b)));
        }
        if lo == n {
            return Some(self.aig.and_all(bools));
        }
        // Outputs up to hi + 1 are needed; a totalizer that large costs about n·(hi + 1) gates.
        let limit = usize::try_from((hi + 1).min(n)).ok()?;
        if bools.len().saturating_mul(limit) > TOTALIZER_BUDGET {
            return None;
        }
        let outputs = self.totalizer(&bools, limit);
        let mut bounds = Vec::new();
        if lo > 0 {
            bounds.push(outputs[usize::try_from(lo).ok()? - 1]);
        }
        if hi < n {
            bounds.push(!outputs[usize::try_from(hi).ok()?]);
        }
        Some(self.aig.and_all(bounds))
    }

    /// The booleans of a count and its constant part, if `id` is one: a sum (any tree of `+`)
    /// of `b ? 1 : 0` (or `b ? 0 : 1`) terms and constants, as `count(…)` produces.
    fn as_count(&self, id: TermId) -> Option<(Vec<Edge>, BigInt)> {
        let mut bools = Vec::new();
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
                    let edge = self.of(*cond).edge();
                    match (value(*then)?, value(*otherwise)?) {
                        (one, zero) if one.is_one() && zero.is_zero() => bools.push(edge),
                        (zero, one) if one.is_one() && zero.is_zero() => bools.push(!edge),
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        (bools.len() >= 2).then_some((bools, offset))
    }

    /// A totalizer (Bailleux and Boufkhad, 2003): `outputs[j]` holds exactly when at least
    /// `j + 1` of `bools` hold, for `j < limit` (outputs beyond what a bound needs are left out).
    fn totalizer(&mut self, bools: &[Edge], limit: usize) -> Vec<Edge> {
        if bools.len() == 1 {
            return bools.to_vec();
        }
        let (left, right) = bools.split_at(bools.len() / 2);
        let (left, right) = (self.totalizer(left, limit), self.totalizer(right, limit));
        let size = (left.len() + right.len()).min(limit);
        (1..=size)
            .map(|total| {
                let ways: Vec<Edge> = (0..=total.min(left.len()))
                    .filter(|&i| total - i <= right.len())
                    .map(|i| {
                        let a = if i == 0 { Edge::TRUE } else { left[i - 1] };
                        let j = total - i;
                        let b = if j == 0 { Edge::TRUE } else { right[j - 1] };
                        self.aig.and(a, b)
                    })
                    .collect();
                self.aig.or_all(ways)
            })
            .collect()
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
