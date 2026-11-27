//! Integers as AIG bit-vectors, unsigned or two's complement, with **exact** arithmetic.
//!
//! Every [`IntVec`] has a [`Layout`]: a width and a signedness. Operations come in two flavours:
//!
//! * `add`, `sub`, `mul`, … derive the result layout from the operands' ranges, so the result
//!   can hold every possible value: overflow is impossible by construction;
//! * `add_in`, `sub_in`, `mul_in`, … compute the result modulo `2^width` in a layout chosen by
//!   the caller. This is exact whenever the true value fits that layout. The language front end
//!   uses it with layouts from interval analysis, which can be much tighter than what the
//!   operand layouts alone allow.

use num_bigint::{BigInt, BigUint};
use num_traits::{One, Signed, Zero};

use crate::aig::{Aig, Edge};

/// How an integer is laid out in bits: its width, and whether it is two's complement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Layout {
    /// Number of bits.
    pub width: usize,
    /// Two's complement (`true`) or unsigned (`false`).
    pub signed: bool,
}

impl Layout {
    /// The smallest layout that can represent every integer in `lo..=hi`. Non-negative ranges
    /// get an unsigned layout, so they never pay for a sign bit.
    ///
    /// # Panics
    ///
    /// If `lo > hi`.
    #[must_use]
    pub fn for_range(lo: &BigInt, hi: &BigInt) -> Self {
        assert!(lo <= hi, "empty range {lo}..={hi}");
        if lo.is_negative() {
            Self {
                width: signed_width(lo).max(signed_width(hi)),
                signed: true,
            }
        } else {
            Self {
                width: bit_count(hi),
                signed: false,
            }
        }
    }

    /// The smallest representable value.
    #[must_use]
    pub fn min(self) -> BigInt {
        if self.signed && self.width > 0 {
            -(BigInt::one() << (self.width - 1))
        } else {
            BigInt::zero()
        }
    }

    /// The largest representable value.
    #[must_use]
    pub fn max(self) -> BigInt {
        match (self.signed, self.width) {
            (_, 0) => BigInt::zero(),
            (true, width) => (BigInt::one() << (width - 1)) - 1,
            (false, width) => (BigInt::one() << width) - 1,
        }
    }

    /// Whether `value` is representable in this layout.
    #[must_use]
    pub fn contains(self, value: &BigInt) -> bool {
        &self.min() <= value && value <= &self.max()
    }
}

/// Bits needed for a non-negative magnitude.
fn bit_count(value: &BigInt) -> usize {
    usize::try_from(value.bits()).expect("integer too large")
}

/// Bits needed to hold `value` in two's complement.
fn signed_width(value: &BigInt) -> usize {
    if value.is_negative() {
        bit_count(&(-value - 1)) + 1
    } else {
        bit_count(value) + 1
    }
}

/// An integer as AIG edges, least significant bit first.
///
/// Bits beyond the width read as the implicit extension: the sign bit for signed vectors and
/// `false` for unsigned ones. Vectors of different layouts therefore combine without explicit
/// conversion. The zero-width unsigned vector is the constant 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntVec {
    bits: Vec<Edge>,
    signed: bool,
}

impl IntVec {
    /// Wraps the given bits, least significant first.
    #[must_use]
    pub fn from_bits(bits: Vec<Edge>, signed: bool) -> Self {
        Self { bits, signed }
    }

    /// An unknown integer: `layout.width` fresh circuit inputs.
    pub fn new_input(aig: &mut Aig, layout: Layout) -> Self {
        Self {
            bits: (0..layout.width).map(|_| aig.new_input()).collect(),
            signed: layout.signed,
        }
    }

    /// The constant `value` in its smallest layout.
    #[must_use]
    pub fn constant(value: &BigInt) -> Self {
        Self::constant_in(value, Layout::for_range(value, value))
    }

