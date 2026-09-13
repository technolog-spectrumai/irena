//! The golden vector file format and the cases it covers.
//!
//! Each file is self-contained: every input needed to rebuild the block is spelled out
//! in human-readable form, and every derived value — canonical bytes, hashes,
//! signatures, proofs — is recorded as hex. A conformance test rebuilds the block from
//! the input alone and compares byte for byte.

use prunella_canonical::Canonical;
use prunella_core::{
    Block, BlockDraft, BlockHeight, GenesisSpec, Hash, Namespace, NetworkId, SchemaVersion, Side,
    Transaction, TransactionDraft,
};
use prunella_crypto::SigningKey;
use serde::{Deserialize, Serialize};

/// The protocol identifier every V1 vector carries.
pub const PROTOCOL: &str = "prunella-v1";

/// One golden vector.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VectorFile {
    /// Always [`PROTOCOL`].
    pub protocol: String,
    /// File stem, unique among vectors.
    pub name: String,
    /// What the vector exercises.
    pub description: String,
    /// Signing keys referenced by index from transactions.
    pub keys: Vec<KeyVector>,
    /// The block and everything derived from it.
    pub block: BlockVector,
}

/// A deterministic signing key.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct KeyVector {
    /// 32-byte Ed25519 seed.
    pub seed_hex: String,
    /// The matching public key.
    pub public_key_hex: String,
}

/// Input and expected output for one block.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BlockVector {
    /// Everything needed to rebuild the block.
    pub input: BlockInput,
    /// Everything the block derives.
    pub expected: BlockExpected,
}

/// Human-readable block input.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BlockInput {
    /// Chain identifier.
    pub network_id: String,
    /// Block height.
    pub height: u64,
    /// Parent block hash; all zero for genesis.
    pub previous_hash_hex: String,
    /// Declared timestamp.
    pub timestamp_millis: u64,
    /// Transactions in block order.
    pub transactions: Vec<TxInput>,
}

/// Human-readable transaction input.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TxInput {
    /// Namespace label.
    pub namespace: String,
    /// Opaque schema version.
    pub schema_version: u32,
    /// Opaque payload bytes.
    pub payload_hex: String,
    /// Index into [`VectorFile::keys`].
    pub signer_key_index: usize,
    /// Opaque nonce.
    pub nonce: u64,
}

/// Everything a block derives.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BlockExpected {
    /// §4.4 canonical header bytes.
    pub header_hex: String,
    /// §8 block hash.
    pub block_hash_hex: String,
    /// §7 transactions root.
    pub tx_root_hex: String,
    /// Header transaction count.
    pub tx_count: u32,
    /// §4.5 canonical block bytes.
    pub canonical_block_hex: String,
    /// Per-transaction derivations, in block order.
    pub transactions: Vec<TxExpected>,
    /// §7 leaf hashes, in block order.
    pub leaf_hashes_hex: Vec<String>,
    /// §7.1 proofs, one per transaction, in block order.
    pub inclusion_proofs: Vec<ProofVector>,
}

/// Everything a transaction derives.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TxExpected {
    /// §4.1 canonical bytes.
    pub signing_preimage_hex: String,
    /// §5.
    pub signing_message_hex: String,
    /// §1.9 signature over the signing message.
    pub signature_hex: String,
    /// §4.2 canonical bytes.
    pub id_preimage_hex: String,
    /// §6.
    pub id_hex: String,
    /// §4.3 canonical bytes.
    pub canonical_transaction_hex: String,
}

/// One inclusion proof.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ProofVector {
    /// Transaction position.
    pub index: u32,
    /// Audit path, leaf-most first.
    pub steps: Vec<StepVector>,
    /// §4.6 canonical bytes.
    pub canonical_proof_hex: String,
}

/// One proof step.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StepVector {
    /// `"left"` or `"right"`.
    pub side: String,
    /// Sibling subtree root.
    pub hash_hex: String,
}

/// A deterministic seed for key `index`.
fn seed(index: u8) -> [u8; 32] {
    [index + 1; 32]
}

