//! Integer domains: intervals, bound propagation, and exact sizing.
//!
//! This is how RedSAT gets rid of overflow without asking for bit widths (design doc §4.5):
//!
//! 1. [`propagate`] narrows the domain of every integer unknown using the program's top-level
//!    constraints (`x > 1`, `x * y == n`, …), until nothing changes or an iteration cap is hit.
//! 2. [`forward_sizes`] then computes, *from the unknowns' domains only*, a range that each
//!    intermediate term is guaranteed to lie in, whatever the constraints say. These ranges
//!    decide the bit widths. Using only forward reasoning here is essential: a range narrowed by
//!    a constraint like `x * y == n` holds in solutions, but sizing `x * y` by it would let
//!    non-solutions overflow into fake solutions.

use std::fmt;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, Zero};

use crate::ir::{Term, TermId, TermStore};

/// An extended integer: a finite value or ±∞.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ext {
    /// −∞.
    NegInf,
    /// A finite value.
    Fin(BigInt),
    /// +∞.
    PosInf,
}

impl Ext {
    fn neg(&self) -> Ext {
        match self {
            Ext::NegInf => Ext::PosInf,
            Ext::Fin(value) => Ext::Fin(-value),
            Ext::PosInf => Ext::NegInf,
        }
    }

    /// Sum; `∞ + −∞` never arises in interval arithmetic and is treated as unbounded.
    fn add(&self, other: &Ext) -> Ext {
        match (self, other) {
            (Ext::Fin(a), Ext::Fin(b)) => Ext::Fin(a + b),
            (Ext::NegInf, Ext::PosInf) | (Ext::PosInf, Ext::NegInf) => Ext::PosInf,
            (Ext::NegInf, _) | (_, Ext::NegInf) => Ext::NegInf,
            _ => Ext::PosInf,
        }
    }

    /// Product, with `0 · ±∞ = 0` (one factor is exactly zero).
    fn mul(&self, other: &Ext) -> Ext {
        match (self, other) {
            (Ext::Fin(a), Ext::Fin(b)) => Ext::Fin(a * b),
            (Ext::Fin(a), infinite) | (infinite, Ext::Fin(a)) => {
                if a.is_zero() {
                    Ext::Fin(BigInt::zero())
                } else if a.is_positive() == (*infinite == Ext::PosInf) {
                    Ext::PosInf
                } else {
                    Ext::NegInf
                }
            }
            (a, b) if a == b => Ext::PosInf,
            _ => Ext::NegInf,
        }
    }
}

/// A set of consecutive integers `lo..=hi`; either end may be infinite. Empty when `lo > hi`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interval {
    /// The lower end.
    pub lo: Ext,
    /// The upper end.
    pub hi: Ext,
}

impl Interval {
    /// All integers.
    #[must_use]
    pub fn full() -> Self {
        Self {
            lo: Ext::NegInf,
            hi: Ext::PosInf,
        }
    }

    /// `lo..=hi`.
    #[must_use]
    pub fn new(lo: BigInt, hi: BigInt) -> Self {
        Self {
            lo: Ext::Fin(lo),
            hi: Ext::Fin(hi),
        }
    }

    /// The single value `value`.
    #[must_use]
    pub fn point(value: BigInt) -> Self {
        Self::new(value.clone(), value)
    }

