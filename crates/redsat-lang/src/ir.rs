//! The word-level intermediate representation: a hash-consed DAG of typed terms.
//!
//! Elaboration turns every operation on unknowns into a term. Terms are created bottom-up, so
//! a term's operands always have smaller ids; passes can therefore walk the store in id order
//! (forwards or backwards) without recursion. Constructors apply light simplifications such as
//! constant folding, `x + 0 = x` and `x == x`. Identical terms are shared, which gives common
//! subexpression elimination for free.

use std::collections::HashMap;
use std::fmt;

use num_bigint::BigInt;
use num_traits::{One, Zero};

use crate::domain::Interval;
use crate::source::Span;

/// Identifies a term in a [`TermStore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TermId(u32);

impl TermId {
    /// The position of the term in its store.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifies an unknown in a [`TermStore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnknownId(u32);

impl UnknownId {
    /// The position of the unknown in its store.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The sort (type) of a term.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    /// A truth value.
    Bool,
    /// An integer.
    Int,
}

/// A concrete value of a term.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Constant {
    /// A truth value.
    Bool(bool),
    /// An integer.
    Int(BigInt),
}

impl Constant {
    /// The truth value. Panics on integers (the IR is well-sorted by construction).
    #[must_use]
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Int(_) => panic!("expected a bool constant"),
        }
    }

    /// The integer. Panics on truth values.
    #[must_use]
    pub fn as_int(&self) -> &BigInt {
        match self {
            Self::Int(value) => value,
            Self::Bool(_) => panic!("expected an int constant"),
        }
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
        }
    }
}

/// A term. Operands are referred to by id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum Term {
    Const(Constant),
    Unknown(UnknownId),
    Not(TermId),
    And(TermId, TermId),
    Or(TermId, TermId),
    Xor(TermId, TermId),
    /// `cond ? then : else`, of either sort.
    Ite(TermId, TermId, TermId),
    Neg(TermId),
    Add(TermId, TermId),
    Sub(TermId, TermId),
    Mul(TermId, TermId),
    /// Euclidean division; `x / 0 = 0`.
    Div(TermId, TermId),
    /// Euclidean remainder, always in `0..|divisor|`; `x % 0 = x`.
    Mod(TermId, TermId),
    BitAnd(TermId, TermId),
    BitOr(TermId, TermId),
    BitXor(TermId, TermId),
    /// No two of the integers are equal (encoded with a one-hot view when domains are small).
    AllDifferent(Vec<TermId>),
    Eq(TermId, TermId),
    Lt(TermId, TermId),
    Le(TermId, TermId),
}

impl Term {
    /// The operand ids, in order.
    #[must_use]
    pub fn operands(&self) -> Vec<TermId> {
        if let Self::AllDifferent(items) = self {
            return items.clone();
        }
        match *self {
            Self::Const(_) | Self::Unknown(_) => vec![],
            Self::Not(a) | Self::Neg(a) => vec![a],
            Self::And(a, b)
            | Self::Or(a, b)
            | Self::Xor(a, b)
            | Self::Add(a, b)
            | Self::Sub(a, b)
            | Self::Mul(a, b)
            | Self::Div(a, b)
            | Self::Mod(a, b)
            | Self::BitAnd(a, b)
            | Self::BitOr(a, b)
            | Self::BitXor(a, b)
            | Self::Eq(a, b)
            | Self::Lt(a, b)
            | Self::Le(a, b) => vec![a, b],
            Self::Ite(c, a, b) => vec![c, a, b],
            Self::AllDifferent(_) => unreachable!("handled above"),
        }
    }
}

/// A bitwise operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitOp {
    /// `&`
    And,
    /// `|`
    Or,
    /// `xor` on integers
    Xor,
}

