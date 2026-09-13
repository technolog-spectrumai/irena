# Public Rust API

Every type below is re-exported from its crate root. The CLI uses exactly these and
nothing else.

## `prunella-canonical`

Deterministic encoding and domain-separated hashing. Knows nothing about ledgers.

```rust
pub trait Canonical: BorshSerialize + BorshDeserialize + Sized {
    fn canonical_bytes(&self) -> Vec<u8>;
    fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, CanonicalError>;
}

pub fn encode<T: BorshSerialize + ?Sized>(value: &T) -> Result<Vec<u8>, CanonicalError>;
pub fn decode<T: BorshDeserialize>(bytes: &[u8]) -> Result<T, CanonicalError>;

pub fn hash_domain(tag: &str, bytes: &[u8]) -> Digest;
pub fn hash_canonical<T: Canonical>(tag: &str, value: &T) -> Digest;
pub struct DomainHasher { /* new(tag), update(bytes), finalize() */ }

pub mod domain { /* TX_SIGN, TX_ID, TX_ROOT, BLOCK_HEADER */ }
pub type Digest = [u8; 32];
pub enum CanonicalError { Encode(String), Decode(String), TrailingBytes { trailing: usize } }
```

`decode` rejects trailing bytes. See [canonicalization.md](canonicalization.md).

## `prunella-core`

Types and the four hash derivations. No I/O, no cryptographic operations.

```rust
pub struct Hash([u8; 32]);        // ZERO, from_bytes, to_bytes, as_bytes, is_zero,
                                  // from_hex, to_hex, Display, FromStr
pub struct TxId(Hash);            // from_hash, hash, from_hex, to_hex
pub struct PublicKey([u8; 32]);   // opaque container; no crypto here
pub struct Signature([u8; 64]);
pub struct NetworkId(String);     // new() validates the label grammar
pub struct Namespace(String);     // same grammar; never interpreted
pub struct SchemaVersion(pub u32);
pub struct BlockHeight(pub u64);  // GENESIS, next(), value(), is_genesis()
pub struct ChainHead { pub height: BlockHeight, pub hash: Hash }

pub struct Transaction {
    pub id: TxId, pub namespace: Namespace, pub schema_version: SchemaVersion,
    pub payload: Vec<u8>, pub signer: PublicKey, pub nonce: u64,
    pub signature: Signature,
}
impl Transaction {
    pub fn signing_message(&self) -> Digest;
    pub fn compute_id(&self) -> TxId;
    pub fn has_consistent_id(&self) -> bool;
    pub fn compute_root(transactions: &[Self]) -> Result<Hash, CoreError>;
}

pub struct TransactionDraft { /* unsigned body */ }
impl TransactionDraft {
    pub fn signing_message(&self) -> Digest;
    pub fn into_transaction(self, signature: Signature) -> Transaction;
}

pub struct BlockHeader { pub version: u16, pub network_id: NetworkId,
    pub height: BlockHeight, pub previous_hash: Hash, pub tx_root: Hash,
    pub tx_count: u32, pub timestamp_millis: u64 }
impl BlockHeader {
    pub fn block_hash(&self) -> Hash;
    pub fn child_draft(&self, txs: Vec<Transaction>, ts: u64) -> Result<BlockDraft, CoreError>;
}

pub struct Block { pub header: BlockHeader, pub transactions: Vec<Transaction> }
pub struct BlockDraft { /* build() derives tx_root and tx_count */ }
pub struct GenesisSpec { pub network_id: NetworkId, pub timestamp_millis: u64,
                         pub transactions: Vec<Transaction> }
```

`BlockDraft::build` derives `tx_root` and `tx_count`, so they cannot be made to disagree
with the transaction list. `TransactionDraft::into_transaction` derives the id after the
signature is attached, so an id can never be computed before the signature it commits to.

The `serde` implementations render hex text for reporting and are never hash pre-images.

## `prunella-crypto`

The only place a signature operation happens.

