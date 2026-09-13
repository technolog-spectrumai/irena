//! The persistent chain store.

use crate::acceptance::{AcceptanceContext, BlockAcceptancePolicy, LocalDeterministicPolicy};
use crate::error::StoreError;
use crate::storage::{AppendOutcome, AppendStatus, BatchOutcome, ChainStorage, source_error};
use crate::tables::{
    BLOCKS, HEIGHT_BY_HASH, META, STORE_FORMAT_VERSION, TX_LOCATION, decode_location,
    encode_location, meta_key,
};
use prunella_canonical::Canonical;
use prunella_core::{
    Block, BlockHeader, BlockHeight, ChainHead, GenesisSpec, Hash, NetworkId, Transaction, TxId,
};
use prunella_verify::{BlockSource, SourceError, TxIdLookup};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableError};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A transaction located within the chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedTransaction {
    /// Height of the block holding it.
    pub height: BlockHeight,
    /// Index within that block.
    pub index: u32,
    /// The transaction itself.
    pub transaction: Transaction,
}

/// Summary of a chain, without walking it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainStatus {
    /// The chain identifier.
    pub network_id: NetworkId,
    /// The genesis hash recorded at creation.
    pub genesis_hash: Hash,
    /// The current head.
    pub head: ChainHead,
    /// Number of blocks, including genesis.
    pub block_count: u64,
    /// Number of transactions across the whole chain.
    pub transaction_count: u64,
    /// The store format version on disk.
    pub format_version: u32,
    /// The acceptance policy currently in force.
    pub acceptance_policy: &'static str,
}

/// An append-only chain on disk.
///
/// Committed blocks are immutable: there is no method here that rewrites or removes
/// one. Appends go through the configured [`BlockAcceptancePolicy`] and commit in a
/// single database transaction, so an append either happens completely or not at all.
pub struct LocalChainStore {
    database: Database,
    path: PathBuf,
    network_id: NetworkId,
    genesis_hash: Hash,
    policy: Box<dyn BlockAcceptancePolicy>,
}

impl LocalChainStore {
    /// Creates a chain and writes its genesis block.
    ///
    /// The genesis block is derived from the specification, so two instances given the
    /// same specification produce the same genesis hash without communicating.
    ///
    /// Genesis is written exactly once. This fails rather than overwriting an existing
    /// chain, because replacing a chain's root is never something to do by accident.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::AlreadyExists`] if the path is occupied, or a database
    /// error if the file could not be created.
    pub fn init_genesis(path: impl AsRef<Path>, genesis: GenesisSpec) -> Result<Self, StoreError> {
        Self::init_genesis_with_policy(path, genesis, Box::new(LocalDeterministicPolicy))
    }

    /// Creates a chain with a specific acceptance policy.
    ///
    /// # Errors
    ///
    /// As [`LocalChainStore::init_genesis`], plus [`StoreError::NotAccepted`] if the
    /// policy refuses the genesis block.
    pub fn init_genesis_with_policy(
        path: impl AsRef<Path>,
        genesis: GenesisSpec,
        policy: Box<dyn BlockAcceptancePolicy>,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(StoreError::AlreadyExists {
                path: path.display().to_string(),
            });
        }
        let network_id = genesis.network_id.clone();
        let block = genesis.build()?;
        let genesis_hash = block.hash();

