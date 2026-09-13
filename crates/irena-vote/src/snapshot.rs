//! What a vote froze.

use bornite_core::VoterIdV1;
use borsh::{BorshDeserialize, BorshSerialize};
use irena_core::CompanyIdV1;
use prunella_canonical::{Canonical, hash_canonical};
use prunella_core::{BlockHeight, Hash, PublicKey, TxId};

/// Domain tag for a vote id: the digest of the canonical snapshot.
pub const VOTE_ID_TAG: &str = "IRENA/vote/v1/id";

/// Identifies a vote: the digest of its frozen snapshot.
///
/// Two votes frozen from identical inputs are the same vote, which is what makes a
/// ballot's reference to one unambiguous.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct VoteIdV1(Hash);

impl VoteIdV1 {
    /// Wraps a digest.
    #[must_use]
    pub const fn from_hash(hash: Hash) -> Self {
        Self(hash)
    }

    /// The digest.
    #[must_use]
    pub const fn hash(self) -> Hash {
        self.0
    }
}

impl core::fmt::Display for VoteIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl core::fmt::Debug for VoteIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl serde::Serialize for VoteIdV1 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.0, serializer)
    }
}

/// One voter as frozen: id, weight, and the key they may sign with.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ElectorateEntryV1 {
    /// The voter id, as text so the snapshot is self-contained.
    pub id: String,
    /// Voting weight.
    pub weight: u64,
    /// Whether the rules may exclude this voter. Always false for a share register.
    pub excluded: bool,
    /// The registered signing key, if any.
    pub key: Option<PublicKey>,
}

/// Everything a vote is decided against, fixed at freeze time.
///
/// Records are pinned by transaction id: a Prunella transaction id commits to the
/// payload bytes, so pinning the id pins the exact register and rules. The electorate
/// is included in full so the snapshot can be evaluated and audited on its own, and
/// re-derived from the pinned register by a verifier.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct VoteSnapshotV1 {
    /// Which company.
    pub company: String,
    /// What is being voted on, as an opaque label.
    pub subject: String,
    /// Digest of the proposal document. Never interpreted.
    pub proposal_digest: Hash,
    /// The height the company was resolved at.
    pub height: BlockHeight,
    /// The founding record in force at that height.
    pub genesis_tx_id: TxId,
    /// The share register in force at that height.
    pub shares_tx_id: TxId,
    /// The voting rules in force at that height.
    pub rules_tx_id: TxId,
    /// The derived electorate, in voter id order.
    pub electorate: Vec<ElectorateEntryV1>,
}

impl Canonical for VoteSnapshotV1 {}

impl VoteSnapshotV1 {
    /// The vote id: the digest of this snapshot's canonical bytes.
    #[must_use]
    pub fn id(&self) -> VoteIdV1 {
        VoteIdV1(Hash::from_bytes(hash_canonical(VOTE_ID_TAG, self)))
    }

    /// The company label, validated.
    ///
    /// # Errors
    ///
    /// Returns [`irena_core::IrenaError`] if a decoded snapshot carries a label that
    /// is not one.
    pub fn company(&self) -> Result<CompanyIdV1, irena_core::IrenaError> {
        CompanyIdV1::new(self.company.clone())
    }

    /// Finds a frozen voter by id.
    #[must_use]
    pub fn entry(&self, id: &VoterIdV1) -> Option<&ElectorateEntryV1> {
        self.electorate
            .binary_search_by(|entry| entry.id.as_str().cmp(id.as_str()))
            .ok()
            .map(|index| &self.electorate[index])
    }

    /// Rebuilds the Bornite electorate from the frozen entries.
    ///
    /// # Errors
    ///
    /// Returns [`bornite_core::CoreError`] if a decoded snapshot's entries are not a
    /// valid electorate (a bad id, a zero weight, a duplicate).
    pub fn electorate(&self) -> Result<bornite_core::ElectorateV1, bornite_core::CoreError> {
        let voters = self
            .electorate
            .iter()
            .map(|entry| {
                Ok(bornite_core::VoterV1 {
                    id: VoterIdV1::new(entry.id.clone())?,
                    weight: bornite_core::WeightV1::new(entry.weight)?,
                    excluded: entry.excluded,
                })
            })
            .collect::<Result<Vec<_>, bornite_core::CoreError>>()?;
        bornite_core::ElectorateV1::new(voters)
    }

    /// Whether the entries are in strict voter id order, which `entry` relies on.
    #[must_use]
    pub fn is_sorted(&self) -> bool {
        self.electorate
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id)
    }
}