```rust
pub struct SigningKey;
impl SigningKey {
    pub fn generate() -> Result<Self, CryptoError>;
    pub fn from_seed(seed: [u8; 32]) -> Self;
    pub fn to_seed(&self) -> [u8; 32];
    pub fn public_key(&self) -> PublicKey;
    pub fn sign(&self, message: &[u8]) -> Signature;
    pub fn sign_transaction(&self, draft: TransactionDraft) -> Transaction;
}

pub fn verify(key: &PublicKey, message: &[u8], signature: &Signature) -> Result<(), CryptoError>;
pub fn verify_transaction(transaction: &Transaction) -> Result<(), CryptoError>;

pub enum CryptoError { MalformedPublicKey { signer: String }, MalformedSignature,
                       SignatureMismatch { signer: String }, RandomSource(String) }
```

`sign_transaction` overwrites the draft's declared signer with the key's own public key,
so a transaction can never claim a signer that did not sign it. Verification is strict:
small-order keys and non-canonical signature encodings are rejected. `SigningKey`'s
`Debug` shows only the public key.

## `prunella-verify`

The single implementation of block validity.

```rust
pub fn check_block(context: &BlockContext<'_>, block: &Block) -> Vec<Finding>;
pub fn verify_chain<S: BlockSource + ?Sized>(source: &S, options: VerifyOptions)
    -> VerificationReport;

pub struct BlockContext<'a> {
    pub expected_network: &'a NetworkId,
    pub parent: Option<&'a BlockHeader>,
    pub expected_hash: Option<Hash>,
    pub committed_transactions: Option<&'a dyn TxIdLookup>,
}
impl<'a> BlockContext<'a> {
    pub fn genesis(network: &'a NetworkId) -> Self;
    pub fn child_of(network: &'a NetworkId, parent: &'a BlockHeader) -> Self;
    pub fn expecting_hash(self, hash: Hash) -> Self;
    pub fn with_committed_transactions(self, lookup: &'a dyn TxIdLookup) -> Self;
}

pub trait TxIdLookup { fn contains(&self, id: &TxId) -> Result<bool, SourceError>; }

pub trait BlockSource {
    fn network_id(&self) -> &NetworkId;
    fn genesis_hash(&self) -> Hash;
    fn head(&self) -> Result<ChainHead, SourceError>;
    fn block_at(&self, height: BlockHeight) -> Result<Option<Block>, SourceError>;
}

pub struct VerifyOptions { pub from: Option<BlockHeight>, pub to: Option<BlockHeight>,
                           pub detect_duplicate_transactions: bool }
pub struct VerificationReport { /* … */ pub findings: Vec<Finding> }
impl VerificationReport { pub fn is_valid(&self) -> bool; }

pub struct Finding { pub kind: FindingKind, pub location: Location, pub detail: String }
pub struct Location { pub height: Option<BlockHeight>, pub block_hash: Option<Hash>,
                      pub tx_index: Option<u32>, pub tx_id: Option<TxId> }
pub enum FindingKind { /* 16 variants, see validation.md */ }
```

An empty `Vec<Finding>` means the block is acceptable. See
[validation.md](validation.md).

## `prunella-store`

Persistent, append-only storage.