        let database = Database::create(&path).map_err(StoreError::database)?;
        let store = Self {
            database,
            path,
            network_id,
            genesis_hash,
            policy,
        };
        store.write_genesis(&block)?;
        Ok(store)
    }

    /// Opens an existing chain.
    ///
    /// Verifies the store's own bookkeeping against its contents and refuses to open a
    /// chain that disagrees with itself. It never repairs: a damaged chain is reported
    /// so it can be restored from a backup, not quietly rewritten.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotAChain`], [`StoreError::UnsupportedFormatVersion`] or
    /// [`StoreError::Inconsistent`] when the file is not a usable chain.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_policy(path, Box::new(LocalDeterministicPolicy))
    }

    /// Opens an existing chain with a specific acceptance policy.
    ///
    /// # Errors
    ///
    /// As [`LocalChainStore::open`].
    pub fn open_with_policy(
        path: impl AsRef<Path>,
        policy: Box<dyn BlockAcceptancePolicy>,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        let display = path.display().to_string();
        if !path.exists() {
            return Err(StoreError::NotAChain {
                path: display,
                detail: "no such file".to_owned(),
            });
        }
        let database = Database::open(&path).map_err(|error| StoreError::NotAChain {
            path: display.clone(),
            detail: error.to_string(),
        })?;

        let read = database.begin_read().map_err(StoreError::database)?;
        let meta = match read.open_table(META) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => {
                return Err(StoreError::NotAChain {
                    path: display,
                    detail: "the chain metadata table is missing".to_owned(),
                });
            }
            Err(error) => return Err(StoreError::database(error)),
        };

        let format_version = read_u32(&meta, meta_key::FORMAT_VERSION, &display)?;
        if format_version != STORE_FORMAT_VERSION {
            return Err(StoreError::UnsupportedFormatVersion {
                found: format_version,
                supported: STORE_FORMAT_VERSION,
            });
        }
        let network_id = NetworkId::new(read_text(&meta, meta_key::NETWORK_ID, &display)?)?;
        let genesis_hash = read_hash(&meta, meta_key::GENESIS_HASH, &display)?;
        drop(meta);
        drop(read);

        let store = Self {
            database,
            path,
            network_id,
            genesis_hash,
            policy,
        };
        store.check_consistency()?;
        Ok(store)
    }

    /// The chain this store holds.
    #[must_use]
    pub fn network_id(&self) -> &NetworkId {
        &self.network_id
    }

    /// The genesis hash recorded when the chain was created.
    #[must_use]
    pub fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    /// The path of the chain file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The acceptance policy currently in force.
    #[must_use]
    pub fn acceptance_policy(&self) -> &'static str {
        self.policy.name()
    }

    /// Reads the current head.
    ///
    /// # Errors
    ///
    /// Returns a database error, or [`StoreError::Inconsistent`] if the head record is
    /// missing or malformed.
    pub fn head(&self) -> Result<ChainHead, StoreError> {
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let meta = read.open_table(META).map_err(StoreError::database)?;
        read_head(&meta, &self.path.display().to_string())
    }

    /// Reads a summary of the chain without walking it.
    ///
    /// # Errors
    ///
    /// Returns a database error, or [`StoreError::Inconsistent`] for malformed records.
    pub fn status(&self) -> Result<ChainStatus, StoreError> {
        let display = self.path.display().to_string();
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let meta = read.open_table(META).map_err(StoreError::database)?;
        let head = read_head(&meta, &display)?;
        Ok(ChainStatus {
            network_id: self.network_id.clone(),
            genesis_hash: self.genesis_hash,
            head,
            block_count: head.block_count(),
            transaction_count: read_u64(&meta, meta_key::TRANSACTION_COUNT, &display)?,
            format_version: read_u32(&meta, meta_key::FORMAT_VERSION, &display)?,
            acceptance_policy: self.policy.name(),
        })
    }

    /// Reads the block at a height.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::CorruptBlock`] if the stored bytes are not a block.
    pub fn get_block(&self, height: BlockHeight) -> Result<Option<Block>, StoreError> {
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let blocks = read.open_table(BLOCKS).map_err(StoreError::database)?;
        read_block(&blocks, height)
    }

    /// Reads the block with a given hash.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::CorruptBlock`] if the stored bytes are not a block, or
    /// [`StoreError::Inconsistent`] if the hash index points at a missing block.
    pub fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, StoreError> {
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let index = read
            .open_table(HEIGHT_BY_HASH)
            .map_err(StoreError::database)?;
        let Some(height) = index.get(hash.as_bytes()).map_err(StoreError::database)? else {
            return Ok(None);
        };
        let height = BlockHeight(height.value());
        let blocks = read.open_table(BLOCKS).map_err(StoreError::database)?;
        read_block(&blocks, height)?.map_or_else(
            || {
                Err(StoreError::Inconsistent {
                    detail: format!(
                        "hash index points at height {height}, where no block is stored"
                    ),
                })
            },
            |block| Ok(Some(block)),
        )
    }

    /// Reads a transaction by id, with the position it was committed at.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Inconsistent`] if the index points somewhere the
    /// transaction is not.
    pub fn get_transaction(&self, id: &TxId) -> Result<Option<LocatedTransaction>, StoreError> {
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let locations = read.open_table(TX_LOCATION).map_err(StoreError::database)?;
        let Some(raw) = locations.get(id.as_bytes()).map_err(StoreError::database)? else {
            return Ok(None);
        };
        let Some((height, index)) = decode_location(raw.value()) else {
            return Err(StoreError::Inconsistent {
                detail: format!("transaction index entry for {id} is malformed"),
            });
        };
        let height = BlockHeight(height);
        let blocks = read.open_table(BLOCKS).map_err(StoreError::database)?;
        let Some(block) = read_block(&blocks, height)? else {
            return Err(StoreError::Inconsistent {
                detail: format!(
                    "transaction {id} is indexed at height {height}, where no block is stored"
                ),
            });
        };
        let Some(transaction) = block.transactions.get(index as usize) else {
            return Err(StoreError::Inconsistent {
                detail: format!(
                    "transaction {id} is indexed at height {height} index {index}, which the block does not have"
                ),
            });
        };
        if transaction.id != *id {
            return Err(StoreError::Inconsistent {
                detail: format!(
                    "transaction index entry for {id} points at {}",
                    transaction.id
                ),
            });
        }
        Ok(Some(LocatedTransaction {
            height,
            index,
            transaction: transaction.clone(),
        }))
    }

    /// Whether a transaction id is already committed anywhere in this chain.
    ///
    /// # Errors
    ///
    /// Returns a database error if the index could not be read.
    pub fn contains_transaction(&self, id: &TxId) -> Result<bool, StoreError> {
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let locations = read.open_table(TX_LOCATION).map_err(StoreError::database)?;
        Ok(locations
            .get(id.as_bytes())
            .map_err(StoreError::database)?
            .is_some())
    }

    /// Iterates blocks over the inclusive height range `start..=end`.
    ///
    /// Read from a single consistent snapshot, so a concurrent append cannot make the
    /// range internally inconsistent. An empty range yields nothing.
    ///
    /// # Errors
    ///
    /// Returns a database error if the snapshot could not be opened.
    pub fn iter_blocks(
        &self,
        start: BlockHeight,
        end: BlockHeight,
    ) -> Result<BlockRange, StoreError> {
        let transaction = self.database.begin_read().map_err(StoreError::database)?;
        Ok(BlockRange {
            transaction,
            next: (start <= end).then_some(start),
            end,
        })
    }

    /// Validates a block and, if it is acceptable, commits it atomically.
    ///
    /// Returns [`AppendStatus::AlreadyPresent`] when a byte-identical block is already
    /// committed at that height: re-offering a block the chain already holds is how a
    /// retried transfer behaves, and it is not an error. A *different* block at a
    /// committed height is [`StoreError::ForkedHistory`], because storing it would
    /// rewrite history.
    ///
    /// A successful append means locally accepted after full deterministic validation.
    /// It does not mean distributed finality; there is no consensus here.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotAccepted`] if the block breaks a rule,
    /// [`StoreError::NonContiguous`] if it does not directly follow the head, or
    /// [`StoreError::ForkedHistory`] if it contradicts a committed block.
    pub fn append_block(&self, block: Block) -> Result<AppendOutcome, StoreError> {
        let outcome = self.append_blocks(vec![block])?;
        Ok(AppendOutcome {
            status: if outcome.appended == 1 {
                AppendStatus::Committed
            } else {
                AppendStatus::AlreadyPresent
            },
            head: outcome.head,
        })
    }

    /// Appends a run of blocks in one atomic database transaction.
    ///
    /// Either every block is committed or none is: a failure anywhere returns before
    /// the commit, so the chain and all of its indexes are left exactly as they were.
    ///
    /// Blocks already committed identically are counted and skipped, so re-applying a
    /// run that was partly applied before is safe.
    ///
    /// # Errors
    ///
    /// As [`LocalChainStore::append_block`].
    pub fn append_blocks(&self, blocks: Vec<Block>) -> Result<BatchOutcome, StoreError> {
        if blocks.is_empty() {
            return Ok(BatchOutcome {
                appended: 0,
                already_present: 0,
                head: self.head()?,
            });
        }
        let display = self.path.display().to_string();
        let write = self.begin_durable_write()?;
        let outcome = {
            let mut meta = write.open_table(META).map_err(StoreError::database)?;
            let mut block_table = write.open_table(BLOCKS).map_err(StoreError::database)?;
            let mut hash_index = write
                .open_table(HEIGHT_BY_HASH)
                .map_err(StoreError::database)?;
            let mut tx_index = write
                .open_table(TX_LOCATION)
                .map_err(StoreError::database)?;

            let mut head = read_head(&meta, &display)?;
            let mut transaction_count = read_u64(&meta, meta_key::TRANSACTION_COUNT, &display)?;
            let mut appended = 0u64;
            let mut already_present = 0u64;
            let mut parent: Option<BlockHeader> = None;
            let mut pending: HashSet<TxId> = HashSet::new();

            for block in blocks {
                let height = block.header.height;

                if height.value() <= head.height.value() {
                    let committed = read_block(&block_table, height)?.ok_or_else(|| {
                        StoreError::Inconsistent {
                            detail: format!(
                                "head is {head} but no block is stored at height {height}"
                            ),
                        }
                    })?;
                    // Identical is a no-op; different is a rewrite of history and is
                    // refused. Comparing the whole block, not just its hash, means a
                    // block that hashes the same but differs in some byte the header
                    // does not commit to is still caught.
                    if committed == block {
                        already_present += 1;
                        continue;
                    }
                    return Err(StoreError::ForkedHistory {
                        height,
                        existing: committed.hash(),
                        offered: block.hash(),
                    });
                }

                let expected = head.height.next()?;
                if height != expected {
                    return Err(StoreError::NonContiguous {
                        expected,
                        found: height,
                    });
                }

                let parent_header = match parent.take() {
                    Some(header) => header,
                    None => {
                        read_block(&block_table, head.height)?
                            .ok_or_else(|| StoreError::Inconsistent {
                                detail: format!("head is {head} but no block is stored there"),
                            })?
                            .header
                    }
                };

                {
                    let lookup = CommittedTransactions {
                        table: &tx_index,
                        pending: &pending,
                    };
                    let context = AcceptanceContext {
                        network_id: &self.network_id,
                        genesis_hash: self.genesis_hash,
                        head: Some(head),
                        parent: Some(&parent_header),
                        committed_transactions: &lookup,
                    };
                    self.policy.evaluate(&context, &block)?;
                }

                let hash = block.hash();
                insert_block(
                    &mut block_table,
                    &mut hash_index,
                    &mut tx_index,
                    &block,
                    hash,
                )?;
                for transaction in &block.transactions {
                    pending.insert(transaction.id);
                }
                transaction_count += block.transactions.len() as u64;
                head = ChainHead::new(height, hash);
                parent = Some(block.header);
                appended += 1;
            }

            write_head(&mut meta, head)?;
            write_u64(&mut meta, meta_key::TRANSACTION_COUNT, transaction_count)?;
            BatchOutcome {
                appended,
                already_present,
                head,
            }
        };
        // Nothing above this line is visible to any reader. The commit is what makes
        // the block, the hash index, the transaction index and the head record appear,
        // all at once, and it is durable by the time it returns.
        write.commit().map_err(StoreError::database)?;
        Ok(outcome)
    }

    /// Closes the chain, flushing and releasing the database file.
    ///
    /// Dropping the store closes it too. This exists so a caller that wants to know the
    /// close succeeded can find out, rather than discovering a problem later.
    ///
    /// # Errors
    ///
    /// Returns a database error if the file could not be released cleanly.
    pub fn close(self) -> Result<(), StoreError> {
        // Every commit is already durable, so there is nothing buffered to flush. The
        // drop releases the file lock.
        drop(self);
        Ok(())
    }

    /// Begins a write transaction that is durable by the time it commits.
    ///
    /// Stated explicitly rather than relied on as a default: a ledger whose commits
    /// were buffered would lose blocks it had already reported as appended.
    fn begin_durable_write(&self) -> Result<redb::WriteTransaction, StoreError> {
        let mut write = self.database.begin_write().map_err(StoreError::database)?;
        write
            .set_durability(Durability::Immediate)
            .map_err(StoreError::database)?;
        Ok(write)
    }

    /// Writes the genesis block and the chain metadata in one transaction.
    fn write_genesis(&self, block: &Block) -> Result<(), StoreError> {
        let write = self.begin_durable_write()?;
        {
            let mut meta = write.open_table(META).map_err(StoreError::database)?;
            let mut block_table = write.open_table(BLOCKS).map_err(StoreError::database)?;
            let mut hash_index = write
                .open_table(HEIGHT_BY_HASH)
                .map_err(StoreError::database)?;
            let mut tx_index = write
                .open_table(TX_LOCATION)
                .map_err(StoreError::database)?;

            {
                let pending = HashSet::new();
                let lookup = CommittedTransactions {
                    table: &tx_index,
                    pending: &pending,
                };
                let context = AcceptanceContext {
                    network_id: &self.network_id,
                    genesis_hash: self.genesis_hash,
                    head: None,
                    parent: None,
                    committed_transactions: &lookup,
                };
                self.policy.evaluate(&context, block)?;
            }

            insert_block(
                &mut block_table,
                &mut hash_index,
                &mut tx_index,
                block,
                self.genesis_hash,
            )?;
            write_u32(&mut meta, meta_key::FORMAT_VERSION, STORE_FORMAT_VERSION)?;
            write_bytes(
                &mut meta,
                meta_key::NETWORK_ID,
                self.network_id.as_str().as_bytes(),
            )?;
            write_bytes(
                &mut meta,
                meta_key::GENESIS_HASH,
                self.genesis_hash.as_bytes(),
            )?;
            write_head(
                &mut meta,
                ChainHead::new(BlockHeight::GENESIS, self.genesis_hash),
            )?;
            write_u64(
                &mut meta,
                meta_key::TRANSACTION_COUNT,
                block.transactions.len() as u64,
            )?;
        }
        write.commit().map_err(StoreError::database)?;
        Ok(())
    }

    /// Checks the store's bookkeeping against its contents, without repairing anything.
    ///
    /// This is a constant-time sanity check, not a full audit: it confirms that the
    /// chain's own records agree with the blocks at genesis and at the head. A full
    /// walk is `prunella-verify`'s job.
    fn check_consistency(&self) -> Result<(), StoreError> {
        let display = self.path.display().to_string();
        let read = self.database.begin_read().map_err(StoreError::database)?;
        let meta = read.open_table(META).map_err(StoreError::database)?;
        let blocks = read.open_table(BLOCKS).map_err(StoreError::database)?;
        let head = read_head(&meta, &display)?;

        let genesis =
            read_block(&blocks, BlockHeight::GENESIS)?.ok_or_else(|| StoreError::Inconsistent {
                detail: "the chain has no block at height 0".to_owned(),
            })?;
        if genesis.hash() != self.genesis_hash {
            return Err(StoreError::Inconsistent {
                detail: format!(
                    "chain records genesis {} but the block at height 0 hashes to {}",
                    self.genesis_hash,
                    genesis.hash()
                ),
            });
        }
        if genesis.header.network_id != self.network_id {
            return Err(StoreError::Inconsistent {
                detail: format!(
                    "chain records network {} but genesis declares {}",
                    self.network_id, genesis.header.network_id
                ),
            });
        }

        let head_block =
            read_block(&blocks, head.height)?.ok_or_else(|| StoreError::Inconsistent {
                detail: format!(
                    "head is {head} but no block is stored at height {}",
                    head.height
                ),
            })?;
        if head_block.hash() != head.hash {
            return Err(StoreError::Inconsistent {
                detail: format!(
                    "head records {} at height {} but the block there hashes to {}",
                    head.hash,
                    head.height,
                    head_block.hash()
                ),
            });
        }
        Ok(())
    }
}

