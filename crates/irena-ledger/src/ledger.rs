//! Putting records on the chain: founding a company and amending it.

use crate::error::LedgerError;
use crate::state::{CompanyStateV1, RecordRefV1, reconstruct};
use irena_core::{
    CompanyIdV1, NotarisationV1, RECORD_SCHEMA_VERSION, RecordKindV1, compose_record, read_record,
};
use prunella_core::{
    BlockHeight, GenesisSpec, Namespace, NetworkId, SchemaVersion, Transaction, TransactionDraft,
};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;

fn transaction_for(
    key: &SigningKey,
    kind: RecordKindV1,
    payload: String,
    nonce: u64,
) -> Result<Transaction, LedgerError> {
    Ok(key.sign_transaction(TransactionDraft {
        namespace: Namespace::new(kind.namespace())?,
        schema_version: SchemaVersion(RECORD_SCHEMA_VERSION),
        payload: payload.into_bytes(),
        signer: key.public_key(),
        nonce,
    }))
}

/// Builds a genesis specification whose genesis block founds the company.
///
/// The genesis body is the whole company (`irena_core::CompanyGenesisV1`). The
/// company exists from height 0, and two parties given the same inputs derive the
/// same genesis hash.
///
/// # Errors
///
/// Returns [`LedgerError::Record`] if the genesis body or notarisation is not valid.
pub fn genesis_with_company(
    network: NetworkId,
    key: &SigningKey,
    company: &CompanyIdV1,
    genesis_xml: &str,
    notarisation: &NotarisationV1,
    timestamp_millis: u64,
) -> Result<GenesisSpec, LedgerError> {
    let payload = compose_record(
        RecordKindV1::CompanyGenesis,
        company,
        None,
        notarisation,
        genesis_xml,
    )?;
    let transaction = transaction_for(key, RecordKindV1::CompanyGenesis, payload, 0)?;
    Ok(GenesisSpec {
        network_id: network,
        timestamp_millis,
        transactions: vec![transaction],
    })
}

/// The company at the chain head.
///
/// # Errors
///
/// As [`reconstruct`].
pub fn company_now(store: &LocalChainStore) -> Result<CompanyStateV1, LedgerError> {
    reconstruct(store, store.head()?.height)
}

/// Publishes an amendment to one part of the company, in its own block.
///
/// `kind` must be an amendment kind — a company is founded once, by
/// [`genesis_with_company`]. `supersedes` must name the transaction currently
/// providing that part (the genesis, or the last amendment of the part), or the record
/// is a stale amendment and is refused before the ledger is touched. The body is
/// validated and embedded verbatim (`irena_core::compose_record`).
///
/// # Errors
///
/// [`LedgerError::StaleAmendment`]; [`LedgerError::Record`] for an invalid body or
/// notarisation; the reconstruction errors if the chain is not a company; or a
/// ledger error.
pub fn publish(
    store: &LocalChainStore,
    key: &SigningKey,
    kind: RecordKindV1,
    body_xml: &str,
    supersedes: Option<prunella_core::TxId>,
    notarisation: &NotarisationV1,
    timestamp_millis: u64,
) -> Result<RecordRefV1, LedgerError> {
    if kind == RecordKindV1::CompanyGenesis {
        return Err(LedgerError::Malformed {
            detail: "a company is founded once, at genesis; publish an amendment instead"
                .to_owned(),
        });
    }
    let state = company_now(store)?;
    let expected = state.provider_of(kind);
    if supersedes != Some(expected) {
        return Err(LedgerError::StaleAmendment {
            kind,
            expected: expected.to_string(),
            found: supersedes.map_or_else(|| "none".to_owned(), |id| id.to_string()),
        });
    }

    let payload = compose_record(kind, &state.company, supersedes, notarisation, body_xml)?;
    let head = store.head()?;
    let parent = store
        .get_block(head.height)?
        .ok_or_else(|| LedgerError::Malformed {
            detail: format!("the chain head is {head} but no block is stored there"),
        })?;
    let height = head.height.next()?;
    let transaction = transaction_for(key, kind, payload, height.value())?;
    let tx_id = transaction.id;
    let block = parent
        .header
        .child_draft(vec![transaction], timestamp_millis)?
        .build()?;
    store.append_block(block)?;

    let committed = store
        .get_transaction(&tx_id)?
        .ok_or_else(|| LedgerError::Malformed {
            detail: format!("transaction {tx_id} was appended but cannot be read back"),
        })?;
    let record = read_record(
        core::str::from_utf8(&committed.transaction.payload).map_err(|error| {
            LedgerError::Malformed {
                detail: format!("composed record is not UTF-8: {error}"),
            }
        })?,
    )?;
    Ok(RecordRefV1 {
        height,
        index: committed.index,
        tx_id,
        signer: key.public_key(),
        record,
    })
}

/// The records that have provided a part up to `at`, in chain order: the genesis,
/// then every amendment of that part.
///
/// # Errors
///
/// As [`reconstruct`].
pub fn history(
    store: &LocalChainStore,
    kind: RecordKindV1,
    at: BlockHeight,
) -> Result<Vec<RecordRefV1>, LedgerError> {
    Ok(reconstruct(store, at)?
        .history_of(kind)
        .into_iter()
        .cloned()
        .collect())
}
