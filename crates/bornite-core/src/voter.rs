//! Voter identity, weight, and the frozen electorate.

use crate::error::CoreError;
use std::collections::BTreeSet;

/// Maximum length of a voter id, in bytes.
pub const MAX_VOTER_ID_LEN: usize = 128;

/// Identifies a voter.
///
/// 1 to 128 bytes of ASCII from `A-Z a-z 0-9 . _ : + @ -`. Ids are compared **byte for
/// byte**: `Alice` and `alice` are two different voters. Case folding depends on locale
/// rules and could silently merge two people, so it is never done.
///
/// Bornite assigns no meaning to an id. It may be a name, a share certificate number, a
/// membership number or a drone's node id; the engine only ever tests ids for equality
/// and orders them by their bytes.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct VoterIdV1(String);

impl VoterIdV1 {
    /// Validates and wraps a voter id.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidVoterId`] if the value is empty, longer than 128
    /// bytes, or contains a byte outside the permitted set.
    pub fn new(value: impl Into<String>) -> Result<Self, CoreError> {
        let value = value.into();
        let reject = |reason| CoreError::InvalidVoterId {
            value: value.clone(),
            reason,
        };
        if value.is_empty() {
            return Err(reject("must not be empty"));
        }
        if value.len() > MAX_VOTER_ID_LEN {
            return Err(reject("must be at most 128 bytes"));
        }
        if !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'+' | b'@' | b'-')
        }) {
            return Err(reject(
                "may only contain ASCII letters, digits, '.', '_', ':', '+', '@' and '-'",
            ));
        }
        Ok(Self(value))
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for VoterIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::fmt::Debug for VoterIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl core::str::FromStr for VoterIdV1 {
    type Err = CoreError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

/// A voting weight. Always at least one.
///
/// Weights are integers by design: a share of a vote that cannot be written as a whole
/// number of units has no place in a result that must be reproduced exactly. Anyone
/// with fractional entitlements scales the whole electorate up until they are whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct WeightV1(u64);

impl WeightV1 {
    /// The weight of one vote.
    pub const ONE: Self = Self(1);

    /// Validates and wraps a weight.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ZeroWeight`] for zero.
    pub const fn new(value: u64) -> Result<Self, CoreError> {
        if value == 0 {
            Err(CoreError::ZeroWeight)
        } else {
            Ok(Self(value))
        }
    }

    /// The weight as an integer.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for WeightV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A sum of weights. May be zero, for an empty set.
///
/// Every addition is checked; there is no path by which a total can wrap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct WeightTotalV1(u64);

impl WeightTotalV1 {
    /// The empty total.
    pub const ZERO: Self = Self(0);

    /// Adds one weight.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::WeightOverflow`] if the sum would exceed `u64::MAX`.
    pub fn checked_add(self, weight: WeightV1) -> Result<Self, CoreError> {
        self.0
            .checked_add(weight.0)
            .map(Self)
            .ok_or(CoreError::WeightOverflow)
    }

    /// Adds another total.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::WeightOverflow`] if the sum would exceed `u64::MAX`.
    pub fn checked_add_total(self, other: Self) -> Result<Self, CoreError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(CoreError::WeightOverflow)
    }

    /// Subtracts a total that is known to be part of this one.
    ///
    /// Returns `None` if `other` is larger, which for a part of a whole can only mean
    /// a bug upstream; there is no negative weight to return.
    #[must_use]
    pub fn checked_sub_total(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Sums a sequence of weights.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::WeightOverflow`] if any partial sum would exceed `u64::MAX`.
    pub fn sum(weights: impl IntoIterator<Item = WeightV1>) -> Result<Self, CoreError> {
        weights.into_iter().try_fold(Self::ZERO, Self::checked_add)
    }

    /// The total as an integer.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }

    /// Whether nothing has been added.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl core::fmt::Display for WeightTotalV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One member of an electorate.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VoterV1 {
    /// Who.
    pub id: VoterIdV1,
    /// How much their vote counts, when the rules use electorate weights.
    pub weight: WeightV1,
    /// Whether the rules may exclude this voter from the effective electorate.
    ///
    /// Bornite does not know why. A conflict of interest, a suspended membership, a
    /// drone under maintenance — the reason belongs to whoever froze the electorate.
    pub excluded: bool,
}

/// A frozen electorate.
///
/// Sorted by voter id and free of duplicates from the moment it is built, so no later
/// code can observe an order that depends on how the input happened to be listed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct ElectorateV1 {
    voters: Vec<VoterV1>,
}

impl ElectorateV1 {
    /// Builds an electorate, sorting by id and refusing duplicates.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::DuplicateVoter`] naming the first repeated id in sorted
    /// order.
    pub fn new(mut voters: Vec<VoterV1>) -> Result<Self, CoreError> {
        voters.sort_by(|left, right| left.id.cmp(&right.id));
        let mut seen: BTreeSet<&VoterIdV1> = BTreeSet::new();
        for voter in &voters {
            if !seen.insert(&voter.id) {
                return Err(CoreError::DuplicateVoter {
                    id: voter.id.clone(),
                });
            }
        }
        Ok(Self { voters })
    }

    /// The voters, in id order.
    #[must_use]
    pub fn voters(&self) -> &[VoterV1] {
        &self.voters
    }

    /// Finds a voter by id.
    #[must_use]
    pub fn get(&self, id: &VoterIdV1) -> Option<&VoterV1> {
        self.voters
            .binary_search_by(|voter| voter.id.cmp(id))
            .ok()
            .map(|index| &self.voters[index])
    }

    /// Number of voters, including excluded ones.
    #[must_use]
    pub fn len(&self) -> usize {
        self.voters.len()
    }

    /// Whether there are no voters at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.voters.is_empty()
    }
}
