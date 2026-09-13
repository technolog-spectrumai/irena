//! Signed ballots and the commitment over them.

use crate::error::BallotRejectionV1;
use crate::snapshot::{VoteIdV1, VoteSnapshotV1};
use bornite_core::{ChoiceV1, VoterIdV1};
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::{Canonical, hash_canonical};
use prunella_core::merkle::{self, TreeTags};
use prunella_core::{Hash, Signature};
use prunella_crypto::SigningKey;

/// Domain tag for the message a holder signs: the canonical ballot body.
pub const BALLOT_SIGN_TAG: &str = "IRENA/vote/v1/ballot-sign";

/// Domain tag for a ballot's digest, the leaf content of the commitment tree.
const BALLOT_DIGEST_TAG: &str = "IRENA/vote/v1/ballot";

/// The Merkle domains of the ballot commitment.
///
/// Irena's own, so a ballot tree can never equal a Prunella block tree over the same
/// leaves. The tree itself is Prunella's implementation, unchanged.
pub const COMMITMENT_TAGS: TreeTags = TreeTags {
    leaf: "IRENA/vote/v1/leaf",
    node: "IRENA/vote/v1/node",
    empty: "IRENA/vote/v1/empty",
};

/// A ballot choice, as it is encoded in a ballot.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BallotChoiceV1 {
    /// For.
    Yes,
    /// Against.
    No,
    /// Present, not deciding.
    Abstain,
}

impl BallotChoiceV1 {
    /// Bornite's choice.
    #[must_use]
    pub const fn to_bornite(self) -> ChoiceV1 {
        match self {
            Self::Yes => ChoiceV1::Yes,
            Self::No => ChoiceV1::No,
            Self::Abstain => ChoiceV1::Abstain,
        }
    }

    /// Parses `yes`, `no` or `abstain`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "yes" => Some(Self::Yes),
            "no" => Some(Self::No),
            "abstain" => Some(Self::Abstain),
            _ => None,
        }
    }

    /// The text form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Abstain => "abstain",
        }
    }
}

/// What a holder signs.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct BallotBodyV1 {
    /// The vote this ballot is for.
    pub vote_id: VoteIdV1,
    /// Who is voting.
    pub voter: String,
    /// What they say.
    pub choice: BallotChoiceV1,
}

impl Canonical for BallotBodyV1 {}

impl BallotBodyV1 {
    /// The message the holder signs: `hash(BALLOT_SIGN_TAG, canonical(body))`.
    #[must_use]
    pub fn signing_message(&self) -> [u8; 32] {
        hash_canonical(BALLOT_SIGN_TAG, self)
    }
}

/// A ballot with the holder's signature over it.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SignedBallotV1 {
    /// The signed body.
    pub body: BallotBodyV1,
    /// Ed25519 signature over [`BallotBodyV1::signing_message`].
    pub signature: Signature,
}

impl Canonical for SignedBallotV1 {}

impl SignedBallotV1 {
    /// Signs a ballot for `vote_id` as `voter`.
    #[must_use]
    pub fn sign(
        key: &SigningKey,
        vote_id: VoteIdV1,
        voter: &VoterIdV1,
        choice: BallotChoiceV1,
    ) -> Self {
        let body = BallotBodyV1 {
            vote_id,
            voter: voter.as_str().to_owned(),
            choice,
        };
        let signature = key.sign(&body.signing_message());
        Self { body, signature }
    }

    /// This ballot's digest: the leaf content of the commitment tree.
    #[must_use]
    pub fn digest(&self) -> Hash {
        Hash::from_bytes(hash_canonical(BALLOT_DIGEST_TAG, self))
    }

    /// Checks this ballot against a frozen snapshot, in the documented order: right
    /// vote, well-formed voter, in the electorate, not excluded, has a key, signature
    /// verifies. Duplicates are the caller's check, since they depend on what was
    /// already accepted.
    ///
    /// # Errors
    ///
    /// Returns the first [`BallotRejectionV1`] that applies.
    pub fn check(&self, snapshot: &VoteSnapshotV1) -> Result<VoterIdV1, BallotRejectionV1> {
        let vote_id = snapshot.id();
        if self.body.vote_id != vote_id {
            return Err(BallotRejectionV1::WrongVote {
                expected: vote_id.to_string(),
                found: self.body.vote_id.to_string(),
            });
        }
        let voter = VoterIdV1::new(self.body.voter.clone()).map_err(|error| {
            BallotRejectionV1::InvalidVoter {
                reason: error.to_string(),
            }
        })?;
        let entry = snapshot
            .entry(&voter)
            .ok_or_else(|| BallotRejectionV1::NotInElectorate {
                voter: voter.to_string(),
            })?;
        if entry.excluded {
            return Err(BallotRejectionV1::Excluded {
                voter: voter.to_string(),
            });
        }
        let key = entry.key.as_ref().ok_or_else(|| BallotRejectionV1::NoKey {
            voter: voter.to_string(),
        })?;
        prunella_crypto::verify(key, &self.body.signing_message(), &self.signature).map_err(
            |error| BallotRejectionV1::BadSignature {
                voter: voter.to_string(),
                detail: error.to_string(),
            },
        )?;
        Ok(voter)
    }
}

/// The Merkle root over ballot digests, in the order given.
///
/// Callers pass ballots sorted by voter id, which is the only order a final record
/// carries them in, so the commitment never depends on arrival order.
#[must_use]
pub fn ballot_commitment(ballots: &[SignedBallotV1]) -> Hash {
    let leaves: Vec<Hash> = ballots.iter().map(SignedBallotV1::digest).collect();
    merkle::root(COMMITMENT_TAGS, &leaves)
}
