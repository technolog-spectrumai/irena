//! The `governance` command line interface.
//!
//! Puts voting rules and electorate rolls on a Prunella ledger, resolves what is in
//! force at a height, and evaluates ballots against ledger truth. Every command is a
//! thin call into `governance-bridge`; the only thing this binary adds is a clock for
//! block timestamps, which the library deliberately does not have.
//!
//! Exit codes: `0` success (for `evaluate`, the motion was accepted); `1` the motion
//! was rejected; `2` invalid input or a refused operation.

use bornite_eval::OutcomeV1;
use clap::{Parser, Subcommand};
use governance_bridge::{
    NotarisationV1, RecordKindV1, SubjectV1, evaluate_at, genesis_with_rules, history,
    publish_roll, publish_rules, read_ballots_document, roll_in_force, rules_in_force,
};
use prunella_core::{BlockHeight, Hash, NetworkId, TxId};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
const EXIT_REJECTED: u8 = 1;
const EXIT_ERROR: u8 = 2;

/// Voting rules and rolls on a ledger.
#[derive(Parser, Debug)]
#[command(name = "governance", version, about, long_about = None)]
struct Cli {
    /// Path to the chain file.
    #[arg(
        long,
        global = true,
        env = "PRUNELLA_CHAIN",
        default_value = "prunella.chain"
    )]
    chain: PathBuf,

    /// Emit JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

/// Who attests to a record. Optional on every publishing command.
#[derive(clap::Args, Debug, Clone)]
struct NotaryArgs {
    /// An opaque notary identifier.
    #[arg(long)]
    notary: Option<String>,
    /// Free-text statement.
    #[arg(long, requires = "notary")]
    statement: Option<String>,
    /// 64-character hex digest of an external document the notary attests to.
    #[arg(long, requires = "notary")]
    source_digest: Option<String>,
}

