//! Unsigned bit-vectors over AIG edges, with **exact** arithmetic: every result is wide enough to
//! hold its true mathematical value, so overflow cannot happen.
//!
//! These gadgets are the bit-blasting toolbox for the language front end. Signed (two's
//! complement) vectors arrive together with the front end in Phase 1.

use num_bigint::BigUint;

use crate::aig::{Aig, Edge};

/// An unsigned integer as a little-endian vector of AIG edges (bit 0 first).
///
/// Bits beyond the width read as `false`, so vectors of different widths combine naturally. The
/// zero-width vector is the constant 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BitVec {
    bits: Vec<Edge>,
}

impl BitVec {
    /// Wraps the given bits, least significant first.
    #[must_use]
    pub fn from_bits(bits: Vec<Edge>) -> Self {
        Self { bits }
    }

    /// The constant `value`, in the minimal number of bits.
    #[must_use]
    pub fn constant(value: &BigUint) -> Self {
        Self {
            bits: (0..value.bits())
                .map(|i| Edge::constant(value.bit(i)))
                .collect(),
        }
    }

    /// A vector of `width` fresh inputs, i.e. an unknown in `0..2^width`.
    pub fn new_input(aig: &mut Aig, width: usize) -> Self {
        Self {
            bits: (0..width).map(|_| aig.new_input()).collect(),
        }
    }

    /// The number of bits.
    #[must_use]
    pub fn width(&self) -> usize {
        self.bits.len()
    }

    /// The bits, least significant first.
    #[must_use]
    pub fn bits(&self) -> &[Edge] {
        &self.bits
    }

    /// Bit `index`, which is `false` beyond the width.
    #[must_use]
    pub fn bit(&self, index: usize) -> Edge {
        self.bits.get(index).copied().unwrap_or(Edge::FALSE)
    }

    /// The value, if every bit is constant.
    #[must_use]
    pub fn as_constant(&self) -> Option<BigUint> {
        let mut value = BigUint::default();
        for (index, bit) in (0u64..).zip(&self.bits) {
            value.set_bit(index, bit.as_constant()?);
        }
        Some(value)
    }

    /// The value under an assignment of the circuit, given as the value of each edge.
    pub fn evaluate(&self, value_of: impl Fn(Edge) -> bool) -> BigUint {
        let mut value = BigUint::default();
        for (index, &bit) in (0u64..).zip(&self.bits) {
            if value_of(bit) {
                value.set_bit(index, true);
            }
        }
        value
    }

    /// Drops most significant bits that are constant `false`, which never changes the value.
    fn trimmed(mut self) -> Self {
        while self.bits.last() == Some(&Edge::FALSE) {
            self.bits.pop();
        }
        self
    }
}

/// `a + b`, exactly: the result has at most `max(width(a), width(b)) + 1` bits.
///
/// A ripple-carry chain of full adders; O(n) gates.
pub fn add(aig: &mut Aig, a: &BitVec, b: &BitVec) -> BitVec {
    let width = a.width().max(b.width());
    let mut bits = Vec::with_capacity(width + 1);
    let mut carry = Edge::FALSE;
    for index in 0..width {
        let (sum, carry_out) = full_adder(aig, a.bit(index), b.bit(index), carry);
        bits.push(sum);
        carry = carry_out;
    }
    bits.push(carry);
    BitVec::from_bits(bits).trimmed()
}

/// `a · b`, exactly: the result has at most `width(a) + width(b)` bits.
///
/// A shift-and-add array multiplier; O(n·m) gates. Constant operand bits fold away, so
/// multiplying by a constant only builds the rows for its one-bits.
pub fn mul(aig: &mut Aig, a: &BitVec, b: &BitVec) -> BitVec {
    let mut product = BitVec::default();
    for (shift, &multiplier_bit) in b.bits().iter().enumerate() {
        let row: Vec<Edge> = std::iter::repeat_n(Edge::FALSE, shift)
            .chain(a.bits().iter().map(|&bit| aig.and(bit, multiplier_bit)))
            .collect();
        product = add(aig, &product, &BitVec::from_bits(row));
    }
    product
}

/// `a == b`.
pub fn eq(aig: &mut Aig, a: &BitVec, b: &BitVec) -> Edge {
    let width = a.width().max(b.width());
    let mut equal = Edge::TRUE;
    for index in 0..width {
        let same = aig.iff(a.bit(index), b.bit(index));
        equal = aig.and(equal, same);
    }
    equal
}

/// `a < b` (unsigned); O(n) gates.
pub fn lt(aig: &mut Aig, a: &BitVec, b: &BitVec) -> Edge {
    // Scan from the least significant bit up: a differing bit decides the comparison unless a
    // more significant differing bit overrides it.
    let width = a.width().max(b.width());
    let mut less = Edge::FALSE;
    for index in 0..width {
        let (x, y) = (a.bit(index), b.bit(index));
        let less_here = aig.and(!x, y);
        let same = aig.iff(x, y);
        let less_below = aig.and(same, less);
        less = aig.or(less_here, less_below);
    }
    less
}

/// `a <= b` (unsigned).
pub fn le(aig: &mut Aig, a: &BitVec, b: &BitVec) -> Edge {
    !lt(aig, b, a)
}

