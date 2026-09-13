//! append and keygen.

use crate::args::{AppendArgs, KeygenArgs};
use crate::keyfile;
use crate::output::{EXIT_OK, Format};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use prunella_canonical::Canonical;
use prunella_core::{Namespace, SchemaVersion, Transaction, TransactionDraft};
use prunella_crypto::SigningKey;
use prunella_store::ChainStore;
use serde_json::json;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Signs and appends one block.
pub fn append(path: &Path, args: &AppendArgs, format: Format) -> Result<u8, String> {
    let store = ChainStore::open(path).map_err(|error| error.to_string())?;

    let mut transactions = Vec::new();
    for file in &args.tx_file {
        transactions.push(read_transaction(file)?);
    }
    if let Some(key_path) = &args.signing_key {
        transactions.push(sign_one(key_path, args)?);
    }
    if transactions.is_empty() {
        return Err("nothing to append: supply --signing-key with a payload, or --tx-file".into());
    }

    let head = store.head().map_err(|error| error.to_string())?;
    let parent = store
        .block_at(head.height)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("the chain head is {head} but no block is stored there"))?;

    // A block's timestamp may not precede its parent's. An explicit --timestamp that
    // does is refused rather than adjusted: quietly changing a value the operator asked
    // for would mean the block committed is not the block they described. The system
    // clock, which nobody chose, is allowed to be nudged forward.
    let parent_timestamp = parent.header.timestamp_millis;
    let timestamp = match args.timestamp {
        Some(chosen) if chosen < parent_timestamp => {
            return Err(format!(
                "--timestamp {chosen} precedes the parent block's timestamp \
                 {parent_timestamp}; a block may not move the chain's clock backwards"
            ));
        }
        Some(chosen) => chosen,
        None => now_millis().max(parent_timestamp),
    };
    let block = parent
        .header
        .child_draft(transactions, timestamp)
        .and_then(prunella_core::BlockDraft::build)
        .map_err(|error| error.to_string())?;

    let hash = block.hash();
    let ids: Vec<_> = block.transactions.iter().map(|t| t.id).collect();
    let count = block.transactions.len();
    let head = store
        .append_block(block)
        .map_err(|error| error.to_string())?;

    format.emit(
        &format!(
            "appended block {} at height {}\ntransactions: {count}\n{}",
            hash,
            head.height,
            ids.iter()
                .map(|id| format!("  {id}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        &json!({
            "block_hash": hash,
            "height": head.height,
            "transaction_ids": ids,
            "head": head,
        }),
    );
    Ok(EXIT_OK)
}

/// Generates a signing key.
pub fn keygen(args: &KeygenArgs, format: Format) -> Result<u8, String> {
    let key = SigningKey::generate().map_err(|error| error.to_string())?;
    let public = key.public_key();

    match &args.out {
        Some(path) => {
            keyfile::write(path, &key)?;
            format.emit(
                &format!("wrote {}\npublic key: {public}", path.display()),
                &json!({ "path": path.display().to_string(), "public_key": public }),
            );
        }
        None => {
            // Without a destination the seed goes to standard output, where the caller
            // is responsible for what happens to it.
            format.emit(
                &format!("seed: {}\npublic key: {public}", hex::encode(key.to_seed())),
                &json!({ "seed": hex::encode(key.to_seed()), "public_key": public }),
            );
        }
    }
    Ok(EXIT_OK)
}

/// Builds and signs a transaction from the command line arguments.
fn sign_one(key_path: &Path, args: &AppendArgs) -> Result<Transaction, String> {
    let key = keyfile::read(key_path)?;
    let namespace_text = args
        .namespace
        .as_ref()
        .ok_or("--namespace is required when signing a transaction")?;
    let namespace = Namespace::new(namespace_text.clone()).map_err(|error| error.to_string())?;

    Ok(key.sign_transaction(TransactionDraft {
        namespace,
        schema_version: SchemaVersion(args.schema_version),
        payload: read_payload(args)?,
        signer: key.public_key(),
        nonce: args.nonce,
    }))
}

/// Resolves the payload from whichever source was given.
///
/// No source means an empty payload, which is a legitimate transaction: Prunella never
/// inspects payload bytes, so it has no opinion about how many there should be.
fn read_payload(args: &AppendArgs) -> Result<Vec<u8>, String> {
    if let Some(file) = &args.payload_file {
        return std::fs::read(file)
            .map_err(|error| format!("could not read the payload {}: {error}", file.display()));
    }
    if let Some(text) = &args.payload_hex {
        return hex::decode(text).map_err(|error| format!("--payload-hex is not hex: {error}"));
    }
    if let Some(text) = &args.payload_base64 {
        return BASE64
            .decode(text.as_bytes())
            .map_err(|error| format!("--payload-base64 is not base64: {error}"));
    }
    Ok(Vec::new())
}

/// Reads a pre-signed transaction in canonical encoding.
fn read_transaction(path: &Path) -> Result<Transaction, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Transaction::from_canonical_bytes(&bytes).map_err(|error| {
        format!(
            "{} does not hold a canonically encoded transaction: {error}",
            path.display()
        )
    })
}

/// The system clock in milliseconds since the Unix epoch, or zero if it is before it.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
