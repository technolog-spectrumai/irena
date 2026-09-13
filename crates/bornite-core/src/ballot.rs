//! Ballots and the set of ballots cast in one vote.

use crate::error::CoreError;
use crate::voter::VoterIdV1;
use std::collections::BTreeSet;

/// What a ballot says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChoiceV1 {
    /// In favour.
    Yes,
    /// Against.
    No,
    /// Present, participating, but taking no side.
    ///
    /// An abstention counts toward participation and quorum. Whether it counts in the
    /// threshold denominator is decided by the rules, not by the ballot.
    Abstain,
}

impl ChoiceV1 {
    /// The lowercase text form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Abstain => "abstain",
        }
    }

    /// Parses the lowercase text form exactly.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            "abstain" => Some(Self::Abstain),
            _ => None,
        }
    }
}

impl core::fmt::Display for ChoiceV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One cast ballot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct BallotV1 {
    /// Who cast it.
    pub voter: VoterIdV1,
    /// What it says.
    pub choice: ChoiceV1,
}

/// Every ballot cast in one vote.
///
/// Sorted by voter and free of duplicates from construction. One voter, one ballot: a
/// second ballot from the same voter is refused rather than treated as a correction,
/// because which of the two was "meant" is not something the engine can know.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct BallotSetV1 {
    ballots: Vec<BallotV1>,
}

impl BallotSetV1 {
    /// Builds a ballot set, sorting by voter and refusing duplicates.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::DuplicateBallot`] naming the first repeated voter in sorted
    /// order.
    pub fn new(mut ballots: Vec<BallotV1>) -> Result<Self, CoreError> {
        ballots.sort_by(|left, right| left.voter.cmp(&right.voter));
        let mut seen: BTreeSet<&VoterIdV1> = BTreeSet::new();
        for ballot in &ballots {
            if !seen.insert(&ballot.voter) {
                return Err(CoreError::DuplicateBallot {
                    voter: ballot.voter.clone(),
                });
            }
        }
        Ok(Self { ballots })
    }

    /// An empty ballot set.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ballots: Vec::new(),
        }
    }

    /// The ballots, in voter order.
    #[must_use]
    pub fn ballots(&self) -> &[BallotV1] {
        &self.ballots
    }

    /// Number of ballots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ballots.len()
    }

    /// Whether no ballots were cast.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ballots.is_empty()
    }
}
