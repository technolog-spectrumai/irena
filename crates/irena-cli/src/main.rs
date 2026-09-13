//! The `irena` command line interface.
//!
//! Founds a company on a Prunella ledger, amends its identity, share register and
//! voting rules, and shows what the company is at any height. One company per chain,
//! so no command but `init` names it. Every command is a thin call into
//! `irena-ledger`; the only things this binary adds are file reading, a clock for
//! block timestamps, and output formatting.
//!
//! The `vote` and `meeting` subcommands carry a vote or a meeting through its
//! lifecycle as a state file, so each step is one invocation; see `vote.rs` and
//! `meeting.rs`.
//!
//! Exit codes: `0` success; `1` a finding (a broken amendment chain from
//! `verify-structure`, a rejected motion from `vote evaluate`, a record that fails
//! `vote verify`); `2` invalid input or a refused operation.

mod company;
mod meeting;
mod vote;

use clap::{Parser, Subcommand};
use irena_core::{CompanyIdV1, NotarisationV1, NotaryIdV1, NotaryTimeV1};
use prunella_core::{BlockHeight, Hash, TxId};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub(crate) const EXIT_OK: u8 = 0;
pub(crate) const EXIT_FINDING: u8 = 1;
pub(crate) const EXIT_ERROR: u8 = 2;

/// A company and its votes on a ledger.
#[derive(Parser, Debug)]
#[command(name = "irena", version, about, long_about = None)]
pub(crate) struct Cli {
    /// Path to the chain file.
    #[arg(
        long,
        global = true,
        env = "IRENA_CHAIN",
        default_value = "irena.chain"
    )]
    pub(crate) chain: PathBuf,

    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    #[command(subcommand)]
    pub(crate) command: Command,
}

/// Who attests to a record, and when. Required on every publishing command: every
/// record is entered by hand by someone with the authority to say the company is now
/// like this, and the record says who that was.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct NotaryArgs {
    /// The notary's stable identifier (letters, digits, `. _ : + @ -`).
    #[arg(long)]
    notary_id: String,
    /// The notary's name.
    #[arg(long)]
    notary_name: String,
    /// The notary's address.
    #[arg(long)]
    notary_address: Option<String>,
    /// When the notary says the change took effect: `YYYY-MM-DDTHH:MM:SSZ`, UTC.
    #[arg(long)]
    notary_at: String,
    /// Free-text statement.
    #[arg(long)]
    statement: Option<String>,
    /// 64-character hex digest of an external document the notary attests to.
    #[arg(long)]
    source_digest: Option<String>,
}

impl NotaryArgs {
    pub(crate) fn build(&self) -> Result<NotarisationV1, String> {
        let source_digest = self
            .source_digest
            .as_deref()
            .map(Hash::from_hex)
            .transpose()
            .map_err(|error| format!("--source-digest: {error}"))?;
        Ok(NotarisationV1 {
            id: NotaryIdV1::new(&self.notary_id)
                .map_err(|error| format!("--notary-id: {error}"))?,
            name: self.notary_name.clone(),
            address: self.notary_address.clone(),
            at: NotaryTimeV1::parse(&self.notary_at)
                .map_err(|error| format!("--notary-at: {error}"))?,
            statement: self.statement.clone(),
            source_digest,
        })
    }
}