/// Rebuilds a transaction from its input using the implementation.
///
/// # Panics
///
/// Panics if the input is not a valid V1 transaction, which a committed vector never is.
pub fn build_transaction(keys: &[KeyVector], input: &TxInput) -> Transaction {
    let seed: [u8; 32] = hex::decode(&keys[input.signer_key_index].seed_hex)
        .expect("seed hex")
        .try_into()
        .expect("32-byte seed");
    let key = SigningKey::from_seed(seed);
    key.sign_transaction(TransactionDraft {
        namespace: Namespace::new(input.namespace.clone()).expect("valid namespace"),
        schema_version: SchemaVersion(input.schema_version),
        payload: hex::decode(&input.payload_hex).expect("payload hex"),
        signer: key.public_key(),
        nonce: input.nonce,
    })
}

/// Rebuilds a block from its input using the implementation.
///
/// # Panics
///
/// Panics if the input is not a valid V1 block, which a committed vector never is.
pub fn build_block(keys: &[KeyVector], input: &BlockInput) -> Block {
    let transactions = input
        .transactions
        .iter()
        .map(|tx| build_transaction(keys, tx))
        .collect();
    BlockDraft {
        network_id: NetworkId::new(input.network_id.clone()).expect("valid network id"),
        height: BlockHeight(input.height),
        previous_hash: Hash::from_hex(&input.previous_hash_hex).expect("previous hash hex"),
        timestamp_millis: input.timestamp_millis,
        transactions,
    }
    .build()
    .expect("build block")
}

/// Derives every expected value for a block using the implementation.
///
/// # Panics
///
/// Panics only on inputs no committed vector has.
pub fn derive_expected(block: &Block) -> BlockExpected {
    let ids: Vec<_> = block.transactions.iter().map(|tx| tx.id).collect();
    BlockExpected {
        header_hex: hex::encode(block.header.canonical_bytes()),
        block_hash_hex: block.hash().to_hex(),
        tx_root_hex: block.header.tx_root.to_hex(),
        tx_count: block.header.tx_count,
        canonical_block_hex: hex::encode(block.canonical_bytes()),
        transactions: block
            .transactions
            .iter()
            .map(|tx| TxExpected {
                signing_preimage_hex: hex::encode(tx.signing_preimage()),
                signing_message_hex: hex::encode(tx.signing_message()),
                signature_hex: tx.signature.to_hex(),
                id_preimage_hex: hex::encode(tx.id_preimage()),
                id_hex: tx.id.to_hex(),
                canonical_transaction_hex: hex::encode(tx.canonical_bytes()),
            })
            .collect(),
        leaf_hashes_hex: ids
            .iter()
            .map(|id| prunella_core::merkle::leaf_hash(id).to_hex())
            .collect(),
        inclusion_proofs: (0..block.transactions.len())
            .map(|index| {
                let proof = block.inclusion_proof(index).expect("in range");
                ProofVector {
                    index: proof.index,
                    steps: proof
                        .steps
                        .iter()
                        .map(|step| StepVector {
                            side: match step.side {
                                Side::Left => "left".to_owned(),
                                Side::Right => "right".to_owned(),
                            },
                            hash_hex: step.hash.to_hex(),
                        })
                        .collect(),
                    canonical_proof_hex: hex::encode(proof.canonical_bytes()),
                }
            })
            .collect(),
    }
}

fn keys(count: u8) -> Vec<KeyVector> {
    (0..count)
        .map(|index| {
            let seed = seed(index);
            KeyVector {
                seed_hex: hex::encode(seed),
                public_key_hex: SigningKey::from_seed(seed).public_key().to_hex(),
            }
        })
        .collect()
}

fn tx(namespace: &str, schema_version: u32, payload: &[u8], key: usize, nonce: u64) -> TxInput {
    TxInput {
        namespace: namespace.to_owned(),
        schema_version,
        payload_hex: hex::encode(payload),
        signer_key_index: key,
        nonce,
    }
}

fn vector(name: &str, description: &str, keys: Vec<KeyVector>, input: BlockInput) -> VectorFile {
    let block = build_block(&keys, &input);
    VectorFile {
        protocol: PROTOCOL.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
        keys,
        block: BlockVector {
            input,
            expected: derive_expected(&block),
        },
    }
}

