//! The company commands: founding, amending, showing, history, verification.

use crate::{
    Cli, Command, EXIT_FINDING, EXIT_OK, PublishArgs, company_of, emit, height_or_head, open,
    parse_supersedes, read, read_key, render_tx, timestamp_for,
};
use irena_core::{NotarisationV1, RecordKindV1};
use irena_ledger::{
    InForceV1, LedgerError, RecordRefV1, company_now, genesis_with_company, history, publish,
    reconstruct,
};
use irena_vote::derive_electorate;
use prunella_core::NetworkId;
use prunella_store::LocalChainStore;
use serde_json::json;

pub(crate) fn run(cli: &Cli) -> Result<u8, String> {
    match &cli.command {
        Command::Init {
            network,
            company,
            genesis,
            signing_key,
            genesis_timestamp,
            notary,
        } => {
            let company = company_of(company)?;
            let spec = genesis_with_company(
                NetworkId::new(network.clone()).map_err(|e| e.to_string())?,
                &read_key(signing_key)?,
                &company,
                &read(genesis)?,
                &notary.build()?,
                *genesis_timestamp,
            )
            .map_err(|e| e.to_string())?;
            let store =
                LocalChainStore::init_genesis(&cli.chain, spec).map_err(|e| e.to_string())?;
            let state = company_now(&store).map_err(|e| e.to_string())?;
            emit(
                cli.json,
                &format!(
                    "created {}\ngenesis:        {}\ncompany:        {company}\nname:           {}\nholders:        {} ({} shares)\ngenesis record: {}",
                    cli.chain.display(),
                    store.genesis_hash(),
                    state.identity.value.name,
                    state.shares.value.len(),
                    state.shares.value.total_shares(),
                    state.genesis_tx_id
                ),
                &json!({
                    "path": cli.chain.display().to_string(),
                    "genesis_hash": store.genesis_hash(),
                    "company": company,
                    "genesis_tx_id": state.genesis_tx_id,
                }),
            );
            Ok(EXIT_OK)
        }
        Command::PublishIdentity(args) => publish_kind(cli, args, RecordKindV1::Identity),
        Command::PublishShares(args) => publish_kind(cli, args, RecordKindV1::ShareStructure),
        Command::PublishRules(args) => publish_kind(cli, args, RecordKindV1::VotingRules),
        Command::Show { at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let state = reconstruct(&store, at).map_err(|e| e.to_string())?;
            let identity = &state.identity.value;
            let text = format!(
                "company {} at height {at} (founded at height {} by {})\n\
                 name:            {}\n\
                 jurisdiction:    {}\n\
                 registered no.:  {}\n\
                 {}\n\
                 {}\n\
                 {}\n\
                 {} record(s) applied",
                state.company,
                state.genesis_height,
                state.genesis_tx_id,
                identity.name,
                identity.jurisdiction.as_deref().unwrap_or("-"),
                identity.registered_number.as_deref().unwrap_or("-"),
                describe_record("identity", &state.identity),
                describe_record("shares  ", &state.shares),
                describe_record("rules   ", &state.rules),
                state.applied.len(),
            );
            emit(
                cli.json,
                &text,
                &serde_json::to_value(&state).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::Shares { at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let state = reconstruct(&store, at).map_err(|e| e.to_string())?;
            let found = &state.shares;
            let register = &found.value;
            let derived = derive_electorate(register).map_err(|e| e.to_string())?;
            let mut lines = vec![
                format!("share register of {} at height {at}", state.company),
                describe_record("record", found),
                format!(
                    "{} holder(s), {} share(s) in issue; one share, one vote; total weight {}; {} can sign",
                    register.len(),
                    register.total_shares(),
                    derived.total_weight,
                    derived.signing_holders
                ),
            ];
            for holder in &derived.holders {
                lines.push(format!(
                    "  {:<24} shares {:>12}  weight {:>12}  {}",
                    holder.id.as_str(),
                    holder.shares,
                    holder.weight.value(),
                    if holder.can_sign {
                        "can sign"
                    } else {
                        "no key: cannot sign"
                    }
                ));
            }
            emit(
                cli.json,
                &lines.join("\n"),
                &json!({
                    "company": state.company,
                    "at": at,
                    "record": record_json(found),
                    "total_shares": register.total_shares(),
                    "total_weight": derived.total_weight,
                    "signing_holders": derived.signing_holders,
                    "holders": derived.holders.iter().zip(register.holders()).map(|(d, h)| json!({
                        "id": d.id,
                        "name": h.name,
                        "shares": d.shares,
                        "weight": d.weight,
                        "key": d.key,
                        "can_sign": d.can_sign,
                    })).collect::<Vec<_>>(),
                }),
            );
            Ok(EXIT_OK)
        }
        Command::History { kind, at } => {
            let store = open(&cli.chain)?;
            let kind = RecordKindV1::parse(kind)
                .filter(|kind| RecordKindV1::AMENDMENTS.contains(kind))
                .ok_or_else(|| {
                    format!(
                        "--kind must be identity, share-structure or voting-rules, not {kind:?}"
                    )
                })?;
            let at = height_or_head(&store, *at)?;
            let versions = history(&store, kind, at).map_err(|e| e.to_string())?;
            let lines: Vec<String> = versions.iter().map(describe_version).collect();
            emit(
                cli.json,
                &format!(
                    "{} record(s) have provided {kind} up to height {at}\n{}",
                    versions.len(),
                    lines.join("\n")
                ),
                &serde_json::to_value(&versions).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::VerifyStructure { at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            match reconstruct(&store, at) {
                Ok(state) => {
                    let mut lines = vec![format!(
                        "company {} reconstructs at height {at}: {} record(s) applied, every link holds",
                        state.company,
                        state.applied.len()
                    )];
                    for kind in RecordKindV1::AMENDMENTS {
                        lines.push(format!(
                            "  {kind:<16} {} version(s), provided by {}",
                            state.history_of(kind).len(),
                            state.provider_of(kind)
                        ));
                    }
                    lines.push("nothing was repaired because nothing needed it".to_owned());
                    emit(
                        cli.json,
                        &lines.join("\n"),
                        &json!({
                            "company": state.company,
                            "at": at,
                            "intact": true,
                            "applied": state.applied.len(),
                            "providers": RecordKindV1::AMENDMENTS.iter().map(|kind| json!({
                                "kind": kind,
                                "versions": state.history_of(*kind).len(),
                                "provided_by": state.provider_of(*kind),
                            })).collect::<Vec<_>>(),
                        }),
                    );
                    Ok(EXIT_OK)
                }
                Err(
                    error @ (LedgerError::BrokenAmendmentChain { .. }
                    | LedgerError::UnreadableRecord { .. }
                    | LedgerError::SecondGenesis { .. }
                    | LedgerError::ForeignCompany { .. }
                    | LedgerError::NoGenesisFirst { .. }),
                ) => {
                    emit(
                        cli.json,
                        &format!(
                            "the company cannot be reconstructed at height {at}\nBROKEN: {error}\nnothing was repaired"
                        ),
                        &json!({ "at": at, "intact": false, "error": error.to_string() }),
                    );
                    Ok(EXIT_FINDING)
                }
                Err(error) => Err(error.to_string()),
            }
        }
        Command::Vote(_) => unreachable!("dispatched in main"),
    }
}

fn publish_kind(cli: &Cli, args: &PublishArgs, kind: RecordKindV1) -> Result<u8, String> {
    let store = open(&cli.chain)?;
    let published = publish(
        &store,
        &read_key(&args.signing_key)?,
        kind,
        &read(&args.file)?,
        Some(parse_supersedes(&args.supersedes)?),
        &args.notary.build()?,
        timestamp_for(&store, args.timestamp)?,
    )
    .map_err(|e| e.to_string())?;
    emit(
        cli.json,
        &format!(
            "published {kind} for {} at height {} as {}\nsupersedes: {}\n{}",
            published.record.company,
            published.height,
            published.tx_id,
            render_tx(published.record.supersedes),
            describe_notary(&published.record.notarisation)
        ),
        &serde_json::to_value(&published).map_err(|e| e.to_string())?,
    );
    Ok(EXIT_OK)
}

fn describe_notary(notarisation: &NotarisationV1) -> String {
    format!(
        "notary:     {} ({}){} at {}",
        notarisation.name,
        notarisation.id,
        notarisation
            .address
            .as_deref()
            .map_or_else(String::new, |a| format!(", {a}")),
        notarisation.at
    )
}

fn describe_record<T>(label: &str, found: &InForceV1<T>) -> String {
    format!(
        "{label}: {} (height {}, supersedes {})\n  {}",
        found.tx_id,
        found.height,
        render_tx(found.supersedes),
        describe_notary(&found.notarisation)
    )
}

fn describe_version(version: &RecordRefV1) -> String {
    format!(
        "height {:<6} {}  {}  supersedes {}\n  {}",
        version.height,
        version.tx_id,
        version.record.kind(),
        render_tx(version.record.supersedes),
        describe_notary(&version.record.notarisation)
    )
}

fn record_json<T>(found: &InForceV1<T>) -> serde_json::Value {
    json!({
        "tx_id": found.tx_id,
        "height": found.height,
        "supersedes": found.supersedes,
        "notarisation": found.notarisation,
    })
}
