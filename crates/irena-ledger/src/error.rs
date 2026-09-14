//! Why the ledger layer refused.

use irena_core::{RecordFamilyV1, RecordKindV1};
use prunella_core::{BlockHeight, PublicKey, TxId};

/// Failure modes of the ledger layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LedgerError {
    /// The record document is not a valid Irena record.
    #[error("invalid irena record: {0}")]
    Record(#[from] irena_core::IrenaError),
    /// No company has been founded on this chain by the height asked about.
    #[error("no company is founded on this chain by height {at}")]
    NoCompany {
        /// The height asked about.
        at: BlockHeight,
    },
    /// The first Irena record on the chain is not a genesis.
    #[error(
        "the first irena record, transaction {tx_id} at height {height}, is a {kind} record; a company must be founded before it is amended"
    )]
    NoGenesisFirst {
        /// Where it is.
        height: BlockHeight,
        /// Which transaction.
        tx_id: TxId,
        /// What it is.
        kind: RecordKindV1,
    },
    /// A second genesis appeared. One company per chain.
    #[error(
        "transaction {tx_id} at height {height} is a second company genesis; this chain was founded by {first} and holds one company"
    )]
    SecondGenesis {
        /// The founding transaction.
        first: TxId,
        /// Where the second is.
        height: BlockHeight,
        /// The second.
        tx_id: TxId,
    },
    /// A record names a company other than the one founded on this chain.
    #[error(
        "transaction {tx_id} at height {height} is a record for company {found}, but this chain holds {expected}"
    )]
    ForeignCompany {
        /// The chain's company.
        expected: String,
        /// The record's.
        found: String,
        /// Where it is.
        height: BlockHeight,
        /// Which transaction.
        tx_id: TxId,
    },
    /// The record would amend something other than what currently provides the part.
    ///
    /// Amending a version you have not seen is how two editors clobber each other, so
    /// it is refused: `supersedes` must name exactly the transaction currently
    /// providing the part — the genesis, or the last amendment of that part.
    #[error(
        "stale amendment: {kind} is currently provided by {expected}, but the record supersedes {found}"
    )]
    StaleAmendment {
        /// The record kind.
        kind: RecordKindV1,
        /// What provides the part, rendered.
        expected: String,
        /// What the record claims to supersede, rendered.
        found: String,
    },
    /// The key about to sign is not an authorised signer's for this family of record.
    ///
    /// Refused before anything is written. The person is looked up in the identities
    /// in force and their row in the authorisation in force, both at the chain head.
    #[error("unauthorised signer for {family} records: key {signer}: {detail}")]
    UnauthorisedSigner {
        /// The family the record belongs to.
        family: RecordFamilyV1,
        /// The key.
        signer: PublicKey,
        /// Why it does not qualify.
        detail: String,
    },
    /// The record about to be published would leave no `company` signer with a key.
    ///
    /// Refused before anything is written: a company that cannot be amended by anyone
    /// is a company nobody meant to create.
    #[error("the record would lock the company out: {detail}")]
    LockedOut {
        /// Which signers were named and why none counts.
        detail: String,
    },
    /// The ledger holds a company record whose transaction signer was not authorised
    /// when it was written.
    ///
    /// This can only happen if a record was written around this crate. Like a broken
    /// amendment link it is reported, never repaired, and stops reconstruction at that
    /// height: an unauthorised amendment is not part of the company.
    #[error(
        "unauthorised record at height {height}, transaction {tx_id}: a {kind} record signed by {signer}: {detail}"
    )]
    UnauthorisedRecord {
        /// Where it is.
        height: BlockHeight,
        /// Which transaction.
        tx_id: TxId,
        /// What it amends.
        kind: RecordKindV1,
        /// The transaction signer.
        signer: PublicKey,
        /// Why the key did not qualify under the company as it then was.
        detail: String,
    },
    /// The ledger holds a record after which no `company` signer holds a key.
    ///
    /// Written around this crate; reported, never repaired.
    #[error("lockout at height {height}, transaction {tx_id}: {detail}")]
    Lockout {
        /// Where it is.
        height: BlockHeight,
        /// The record that did it.
        tx_id: TxId,
        /// Which signers were named and why none counts.
        detail: String,
    },
    /// The ledger holds a record whose amendment link does not hold.
    ///
    /// This can only happen if a record was written around this crate. It is reported,
    /// never repaired, and until it is resolved the company cannot be reconstructed at
    /// or past that height.
    #[error(
        "broken amendment chain for company {company} ({kind}) at height {height}, transaction {tx_id}: expected supersedes {expected}, found {found}"
    )]
    BrokenAmendmentChain {
        /// The company.
        company: String,
        /// The record kind.
        kind: RecordKindV1,
        /// Where the break is.
        height: BlockHeight,
        /// The offending record.
        tx_id: TxId,
        /// What it should have superseded, rendered.
        expected: String,
        /// What it claims, rendered.
        found: String,
    },
    /// A transaction in an Irena namespace does not hold a readable record that
    /// belongs there.
    ///
    /// Prunella accepts any payload in any namespace, so this can only come from
    /// something writing around this crate. It is reported, not skipped: a reader that
    /// stepped over it could not know whether it was meant to be an amendment.
    #[error(
        "transaction {tx_id} at height {height} is in the {namespace} namespace but does not hold a record that belongs there: {detail}"
    )]
    UnreadableRecord {
        /// Where it is.
        height: BlockHeight,
        /// Which transaction.
        tx_id: TxId,
        /// The namespace it was published under.
        namespace: String,
        /// What was wrong.
        detail: String,
    },
    /// The chain is not in a state this layer can work with.
    #[error("malformed chain: {detail}")]
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// The ledger refused the block or could not be read.
    #[error(transparent)]
    Store(#[from] prunella_store::StoreError),
    /// A core ledger value could not be built.
    #[error(transparent)]
    Ledger(#[from] prunella_core::CoreError),
}
