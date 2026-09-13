//! block and tx.

use crate::args::{BlockArgs, TxArgs};
use crate::output::{EXIT_FINDING, EXIT_OK, Format};
use prunella_core::{Block, BlockHeight, Hash, TxId};
use prunella_store::LocalChainStore;
use serde_json::json;
use std::path::Path;

/// Shows one block, selected by height or by hash.
pub fn block(path: &Path, args: &BlockArgs, format: Format) -> Result<u8, String> {
    let store = LocalChainStore::open(path).map_err(|error| error.to_string())?;

    let found = if let Ok(height) = args.selector.parse::<u64>() {
        store
            .get_block(BlockHeight(height))
            .map_err(|error| error.to_string())?
    } else {
        let hash = Hash::from_hex(&args.selector).map_err(|error| {
            format!(
                "{} is neither a block height nor a block hash: {error}",
                args.selector
            )
        })?;
        store
            .get_block_by_hash(&hash)
            .map_err(|error| error.to_string())?
    };

    let Some(block) = found else {
        eprintln!("no block matches {}", args.selector);
        return Ok(EXIT_FINDING);
    };

    format.emit(
        &render_block(&block, args.transactions),
        &block_json(&block, args.transactions),
    );
    Ok(EXIT_OK)
}

/// Shows one transaction and where it was committed.
pub fn tx(path: &Path, args: &TxArgs, format: Format) -> Result<u8, String> {
    let store = LocalChainStore::open(path).map_err(|error| error.to_string())?;
    let id = TxId::from_hex(&args.id)
        .map_err(|error| format!("{} is not a transaction id: {error}", args.id))?;

    let Some(located) = store
        .get_transaction(&id)
        .map_err(|error| error.to_string())?
    else {
        eprintln!("no transaction matches {id}");
        return Ok(EXIT_FINDING);
    };
    let transaction = &located.transaction;

    format.emit(
        &format!(
            "id:             {}\n\
             height:         {}\n\
             index:          {}\n\
             namespace:      {}\n\
             schema version: {}\n\
             signer:         {}\n\
             nonce:          {}\n\
             signature:      {}\n\
             payload bytes:  {}\n\
             payload hex:    {}",
            transaction.id,
            located.height,
            located.index,
            transaction.namespace,
            transaction.schema_version,
            transaction.signer,
            transaction.nonce,
            transaction.signature,
            transaction.payload.len(),
            hex::encode(&transaction.payload),
        ),
        &json!({
            "height": located.height,
            "index": located.index,
            "transaction": transaction,
        }),
    );
    Ok(EXIT_OK)
}

fn render_block(block: &Block, with_transactions: bool) -> String {
    let header = &block.header;
    let mut text = format!(
        "height:         {}\n\
         hash:           {}\n\
         previous hash:  {}\n\
         header version: {}\n\
         network:        {}\n\
         tx root:        {}\n\
         tx count:       {}\n\
         timestamp:      {}",
        header.height,
        block.hash(),
        header.previous_hash,
        header.version,
        header.network_id,
        header.tx_root,
        header.tx_count,
        header.timestamp_millis,
    );
    for (index, transaction) in block.transactions.iter().enumerate() {
        text.push_str(&format!(
            "\n  [{index}] {} {} nonce={} payload={} bytes",
            transaction.id,
            transaction.namespace,
            transaction.nonce,
            transaction.payload.len()
        ));
        if with_transactions {
            text.push_str(&format!(
                "\n       signer={} signature={}\n       payload_hex={}",
                transaction.signer,
                transaction.signature,
                hex::encode(&transaction.payload)
            ));
        }
    }
    text
}

fn block_json(block: &Block, with_transactions: bool) -> serde_json::Value {
    let mut value = json!({
        "hash": block.hash(),
        "header": block.header,
    });
    if with_transactions {
        value["transactions"] =
            serde_json::to_value(&block.transactions).unwrap_or(serde_json::Value::Null);
    } else {
        value["transaction_ids"] =
            serde_json::to_value(block.transactions.iter().map(|t| t.id).collect::<Vec<_>>())
                .unwrap_or(serde_json::Value::Null);
    }
    value
}