/// Builds every V1 vector, deterministically.
///
/// # Panics
///
/// Panics only if the fixed inputs below stop being valid, which would itself be a
/// protocol break.
#[must_use]
pub fn build_all() -> Vec<VectorFile> {
    let network = "prunella.example";
    let genesis_spec = GenesisSpec::new(NetworkId::new(network).expect("valid"));
    let genesis_hash = genesis_spec.build().expect("genesis").hash().to_hex();
    let zero = Hash::ZERO.to_hex();

    let mut vectors = Vec::new();

    vectors.push(vector(
        "genesis",
        "The genesis block of network prunella.example at timestamp 0 with no transactions. \
         Its hash is the chain's genesis hash.",
        Vec::new(),
        BlockInput {
            network_id: network.to_owned(),
            height: 0,
            previous_hash_hex: zero.clone(),
            timestamp_millis: 0,
            transactions: Vec::new(),
        },
    ));

    vectors.push(vector(
        "empty-block",
        "A block at height 1 carrying no transactions. Exercises the empty transactions \
         root and a non-zero previous hash.",
        Vec::new(),
        BlockInput {
            network_id: network.to_owned(),
            height: 1,
            previous_hash_hex: genesis_hash.clone(),
            timestamp_millis: 1000,
            transactions: Vec::new(),
        },
    ));

    let one = vector(
        "one-transaction",
        "A block at height 1 with a single transaction. The transactions root equals the \
         single leaf hash and the inclusion proof has no steps.",
        keys(1),
        BlockInput {
            network_id: network.to_owned(),
            height: 1,
            previous_hash_hex: genesis_hash.clone(),
            timestamp_millis: 1000,
            transactions: vec![tx("app.demo", 1, b"hello", 0, 1)],
        },
    );
    let one_hash = one.block.expected.block_hash_hex.clone();
    vectors.push(one);

    let three = vector(
        "three-transactions",
        "A block with three transactions from two signers. Three leaves make an unbalanced \
         tree: the first two pair up and the third is promoted to the top level.",
        keys(2),
        BlockInput {
            network_id: network.to_owned(),
            height: 2,
            previous_hash_hex: one_hash,
            timestamp_millis: 2000,
            transactions: vec![
                tx("app.demo", 1, b"first", 0, 2),
                tx("app.other", 7, b"second", 1, 1),
                tx("app.demo", 1, b"third", 0, 3),
            ],
        },
    );
    let three_hash = three.block.expected.block_hash_hex.clone();
    vectors.push(three);

    vectors.push(vector(
        "five-transactions",
        "A block with five transactions. Five leaves split 4 + 1, so the proof for the \
         last leaf has a single step while the others have three.",
        keys(3),
        BlockInput {
            network_id: network.to_owned(),
            height: 3,
            previous_hash_hex: three_hash,
            timestamp_millis: 3000,
            transactions: (0..5u64)
                .map(|n| {
                    tx(
                        "app.demo",
                        1,
                        format!("payload{n}").as_bytes(),
                        (n % 3) as usize,
                        10 + n,
                    )
                })
                .collect(),
        },
    ));

    vectors.push(vector(
        "boundary-values",
        "A genesis block whose network id and one namespace are the maximum 64-byte label, \
         another namespace is the minimum single byte, one payload is empty, and every \
         integer field is at its maximum. Exercises the edges of every V1 field.",
        keys(1),
        BlockInput {
            network_id: "a".repeat(64),
            height: 0,
            previous_hash_hex: zero.clone(),
            timestamp_millis: u64::MAX,
            transactions: vec![
                tx(&"z".repeat(64), u32::MAX, b"", 0, u64::MAX),
                tx("0", 0, b"x", 0, 0),
            ],
        },
    ));

    vectors.push(vector(
        "arbitrary-payload-bytes",
        "A block whose payloads are arbitrary bytes: every byte value in order, a short \
         non-UTF-8 sequence, and a longer repeating pattern. Payloads are opaque and must \
         survive byte for byte.",
        keys(1),
        BlockInput {
            network_id: network.to_owned(),
            height: 1,
            previous_hash_hex: genesis_hash,
            timestamp_millis: 1000,
            transactions: vec![
                tx("app.bytes", 1, &(0..=255u8).collect::<Vec<u8>>(), 0, 1),
                tx("app.bytes", 1, &[0xff, 0xfe, 0x00, 0x80, 0x7f], 0, 2),
                tx(
                    "app.bytes",
                    1,
                    &(0..1000u32).map(|i| (i % 251) as u8).collect::<Vec<u8>>(),
                    0,
                    3,
                ),
            ],
        },
    ));

    vectors
}

/// Renders a vector file exactly as it is committed: pretty JSON plus a final newline.
///
/// # Panics
///
/// Panics if serialization fails, which for these plain structs it cannot.
#[must_use]
pub fn render(vector: &VectorFile) -> String {
    let mut text = serde_json::to_string_pretty(vector).expect("serializable");
    text.push('\n');
    text
}
