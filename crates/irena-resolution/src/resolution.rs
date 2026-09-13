//! What a resolution is: its id, kind, status and the authority it rests on.

use borsh::{BorshDeserialize, BorshSerialize};
use irena_core::{RecordKindV1, normalise_body};
use prunella_canonical::hash_domain;
use prunella_core::{Hash, TxId};

/// Domain tag for the digest that binds a proposal to what is executed.
pub const PROPOSAL_TAG: &str = "IRENA/resolution/v1/proposal";

/// The digest an agenda item must carry for an amendment resolution to execute `body`.
///
/// `hash(IRENA/resolution/v1/proposal, normalise_body(body))`. The body is normalised
/// exactly as `irena_core::compose_record` normalises it before embedding, so the
/// bytes digested here are the bytes the ledger will hold.
///
/// This is what closes the loop: the shareholders vote on an agenda item whose
/// proposal digest is this value, and an execution can only publish a body that
/// digests to it.
#[must_use]
pub fn proposal_digest(body: &str) -> Hash {
    Hash::from_bytes(hash_domain(PROPOSAL_TAG, normalise_body(body).as_bytes()))
}

/// Identifies a resolution: the transaction that recorded it.
///
/// A finalised resolution is unique and immutable once on the chain, so nothing else
/// is needed to name it, and nothing can claim to be a resolution that was never
/// recorded.
#[derive(
    BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash,
)]
pub struct ResolutionIdV1(TxId);

impl ResolutionIdV1 {
    /// Wraps a resolution transaction id.
    #[must_use]
    pub const fn from_tx(tx_id: TxId) -> Self {
        Self(tx_id)
    }

    /// The transaction.
    #[must_use]
    pub const fn tx_id(self) -> TxId {
        self.0
    }
}

impl core::fmt::Display for ResolutionIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl core::fmt::Debug for ResolutionIdV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl serde::Serialize for ResolutionIdV1 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&self.0, serializer)
    }
}

/// Which part of the company an amendment resolution replaces.
///
/// Two in V1, and each is an existing `irena-core` record kind: a resolution
/// authorises an ordinary amendment, it does not invent a new way to change the
/// company.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AmendmentTargetV1 {
    /// Replace the share register.
    ShareStructure,
    /// Replace the voting rules.
    VotingRules,
}

impl AmendmentTargetV1 {
    /// Both targets, in a fixed order.
    pub const ALL: [Self; 2] = [Self::ShareStructure, Self::VotingRules];

    /// The company record kind this target amends.
    #[must_use]
    pub const fn record_kind(self) -> RecordKindV1 {
        match self {
            Self::ShareStructure => RecordKindV1::ShareStructure,
            Self::VotingRules => RecordKindV1::VotingRules,
        }
    }

    /// The attribute text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShareStructure => "share-structure",
            Self::VotingRules => "voting-rules",
        }
    }

    /// Parses the attribute text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.as_str() == text)
    }
}

impl core::fmt::Display for AmendmentTargetV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a resolution does.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ResolutionKindV1 {
    /// A formal decision that changes no reconstructed state.
    ///
    /// The digest is the document the shareholders voted on, and must be the agenda
    /// item's proposal digest. Irena never sees the document.
    Declarative {
        /// Digest of the decision document.
        document_digest: Hash,
    },
    /// Authorises exactly one company amendment.
    Amendment {
        /// Which part is replaced.
        target: AmendmentTargetV1,
        /// The amendment body, carried verbatim. Its [`proposal_digest`] must be the
        /// agenda item's proposal digest, so the executed body is provably the
        /// approved one.
        body: String,
    },
}

impl ResolutionKindV1 {
    /// The `kind` attribute text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Declarative { .. } => "declarative",
            Self::Amendment { .. } => "amendment",
        }
    }

    /// The digest the agenda item must carry for this resolution.
    #[must_use]
    pub fn approved_digest(&self) -> Hash {
        match self {
            Self::Declarative { document_digest } => *document_digest,
            Self::Amendment { body, .. } => proposal_digest(body),
        }
    }

    /// Which part is amended, if any.
    #[must_use]
    pub const fn target(&self) -> Option<AmendmentTargetV1> {
        match self {
            Self::Declarative { .. } => None,
            Self::Amendment { target, .. } => Some(*target),
        }
    }

    /// The amendment body, if any.
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Declarative { .. } => None,
            Self::Amendment { body, .. } => Some(body),
        }
    }
}

/// Where a resolution is in its life.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionStatusV1 {
    /// Drafted; nothing on the chain, nothing verified.
    Draft,
    /// The resolution record is on the chain, its authority checked against it.
    ///
    /// For a declarative resolution this is the end: there is nothing to execute.
    Finalized,
    /// The amendment and the execution record are on the chain.
    Executed,
}

impl core::fmt::Display for ResolutionStatusV1 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Draft => "draft",
            Self::Finalized => "finalized",
            Self::Executed => "executed",
        })
    }
}

/// What authorised a resolution, pinned by transaction id.
///
/// Never a height, never a time, never "the current meeting": the exact finalised
/// meeting record, the exact agenda item on it, and the exact finalised vote that
/// answered that item.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AuthorityV1 {
    /// The transaction carrying the meeting's **final** record — the only record that
    /// says which vote answered which item.
    pub meeting_tx: TxId,
    /// The agenda item number.
    pub item_number: u32,
    /// The transaction carrying that item's final vote record.
    pub vote_tx: TxId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proposal_digest_covers_the_normalised_body() {
        let body = "<share-structure><holder id=\"a\" shares=\"1\"/></share-structure>";
        let digest = proposal_digest(body);
        // Normalisation is exactly what compose_record does: a declaration and
        // surrounding whitespace do not change the digest.
        assert_eq!(digest, proposal_digest(&format!("  {body}\n")));
        assert_eq!(
            digest,
            proposal_digest(&format!("<?xml version=\"1.0\"?>\n{body}"))
        );
        // Anything inside the element does.
        assert_ne!(
            digest,
            proposal_digest("<share-structure><holder id=\"a\" shares=\"2\"/></share-structure>")
        );
        assert_ne!(digest, proposal_digest("<share-structure/>"));
        // And the domain is Irena's own: it is not a bare hash of the bytes.
        assert_ne!(digest.to_bytes(), *blake3::hash(body.as_bytes()).as_bytes());
    }

    #[test]
    fn targets_map_to_company_record_kinds() {
        for target in AmendmentTargetV1::ALL {
            assert_eq!(AmendmentTargetV1::parse(target.as_str()), Some(target));
            assert_eq!(target.record_kind().as_str(), target.as_str());
        }
        assert_eq!(AmendmentTargetV1::parse("identity"), None);
        assert_eq!(AmendmentTargetV1::parse("company-genesis"), None);
    }

    #[test]
    fn a_kind_knows_what_the_shareholders_approved() {
        let body = "<voting-rules version=\"1.0\"/>";
        let amendment = ResolutionKindV1::Amendment {
            target: AmendmentTargetV1::VotingRules,
            body: body.to_owned(),
        };
        assert_eq!(amendment.approved_digest(), proposal_digest(body));
        assert_eq!(amendment.target(), Some(AmendmentTargetV1::VotingRules));
        assert_eq!(amendment.body(), Some(body));

        let document_digest = Hash::from_bytes([7; 32]);
        let declarative = ResolutionKindV1::Declarative { document_digest };
        assert_eq!(declarative.approved_digest(), document_digest);
        assert_eq!(declarative.target(), None);
        assert_eq!(declarative.body(), None);
    }
}