/// Arguments every publishing command shares.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct PublishArgs {
    /// The body document to publish.
    #[arg(long)]
    pub(crate) file: PathBuf,
    /// Hex seed file, as written by `prunella keygen`.
    #[arg(long)]
    pub(crate) signing_key: PathBuf,
    /// Transaction id of the record currently providing this part: the genesis, or
    /// the last amendment of the part. See `show`.
    #[arg(long)]
    pub(crate) supersedes: String,
    /// Block timestamp in milliseconds. Defaults to the system clock.
    #[arg(long)]
    pub(crate) timestamp: Option<u64>,
    #[command(flatten)]
    pub(crate) notary: NotaryArgs,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Create a chain whose genesis block founds the company: identity, register and
    /// rules, in one company-genesis document.
    Init {
        /// Chain identifier.
        #[arg(long)]
        network: String,
        /// The company's label on the ledger.
        #[arg(long)]
        company: String,
        /// The company-genesis document.
        #[arg(long)]
        genesis: PathBuf,
        /// Hex seed file, as written by `prunella keygen`.
        #[arg(long)]
        signing_key: PathBuf,
        /// Genesis timestamp in milliseconds. Defaults to 0 for reproducibility.
        #[arg(long, default_value_t = 0)]
        genesis_timestamp: u64,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Publish an identity record, amending who the company is.
    PublishIdentity(PublishArgs),
    /// Publish a share-structure record, amending the register.
    PublishShares(PublishArgs),
    /// Publish a voting-rules record, amending the rules.
    PublishRules(PublishArgs),
    /// Show what the company is at a height: identity, register and rules together.
    Show {
        /// Reconstruct at this height. Defaults to the head.
        #[arg(long)]
        at: Option<u64>,
    },
    /// Show the share register, with each holder's voting weight.
    Shares {
        #[arg(long)]
        at: Option<u64>,
    },
    /// List every record that has provided one part, in ledger order.
    History {
        /// `identity`, `share-structure` or `voting-rules`.
        #[arg(long)]
        kind: String,
        #[arg(long)]
        at: Option<u64>,
    },
    /// Reconstruct the company record by record and report any break.
    VerifyStructure {
        #[arg(long)]
        at: Option<u64>,
    },
    /// A vote, carried through its lifecycle as a state file.
    #[command(subcommand)]
    Vote(vote::VoteCommand),
    /// A shareholder meeting: an agenda, its votes, and the record of both.
    #[command(subcommand)]
    Meeting(meeting::MeetingCommand),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match &cli.command {
        Command::Vote(command) => vote::run(&cli, command),
        Command::Meeting(command) => meeting::run(&cli, command),
        _ => company::run(&cli),
    };
    match outcome {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

// ---------------------------------------------------------------------------------
// Shared helpers.
// ---------------------------------------------------------------------------------

/// Prints the text or JSON form of a result.
///
/// A closed pipe (`irena … | head -1`) is not an error worth a panic: the write
/// result is ignored rather than unwrapped.
pub(crate) fn emit(json: bool, text: &str, value: &serde_json::Value) {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    let _ = if json {
        writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        )
    } else {
        writeln!(stdout, "{text}")
    };
}

pub(crate) fn open(path: &Path) -> Result<LocalChainStore, String> {
    LocalChainStore::open(path).map_err(|e| e.to_string())
}

pub(crate) fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))
}

pub(crate) fn read_key(path: &Path) -> Result<SigningKey, String> {
    let text = read(path)?;
    let mut seed = [0u8; 32];
    hex::decode_to_slice(text.trim(), &mut seed).map_err(|e| {
        format!(
            "{} does not hold a 64-character hex seed: {e}",
            path.display()
        )
    })?;
    Ok(SigningKey::from_seed(seed))
}

pub(crate) fn company_of(text: &str) -> Result<CompanyIdV1, String> {
    CompanyIdV1::new(text).map_err(|e| format!("--company: {e}"))
}

pub(crate) fn parse_supersedes(text: &str) -> Result<TxId, String> {
    TxId::from_hex(text).map_err(|e| format!("--supersedes: {e}"))
}

pub(crate) fn height_or_head(
    store: &LocalChainStore,
    at: Option<u64>,
) -> Result<BlockHeight, String> {
    match at {
        Some(height) => Ok(BlockHeight(height)),
        None => store
            .head()
            .map(|head| head.height)
            .map_err(|e| e.to_string()),
    }
}

/// A block timestamp: the caller's, or the clock, never earlier than the parent's.
pub(crate) fn timestamp_for(store: &LocalChainStore, chosen: Option<u64>) -> Result<u64, String> {
    let head = store.head().map_err(|e| e.to_string())?;
    let parent = store
        .get_block(head.height)
        .map_err(|e| e.to_string())?
        .ok_or("no head block")?;
    let floor = parent.header.timestamp_millis;
    match chosen {
        Some(t) if t < floor => Err(format!(
            "--timestamp {t} precedes the parent block's timestamp {floor}"
        )),
        Some(t) => Ok(t),
        None => Ok(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(floor, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .max(floor)),
    }
}

pub(crate) fn render_tx(id: Option<TxId>) -> String {
    id.map_or_else(|| "none".to_owned(), |t| t.to_string())
}