```rust
impl ChainStore {
    pub fn create(path, genesis: GenesisSpec) -> Result<Self, StoreError>;
    pub fn create_with_policy(path, genesis, policy: Box<dyn BlockAcceptancePolicy>) -> Result<Self, StoreError>;
    pub fn open(path) -> Result<Self, StoreError>;
    pub fn open_with_policy(path, policy) -> Result<Self, StoreError>;

    pub fn network_id(&self) -> &NetworkId;
    pub fn genesis_hash(&self) -> Hash;
    pub fn path(&self) -> &Path;
    pub fn acceptance_policy(&self) -> &'static str;

    pub fn head(&self) -> Result<ChainHead, StoreError>;
    pub fn status(&self) -> Result<ChainStatus, StoreError>;
    pub fn block_at(&self, height: BlockHeight) -> Result<Option<Block>, StoreError>;
    pub fn block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, StoreError>;
    pub fn transaction(&self, id: &TxId) -> Result<Option<LocatedTransaction>, StoreError>;
    pub fn contains_transaction(&self, id: &TxId) -> Result<bool, StoreError>;
    pub fn blocks_in_range(&self, from, to) -> Result<BlockRange, StoreError>;

    pub fn append_block(&self, block: Block) -> Result<ChainHead, StoreError>;
    pub fn append_blocks(&self, blocks: Vec<Block>, existing: ExistingBlockPolicy)
        -> Result<AppendOutcome, StoreError>;
}

pub enum ExistingBlockPolicy { Reject, SkipIfIdentical }
pub struct AppendOutcome { pub appended: u64, pub skipped: u64, pub head: ChainHead }
pub struct LocatedTransaction { pub height: BlockHeight, pub index: u32,
                                pub transaction: Transaction }
pub struct ChainStatus { /* identity, head, counts, format version, policy name */ }
```

`ChainStore` implements `BlockSource` and `TxIdLookup`. There is no method that rewrites
or removes a committed block, and no method that repairs an inconsistent chain. See
[storage.md](storage.md).

The acceptance boundary — `BlockAcceptancePolicy`, `AcceptanceContext`, `Accepted`,
`AcceptanceError`, `LocalDeterministicPolicy` — is documented in
[consensus-boundary.md](consensus-boundary.md).

## `prunella-xml`

```rust
pub fn export(store: &ChainStore, request: &ExportRequest) -> Result<ChainDocument, XmlError>;
pub fn write_document(document: &ChainDocument) -> Result<String, XmlError>;
pub fn read_document(xml: &str) -> Result<ChainDocument, XmlError>;
pub fn read_document_with_limit(xml: &str, max_bytes: u64) -> Result<ChainDocument, XmlError>;

pub fn plan_import(store: &ChainStore, document: &ChainDocument) -> Result<ImportPlan, XmlError>;
pub fn import(store: &ChainStore, document: &ChainDocument) -> Result<AppendOutcome, XmlError>;
pub fn restore(path, document: &ChainDocument) -> Result<(ChainStore, AppendOutcome), XmlError>;

pub struct ExportRequest { pub from: Option<BlockHeight>, pub to: Option<BlockHeight>,
                           pub namespace: Option<Namespace>, pub exported_at_millis: u64 }
pub enum DocumentKind { Full, Range, Projection }
pub struct ChainDocument { /* … */ pub blocks: Vec<DocumentBlock> }
pub struct DocumentBlock { pub block: Block, pub declared_hash: Hash }
pub struct ImportPlan { /* what an import would do, computed without writing */ }
```

`DocumentBlock` keeps the declared hash separate from the block so a mismatch between
what a document claims and what its contents derive is detectable. See
[xml-format.md](xml-format.md).

## Worked example

```rust
use prunella_core::{GenesisSpec, Namespace, NetworkId, SchemaVersion, TransactionDraft};
use prunella_crypto::SigningKey;
use prunella_store::ChainStore;
use prunella_verify::{VerifyOptions, verify_chain};

let network = NetworkId::new("demo")?;
let store = ChainStore::create("demo.chain", GenesisSpec::new(network.clone()))?;

let key = SigningKey::generate()?;
let transaction = key.sign_transaction(TransactionDraft {
    namespace: Namespace::new("app.demo")?,
    schema_version: SchemaVersion(1),
    payload: b"opaque to prunella".to_vec(),
    signer: key.public_key(),
    nonce: 1,
});

let head = store.head()?;
let parent = store.block_at(head.height)?.expect("head block");
let block = parent.header.child_draft(vec![transaction], 1_000)?.build()?;
store.append_block(block)?;

let report = verify_chain(&store, VerifyOptions::default());
assert!(report.is_valid());
```