impl TxIdLookup for LocalChainStore {
    fn contains(&self, id: &TxId) -> Result<bool, SourceError> {
        Self::contains_transaction(self, id).map_err(|error| SourceError::new(error.to_string()))
    }
}

impl core::fmt::Debug for LocalChainStore {
    /// Renders the chain's identity, never its contents.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalChainStore")
            .field("path", &self.path)
            .field("network_id", &self.network_id)
            .field("genesis_hash", &self.genesis_hash)
            .field("acceptance_policy", &self.policy.name())
            .finish()
    }
}

impl BlockSource for LocalChainStore {
    fn network_id(&self) -> &NetworkId {
        &self.network_id
    }

    fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    fn head(&self) -> Result<ChainHead, SourceError> {
        Self::head(self).map_err(|error| source_error(&error))
    }

    fn block_at(&self, height: BlockHeight) -> Result<Option<Block>, SourceError> {
        Self::get_block(self, height).map_err(|error| source_error(&error))
    }
}

/// The local file-backed implementation of the storage contract.
///
/// Every method forwards to the inherent method of the same name. The inherent methods
/// take priority in method resolution, so calling `store.get_block(h)` on a concrete
/// `LocalChainStore` is unambiguous while generic code over [`ChainStorage`] still
/// works.
impl ChainStorage for LocalChainStore {
    type Location = PathBuf;
    type Error = StoreError;
    type Blocks<'a> = BlockRange;