    /// Whether the interval contains no integer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lo > self.hi
    }

    /// Both ends, if both are finite.
    #[must_use]
    pub fn bounds(&self) -> Option<(&BigInt, &BigInt)> {
        match (&self.lo, &self.hi) {
            (Ext::Fin(lo), Ext::Fin(hi)) => Some((lo, hi)),
            _ => None,
        }
    }

    /// Whether `value` lies in the interval.
    #[must_use]
    pub fn contains(&self, value: &BigInt) -> bool {
        let value = Ext::Fin(value.clone());
        self.lo <= value && value <= self.hi
    }

    fn contains_zero(&self) -> bool {
        self.contains(&BigInt::zero())
    }

    /// The intersection.
    #[must_use]
    pub fn intersect(&self, other: &Interval) -> Interval {
        Interval {
            lo: self.lo.clone().max(other.lo.clone()),
            hi: self.hi.clone().min(other.hi.clone()),
        }
    }

    /// The smallest interval containing both.
    #[must_use]
    pub fn hull(&self, other: &Interval) -> Interval {
        Interval {
            lo: self.lo.clone().min(other.lo.clone()),
            hi: self.hi.clone().max(other.hi.clone()),
        }
    }

    fn neg(&self) -> Interval {
        Interval {
            lo: self.hi.neg(),
            hi: self.lo.neg(),
        }
    }

    fn add(&self, other: &Interval) -> Interval {
        Interval {
            lo: self.lo.add(&other.lo),
            hi: self.hi.add(&other.hi),
        }
    }

    fn sub(&self, other: &Interval) -> Interval {
        self.add(&other.neg())
    }

    fn mul(&self, other: &Interval) -> Interval {
        if self.is_empty() || other.is_empty() {
            return Interval {
                lo: Ext::PosInf,
                hi: Ext::NegInf,
            };
        }
        let corners = [
            self.lo.mul(&other.lo),
            self.lo.mul(&other.hi),
            self.hi.mul(&other.lo),
            self.hi.mul(&other.hi),
        ];
        Interval {
            lo: corners.iter().min().expect("four corners").clone(),
            hi: corners.iter().max().expect("four corners").clone(),
        }
    }

    fn square(&self) -> Interval {
        let product = self.mul(self);
        if self.contains_zero() {
            Interval {
                lo: Ext::Fin(BigInt::zero()),
                hi: product.hi,
            }
        } else {
            let lo = self.lo.mul(&self.lo).min(self.hi.mul(&self.hi));
            Interval { lo, hi: product.hi }
        }
    }

    /// The integers `a` with `a · b = c` for some `b ∈ divisor`, `c ∈ self`. Only meaningful when
    /// `divisor` excludes zero; otherwise returns the full interval.
    fn exact_quotient(&self, divisor: &Interval) -> Interval {
        if divisor.contains_zero() || self.is_empty() || divisor.is_empty() {
            return Interval::full();
        }
        // c / b is monotone in each argument when b keeps its sign, so the extremes are at the
        // corners. Integer solutions lie between the ceiling of the smallest real quotient and
        // the floor of the largest one.
        let mut lo = Ext::PosInf;
        let mut hi = Ext::NegInf;
        for c in [&self.lo, &self.hi] {
            for b in [&divisor.lo, &divisor.hi] {
                let (low, high) = match (c, b) {
                    (Ext::Fin(c), Ext::Fin(b)) => {
                        (Ext::Fin(-((-c).div_floor(b))), Ext::Fin(c.div_floor(b)))
                    }
                    (Ext::Fin(_), _) => (Ext::Fin(BigInt::zero()), Ext::Fin(BigInt::zero())),
                    (infinite, Ext::Fin(b)) => {
                        let sign = if b.is_positive() {
                            infinite.clone()
                        } else {
                            infinite.neg()
                        };
                        (sign.clone(), sign)
                    }
                    _ => return Interval::full(),
                };
                lo = lo.min(low);
                hi = hi.max(high);
            }
        }
        Interval { lo, hi }
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let end = |ext: &Ext| match ext {
            Ext::NegInf => "-∞".to_string(),
            Ext::Fin(value) => value.to_string(),
            Ext::PosInf => "∞".to_string(),
        };
        write!(f, "[{}, {}]", end(&self.lo), end(&self.hi))
    }
}