/// Euclidean division: `a == q * b + r` with `0 <= r < |b|`. Division by zero is defined as
/// `(0, a)`, which keeps the operation total (the language rejects it separately).
#[must_use]
pub fn euclid(a: &BigInt, b: &BigInt) -> (BigInt, BigInt) {
    use num_integer::Integer;
    use num_traits::Signed;
    if b.is_zero() {
        return (BigInt::zero(), a.clone());
    }
    let r = a.mod_floor(&b.abs());
    let q = (a - &r) / b;
    (q, r)
}

/// A declared unknown.
#[derive(Clone, Debug)]
pub struct Unknown {
    /// The variable name it was declared as.
    pub name: String,
    /// Its sort.
    pub sort: Sort,
    /// Its declared domain (full for unannotated integers; unused for booleans).
    pub domain: Interval,
    /// Where it was declared.
    pub span: Span,
    /// The term standing for it.
    pub term: TermId,
}

/// The store of all terms and unknowns of a program.
#[derive(Debug, Default)]
pub struct TermStore {
    terms: Vec<Term>,
    sorts: Vec<Sort>,
    index: HashMap<Term, TermId>,
    unknowns: Vec<Unknown>,
}

impl TermStore {
    /// The number of terms.
    #[must_use]
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// Whether there are no terms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// The term with id `id`.
    #[must_use]
    pub fn term(&self, id: TermId) -> &Term {
        &self.terms[id.index()]
    }

    /// All terms, in id order.
    pub fn terms(&self) -> impl Iterator<Item = (TermId, &Term)> {
        (0u32..)
            .zip(&self.terms)
            .map(|(index, term)| (TermId(index), term))
    }

    /// The sort of `id`.
    #[must_use]
    pub fn sort(&self, id: TermId) -> Sort {
        self.sorts[id.index()]
    }

    /// The unknown `id`.
    #[must_use]
    pub fn unknown(&self, id: UnknownId) -> &Unknown {
        &self.unknowns[id.index()]
    }

    /// All unknowns, in declaration order.
    #[must_use]
    pub fn unknowns(&self) -> &[Unknown] {
        &self.unknowns
    }

    /// Declares a new unknown and returns its term.
    pub fn new_unknown(
        &mut self,
        name: String,
        sort: Sort,
        domain: Interval,
        span: Span,
    ) -> TermId {
        let id = UnknownId(u32::try_from(self.unknowns.len()).expect("too many unknowns"));
        let term = self.intern(Term::Unknown(id), sort);
        self.unknowns.push(Unknown {
            name,
            sort,
            domain,
            span,
            term,
        });
        term
    }

    /// The constant value of `id`, if it is a constant.
    #[must_use]
    pub fn constant_of(&self, id: TermId) -> Option<&Constant> {
        match self.term(id) {
            Term::Const(constant) => Some(constant),
            _ => None,
        }
    }

    fn int_of(&self, id: TermId) -> Option<&BigInt> {
        match self.constant_of(id) {
            Some(Constant::Int(value)) => Some(value),
            _ => None,
        }
    }

