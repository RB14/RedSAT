//! Bit-blasting: lowering word-level terms to an And-Inverter Graph.

use num_bigint::BigInt;
use redsat_circuit::{Aig, Edge, IntVec, Layout, int};

use crate::domain::Interval;
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
    /// Constraints keeping each unknown inside its domain, when its bit layout can represent
    /// more values than the domain allows (e.g. `int[0..=9]` in 4 bits).
    pub(crate) domain_constraints: Vec<Edge>,
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

    let mut bits: Vec<Option<Bits>> = vec![None; store.len()];
    let mut domain_constraints = Vec::new();
    for (id, term) in store.terms() {
        if !reachable[id.index()] {
            continue;
        }
        let of = |operand: TermId| bits[operand.index()].as_ref().expect("operands come first");
        let layout = || layout_of(&sizes[id.index()]);
        let result = match term {
            Term::Const(Constant::Bool(value)) => Bits::Bool(Edge::constant(*value)),
            Term::Const(Constant::Int(value)) => Bits::Int(IntVec::constant(value)),
            Term::Unknown(unknown) => match store.unknown(*unknown).sort {
                Sort::Bool => Bits::Bool(aig.new_input()),
                Sort::Int => {
                    let domain = &domains[unknown.index()];
                    let (lo, hi) = domain.bounds().expect("blasted unknowns are bounded");
                    let layout = Layout::for_range(lo, hi);
                    let vector = IntVec::new_input(aig, layout);
                    if layout.min() < *lo {
                        domain_constraints.push(int::le(aig, &IntVec::constant(lo), &vector));
                    }
                    if layout.max() > *hi {
                        domain_constraints.push(int::le(aig, &vector, &IntVec::constant(hi)));
                    }
                    Bits::Int(vector)
                }
            },
            Term::Not(a) => Bits::Bool(!of(*a).edge()),
            Term::And(a, b) => {
                let (a, b) = (of(*a).edge(), of(*b).edge());
                Bits::Bool(aig.and(a, b))
            }
            Term::Or(a, b) => {
                let (a, b) = (of(*a).edge(), of(*b).edge());
                Bits::Bool(aig.or(a, b))
            }
            Term::Xor(a, b) => {
                let (a, b) = (of(*a).edge(), of(*b).edge());
                Bits::Bool(aig.xor(a, b))
            }
            Term::Ite(c, a, b) => {
                let cond = of(*c).edge();
                match (of(*a), of(*b)) {
                    (Bits::Bool(a), Bits::Bool(b)) => {
                        let (a, b) = (*a, *b);
                        Bits::Bool(aig.ite(cond, a, b))
                    }
                    (a, b) => {
                        let (a, b) = (a.int().clone(), b.int().clone());
                        Bits::Int(int::ite_in(aig, cond, &a, &b, layout()))
                    }
                }
            }
            Term::Neg(a) => {
                let a = of(*a).int().clone();
                Bits::Int(int::neg_in(aig, &a, layout()))
            }
            Term::Add(a, b) | Term::Sub(a, b) | Term::Mul(a, b) => {
                let (x, y) = (of(*a).int().clone(), of(*b).int().clone());
                let layout = layout();
                Bits::Int(match term {
                    Term::Add(..) => int::add_in(aig, &x, &y, layout),
                    Term::Sub(..) => int::sub_in(aig, &x, &y, layout),
                    _ => int::mul_in(aig, &x, &y, layout),
                })
            }
            Term::Eq(a, b) | Term::Lt(a, b) | Term::Le(a, b) => {
                let (x, y) = (of(*a).int().clone(), of(*b).int().clone());
                Bits::Bool(match term {
                    Term::Eq(..) => int::eq(aig, &x, &y),
                    Term::Lt(..) => int::lt(aig, &x, &y),
                    _ => int::le(aig, &x, &y),
                })
            }
        };
        bits[id.index()] = Some(result);
    }
    Blasted {
        bits,
        domain_constraints,
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
