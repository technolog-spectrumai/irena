//! The on-disk layout.
//!
//! Four tables in one redb database file. Block values are `prunella-canonical`
//! encodings, so what is stored is exactly what is hashed: a stored block cannot mean
//! one thing to storage and another to verification.

use redb::TableDefinition;

/// The store format version this build writes and reads.
pub const STORE_FORMAT_VERSION: u32 = 1;

/// Chain-level bookkeeping, keyed by name.
///
/// Keys: `format_version`, `network_id`, `genesis_hash`, `head_height`, `head_hash`,
/// `transaction_count`.
pub const META: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("prunella_meta");

/// Canonical block bytes, keyed by height.
pub const BLOCKS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("prunella_blocks");

/// Height lookup, keyed by block hash.
pub const HEIGHT_BY_HASH: TableDefinition<'static, &[u8], u64> =
    TableDefinition::new("prunella_height_by_hash");

/// Transaction location, keyed by transaction id.
///
/// The value is the block height as little-endian `u64` followed by the index within
/// the block as little-endian `u32`.
pub const TX_LOCATION: TableDefinition<'static, &[u8], &[u8]> =
    TableDefinition::new("prunella_tx_location");

/// Meta key names.
pub mod meta_key {
    /// Store format version.
    pub const FORMAT_VERSION: &str = "format_version";
    /// The chain's network id.
    pub const NETWORK_ID: &str = "network_id";
    /// The hash of the block at height zero, recorded at creation.
    pub const GENESIS_HASH: &str = "genesis_hash";
    /// Height of the current head.
    pub const HEAD_HEIGHT: &str = "head_height";
    /// Hash of the current head.
    pub const HEAD_HASH: &str = "head_hash";
    /// Total transactions committed across the whole chain.
    pub const TRANSACTION_COUNT: &str = "transaction_count";
}

/// Encodes a transaction location value.
#[must_use]
pub fn encode_location(height: u64, index: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&index.to_le_bytes());
    bytes
}

/// Decodes a transaction location value.
#[must_use]
pub fn decode_location(bytes: &[u8]) -> Option<(u64, u32)> {
    if bytes.len() != 12 {
        return None;
    }
    let height = u64::from_le_bytes(bytes[..8].try_into().ok()?);
    let index = u32::from_le_bytes(bytes[8..].try_into().ok()?);
    Some((height, index))
}