    /// The constant `value` in the given layout.
    ///
    /// # Panics
    ///
    /// If `value` is not representable in `layout`.
    #[must_use]
    pub fn constant_in(value: &BigInt, layout: Layout) -> Self {
        assert!(layout.contains(value), "{value} does not fit {layout:?}");
        // Two's complement is the value modulo 2^width.
        let modulus = BigInt::one() << layout.width;
        let encoded: BigUint = (((value % &modulus) + &modulus) % &modulus)
            .to_biguint()
            .expect("non-negative by construction");
        let bits = (0..layout.width as u64)
            .map(|i| Edge::constant(encoded.bit(i)))
            .collect();
        Self {
            bits,
            signed: layout.signed,
        }
    }

    /// The layout of this vector.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout {
            width: self.bits.len(),
            signed: self.signed,
        }
    }

    /// The bits, least significant first.
    #[must_use]
    pub fn bits(&self) -> &[Edge] {
        &self.bits
    }

    /// Bit `index`, including the implicit extension beyond the width.
    #[must_use]
    pub fn bit(&self, index: usize) -> Edge {
        match self.bits.get(index) {
            Some(&bit) => bit,
            None if self.signed => self.bits.last().copied().unwrap_or(Edge::FALSE),
            None => Edge::FALSE,
        }
    }

    /// The same value (when it fits) in another layout: extended or truncated.
    #[must_use]
    pub fn resized(&self, layout: Layout) -> Self {
        Self {
            bits: (0..layout.width).map(|i| self.bit(i)).collect(),
            signed: layout.signed,
        }
    }

    /// The value, if every bit is constant.
    #[must_use]
    pub fn as_constant(&self) -> Option<BigInt> {
        self.bits
            .iter()
            .all(|bit| bit.as_constant().is_some())
            .then(|| self.evaluate(|edge| edge.as_constant().expect("checked above")))
    }

    /// The value under an assignment of the circuit, given as the value of each edge.
    pub fn evaluate(&self, value_of: impl Fn(Edge) -> bool) -> BigInt {
        let mut magnitude = BigUint::zero();
        for (index, &bit) in (0u64..).zip(&self.bits) {
            if value_of(bit) {
                magnitude.set_bit(index, true);
            }
        }
        let value = BigInt::from(magnitude);
        let negative = self.signed && self.bits.last().is_some_and(|&msb| value_of(msb));
        if negative {
            value - (BigInt::one() << self.bits.len())
        } else {
            value
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Arithmetic in a caller-chosen layout (modulo 2^width)
// ---------------------------------------------------------------------------------------------

/// `a + b` modulo `2^layout.width`: a ripple-carry adder, O(width).
pub fn add_in(aig: &mut Aig, a: &IntVec, b: &IntVec, layout: Layout) -> IntVec {
    ripple(aig, a, b, false, Edge::FALSE, layout)
}

/// `a - b` modulo `2^layout.width`, computed as `a + ¬b + 1`.
pub fn sub_in(aig: &mut Aig, a: &IntVec, b: &IntVec, layout: Layout) -> IntVec {
    ripple(aig, a, b, true, Edge::TRUE, layout)
}

/// `-a` modulo `2^layout.width`.
pub fn neg_in(aig: &mut Aig, a: &IntVec, layout: Layout) -> IntVec {
    sub_in(aig, &IntVec::constant(&BigInt::zero()), a, layout)
}

/// `a · b` modulo `2^layout.width`: a truncated shift-and-add multiplier.
///
/// The operands are implicitly extended to the result width. Two's complement is a ring modulo
/// `2^width`, so signed operands need no special treatment. Constant zero bits of either operand
/// fold away, so unsigned operands cost about `width(a) · width(b)` full adders.
pub fn mul_in(aig: &mut Aig, a: &IntVec, b: &IntVec, layout: Layout) -> IntVec {
    let width = layout.width;
    let mut acc = vec![Edge::FALSE; width];
    for shift in 0..width {
        let multiplier_bit = b.bit(shift);
        if multiplier_bit == Edge::FALSE {
            continue;
        }
        let mut carry = Edge::FALSE;
        for (index, slot) in acc.iter_mut().enumerate().skip(shift) {
            let partial = aig.and(a.bit(index - shift), multiplier_bit);
            let (sum, carry_out) = full_adder(aig, *slot, partial, carry);
            *slot = sum;
            carry = carry_out;
        }
    }
    IntVec::from_bits(acc, layout.signed)
}

/// `cond ? a : b` in the given layout.
pub fn ite_in(aig: &mut Aig, cond: Edge, a: &IntVec, b: &IntVec, layout: Layout) -> IntVec {
    let bits = (0..layout.width)
        .map(|i| aig.ite(cond, a.bit(i), b.bit(i)))
        .collect();
    IntVec::from_bits(bits, layout.signed)
}

// ---------------------------------------------------------------------------------------------
// Exact arithmetic: the result layout covers every possible value
// ---------------------------------------------------------------------------------------------

/// `a + b`, exactly.
pub fn add(aig: &mut Aig, a: &IntVec, b: &IntVec) -> IntVec {
    let (a_layout, b_layout) = (a.layout(), b.layout());
    let layout = Layout::for_range(
        &(a_layout.min() + b_layout.min()),
        &(a_layout.max() + b_layout.max()),
    );
    add_in(aig, a, b, layout)
}

/// `a - b`, exactly.
pub fn sub(aig: &mut Aig, a: &IntVec, b: &IntVec) -> IntVec {
    let (a_layout, b_layout) = (a.layout(), b.layout());
    let layout = Layout::for_range(
        &(a_layout.min() - b_layout.max()),
        &(a_layout.max() - b_layout.min()),
    );
    sub_in(aig, a, b, layout)
}

/// `-a`, exactly.
pub fn neg(aig: &mut Aig, a: &IntVec) -> IntVec {
    let layout = Layout::for_range(&-a.layout().max(), &-a.layout().min());
    neg_in(aig, a, layout)
}

/// `a · b`, exactly.
pub fn mul(aig: &mut Aig, a: &IntVec, b: &IntVec) -> IntVec {
    let (a_layout, b_layout) = (a.layout(), b.layout());
    let corners = [
        a_layout.min() * b_layout.min(),
        a_layout.min() * b_layout.max(),
        a_layout.max() * b_layout.min(),
        a_layout.max() * b_layout.max(),
    ];
    let lo = corners.iter().min().expect("four corners");
    let hi = corners.iter().max().expect("four corners");
    mul_in(aig, a, b, Layout::for_range(lo, hi))
}

/// `cond ? a : b`, exactly.
pub fn ite(aig: &mut Aig, cond: Edge, a: &IntVec, b: &IntVec) -> IntVec {
    let (a_layout, b_layout) = (a.layout(), b.layout());
    let layout = Layout::for_range(
        &a_layout.min().min(b_layout.min()),
        &a_layout.max().max(b_layout.max()),
    );
    ite_in(aig, cond, a, b, layout)
}

// ---------------------------------------------------------------------------------------------
// Comparisons (always exact, whatever the operand layouts)
// ---------------------------------------------------------------------------------------------

/// The width at which both operands can be compared as signed numbers without loss: an
/// unsigned operand needs one extra (zero) bit to stay non-negative.
fn comparison_width(a: &IntVec, b: &IntVec) -> usize {
    let effective = |x: &IntVec| x.bits.len() + usize::from(!x.signed);
    effective(a).max(effective(b)).max(1)
}

/// `a == b`.
pub fn eq(aig: &mut Aig, a: &IntVec, b: &IntVec) -> Edge {
    let width = comparison_width(a, b);
    let mut equal = Edge::TRUE;
    for index in 0..width {
        let same = aig.iff(a.bit(index), b.bit(index));
        equal = aig.and(equal, same);
    }
    equal
}

/// `a < b`; O(width).
pub fn lt(aig: &mut Aig, a: &IntVec, b: &IntVec) -> Edge {
    // A signed comparison is an unsigned comparison with both sign bits flipped. Scan from the
    // least significant bit up: a differing bit decides unless a higher differing bit overrides.
    let width = comparison_width(a, b);
    let mut less = Edge::FALSE;
    for index in 0..width {
        let flip = index == width - 1;
        let (x, y) = (a.bit(index).negate_if(flip), b.bit(index).negate_if(flip));
        let less_here = aig.and(!x, y);
        let same = aig.iff(x, y);
        let less_below = aig.and(same, less);
        less = aig.or(less_here, less_below);
    }
    less
}

/// `a <= b`.
pub fn le(aig: &mut Aig, a: &IntVec, b: &IntVec) -> Edge {
    !lt(aig, b, a)
}

/// A ripple-carry chain computing `a + (b or ¬b) + carry_in` modulo `2^layout.width`.
fn ripple(
    aig: &mut Aig,
    a: &IntVec,
    b: &IntVec,
    invert_b: bool,
    carry_in: Edge,
    layout: Layout,
) -> IntVec {
    let mut carry = carry_in;
    let mut bits = Vec::with_capacity(layout.width);
    for index in 0..layout.width {
        let (sum, carry_out) =
            full_adder(aig, a.bit(index), b.bit(index).negate_if(invert_b), carry);
        bits.push(sum);
        carry = carry_out;
    }
    IntVec::from_bits(bits, layout.signed)
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
    use crate::aig::Simulation;

    /// Every small layout: unsigned widths 0..=3 and signed widths 1..=3.
    fn layouts() -> Vec<Layout> {
        (0..=3)
            .map(|width| Layout {
                width,
                signed: false,
            })
            .chain((1..=3).map(|width| Layout {
                width,
                signed: true,
            }))
            .collect()
    }

    fn range(layout: Layout) -> Vec<i64> {
        let (lo, hi) = (
            i64::try_from(layout.min()).unwrap(),
            i64::try_from(layout.max()).unwrap(),
        );
        (lo..=hi).collect()
    }

    /// Builds `build(a, b)` over inputs of every pair of small layouts and checks `check` for
    /// every pair of input values by simulation.
    fn for_all_operands<R>(
        build: impl Fn(&mut Aig, &IntVec, &IntVec) -> R,
        check: impl Fn(i64, i64, &R, &Simulation),
    ) {
        for a_layout in layouts() {
            for b_layout in layouts() {
                let mut aig = Aig::new();
                let a = IntVec::new_input(&mut aig, a_layout);
                let b = IntVec::new_input(&mut aig, b_layout);
                let result = build(&mut aig, &a, &b);
                for va in range(a_layout) {
                    for vb in range(b_layout) {
                        let bits_of = |value: i64, width: usize| {
                            (0..width).map(move |i| (value >> i) & 1 == 1)
                        };
                        let inputs: Vec<bool> = bits_of(va, a_layout.width)
                            .chain(bits_of(vb, b_layout.width))
                            .collect();
                        check(va, vb, &result, &aig.simulate(&inputs));
                    }
                }
            }
        }
    }

    fn value(int: &IntVec, sim: &Simulation) -> i64 {
        i64::try_from(int.evaluate(|edge| sim.value(edge))).unwrap()
    }

    #[test]
    fn layouts_fit_their_ranges_tightly() {
        let layout = |lo: i64, hi: i64| Layout::for_range(&lo.into(), &hi.into());
        assert_eq!(
            layout(0, 0),
            Layout {
                width: 0,
                signed: false
            }
        );
        assert_eq!(
            layout(0, 9),
            Layout {
                width: 4,
                signed: false
            }
        );
        assert_eq!(
            layout(2, 500_018_000_049),
            Layout {
                width: 39,
                signed: false
            }
        );
        assert_eq!(
            layout(-1, 0),
            Layout {
                width: 1,
                signed: true
            }
        );
        assert_eq!(
            layout(-40, 50),
            Layout {
                width: 7,
                signed: true
            }
        );
        assert_eq!(
            layout(-128, 127),
            Layout {
                width: 8,
                signed: true
            }
        );
        assert_eq!(
            layout(-129, 0),
            Layout {
                width: 9,
                signed: true
            }
        );
        let byte = Layout {
            width: 8,
            signed: false,
        };
        assert_eq!((byte.min(), byte.max()), (0.into(), 255.into()));
    }

    #[test]
    fn constants_round_trip() {
        for value in -20i64..=20 {
            let int = IntVec::constant(&value.into());
            assert_eq!(int.as_constant(), Some(value.into()));
            let wide = IntVec::constant_in(
                &value.into(),
                Layout {
                    width: 8,
                    signed: true,
                },
            );
            assert_eq!(wide.as_constant(), Some(value.into()));
        }
    }

    #[test]
    fn exact_arithmetic_matches_integers() {
        for_all_operands(
            |aig, a, b| [add(aig, a, b), sub(aig, a, b), mul(aig, a, b), neg(aig, a)],
            |va, vb, [sum, difference, product, negation], sim| {
                assert_eq!(value(sum, sim), va + vb, "{va} + {vb}");
                assert_eq!(value(difference, sim), va - vb, "{va} - {vb}");
                assert_eq!(value(product, sim), va * vb, "{va} * {vb}");
                assert_eq!(value(negation, sim), -va, "-{va}");
            },
        );
    }

    #[test]
    fn comparisons_match_integers_across_layouts() {
        for_all_operands(
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
        for_all_operands(
            |aig, a, b| {
                let a_smaller = lt(aig, a, b);
                ite(aig, a_smaller, a, b)
            },
            |va, vb, min, sim| assert_eq!(value(min, sim), va.min(vb)),
        );
    }

    #[test]
    fn tight_layouts_stay_exact_when_the_value_fits() {
        // x, y ∈ [0, 15] but known (e.g. from interval analysis) to satisfy x·y ≤ 20: a 5-bit
        // result layout suffices instead of the 8 bits the operand layouts would demand.
        let mut aig = Aig::new();
        let nibble = Layout {
            width: 4,
            signed: false,
        };
        let (x, y) = (
            IntVec::new_input(&mut aig, nibble),
            IntVec::new_input(&mut aig, nibble),
        );
        let tight = Layout::for_range(&0.into(), &20.into());
        let product = mul_in(&mut aig, &x, &y, tight);
        assert_eq!(product.layout().width, 5);
        for vx in 0..16i64 {
            for vy in 0..16i64 {
                let inputs: Vec<bool> = (0..4)
                    .map(|i| vx >> i & 1 == 1)
                    .chain((0..4).map(|i| vy >> i & 1 == 1))
                    .collect();
                let got = value(&product, &aig.simulate(&inputs));
                if vx * vy <= 20 {
                    assert_eq!(got, vx * vy);
                } else {
                    assert_eq!(got, (vx * vy) % 32, "modular when it does not fit");
                }
            }
        }
    }

    #[test]
    fn constant_arithmetic_needs_no_gates() {
        let mut aig = Aig::new();
        let six = IntVec::constant(&6.into());
        let minus_seven = IntVec::constant(&(-7).into());
        assert_eq!(
            add(&mut aig, &six, &minus_seven).as_constant(),
            Some((-1).into())
        );
        assert_eq!(
            mul(&mut aig, &six, &minus_seven).as_constant(),
            Some((-42).into())
        );
        assert_eq!(lt(&mut aig, &minus_seven, &six), Edge::TRUE);
        assert_eq!(aig.num_ands(), 0);
    }

    #[test]
    fn multiplying_by_a_power_of_two_is_a_shift() {
        let mut aig = Aig::new();
        let a = IntVec::new_input(
            &mut aig,
            Layout {
                width: 8,
                signed: false,
            },
        );
        let product = mul(&mut aig, &a, &IntVec::constant(&128.into()));
        assert_eq!(aig.num_ands(), 0);
        assert_eq!(product.bits()[7..15], *a.bits());
    }
}