    fn bool_of(&self, id: TermId) -> Option<bool> {
        match self.constant_of(id) {
            Some(Constant::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    fn intern(&mut self, term: Term, sort: Sort) -> TermId {
        if let Some(&id) = self.index.get(&term) {
            return id;
        }
        let id = TermId(u32::try_from(self.terms.len()).expect("too many terms"));
        self.terms.push(term.clone());
        self.sorts.push(sort);
        self.index.insert(term, id);
        id
    }

    /// The boolean constant `value`.
    pub fn bool(&mut self, value: bool) -> TermId {
        self.intern(Term::Const(Constant::Bool(value)), Sort::Bool)
    }

    /// The integer constant `value`.
    pub fn int(&mut self, value: BigInt) -> TermId {
        self.intern(Term::Const(Constant::Int(value)), Sort::Int)
    }

    /// The constant `value`.
    pub fn constant(&mut self, value: Constant) -> TermId {
        match value {
            Constant::Bool(value) => self.bool(value),
            Constant::Int(value) => self.int(value),
        }
    }

    /// `not a`.
    pub fn not(&mut self, a: TermId) -> TermId {
        match self.term(a) {
            Term::Const(Constant::Bool(value)) => {
                let value = !*value;
                self.bool(value)
            }
            Term::Not(inner) => *inner,
            _ => self.intern(Term::Not(a), Sort::Bool),
        }
    }

    /// `a and b`.
    pub fn and(&mut self, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        match (self.bool_of(a), self.bool_of(b)) {
            (Some(false), _) | (_, Some(false)) => self.bool(false),
            (Some(true), _) => b,
            (_, Some(true)) => a,
            _ if a == b => a,
            _ if self.are_complements(a, b) => self.bool(false),
            _ => self.intern(Term::And(a, b), Sort::Bool),
        }
    }

    /// `a or b`.
    pub fn or(&mut self, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        match (self.bool_of(a), self.bool_of(b)) {
            (Some(true), _) | (_, Some(true)) => self.bool(true),
            (Some(false), _) => b,
            (_, Some(false)) => a,
            _ if a == b => a,
            _ if self.are_complements(a, b) => self.bool(true),
            _ => self.intern(Term::Or(a, b), Sort::Bool),
        }
    }

    /// `a xor b`.
    pub fn xor(&mut self, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        match (self.bool_of(a), self.bool_of(b)) {
            (Some(x), Some(y)) => self.bool(x != y),
            (Some(false), _) => b,
            (_, Some(false)) => a,
            (Some(true), _) => self.not(b),
            (_, Some(true)) => self.not(a),
            _ if a == b => self.bool(false),
            _ => self.intern(Term::Xor(a, b), Sort::Bool),
        }
    }

    fn are_complements(&self, a: TermId, b: TermId) -> bool {
        matches!(self.term(a), Term::Not(x) if *x == b)
            || matches!(self.term(b), Term::Not(x) if *x == a)
    }

    /// `a / b` (Euclidean; `a / 0 = 0`).
    pub fn div(&mut self, a: TermId, b: TermId) -> TermId {
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let quotient = euclid(x, y).0;
                self.int(quotient)
            }
            (_, Some(y)) if y.is_one() => a,
            _ => self.intern(Term::Div(a, b), Sort::Int),
        }
    }

    /// `a % b` (Euclidean; `a % 0 = a`).
    pub fn rem(&mut self, a: TermId, b: TermId) -> TermId {
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let remainder = euclid(x, y).1;
                self.int(remainder)
            }
            (_, Some(y)) if y.is_one() => self.int(BigInt::zero()),
            _ => self.intern(Term::Mod(a, b), Sort::Int),
        }
    }

    /// Bitwise `a & b`, `a | b` or `a xor b` on two's-complement integers.
    pub fn bitwise(&mut self, op: BitOp, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        if let (Some(x), Some(y)) = (self.int_of(a), self.int_of(b)) {
            let value = match op {
                BitOp::And => x & y,
                BitOp::Or => x | y,
                BitOp::Xor => x ^ y,
            };
            return self.int(value);
        }
        let term = match op {
            BitOp::And => Term::BitAnd(a, b),
            BitOp::Or => Term::BitOr(a, b),
            BitOp::Xor => Term::BitXor(a, b),
        };
        self.intern(term, Sort::Int)
    }

    /// No two of the integer terms are equal.
    pub fn all_different(&mut self, mut items: Vec<TermId>) -> TermId {
        items.sort_unstable();
        if items.windows(2).any(|pair| pair[0] == pair[1]) {
            return self.bool(false);
        }
        let constants: Vec<&BigInt> = items.iter().filter_map(|&item| self.int_of(item)).collect();
        if constants.len() == items.len() {
            let mut seen = std::collections::HashSet::new();
            let distinct = constants
                .into_iter()
                .all(|value| seen.insert(value.clone()));
            return self.bool(distinct);
        }
        if items.len() < 2 {
            return self.bool(true);
        }
        self.intern(Term::AllDifferent(items), Sort::Bool)
    }

    /// `a implies b`.
    pub fn implies(&mut self, a: TermId, b: TermId) -> TermId {
        let not_a = self.not(a);
        self.or(not_a, b)
    }

    /// `a iff b`.
    pub fn iff(&mut self, a: TermId, b: TermId) -> TermId {
        let differ = self.xor(a, b);
        self.not(differ)
    }

    /// `cond ? then : otherwise`; `then` and `otherwise` must have the same sort.
    pub fn ite(&mut self, cond: TermId, then: TermId, otherwise: TermId) -> TermId {
        if then == otherwise {
            return then;
        }
        if let Some(value) = self.bool_of(cond) {
            return if value { then } else { otherwise };
        }
        let sort = self.sort(then);
        if sort == Sort::Bool {
            match (self.bool_of(then), self.bool_of(otherwise)) {
                (Some(true), Some(false)) => return cond,
                (Some(false), Some(true)) => return self.not(cond),
                _ => {}
            }
        }
        self.intern(Term::Ite(cond, then, otherwise), sort)
    }

    /// `-a`.
    pub fn neg(&mut self, a: TermId) -> TermId {
        if let Some(value) = self.int_of(a) {
            let negated = -value;
            return self.int(negated);
        }
        if let Term::Neg(inner) = self.term(a) {
            return *inner;
        }
        self.intern(Term::Neg(a), Sort::Int)
    }

    /// `a + b`.
    pub fn add(&mut self, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let sum = x + y;
                self.int(sum)
            }
            (Some(x), _) if x.is_zero() => b,
            (_, Some(y)) if y.is_zero() => a,
            _ => self.intern(Term::Add(a, b), Sort::Int),
        }
    }

    /// `a - b`.
    pub fn sub(&mut self, a: TermId, b: TermId) -> TermId {
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let difference = x - y;
                self.int(difference)
            }
            (_, Some(y)) if y.is_zero() => a,
            (Some(x), _) if x.is_zero() => self.neg(b),
            _ if a == b => self.int(BigInt::zero()),
            _ => self.intern(Term::Sub(a, b), Sort::Int),
        }
    }

    /// `a * b`.
    pub fn mul(&mut self, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let product = x * y;
                self.int(product)
            }
            (Some(x), _) | (_, Some(x)) if x.is_zero() => self.int(BigInt::zero()),
            (Some(x), _) if x.is_one() => b,
            (_, Some(y)) if y.is_one() => a,
            _ => self.intern(Term::Mul(a, b), Sort::Int),
        }
    }

    /// `a == b` for integers (use [`TermStore::iff`] for truth values).
    pub fn eq(&mut self, a: TermId, b: TermId) -> TermId {
        let (a, b) = (a.min(b), a.max(b));
        if a == b {
            return self.bool(true);
        }
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let equal = x == y;
                self.bool(equal)
            }
            _ => self.intern(Term::Eq(a, b), Sort::Bool),
        }
    }

    /// `a < b`.
    pub fn lt(&mut self, a: TermId, b: TermId) -> TermId {
        if a == b {
            return self.bool(false);
        }
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let less = x < y;
                self.bool(less)
            }
            _ => self.intern(Term::Lt(a, b), Sort::Bool),
        }
    }

    /// `a <= b`.
    pub fn le(&mut self, a: TermId, b: TermId) -> TermId {
        if a == b {
            return self.bool(true);
        }
        match (self.int_of(a), self.int_of(b)) {
            (Some(x), Some(y)) => {
                let less_or_equal = x <= y;
                self.bool(less_or_equal)
            }
            _ => self.intern(Term::Le(a, b), Sort::Bool),
        }
    }

    /// Evaluates every term, given a value for every unknown.
    #[must_use]
    pub fn evaluate(&self, unknown_values: &[Constant]) -> Vec<Constant> {
        let mut values: Vec<Constant> = Vec::with_capacity(self.terms.len());
        for term in &self.terms {
            let int = |id: TermId| values[id.index()].as_int();
            let boolean = |id: TermId| values[id.index()].as_bool();
            let value = match term {
                Term::Const(constant) => constant.clone(),
                Term::Unknown(unknown) => unknown_values[unknown.index()].clone(),
                Term::Not(a) => Constant::Bool(!boolean(*a)),
                Term::And(a, b) => Constant::Bool(boolean(*a) && boolean(*b)),
                Term::Or(a, b) => Constant::Bool(boolean(*a) || boolean(*b)),
                Term::Xor(a, b) => Constant::Bool(boolean(*a) != boolean(*b)),
                Term::Ite(c, a, b) => values[if boolean(*c) { a } else { b }.index()].clone(),
                Term::Neg(a) => Constant::Int(-int(*a)),
                Term::Add(a, b) => Constant::Int(int(*a) + int(*b)),
                Term::Sub(a, b) => Constant::Int(int(*a) - int(*b)),
                Term::Mul(a, b) => Constant::Int(int(*a) * int(*b)),
                Term::Div(a, b) => Constant::Int(euclid(int(*a), int(*b)).0),
                Term::Mod(a, b) => Constant::Int(euclid(int(*a), int(*b)).1),
                Term::BitAnd(a, b) => Constant::Int(int(*a) & int(*b)),
                Term::BitOr(a, b) => Constant::Int(int(*a) | int(*b)),
                Term::BitXor(a, b) => Constant::Int(int(*a) ^ int(*b)),
                Term::AllDifferent(items) => {
                    let mut seen = std::collections::HashSet::new();
                    Constant::Bool(items.iter().all(|item| seen.insert(int(*item).clone())))
                }
                Term::Eq(a, b) => Constant::Bool(int(*a) == int(*b)),
                Term::Lt(a, b) => Constant::Bool(int(*a) < int(*b)),
                Term::Le(a, b) => Constant::Bool(int(*a) <= int(*b)),
            };
            values.push(value);
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_unknowns() -> (TermStore, TermId, TermId) {
        let mut store = TermStore::default();
        let x = store.new_unknown("x".into(), Sort::Int, Interval::full(), Span::default());
        let y = store.new_unknown("y".into(), Sort::Int, Interval::full(), Span::default());
        (store, x, y)
    }

    #[test]
    fn folds_constants_and_identities() {
        let (mut store, x, _) = store_with_unknowns();
        let (zero, one, two, three) = (
            store.int(0.into()),
            store.int(1.into()),
            store.int(2.into()),
            store.int(3.into()),
        );
        let five = store.add(two, three);
        assert_eq!(store.constant_of(five), Some(&Constant::Int(5.into())));
        assert_eq!(store.add(x, zero), x);
        assert_eq!(store.mul(one, x), x);
        assert_eq!(store.mul(x, zero), zero);
        let truth = store.eq(x, x);
        assert_eq!(store.constant_of(truth), Some(&Constant::Bool(true)));
        let lt = store.lt(two, three);
        assert_eq!(store.constant_of(lt), Some(&Constant::Bool(true)));
    }

    #[test]
    fn shares_identical_terms_regardless_of_operand_order() {
        let (mut store, x, y) = store_with_unknowns();
        assert_eq!(store.add(x, y), store.add(y, x));
        assert_eq!(store.mul(x, y), store.mul(y, x));
        assert_ne!(store.sub(x, y), store.sub(y, x));
    }

    #[test]
    fn evaluates_under_an_assignment() {
        let (mut store, x, y) = store_with_unknowns();
        let product = store.mul(x, y);
        let ten = store.int(10.into());
        let check = store.lt(product, ten);
        let values = store.evaluate(&[Constant::Int(3.into()), Constant::Int(4.into())]);
        assert_eq!(values[product.index()], Constant::Int(12.into()));
        assert_eq!(values[check.index()], Constant::Bool(false));
    }
}
