//! Command line surface.
//!
//! Every command here is a thin call into the Prunella libraries. No hashing,
//! validation, encoding or storage logic lives in this crate: if the CLI and a library
//! could ever disagree about what a chain says, the CLI would be a second source of
//! truth, and a ledger cannot have two.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Prunella: a standalone, organization-agnostic immutable ledger.
#[derive(Parser, Debug)]
#[command(name = "prunella", version, about, long_about = None)]
pub struct Cli {
    /// Path to the chain file.
    #[arg(
        long,
        global = true,
        env = "PRUNELLA_CHAIN",
        default_value = "prunella.chain"
    )]
    pub chain: PathBuf,

    /// Emit machine-readable JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create a new chain from a genesis specification.
    Init(InitArgs),
    /// Show the chain's identity, head and counts.
    Status,
    /// Verify the chain from genesis to head and report every defect.
    Verify(VerifyArgs),
    /// Show one block, by height or by hash.
    Block(BlockArgs),
    /// Show one transaction, by id.
    Tx(TxArgs),
    /// Sign and append a block.
    Append(AppendArgs),
    /// Write the chain, or part of it, as XML.
    Export(ExportArgs),
    /// Apply an XML document to a chain.
    Import(ImportArgs),
    /// Generate an ed25519 signing key.
    Keygen(KeygenArgs),
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Chain identifier. Lowercase letters, digits, '.', '_' and '-'.
    #[arg(long)]
    pub network: String,

    /// Genesis timestamp in milliseconds.
    ///
    /// Defaults to zero so that the same network identifier always produces the same
    /// genesis hash, on any machine, at any time.
    #[arg(long, default_value_t = 0)]
    pub genesis_timestamp: u64,
}

#[derive(clap::Args, Debug)]
pub struct VerifyArgs {
    /// First height to check. Defaults to genesis.
    #[arg(long)]
    pub from: Option<u64>,

    /// Last height to check. Defaults to the head.
    #[arg(long)]
    pub to: Option<u64>,

    /// Skip the check for a transaction committed in more than one block.
    ///
    /// That check holds every transaction id seen so far in memory. Skipping it makes a
    /// spot check of a very long chain cheaper, at the cost of not detecting a replay.
    #[arg(long)]
    pub skip_duplicate_check: bool,
}

#[derive(clap::Args, Debug)]
pub struct BlockArgs {
    /// A block height, or a 64-character block hash.
    pub selector: String,

    /// Include every transaction in full.
    #[arg(long)]
    pub transactions: bool,
}

#[derive(clap::Args, Debug)]
pub struct TxArgs {
    /// A 64-character transaction id.
    pub id: String,
}

#[derive(clap::Args, Debug)]
pub struct AppendArgs {
    /// File holding a 64-character hex ed25519 seed, as written by `keygen`.
    #[arg(long, required_unless_present = "tx_file")]
    pub signing_key: Option<PathBuf>,

    /// Application domain label for the transaction.
    #[arg(long, required_unless_present = "tx_file")]
    pub namespace: Option<String>,

    /// Payload schema version. Opaque to Prunella.
    #[arg(long, default_value_t = 1)]
    pub schema_version: u32,

    /// Signer-scoped ordinal.
    #[arg(long, default_value_t = 0)]
    pub nonce: u64,

    /// Read the payload from a file.
    #[arg(long, group = "payload")]
    pub payload_file: Option<PathBuf>,

    /// Payload as lowercase hex.
    #[arg(long, group = "payload")]
    pub payload_hex: Option<String>,

    /// Payload as base64.
    #[arg(long, group = "payload")]
    pub payload_base64: Option<String>,

    /// Append pre-signed transactions, in canonical encoding, instead of signing here.
    ///
    /// Repeatable. Lets a key stay on a machine that never touches the chain.
    #[arg(long)]
    pub tx_file: Vec<PathBuf>,

    /// Block timestamp in milliseconds. Defaults to the system clock.
    #[arg(long)]
    pub timestamp: Option<u64>,
}

#[derive(clap::Args, Debug)]
pub struct ExportArgs {
    /// Where to write the document. Use `-` for standard output.
    #[arg(long, short)]
    pub out: PathBuf,

    /// First height. Defaults to genesis.
    #[arg(long)]
    pub from: Option<u64>,

    /// Last height. Defaults to the head.
    #[arg(long)]
    pub to: Option<u64>,

    /// Keep only transactions in this namespace.
    ///
    /// Produces a projection: a partial view, not a chain backup. It cannot be
    /// imported, and it is marked as such in the document.
    #[arg(long)]
    pub namespace: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct ImportArgs {
    /// The document to apply.
    #[arg(long, short)]
    pub r#in: PathBuf,

    /// Check the document and report what would happen, without writing.
    #[arg(long)]
    pub dry_run: bool,

    /// Create the chain from the document, which must be a full export.
    #[arg(long)]
    pub create: bool,
}

#[derive(clap::Args, Debug)]
pub struct KeygenArgs {
    /// Where to write the key. Defaults to standard output.
    #[arg(long, short)]
    pub out: Option<PathBuf>,
}
