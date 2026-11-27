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
            Term::Eq(a, b) | Term::Lt(a, b) | Term::Le(a, b) => {
                let (x, y) = (self.int_of(*a), self.int_of(*b));
                Bits::Bool(match term {
                    Term::Eq(..) => int::eq(self.aig, &x, &y),
                    Term::Lt(..) => int::lt(self.aig, &x, &y),
                    _ => int::le(self.aig, &x, &y),
                })
            }
        }
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
