//! Propositional variables and literals.

use std::fmt;
use std::ops::Not;

/// A propositional variable, identified by a dense zero-based index.
///
/// In DIMACS (and IPASIR) notation, which is one-based, `Var::new(i)` is written as `i + 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Var(u32);

impl Var {
    /// The largest supported index. It keeps every DIMACS literal within `i32`.
    pub const MAX_INDEX: u32 = i32::MAX as u32 - 1;

    /// Creates the variable with the given zero-based index.
    ///
    /// # Panics
    ///
    /// If `index` exceeds [`Var::MAX_INDEX`].
    #[must_use]
    pub const fn new(index: u32) -> Self {
        assert!(index <= Self::MAX_INDEX, "variable index out of range");
        Self(index)
    }

    /// The zero-based index of this variable.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }

    /// The literal stating that this variable is true.
    #[must_use]
    pub const fn pos(self) -> Lit {
        Lit::new(self, false)
    }

    /// The literal stating that this variable is false.
    #[must_use]
    pub const fn neg(self) -> Lit {
        Lit::new(self, true)
    }

    /// The one-based DIMACS number of this variable.
    #[must_use]
    pub const fn to_dimacs(self) -> i32 {
        // Lossless: `index <= MAX_INDEX < i32::MAX`.
        self.0 as i32 + 1
    }
}

impl fmt::Display for Var {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "x{}", self.to_dimacs())
    }
}

/// A literal: a variable or its negation.
///
/// Packed as `2 * var + negated`, so both literals of a variable are adjacent and
/// [`Lit::code`] can index dense per-literal tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lit(u32);

impl Lit {
    /// Creates the literal of `var`, negated if `negated` is true.
    #[must_use]
    pub const fn new(var: Var, negated: bool) -> Self {
        Self((var.0 << 1) | negated as u32)
    }

    /// The variable of this literal.
    #[must_use]
    pub const fn var(self) -> Var {
        Var(self.0 >> 1)
    }

    /// Whether this is the negative literal of its variable.
    #[must_use]
    pub const fn is_negated(self) -> bool {
        self.0 & 1 == 1
    }

    /// A dense code in `0..2 * num_vars`, suitable for indexing per-literal tables.
    #[must_use]
    pub const fn code(self) -> u32 {
        self.0
    }

    /// The signed, one-based DIMACS representation of this literal.
    #[must_use]
    pub const fn to_dimacs(self) -> i32 {
        let var = self.var().to_dimacs();
        if self.is_negated() { -var } else { var }
    }

    /// Parses a signed, one-based DIMACS literal.
    ///
    /// # Panics
    ///
    /// If `value` is `0` (the DIMACS clause terminator) or out of range.
    #[must_use]
    pub fn from_dimacs(value: i32) -> Self {
        assert!(value != 0, "0 is a clause terminator, not a literal");
        Self::new(Var::new(value.unsigned_abs() - 1), value < 0)
    }
}

impl Not for Lit {
    type Output = Lit;

    fn not(self) -> Lit {
        Lit(self.0 ^ 1)
    }
}

impl fmt::Display for Lit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_dimacs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_pack_variable_and_sign() {
        let var = Var::new(41);
        assert_eq!(var.pos().var(), var);
        assert_eq!(var.neg().var(), var);
        assert!(!var.pos().is_negated());
        assert!(var.neg().is_negated());
        assert_eq!(!var.pos(), var.neg());
        assert_eq!(!!var.pos(), var.pos());
        assert_eq!(var.pos().code() + 1, var.neg().code());
    }

    #[test]
    fn dimacs_numbering_is_one_based_and_signed() {
        assert_eq!(Var::new(0).to_dimacs(), 1);
        assert_eq!(Var::new(4).neg().to_dimacs(), -5);
        for value in [1, -1, 7, -7, i32::MAX, -i32::MAX] {
            assert_eq!(Lit::from_dimacs(value).to_dimacs(), value);
        }
    }

    #[test]
    #[should_panic(expected = "clause terminator")]
    fn zero_is_not_a_literal() {
        let _ = Lit::from_dimacs(0);
    }
}