/// The range of an integer term, computed from its operands' ranges.
fn forward(store: &TermStore, term: &Term, ranges: &[Interval]) -> Interval {
    let of = |id: TermId| &ranges[id.index()];
    match term {
        Term::Const(constant) => match constant {
            crate::ir::Constant::Int(value) => Interval::point(value.clone()),
            crate::ir::Constant::Bool(_) => Interval::full(),
        },
        Term::Unknown(unknown) => store.unknown(*unknown).domain.clone(),
        Term::Neg(a) => of(*a).neg(),
        Term::Add(a, b) => of(*a).add(of(*b)),
        Term::Sub(a, b) => of(*a).sub(of(*b)),
        Term::Mul(a, b) if a == b => of(*a).square(),
        Term::Mul(a, b) => of(*a).mul(of(*b)),
        Term::Ite(_, a, b) => of(*a).hull(of(*b)),
        _ => Interval::full(), // boolean terms carry no integer range
    }
}

/// The maximum number of propagation rounds. Each round costs time linear in the program size,
/// so the cap keeps domain inference polynomial even when bounds would otherwise creep forever
/// (e.g. `x < y and y < x` shrinks both by one per round).
const MAX_ROUNDS: usize = 64;

/// Narrows integer ranges using the `required` constraints. Returns a range per term (only
/// integer terms are meaningful), or `None` if some range became empty, which proves that the
/// constraints have no solution.
#[must_use]
pub fn propagate(store: &TermStore, required: &[TermId]) -> Option<Vec<Interval>> {
    let mut ranges = vec![Interval::full(); store.len()];
    for (id, term) in store.terms() {
        ranges[id.index()] = forward(store, term, &ranges);
    }

    let atoms = required_atoms(store, required);
    for _ in 0..MAX_ROUNDS {
        let mut changed = false;
        let mut narrow = |ranges: &mut Vec<Interval>, id: TermId, bound: Interval| -> bool {
            let narrowed = ranges[id.index()].intersect(&bound);
            if narrowed != ranges[id.index()] {
                ranges[id.index()] = narrowed;
                changed = true;
            }
            !ranges[id.index()].is_empty()
        };

        // From the constraints down to their operands.
        for &(atom, negated) in &atoms {
            let ok = match (store.term(atom), negated) {
                (Term::Lt(a, b), false) => below(&mut ranges, &mut narrow, *a, *b, true),
                (Term::Lt(a, b), true) => below(&mut ranges, &mut narrow, *b, *a, false),
                (Term::Le(a, b), false) => below(&mut ranges, &mut narrow, *a, *b, false),
                (Term::Le(a, b), true) => below(&mut ranges, &mut narrow, *b, *a, true),
                (Term::Eq(a, b), false) => {
                    let common = ranges[a.index()].intersect(&ranges[b.index()]);
                    narrow(&mut ranges, *a, common.clone()) && narrow(&mut ranges, *b, common)
                }
                _ => true,
            };
            if !ok {
                return None;
            }
        }

        // From each term down to its operands, in reverse creation order.
        for (id, term) in store.terms().collect::<Vec<_>>().into_iter().rev() {
            let result = ranges[id.index()].clone();
            let ok = match *term {
                Term::Neg(a) => narrow(&mut ranges, a, result.neg()),
                Term::Add(a, b) => {
                    let for_a = result.sub(&ranges[b.index()]);
                    let for_b = result.sub(&ranges[a.index()]);
                    narrow(&mut ranges, a, for_a) && narrow(&mut ranges, b, for_b)
                }
                Term::Sub(a, b) => {
                    let for_a = result.add(&ranges[b.index()]);
                    let for_b = ranges[a.index()].sub(&result);
                    narrow(&mut ranges, a, for_a) && narrow(&mut ranges, b, for_b)
                }
                Term::Mul(a, b) if a == b => match &result.hi {
                    Ext::Fin(hi) if hi.is_negative() => false,
                    Ext::Fin(hi) => {
                        let root = hi.sqrt();
                        narrow(&mut ranges, a, Interval::new(-root.clone(), root))
                    }
                    _ => true,
                },
                Term::Mul(a, b) => {
                    let for_a = result.exact_quotient(&ranges[b.index()]);
                    let for_b = result.exact_quotient(&ranges[a.index()]);
                    narrow(&mut ranges, a, for_a) && narrow(&mut ranges, b, for_b)
                }
                _ => true,
            };
            if !ok {
                return None;
            }
        }

        // From operands back up to the terms built from them.
        for (id, term) in store.terms() {
            if store.sort(id) == crate::ir::Sort::Int {
                let bound = forward(store, term, &ranges);
                if !narrow(&mut ranges, id, bound) {
                    return None;
                }
            }
        }

        if !changed {
            break;
        }
    }
    Some(ranges)
}

