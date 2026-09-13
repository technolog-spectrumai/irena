//! An independent implementation of Protocol V1, written from `PROTOCOL_V1.md` alone.
//!
//! Nothing here uses `prunella-canonical`, Borsh derives, or any Prunella hashing
//! helper. Integers are written by hand, strings and byte strings are length-prefixed
//! by hand, BLAKE3 and Ed25519 are called directly. If the specification and the
//! implementation ever disagree, the golden vectors fail against one of them and the
//! disagreement is visible instead of being frozen into both.

use ed25519_dalek::Signer as _;

/// Appends a `u16` little-endian.
pub fn u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a `u32` little-endian.
pub fn u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a `u64` little-endian.
pub fn u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a length-prefixed byte string (§1.3).
///
/// # Panics
///
/// Panics if the value is longer than `u32::MAX`, which no test vector is.
pub fn bytes(out: &mut Vec<u8>, value: &[u8]) {
    u32(
        out,
        u32::try_from(value.len()).expect("vector inputs fit in u32"),
    );
    out.extend_from_slice(value);
}

/// Appends a length-prefixed UTF-8 string (§1.3).
pub fn string(out: &mut Vec<u8>, value: &str) {
    bytes(out, value.as_bytes());
}

/// §1.8: `BLAKE3(u32_le(len(tag)) || tag || data...)`.
///
/// # Panics
///
/// Panics if the tag is longer than `u32::MAX` bytes, which no tag is.
pub fn hash_domain(tag: &str, parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(
        &u32::try_from(tag.len())
            .expect("tags are short")
            .to_le_bytes(),
    );
    hasher.update(tag.as_bytes());
    for part in parts {
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

/// The fields of a transaction a signer commits to, in spec order.
pub struct TxInput<'a> {
    /// §4.1 field 1.
    pub namespace: &'a str,
    /// §4.1 field 2.
    pub schema_version: u32,
    /// §4.1 field 3.
    pub payload: &'a [u8],
    /// §4.1 field 4.
    pub signer: [u8; 32],
    /// §4.1 field 5.
    pub nonce: u64,
}

/// §4.1 `TxSigningBody`.
pub fn signing_preimage(tx: &TxInput<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    string(&mut out, tx.namespace);
    u32(&mut out, tx.schema_version);
    bytes(&mut out, tx.payload);
    out.extend_from_slice(&tx.signer);
    u64(&mut out, tx.nonce);
    out
}

/// §5.
pub fn signing_message(tx: &TxInput<'_>) -> [u8; 32] {
    hash_domain("PRUNELLA/v1/tx-sign", &[&signing_preimage(tx)])
}

/// §1.9: Ed25519 over the signing message, straight from the seed.
pub fn sign(seed: [u8; 32], message: &[u8; 32]) -> [u8; 64] {
    ed25519_dalek::SigningKey::from_bytes(&seed)
        .sign(message)
        .to_bytes()
}

/// §1.9: the public key for a seed.
pub fn public_key(seed: [u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes()
}

/// §4.2 `TxIdBody`.
pub fn id_preimage(tx: &TxInput<'_>, signature: &[u8; 64]) -> Vec<u8> {
    let mut out = signing_preimage(tx);
    out.extend_from_slice(signature);
    out
}

/// §6.
pub fn tx_id(tx: &TxInput<'_>, signature: &[u8; 64]) -> [u8; 32] {
    hash_domain("PRUNELLA/v1/tx-id", &[&id_preimage(tx, signature)])
}

/// §4.3 `TransactionV1`.
pub fn transaction(id: &[u8; 32], tx: &TxInput<'_>, signature: &[u8; 64]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(id);
    string(&mut out, tx.namespace);
    u32(&mut out, tx.schema_version);
    bytes(&mut out, tx.payload);
    out.extend_from_slice(&tx.signer);
    u64(&mut out, tx.nonce);
    out.extend_from_slice(signature);
    out
}

/// §7 leaf.
pub fn leaf(id: &[u8; 32]) -> [u8; 32] {
    hash_domain("PRUNELLA/v1/tx-leaf", &[id])
}

/// §7 node.
pub fn node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    hash_domain("PRUNELLA/v1/tx-node", &[left, right])
}

/// §7 root.
pub fn merkle_root(ids: &[[u8; 32]]) -> [u8; 32] {
    match ids.len() {
        0 => hash_domain("PRUNELLA/v1/tx-root", &[]),
        1 => leaf(&ids[0]),
        n => {
            let k = split(n);
            node(&merkle_root(&ids[..k]), &merkle_root(&ids[k..]))
        }
    }
}

/// §7: the largest power of two strictly below `n`.
fn split(n: usize) -> usize {
    let mut k = 1;
    while k * 2 < n {
        k *= 2;
    }
    k
}

/// One step of a §7.1 audit path: `(is_left_sibling, sibling_root)`.
pub type Step = (bool, [u8; 32]);

/// §7.1: the audit path for `index`, leaf-most first.
pub fn audit_path(ids: &[[u8; 32]], index: usize) -> Vec<Step> {
    let mut steps = Vec::new();
    fn walk(ids: &[[u8; 32]], index: usize, steps: &mut Vec<Step>) {
        if ids.len() <= 1 {
            return;
        }
        let k = split(ids.len());
        if index < k {
            walk(&ids[..k], index, steps);
            steps.push((false, merkle_root(&ids[k..])));
        } else {
            walk(&ids[k..], index - k, steps);
            steps.push((true, merkle_root(&ids[..k])));
        }
    }
    walk(ids, index, &mut steps);
    steps
}

/// §4.6 `InclusionProofV1`.
pub fn proof(index: u32, steps: &[Step]) -> Vec<u8> {
    let mut out = Vec::new();
    u32(&mut out, index);
    u32(&mut out, u32::try_from(steps.len()).expect("short paths"));
    for (is_left, hash) in steps {
        // §4.6: Side is a u8 discriminant, 0 = Left, 1 = Right.
        out.push(if *is_left { 0 } else { 1 });
        out.extend_from_slice(hash);
    }
    out
}

/// The fields of a block header, in spec order.
pub struct HeaderInput<'a> {
    /// §4.4 field 2.
    pub network_id: &'a str,
    /// §4.4 field 3.
    pub height: u64,
    /// §4.4 field 4.
    pub previous_hash: [u8; 32],
    /// §4.4 field 5.
    pub tx_root: [u8; 32],
    /// §4.4 field 6.
    pub tx_count: u32,
    /// §4.4 field 7.
    pub timestamp_millis: u64,
}

/// §4.4 `BlockHeaderV1` with `version = 1`.
pub fn header(h: &HeaderInput<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    u16(&mut out, 1);
    string(&mut out, h.network_id);
    u64(&mut out, h.height);
    out.extend_from_slice(&h.previous_hash);
    out.extend_from_slice(&h.tx_root);
    u32(&mut out, h.tx_count);
    u64(&mut out, h.timestamp_millis);
    out
}

/// §8.
pub fn block_hash(header_bytes: &[u8]) -> [u8; 32] {
    hash_domain("PRUNELLA/v1/block-header", &[header_bytes])
}

/// §4.5 `BlockV1`.
pub fn block(header_bytes: &[u8], transactions: &[Vec<u8>]) -> Vec<u8> {
    let mut out = header_bytes.to_vec();
    u32(
        &mut out,
        u32::try_from(transactions.len()).expect("vector blocks are small"),
    );
    for transaction in transactions {
        out.extend_from_slice(transaction);
    }
    out
}
