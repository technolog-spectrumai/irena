//! Exact fractions.
//!
//! A fraction is an integer numerator over a non-zero integer denominator, and it is
//! never reduced, divided or rounded. The only operation Bornite needs is a comparison
//! — "is this weight above, exactly at, or below this share of that total?" — and that
//! is done by cross-multiplication in `u128`, which cannot overflow for `u64` operands.

use crate::error::CoreError;
use core::cmp::Ordering;
use core::num::NonZeroU64;

/// An exact fraction `numerator / denominator`.
///
/// Equality and ordering are **structural** — `1/2` and `2/4` are different values that
/// sort by numerator then denominator — because a rule that says `2/4` should be echoed
/// as `2/4`. Numeric comparison is [`FractionV1::compare_share`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct FractionV1 {
    /// The numerator.
    pub numerator: u64,
    /// The denominator, never zero.
    pub denominator: NonZeroU64,
}

impl FractionV1 {
    /// Exactly one half.
    pub const HALF: Self = Self {
        numerator: 1,
        denominator: NonZeroU64::new(2).expect("2 is non-zero"),
    };

    /// Builds a fraction.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ZeroDenominator`] for a zero denominator.
    pub const fn new(numerator: u64, denominator: u64) -> Result<Self, CoreError> {
        match NonZeroU64::new(denominator) {
            Some(denominator) => Ok(Self {
                numerator,
                denominator,
            }),
            None => Err(CoreError::ZeroDenominator),
        }
    }

    /// Builds a fraction that must be a proportion, at most one.
    ///
    /// A quorum or threshold is a share of something, and a share above the whole is
    /// not a strict rule but a mistake, so it is refused.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ZeroDenominator`] or [`CoreError::ImproperFraction`].
    pub const fn proportion(numerator: u64, denominator: u64) -> Result<Self, CoreError> {
        if numerator > denominator {
            return Err(CoreError::ImproperFraction {
                numerator,
                denominator,
            });
        }
        Self::new(numerator, denominator)
    }

    /// Compares `value` against this fraction of `basis`.
    ///
    /// Returns how `value / basis` stands relative to `numerator / denominator`, computed
    /// as `value × denominator` against `numerator × basis` in 128-bit arithmetic. Both
    /// products are of two `u64` values and `(2^64 − 1)^2 < 2^128`, so this can never
    /// overflow and never fails.
    #[must_use]
    pub fn compare_share(self, value: u64, basis: u64) -> Ordering {
        let lhs = u128::from(value) * u128::from(self.denominator.get());
        let rhs = u128::from(self.numerator) * u128::from(basis);
        lhs.cmp(&rhs)
    }

    /// Whether the fraction is a proportion, at most one.
    #[must_use]
    pub const fn is_proportion(self) -> bool {
        self.numerator <= self.denominator.get()
    }
}

impl core::fmt::Display for FractionV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{}", self.numerator, self.denominator)
    }
}