    fn init_genesis(location: Self::Location, genesis: GenesisSpec) -> Result<Self, Self::Error> {
        Self::init_genesis(location, genesis)
    }

    fn get_block(&self, height: BlockHeight) -> Result<Option<Block>, Self::Error> {
        Self::get_block(self, height)
    }

    fn get_block_by_hash(&self, hash: &Hash) -> Result<Option<Block>, Self::Error> {
        Self::get_block_by_hash(self, hash)
    }

    fn get_transaction(&self, id: &TxId) -> Result<Option<LocatedTransaction>, Self::Error> {
        Self::get_transaction(self, id)
    }

    fn iter_blocks(
        &self,
        start: BlockHeight,
        end: BlockHeight,
    ) -> Result<Self::Blocks<'_>, Self::Error> {
        Self::iter_blocks(self, start, end)
    }

    fn append_block(&self, block: Block) -> Result<AppendOutcome, Self::Error> {
        Self::append_block(self, block)
    }
}

/// An iterator over a contiguous run of blocks from one snapshot.
pub struct BlockRange {
    transaction: redb::ReadTransaction,
    next: Option<BlockHeight>,
    end: BlockHeight,
}

impl Iterator for BlockRange {
    type Item = Result<Block, StoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        let height = self.next?;
        self.next = if height >= self.end {
            None
        } else {
            height.next().ok()
        };