impl NotaryArgs {
    fn build(&self) -> Result<Option<NotarisationV1>, String> {
        let Some(notary) = &self.notary else {
            return Ok(None);
        };
        let source_digest = self
            .source_digest
            .as_deref()
            .map(Hash::from_hex)
            .transpose()
            .map_err(|error| format!("--source-digest: {error}"))?;
        Ok(Some(NotarisationV1 {
            notary: notary.clone(),
            statement: self.statement.clone(),
            source_digest,
        }))
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Create a chain whose genesis block carries a rules record.
    Init {
        /// Chain identifier.
        #[arg(long)]
        network: String,
        /// What the rules govern.
        #[arg(long)]
        subject: String,
        /// The voting-rules document.
        #[arg(long)]
        rules: PathBuf,
        /// Hex seed file, as written by `prunella keygen`.
        #[arg(long)]
        signing_key: PathBuf,
        /// Genesis timestamp in milliseconds. Defaults to 0 for reproducibility.
        #[arg(long, default_value_t = 0)]
        genesis_timestamp: u64,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Publish a rules record, amending the one in force if any.
    PublishRules {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        rules: PathBuf,
        #[arg(long)]
        signing_key: PathBuf,
        /// Transaction id of the rules record currently in force. Required when one is.
        #[arg(long)]
        supersedes: Option<String>,
        /// Block timestamp in milliseconds. Defaults to the system clock.
        #[arg(long)]
        timestamp: Option<u64>,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Publish a roll record, amending the one in force if any.
    PublishRoll {
        #[arg(long)]
        subject: String,
        /// The electorate document.
        #[arg(long)]
        roll: PathBuf,
        #[arg(long)]
        signing_key: PathBuf,
        #[arg(long)]
        supersedes: Option<String>,
        #[arg(long)]
        timestamp: Option<u64>,
        #[command(flatten)]
        notary: NotaryArgs,
    },
    /// Show the rules in force for a subject.
    ShowRules {
        #[arg(long)]
        subject: String,
        /// Resolve at this height. Defaults to the head.
        #[arg(long)]
        at: Option<u64>,
    },
    /// Show the roll in force for a subject.
    ShowRoll {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        at: Option<u64>,
    },
    /// List every version of a subject's rules or roll, in ledger order.
    History {
        #[arg(long)]
        subject: String,
        /// `voting-rules` or `roll`.
        #[arg(long)]
        kind: String,
        #[arg(long)]
        at: Option<u64>,
    },
    /// Evaluate ballots against the rules and roll in force.
    Evaluate {
        #[arg(long)]
        subject: String,
        /// A `<ballots>` document.
        #[arg(long)]
        ballots: PathBuf,
        #[arg(long)]
        at: Option<u64>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run(cli: &Cli) -> Result<u8, String> {
    match &cli.command {
        Command::Init {
            network,
            subject,
            rules,
            signing_key,
            genesis_timestamp,
            notary,
        } => {
            let spec = genesis_with_rules(
                NetworkId::new(network.clone()).map_err(|e| e.to_string())?,
                &read_key(signing_key)?,
                &subject_of(subject)?,
                &read(rules)?,
                notary.build()?.as_ref(),
                *genesis_timestamp,
            )
            .map_err(|e| e.to_string())?;
            let store =
                LocalChainStore::init_genesis(&cli.chain, spec).map_err(|e| e.to_string())?;
            let in_force = rules_in_force(&store, &subject_of(subject)?, BlockHeight::GENESIS)
                .map_err(|e| e.to_string())?;
            emit(
                cli.json,
                &format!(
                    "created {}\ngenesis: {}\nrules record: {}",
                    cli.chain.display(),
                    store.genesis_hash(),
                    in_force.tx_id
                ),
                &serde_json::json!({ "path": cli.chain.display().to_string(), "genesis_hash": store.genesis_hash(), "rules_tx_id": in_force.tx_id }),
            );
            Ok(EXIT_OK)
        }
        Command::PublishRules {
            subject,
            rules,
            signing_key,
            supersedes,
            timestamp,
            notary,
        } => {
            let store = open(&cli.chain)?;
            let published = publish_rules(
                &store,
                &read_key(signing_key)?,
                &subject_of(subject)?,
                &read(rules)?,
                parse_supersedes(supersedes.as_deref())?,
                notary.build()?.as_ref(),
                timestamp_for(&store, *timestamp)?,
            )
            .map_err(|e| e.to_string())?;
            emit(
                cli.json,
                &format!(
                    "published rules for {subject} at height {} as {}",
                    published.height, published.tx_id
                ),
                &serde_json::to_value(&published).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::PublishRoll {
            subject,
            roll,
            signing_key,
            supersedes,
            timestamp,
            notary,
        } => {
            let store = open(&cli.chain)?;
            let published = publish_roll(
                &store,
                &read_key(signing_key)?,
                &subject_of(subject)?,
                &read(roll)?,
                parse_supersedes(supersedes.as_deref())?,
                notary.build()?.as_ref(),
                timestamp_for(&store, *timestamp)?,
            )
            .map_err(|e| e.to_string())?;
            emit(
                cli.json,
                &format!(
                    "published roll for {subject} at height {} as {}",
                    published.height, published.tx_id
                ),
                &serde_json::to_value(&published).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::ShowRules { subject, at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let found =
                rules_in_force(&store, &subject_of(subject)?, at).map_err(|e| e.to_string())?;
            emit(
                cli.json,
                &format!(
                    "rules for {subject} at height {at}\nrecord:     {} (height {})\nsupersedes: {}\nnotary:     {}\n{:#?}",
                    found.tx_id,
                    found.height,
                    found
                        .supersedes
                        .map_or_else(|| "none".to_owned(), |t| t.to_string()),
                    found
                        .notarisation
                        .as_ref()
                        .map_or("none", |n| n.notary.as_str()),
                    found.value
                ),
                &serde_json::to_value(&found).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::ShowRoll { subject, at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let found =
                roll_in_force(&store, &subject_of(subject)?, at).map_err(|e| e.to_string())?;
            let voters: Vec<String> = found
                .value
                .voters()
                .iter()
                .map(|v| {
                    format!(
                        "  {} weight {}{}",
                        v.id,
                        v.weight,
                        if v.excluded { " (excluded)" } else { "" }
                    )
                })
                .collect();
            emit(
                cli.json,
                &format!(
                    "roll for {subject} at height {at}\nrecord: {} (height {})\n{}",
                    found.tx_id,
                    found.height,
                    voters.join("\n")
                ),
                &serde_json::to_value(&found).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::History { subject, kind, at } => {
            let store = open(&cli.chain)?;
            let kind = RecordKindV1::parse(kind)
                .ok_or_else(|| format!("--kind must be voting-rules or roll, not {kind:?}"))?;
            let at = height_or_head(&store, *at)?;
            let versions =
                history(&store, &subject_of(subject)?, kind, at).map_err(|e| e.to_string())?;
            let lines: Vec<String> = versions
                .iter()
                .map(|r| {
                    format!(
                        "height {:<6} {}  supersedes {}  notary {}",
                        r.height,
                        r.tx_id,
                        r.record
                            .supersedes
                            .map_or_else(|| "none".to_owned(), |t| t.to_string()),
                        r.record
                            .notarisation
                            .as_ref()
                            .map_or("none", |n| n.notary.as_str())
                    )
                })
                .collect();
            emit(
                cli.json,
                &format!(
                    "{} {kind} version(s) for {subject} up to height {at}\n{}",
                    versions.len(),
                    lines.join("\n")
                ),
                &serde_json::to_value(&versions).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::Evaluate {
            subject,
            ballots,
            at,
        } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let cast = read_ballots_document(&read(ballots)?).map_err(|e| e.to_string())?;
            let result =
                evaluate_at(&store, &subject_of(subject)?, at, &cast).map_err(|e| e.to_string())?;
            let e = &result.evaluation;
            emit(
                cli.json,
                &format!(
                    "evaluated {subject} at height {at}\nrules: {} (height {})\nroll:  {} (height {})\noutcome: {:?}\nreason:  {}\ntally:   yes {} no {} abstain {}",
                    result.rules.tx_id,
                    result.rules.height,
                    result.roll.tx_id,
                    result.roll.height,
                    e.outcome,
                    e.reason,
                    e.tally.yes_weight,
                    e.tally.no_weight,
                    e.tally.abstain_weight
                ),
                &serde_json::to_value(&result).map_err(|e| e.to_string())?,
            );
            Ok(match e.outcome {
                OutcomeV1::Accepted => EXIT_OK,
                OutcomeV1::Rejected => EXIT_REJECTED,
            })
        }
    }
}

fn emit(json: bool, text: &str, value: &serde_json::Value) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    } else {
        println!("{text}");
    }
}

fn open(path: &Path) -> Result<LocalChainStore, String> {
    LocalChainStore::open(path).map_err(|e| e.to_string())
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))
}

fn read_key(path: &Path) -> Result<SigningKey, String> {
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

fn subject_of(text: &str) -> Result<SubjectV1, String> {
    SubjectV1::new(text).map_err(|e| e.to_string())
}

fn parse_supersedes(text: Option<&str>) -> Result<Option<TxId>, String> {
    text.map(TxId::from_hex)
        .transpose()
        .map_err(|e| format!("--supersedes: {e}"))
}

fn height_or_head(store: &LocalChainStore, at: Option<u64>) -> Result<BlockHeight, String> {
    match at {
        Some(height) => Ok(BlockHeight(height)),
        None => store
            .head()
            .map(|head| head.height)
            .map_err(|e| e.to_string()),
    }
}

/// A block timestamp: the caller's, or the clock, never earlier than the parent's.
fn timestamp_for(store: &LocalChainStore, chosen: Option<u64>) -> Result<u64, String> {
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
