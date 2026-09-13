//! An in-memory chain used to exercise verification without a storage backend.

use prunella_core::{
    Block, BlockDraft, BlockHeight, ChainHead, GenesisSpec, Hash, Namespace, NetworkId,
    SchemaVersion, Transaction, TransactionDraft,
};
use prunella_crypto::SigningKey;
use prunella_verify::{BlockSource, SourceError};

/// A chain held entirely in memory, indexed by height.
pub struct MemoryChain {
    pub network_id: NetworkId,
    pub genesis_hash: Hash,
    pub blocks: Vec<Option<Block>>,
    pub head: Option<ChainHead>,
    pub read_failure_at: Option<BlockHeight>,
}

impl MemoryChain {
    /// Builds genesis plus `count` further blocks, one transaction each.
    pub fn with_blocks(count: u64) -> Self {
        let network_id = NetworkId::new("testnet").expect("valid network id");
        let genesis = GenesisSpec::new(network_id.clone())
            .build()
            .expect("genesis");
        let mut chain = Self {
            network_id,
            genesis_hash: genesis.hash(),
            blocks: vec![Some(genesis)],
            head: None,
            read_failure_at: None,
        };
        for index in 1..=count {
            let block = chain.next_block(vec![signed_transaction(1, &format!("tx{index}"), index)]);
            chain.push(block);
        }
        chain
    }

    /// Derives the block that would follow the current tip.
    pub fn next_block(&self, transactions: Vec<Transaction>) -> Block {
        let parent = self.tip();
        parent
            .header
            .child_draft(transactions, parent.header.timestamp_millis + 1)
            .expect("child draft")
            .build()
            .expect("build")
    }

    pub fn tip(&self) -> &Block {
        self.blocks
            .iter()
            .rev()
            .flatten()
            .next()
            .expect("a chain always has a genesis")
    }

    pub fn push(&mut self, block: Block) {
        self.blocks.push(Some(block));
        self.head = None;
    }

    /// Replaces a block without touching any other, simulating tampering.
    pub fn replace(&mut self, height: BlockHeight, block: Block) {
        self.blocks[height.value() as usize] = Some(block);
    }

    pub fn remove(&mut self, height: BlockHeight) {
        self.blocks[height.value() as usize] = None;
    }

    fn resolved_head(&self) -> ChainHead {
        if let Some(head) = self.head {
            return head;
        }
        let height = BlockHeight(self.blocks.len() as u64 - 1);
        let hash = self.blocks[height.value() as usize]
            .as_ref()
            .map_or(Hash::ZERO, prunella_core::Block::hash);
        ChainHead::new(height, hash)
    }
}

impl BlockSource for MemoryChain {
    fn network_id(&self) -> &NetworkId {
        &self.network_id
    }

    fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    fn head(&self) -> Result<ChainHead, SourceError> {
        Ok(self.resolved_head())
    }

    fn block_at(&self, height: BlockHeight) -> Result<Option<Block>, SourceError> {
        if self.read_failure_at == Some(height) {
            return Err(SourceError::new("simulated storage corruption"));
        }
        Ok(self
            .blocks
            .get(height.value() as usize)
            .and_then(Clone::clone))
    }
}

pub fn key(seed: u8) -> SigningKey {
    SigningKey::from_seed([seed; 32])
}

pub fn signed_transaction(seed: u8, payload: &str, nonce: u64) -> Transaction {
    key(seed).sign_transaction(TransactionDraft {
        namespace: Namespace::new("app.demo").expect("valid namespace"),
        schema_version: SchemaVersion(1),
        payload: payload.as_bytes().to_vec(),
        signer: key(seed).public_key(),
        nonce,
    })
}

/// Rebuilds a block with the same transactions but an altered draft field.
pub fn rebuild(block: &Block, mutate: impl FnOnce(&mut BlockDraft)) -> Block {
    let mut draft = BlockDraft {
        network_id: block.header.network_id.clone(),
        height: block.header.height,
        previous_hash: block.header.previous_hash,
        timestamp_millis: block.header.timestamp_millis,
        transactions: block.transactions.clone(),
    };
    mutate(&mut draft);
    draft.build().expect("build")
}