/// Applies `a < b` (strict) or `a <= b` to the ranges of `a` and `b`.
fn below(
    ranges: &mut Vec<Interval>,
    narrow: &mut impl FnMut(&mut Vec<Interval>, TermId, Interval) -> bool,
    a: TermId,
    b: TermId,
    strict: bool,
) -> bool {
    let gap = Ext::Fin(BigInt::from(i32::from(strict)));
    let a_max = ranges[b.index()].hi.add(&gap.neg());
    let b_min = ranges[a.index()].lo.add(&gap);
    narrow(
        ranges,
        a,
        Interval {
            lo: Ext::NegInf,
            hi: a_max,
        },
    ) && narrow(
        ranges,
        b,
        Interval {
            lo: b_min,
            hi: Ext::PosInf,
        },
    )
}

/// The comparison atoms that every solution satisfies, with a flag for negation: conjunctions
/// are split and negations pushed inwards through `or` and `not`.
fn required_atoms(store: &TermStore, required: &[TermId]) -> Vec<(TermId, bool)> {
    let mut atoms = Vec::new();
    let mut pending: Vec<(TermId, bool)> = required.iter().map(|&id| (id, false)).collect();
    while let Some((id, negated)) = pending.pop() {
        match (store.term(id), negated) {
            (Term::And(a, b), false) | (Term::Or(a, b), true) => {
                pending.push((*a, negated));
                pending.push((*b, negated));
            }
            (Term::Not(a), _) => pending.push((*a, !negated)),
            (Term::Lt(..) | Term::Le(..) | Term::Eq(..), _) => atoms.push((id, negated)),
            _ => {}
        }
    }
    atoms
}