        let blocks = match self.transaction.open_table(BLOCKS) {
            Ok(table) => table,
            Err(error) => return Some(Err(StoreError::database(error))),
        };
        match read_block(&blocks, height) {
            Ok(Some(block)) => Some(Ok(block)),
            Ok(None) => {
                self.next = None;
                Some(Err(StoreError::Inconsistent {
                    detail: format!("no block is stored at height {height}"),
                }))
            }
            Err(error) => {
                self.next = None;
                Some(Err(error))
            }
        }
    }
}

/// Transaction ids already committed, including those committed earlier in this
/// write transaction but not yet visible to a reader.
struct CommittedTransactions<'a, T> {
    table: &'a T,
    pending: &'a HashSet<TxId>,
}

impl<T: ReadableTable<&'static [u8], &'static [u8]>> TxIdLookup for CommittedTransactions<'_, T> {
    fn contains(&self, id: &TxId) -> Result<bool, SourceError> {
        if self.pending.contains(id) {
            return Ok(true);
        }
        self.table
            .get(id.as_bytes())
            .map(|found| found.is_some())
            .map_err(|error| SourceError::new(error.to_string()))
    }
}

fn insert_block(
    blocks: &mut redb::Table<'_, u64, &'static [u8]>,
    hash_index: &mut redb::Table<'_, &'static [u8], u64>,
    tx_index: &mut redb::Table<'_, &'static [u8], &'static [u8]>,
    block: &Block,
    hash: Hash,
) -> Result<(), StoreError> {
    let height = block.header.height.value();
    blocks
        .insert(height, block.canonical_bytes().as_slice())
        .map_err(StoreError::database)?;
    hash_index
        .insert(hash.as_bytes(), height)
        .map_err(StoreError::database)?;
    for (index, transaction) in block.transactions.iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        tx_index
            .insert(
                transaction.id.as_bytes(),
                encode_location(height, index).as_slice(),
            )
            .map_err(StoreError::database)?;
    }
    Ok(())
}

