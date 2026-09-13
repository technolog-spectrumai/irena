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

```rust
pub mod merkle {
    pub struct TreeTags { pub leaf: &'static str, pub node: &'static str, pub empty: &'static str }
    impl TreeTags { pub const PRUNELLA_V1: Self; }
    pub fn root(tags: TreeTags, leaves: &[Hash]) -> Hash;       // generic tree
    pub fn merkle_root(ids: &[TxId]) -> Hash;                    // the block tree
    pub struct InclusionProof { pub index: u32, pub steps: Vec<ProofStep> }
    impl InclusionProof {
        pub fn generate(ids: &[TxId], index: usize) -> Option<Self>;
        pub fn generate_with(tags: TreeTags, leaves: &[Hash], index: usize) -> Option<Self>;
        pub fn verify(&self, id: &TxId, count: u32, root: &Hash) -> Result<(), ProofError>;
        pub fn verify_with(&self, tags: TreeTags, leaf: &Hash, count: u32, root: &Hash) -> Result<(), ProofError>;
        pub fn verify_against(&self, id: &TxId, header: &BlockHeader) -> Result<(), ProofError>;
    }
}
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

Persistent, append-only storage. [`ChainStorage`] is the contract; `LocalChainStore` is
the one implementation, backed by a local redb file.

```rust
pub trait ChainStorage: BlockSource + Sized {
    type Location;
    type Error;
    type Blocks<'a>: Iterator<Item = Result<Block, Self::Error>> where Self: 'a;

    fn init_genesis(location: Self::Location, genesis: GenesisSpec) -> Result<Self, Self::Error>;
    fn get_block(&self, height: BlockHeight) -> Result<Option<Block>, Self::Error>;
    fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, Self::Error>;
    fn get_transaction(&self, id: &TxId) -> Result<Option<LocatedTransaction>, Self::Error>;
    fn iter_blocks(&self, start: BlockHeight, end: BlockHeight)
        -> Result<Self::Blocks<'_>, Self::Error>;
    fn append_block(&self, block: Block) -> Result<AppendOutcome, Self::Error>;

    /// Provided: defers entirely to `prunella_verify`. Do not override.
    fn verify_from(&self, start_height: BlockHeight) -> VerificationReport { /* ... */ }
}

pub enum AppendStatus { Committed, AlreadyPresent }
pub struct AppendOutcome { pub status: AppendStatus, pub head: ChainHead }
pub struct BatchOutcome { pub appended: u64, pub already_present: u64, pub head: ChainHead }
pub struct LocatedTransaction { pub height: BlockHeight, pub index: u32,
                                pub transaction: Transaction }
pub struct ChainStatus { /* identity, head, counts, format version, policy name */ }
```

`head()` comes from the `BlockSource` supertrait rather than being declared on
`ChainStorage` a second time: declaring it twice would make `store.head()` ambiguous in
code generic over the trait, so there is exactly one.

The trait is not object-safe — `init_genesis` returns `Self` and `iter_blocks` returns an
associated type. Use `BlockSource` where a `dyn` read-only view is needed.

`LocalChainStore` carries inherent methods of the same names. Inherent methods win
method resolution, so `store.get_block(h)` on a concrete store is unambiguous, while
generic code over `ChainStorage` still works. It adds:

```rust
impl LocalChainStore {
    pub fn init_genesis(path, genesis: GenesisSpec) -> Result<Self, StoreError>;
    pub fn init_genesis_with_policy(path, genesis, policy: Box<dyn BlockAcceptancePolicy>)
        -> Result<Self, StoreError>;
    pub fn open(path) -> Result<Self, StoreError>;
    pub fn open_with_policy(path, policy) -> Result<Self, StoreError>;
    pub fn close(self) -> Result<(), StoreError>;

    pub fn network_id(&self) -> &NetworkId;
    pub fn genesis_hash(&self) -> Hash;
    pub fn path(&self) -> &Path;
    pub fn acceptance_policy(&self) -> &'static str;

    pub fn head(&self) -> Result<ChainHead, StoreError>;
    pub fn status(&self) -> Result<ChainStatus, StoreError>;
    pub fn contains_transaction(&self, id: &TxId) -> Result<bool, StoreError>;

    /// Appends a run of blocks in one atomic transaction.
    pub fn append_blocks(&self, blocks: Vec<Block>) -> Result<BatchOutcome, StoreError>;
}
```

`LocalChainStore` implements `ChainStorage`, `BlockSource` and `TxIdLookup`. There is no
method that rewrites or removes a committed block, and no method that repairs an
inconsistent chain. See [storage.md](storage.md).

The acceptance boundary — `BlockAcceptancePolicy`, `AcceptanceContext`, `Accepted`,
`AcceptanceError`, `LocalDeterministicPolicy` — is documented in
[consensus-boundary.md](consensus-boundary.md).

## `prunella-xml`

```rust
pub fn export(store: &LocalChainStore, request: &ExportRequest) -> Result<ChainDocument, XmlError>;
pub fn write_document(document: &ChainDocument) -> Result<String, XmlError>;
pub fn read_document(xml: &str) -> Result<ChainDocument, XmlError>;
pub fn read_document_with_limit(xml: &str, max_bytes: u64) -> Result<ChainDocument, XmlError>;

pub fn plan_import(store: &LocalChainStore, document: &ChainDocument) -> Result<ImportPlan, XmlError>;
pub fn import(store: &LocalChainStore, document: &ChainDocument) -> Result<BatchOutcome, XmlError>;
pub fn restore(path, document: &ChainDocument) -> Result<(LocalChainStore, BatchOutcome), XmlError>;

pub struct ExportRequest { pub from: Option<BlockHeight>, pub to: Option<BlockHeight>,
                           pub namespace: Option<Namespace>, pub exported_at_millis: u64 }
pub enum DocumentKind { Full, Range, Projection }
pub struct ChainDocument { /* … */ pub blocks: Vec<DocumentBlock> }
pub struct DocumentBlock { pub block: Block, pub declared_hash: Hash }
pub struct ImportPlan { /* what an import would do, computed without writing */ }

pub const FORMAT_VERSION: u32 = 2;                 // written
pub const SUPPORTED_FORMAT_VERSIONS: [u32; 2];     // read: 1 and 2
pub enum PayloadEncoding { Base64, Xml }           // choose(&[u8]) is a pure function of the bytes
pub fn is_single_element(text: &str) -> bool;      // the nesting criterion
```

`DocumentBlock` keeps the declared hash separate from the block so a mismatch between
what a document claims and what its contents derive is detectable. See
[xml-format.md](xml-format.md).

## Worked example

```rust
use prunella_core::{BlockHeight, GenesisSpec, Namespace, NetworkId, SchemaVersion,
                    TransactionDraft};
use prunella_crypto::SigningKey;
use prunella_store::{ChainStorage, LocalChainStore};

let network = NetworkId::new("demo")?;
let store = LocalChainStore::init_genesis("demo.chain", GenesisSpec::new(network.clone()))?;

let key = SigningKey::generate()?;
let transaction = key.sign_transaction(TransactionDraft {
    namespace: Namespace::new("app.demo")?,
    schema_version: SchemaVersion(1),
    payload: b"opaque to prunella".to_vec(),
    signer: key.public_key(),
    nonce: 1,
});

let head = store.head()?;
let parent = store.get_block(head.height)?.expect("head block");
let block = parent.header.child_draft(vec![transaction], 1_000)?.build()?;
store.append_block(block)?;

assert!(store.verify_from(BlockHeight::GENESIS).is_valid());
```