/// `cond ? a : b`, bitwise.
pub fn ite(aig: &mut Aig, cond: Edge, a: &BitVec, b: &BitVec) -> BitVec {
    let width = a.width().max(b.width());
    let bits = (0..width)
        .map(|index| aig.ite(cond, a.bit(index), b.bit(index)))
        .collect();
    BitVec::from_bits(bits).trimmed()
}

/// Returns `(sum, carry_out)` of `a + b + carry`.
fn full_adder(aig: &mut Aig, a: Edge, b: Edge, carry: Edge) -> (Edge, Edge) {
    let half = aig.xor(a, b);
    let sum = aig.xor(half, carry);
    let generate = aig.and(a, b);
    let propagate = aig.and(half, carry);
    (sum, aig.or(generate, propagate))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_WIDTH: usize = 4;

    /// Builds `op(a, b)` over fresh inputs `a` (`wa` bits) and `b` (`wb` bits), then checks
    /// `check(a, b, result)` for every input combination by simulating the circuit.
    fn for_all_inputs<R>(
        build: impl Fn(&mut Aig, &BitVec, &BitVec) -> R,
        check: impl Fn(u64, u64, &R, &crate::aig::Simulation),
    ) {
        for wa in 0..=MAX_WIDTH {
            for wb in 0..=MAX_WIDTH {
                let mut aig = Aig::new();
                let a = BitVec::new_input(&mut aig, wa);
                let b = BitVec::new_input(&mut aig, wb);
                let result = build(&mut aig, &a, &b);
                for va in 0..1u64 << wa {
                    for vb in 0..1u64 << wb {
                        let inputs: Vec<bool> = (0..wa)
                            .map(|i| va >> i & 1 == 1)
                            .chain((0..wb).map(|i| vb >> i & 1 == 1))
                            .collect();
                        check(va, vb, &result, &aig.simulate(&inputs));
                    }
                }
            }
        }
    }

    fn value(bits: &BitVec, sim: &crate::aig::Simulation) -> u64 {
        u64::try_from(bits.evaluate(|edge| sim.value(edge))).unwrap()
    }

    #[test]
    fn addition_is_exact() {
        for_all_inputs(
            |aig, a, b| (add(aig, a, b), a.width().max(b.width()) + 1),
            |va, vb, (sum, max_width), sim| {
                assert!(sum.width() <= *max_width);
                assert_eq!(value(sum, sim), va + vb, "{va} + {vb}");
            },
        );
    }

    #[test]
    fn multiplication_is_exact() {
        for_all_inputs(
            |aig, a, b| (mul(aig, a, b), a.width() + b.width()),
            |va, vb, (product, max_width), sim| {
                assert!(product.width() <= *max_width);
                assert_eq!(value(product, sim), va * vb, "{va} * {vb}");
            },
        );
    }

    #[test]
    fn comparisons_match_integer_semantics() {
        for_all_inputs(
            |aig, a, b| [eq(aig, a, b), lt(aig, a, b), le(aig, a, b)],
            |va, vb, [eq, lt, le], sim| {
                assert_eq!(sim.value(*eq), va == vb, "{va} == {vb}");
                assert_eq!(sim.value(*lt), va < vb, "{va} < {vb}");
                assert_eq!(sim.value(*le), va <= vb, "{va} <= {vb}");
            },
        );
    }

    #[test]
    fn ite_selects_per_condition() {
        for_all_inputs(
            |aig, a, b| {
                let cond = lt(aig, a, b);
                ite(aig, cond, a, b)
            },
            |va, vb, min, sim| assert_eq!(value(min, sim), va.min(vb)),
        );
    }

    #[test]
    fn constant_arithmetic_needs_no_gates() {
        let mut aig = Aig::new();
        let six = BitVec::constant(&BigUint::from(6u8));
        let seven = BitVec::constant(&BigUint::from(7u8));
        let sum = add(&mut aig, &six, &seven);
        let product = mul(&mut aig, &six, &seven);
        assert_eq!(sum.as_constant(), Some(BigUint::from(13u8)));
        assert_eq!(product.as_constant(), Some(BigUint::from(42u8)));
        assert_eq!(
            eq(&mut aig, &product, &BitVec::constant(&BigUint::from(42u8))),
            Edge::TRUE
        );
        assert_eq!(aig.num_ands(), 0);
    }

    #[test]
    fn multiplying_by_a_constant_builds_only_its_one_bit_rows() {
        let mut general = Aig::new();
        let a = BitVec::new_input(&mut general, 8);
        let b = BitVec::new_input(&mut general, 8);
        let _ = mul(&mut general, &a, &b);

        let mut by_constant = Aig::new();
        let a = BitVec::new_input(&mut by_constant, 8);
        let power_of_two = BitVec::constant(&BigUint::from(128u8)); // a single one-bit
        let product = mul(&mut by_constant, &a, &power_of_two);

        assert_eq!(
            by_constant.num_ands(),
            0,
            "multiplying by 2^7 is a pure shift"
        );
        assert_eq!(product.bits()[7..], *a.bits());
        assert!(general.num_ands() > 0);
    }
}