fn read_block<T: ReadableTable<u64, &'static [u8]>>(
    blocks: &T,
    height: BlockHeight,
) -> Result<Option<Block>, StoreError> {
    let Some(raw) = blocks.get(height.value()).map_err(StoreError::database)? else {
        return Ok(None);
    };
    Block::from_canonical_bytes(raw.value())
        .map(Some)
        .map_err(|error| StoreError::CorruptBlock {
            height,
            detail: error.to_string(),
        })
}

fn read_meta<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    key: &str,
    path: &str,
) -> Result<Vec<u8>, StoreError> {
    meta.get(key)
        .map_err(StoreError::database)?
        .map(|value| value.value().to_vec())
        .ok_or_else(|| StoreError::NotAChain {
            path: path.to_owned(),
            detail: format!("the chain metadata has no {key} record"),
        })
}

fn read_u32<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    key: &str,
    path: &str,
) -> Result<u32, StoreError> {
    let bytes = read_meta(meta, key, path)?;
    bytes
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| StoreError::Inconsistent {
            detail: format!("the {key} record is not a 4-byte integer"),
        })
}

fn read_u64<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    key: &str,
    path: &str,
) -> Result<u64, StoreError> {
    let bytes = read_meta(meta, key, path)?;
    bytes
        .try_into()
        .map(u64::from_le_bytes)
        .map_err(|_| StoreError::Inconsistent {
            detail: format!("the {key} record is not an 8-byte integer"),
        })
}