/// Computes a guaranteed range for every term from the unknowns' domains alone (forward only).
/// `domains` gives the final domain of each unknown, by unknown index.
#[must_use]
pub fn forward_sizes(store: &TermStore, domains: &[Interval]) -> Vec<Interval> {
    let mut ranges = vec![Interval::full(); store.len()];
    for (id, term) in store.terms() {
        ranges[id.index()] = match term {
            Term::Unknown(unknown) => domains[unknown.index()].clone(),
            _ => forward(store, term, &ranges),
        };
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Sort;
    use crate::source::Span;

    fn int(value: i64) -> BigInt {
        BigInt::from(value)
    }

    fn unknown(store: &mut TermStore, name: &str, domain: Interval) -> TermId {
        store.new_unknown(name.into(), Sort::Int, domain, Span::default())
    }

    #[test]
    fn interval_arithmetic_handles_infinities() {
        let at_least_two = Interval {
            lo: Ext::Fin(int(2)),
            hi: Ext::PosInf,
        };
        let small = Interval::new(int(0), int(5));
        assert_eq!(
            at_least_two.mul(&small),
            Interval {
                lo: Ext::Fin(int(0)),
                hi: Ext::PosInf
            }
        );
        assert_eq!(
            Interval::full().mul(&Interval::point(int(0))),
            Interval::point(int(0))
        );
        assert_eq!(
            Interval::new(int(-3), int(2)).square(),
            Interval::new(int(0), int(9))
        );
        assert_eq!(
            Interval::new(int(2), int(3)).square(),
            Interval::new(int(4), int(9))
        );
    }

    #[test]
    fn quotients_round_inwards_and_respect_signs() {
        let n = Interval::point(int(100));
        assert_eq!(
            n.exact_quotient(&Interval::new(int(3), int(7))),
            Interval::new(int(15), int(33))
        );
        let at_least_two = Interval {
            lo: Ext::Fin(int(2)),
            hi: Ext::PosInf,
        };
        assert_eq!(
            n.exact_quotient(&at_least_two),
            Interval::new(int(0), int(50))
        );
        assert_eq!(
            n.exact_quotient(&Interval::new(int(-1), int(1))),
            Interval::full()
        );
    }

    #[test]
    fn infers_factor_bounds_from_the_product() {
        // x > 1, y > 1, x * y == 1000 → x, y ∈ [2, 500]
        let mut store = TermStore::default();
        let x = unknown(&mut store, "x", Interval::full());
        let y = unknown(&mut store, "y", Interval::full());
        let one = store.int(int(1));
        let n = store.int(int(1000));
        let product = store.mul(x, y);
        let constraints = [store.lt(one, x), store.lt(one, y), store.eq(product, n)];
        let ranges = propagate(&store, &constraints).unwrap();
        assert_eq!(ranges[x.index()], Interval::new(int(2), int(500)));
        assert_eq!(ranges[y.index()], Interval::new(int(2), int(500)));
    }

    #[test]
    fn infers_bounds_through_sums_and_squares() {
        // c + r == 35 with c, r >= 0 → both ≤ 35;  a*a ≤ 50 → |a| ≤ 7
        let mut store = TermStore::default();
        let c = unknown(&mut store, "c", Interval::full());
        let r = unknown(&mut store, "r", Interval::full());
        let a = unknown(&mut store, "a", Interval::full());
        let zero = store.int(int(0));
        let total = store.int(int(35));
        let fifty = store.int(int(50));
        let sum = store.add(c, r);
        let square = store.mul(a, a);
        let constraints = [
            store.eq(sum, total),
            store.le(zero, c),
            store.le(zero, r),
            store.le(square, fifty),
        ];
        let ranges = propagate(&store, &constraints).unwrap();
        assert_eq!(ranges[c.index()], Interval::new(int(0), int(35)));
        assert_eq!(ranges[a.index()], Interval::new(int(-7), int(7)));
    }

    #[test]
    fn proves_infeasibility_by_an_empty_domain() {
        let mut store = TermStore::default();
        let x = unknown(&mut store, "x", Interval::new(int(0), int(9)));
        let ten = store.int(int(10));
        let constraint = store.lt(ten, x);
        assert!(propagate(&store, &[constraint]).is_none());
    }

    #[test]
    fn ignores_constraints_under_disjunctions() {
        // (x < 5 or x > 10) gives no single interval, so nothing may be narrowed.
        let mut store = TermStore::default();
        let x = unknown(&mut store, "x", Interval::new(int(0), int(20)));
        let five = store.int(int(5));
        let ten = store.int(int(10));
        let below = store.lt(x, five);
        let above = store.lt(ten, x);
        let either = store.or(below, above);
        let ranges = propagate(&store, &[either]).unwrap();
        assert_eq!(ranges[x.index()], Interval::new(int(0), int(20)));
    }

    #[test]
    fn negated_comparisons_flip() {
        // not (x < 5) → x ≥ 5
        let mut store = TermStore::default();
        let x = unknown(&mut store, "x", Interval::new(int(0), int(20)));
        let five = store.int(int(5));
        let below = store.lt(x, five);
        let constraint = store.not(below);
        let ranges = propagate(&store, &[constraint]).unwrap();
        assert_eq!(ranges[x.index()], Interval::new(int(5), int(20)));
    }

    #[test]
    fn sizes_come_from_unknown_domains_only() {
        let mut store = TermStore::default();
        let x = unknown(&mut store, "x", Interval::full());
        let y = unknown(&mut store, "y", Interval::full());
        let product = store.mul(x, y);
        let domains = [
            Interval::new(int(2), int(500)),
            Interval::new(int(-3), int(4)),
        ];
        let sizes = forward_sizes(&store, &domains);
        assert_eq!(sizes[product.index()], Interval::new(int(-1500), int(2000)));
    }
}
