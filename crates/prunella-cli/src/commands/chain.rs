//! init, status and verify.

use crate::args::{InitArgs, VerifyArgs};
use crate::output::{EXIT_FINDING, EXIT_OK, Format};
use prunella_core::{BlockHeight, GenesisSpec, NetworkId};
use prunella_store::LocalChainStore;
use prunella_verify::{VerifyOptions, verify_chain};
use serde_json::json;
use std::path::Path;

/// Creates a chain.
pub fn init(path: &Path, args: &InitArgs, format: Format) -> Result<u8, String> {
    let network_id = NetworkId::new(args.network.clone()).map_err(|error| error.to_string())?;
    let spec = GenesisSpec {
        network_id,
        timestamp_millis: args.genesis_timestamp,
        transactions: Vec::new(),
    };
    let store = LocalChainStore::init_genesis(path, spec).map_err(|error| error.to_string())?;

    format.emit(
        &format!(
            "created {}\nnetwork: {}\ngenesis: {}",
            path.display(),
            store.network_id(),
            store.genesis_hash()
        ),
        &json!({
            "path": path.display().to_string(),
            "network_id": store.network_id(),
            "genesis_hash": store.genesis_hash(),
        }),
    );
    Ok(EXIT_OK)
}

/// Reports the chain's identity, head and counts.
pub fn status(path: &Path, format: Format) -> Result<u8, String> {
    let store = LocalChainStore::open(path).map_err(|error| error.to_string())?;
    let status = store.status().map_err(|error| error.to_string())?;

    format.emit(
        &format!(
            "path:              {}\n\
             network:           {}\n\
             genesis:           {}\n\
             head height:       {}\n\
             head hash:         {}\n\
             blocks:            {}\n\
             transactions:      {}\n\
             store format:      {}\n\
             acceptance policy: {}",
            path.display(),
            status.network_id,
            status.genesis_hash,
            status.head.height,
            status.head.hash,
            status.block_count,
            status.transaction_count,
            status.format_version,
            status.acceptance_policy,
        ),
        &json!({
            "path": path.display().to_string(),
            "network_id": status.network_id,
            "genesis_hash": status.genesis_hash,
            "head": status.head,
            "block_count": status.block_count,
            "transaction_count": status.transaction_count,
            "store_format_version": status.format_version,
            "acceptance_policy": status.acceptance_policy,
        }),
    );
    Ok(EXIT_OK)
}

/// Verifies the chain and reports every defect.
pub fn verify(path: &Path, args: &VerifyArgs, format: Format) -> Result<u8, String> {
    let store = LocalChainStore::open(path).map_err(|error| error.to_string())?;
    let options = VerifyOptions {
        from: args.from.map(BlockHeight),
        to: args.to.map(BlockHeight),
        detect_duplicate_transactions: !args.skip_duplicate_check,
    };
    let report = verify_chain(&store, options);

    format.emit(
        &report.to_string(),
        &serde_json::to_value(&report).unwrap_or(serde_json::Value::Null),
    );
    Ok(if report.is_valid() {
        EXIT_OK
    } else {
        EXIT_FINDING
    })
}