fn read_hash<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    key: &str,
    path: &str,
) -> Result<Hash, StoreError> {
    let bytes = read_meta(meta, key, path)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| StoreError::Inconsistent {
        detail: format!("the {key} record is not a 32-byte hash"),
    })?;
    Ok(Hash::from_bytes(bytes))
}

fn read_text<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    key: &str,
    path: &str,
) -> Result<String, StoreError> {
    let bytes = read_meta(meta, key, path)?;
    String::from_utf8(bytes).map_err(|_| StoreError::Inconsistent {
        detail: format!("the {key} record is not valid UTF-8"),
    })
}

fn read_head<T: ReadableTable<&'static str, &'static [u8]>>(
    meta: &T,
    path: &str,
) -> Result<ChainHead, StoreError> {
    Ok(ChainHead::new(
        BlockHeight(read_u64(meta, meta_key::HEAD_HEIGHT, path)?),
        read_hash(meta, meta_key::HEAD_HASH, path)?,
    ))
}

fn write_bytes(
    meta: &mut redb::Table<'_, &'static str, &'static [u8]>,
    key: &str,
    value: &[u8],
) -> Result<(), StoreError> {
    meta.insert(key, value).map_err(StoreError::database)?;
    Ok(())
}

fn write_u32(
    meta: &mut redb::Table<'_, &'static str, &'static [u8]>,
    key: &str,
    value: u32,
) -> Result<(), StoreError> {
    write_bytes(meta, key, &value.to_le_bytes())
}

fn write_u64(
    meta: &mut redb::Table<'_, &'static str, &'static [u8]>,
    key: &str,
    value: u64,
) -> Result<(), StoreError> {
    write_bytes(meta, key, &value.to_le_bytes())
}

fn write_head(
    meta: &mut redb::Table<'_, &'static str, &'static [u8]>,
    head: ChainHead,
) -> Result<(), StoreError> {
    write_u64(meta, meta_key::HEAD_HEIGHT, head.height.value())?;
    write_bytes(meta, meta_key::HEAD_HASH, head.hash.as_bytes())
}
