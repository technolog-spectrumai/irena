//! The company commands: founding, publishing, showing, history, verification.

use crate::{
    Cli, Command, EXIT_FINDING, EXIT_OK, PublishArgs, company_of, emit, height_or_head, open,
    parse_supersedes, read, read_key, render_tx, timestamp_for,
};
use irena_core::{NotarisationV1, RecordKindV1};
use irena_ledger::{
    InForceV1, LedgerError, RecordRefV1, company_at, genesis_in_force, genesis_with_company,
    history, publish, shares_in_force,
};
use irena_vote::derive_electorate;
use prunella_core::{BlockHeight, NetworkId};
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
            let founded = genesis_in_force(&store, &company, BlockHeight::GENESIS)
                .map_err(|e| e.to_string())?;
            emit(
                cli.json,
                &format!(
                    "created {}\ngenesis:        {}\ncompany:        {company}\nname:           {}\ngenesis record: {}",
                    cli.chain.display(),
                    store.genesis_hash(),
                    founded.value.identity.name,
                    founded.tx_id
                ),
                &json!({
                    "path": cli.chain.display().to_string(),
                    "genesis_hash": store.genesis_hash(),
                    "company": company,
                    "genesis_tx_id": founded.tx_id,
                }),
            );
            Ok(EXIT_OK)
        }
        Command::PublishGenesis(args) => publish_kind(cli, args, RecordKindV1::CompanyGenesis),
        Command::PublishShares(args) => publish_kind(cli, args, RecordKindV1::ShareStructure),
        Command::PublishRules(args) => publish_kind(cli, args, RecordKindV1::VotingRules),
        Command::Show { company, at } => {
            let store = open(&cli.chain)?;
            let company = company_of(company)?;
            let at = height_or_head(&store, *at)?;
            let state = company_at(&store, &company, at).map_err(|e| e.to_string())?;
            let identity = &state.genesis.value.identity;
            let text = format!(
                "company {company} at height {at}\n\
                 name:            {}\n\
                 jurisdiction:    {}\n\
                 registered no.:  {}\n\
                 {}\n\
                 {}\n\
                 {}",
                identity.name,
                identity.jurisdiction.as_deref().unwrap_or("-"),
                identity.registered_number.as_deref().unwrap_or("-"),
                describe_record("genesis", &state.genesis),
                describe_record("shares ", &state.shares),
                describe_record("rules  ", &state.rules),
            );
            emit(
                cli.json,
                &text,
                &serde_json::to_value(&state).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::Shares { company, at } => {
            let store = open(&cli.chain)?;
            let company = company_of(company)?;
            let at = height_or_head(&store, *at)?;
            let found = shares_in_force(&store, &company, at).map_err(|e| e.to_string())?;
            let register = &found.value;
            let derived = derive_electorate(register).map_err(|e| e.to_string())?;
            let mut lines = vec![
                format!("share register for {company} at height {at}"),
                describe_record("record", &found),
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
                    holder.id,
                    holder.shares,
                    holder.weight,
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
                    "company": company,
                    "at": at,
                    "record": record_json(&found),
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
        Command::History { company, kind, at } => {
            let store = open(&cli.chain)?;
            let company = company_of(company)?;
            let kind = RecordKindV1::parse(kind).ok_or_else(|| {
                format!(
                    "--kind must be company-genesis, share-structure or voting-rules, not {kind:?}"
                )
            })?;
            let at = height_or_head(&store, *at)?;
            let versions = history(&store, &company, kind, at).map_err(|e| e.to_string())?;
            let lines: Vec<String> = versions.iter().map(describe_version).collect();
            emit(
                cli.json,
                &format!(
                    "{} {kind} version(s) for {company} up to height {at}\n{}",
                    versions.len(),
                    lines.join("\n")
                ),
                &serde_json::to_value(&versions).map_err(|e| e.to_string())?,
            );
            Ok(EXIT_OK)
        }
        Command::VerifyStructure { company, at } => {
            let store = open(&cli.chain)?;
            let company = company_of(company)?;
            let at = height_or_head(&store, *at)?;
            let mut lines = vec![format!("structure of {company} up to height {at}")];
            let mut findings = Vec::new();
            let mut report = Vec::new();
            for kind in RecordKindV1::ALL {
                match history(&store, &company, kind, at) {
                    Ok(versions) => {
                        lines.push(format!(
                            "  {kind:<16} {} version(s), chain intact{}",
                            versions.len(),
                            versions
                                .last()
                                .map_or_else(String::new, |v| format!(", in force: {}", v.tx_id))
                        ));
                        report.push(json!({
                            "kind": kind,
                            "versions": versions.len(),
                            "in_force": versions.last().map(|v| v.tx_id),
                            "intact": true,
                        }));
                    }
                    Err(
                        error @ (LedgerError::BrokenAmendmentChain { .. }
                        | LedgerError::UnreadableRecord { .. }),
                    ) => {
                        lines.push(format!("  {kind:<16} BROKEN: {error}"));
                        report.push(
                            json!({ "kind": kind, "intact": false, "error": error.to_string() }),
                        );
                        findings.push(error.to_string());
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            let ok = findings.is_empty();
            lines.push(if ok {
                "every amendment chain links; nothing was repaired because nothing needed it"
                    .to_owned()
            } else {
                format!("{} broken chain(s); nothing was repaired", findings.len())
            });
            emit(
                cli.json,
                &lines.join("\n"),
                &json!({ "company": company, "at": at, "intact": ok, "kinds": report }),
            );
            Ok(if ok { EXIT_OK } else { EXIT_FINDING })
        }
        Command::Vote(_) => unreachable!("dispatched in main"),
    }
}

fn publish_kind(cli: &Cli, args: &PublishArgs, kind: RecordKindV1) -> Result<u8, String> {
    let store = open(&cli.chain)?;
    let company = company_of(&args.company)?;
    let published = publish(
        &store,
        &read_key(&args.signing_key)?,
        &company,
        kind,
        &read(&args.file)?,
        parse_supersedes(args.supersedes.as_deref())?,
        &args.notary.build()?,
        timestamp_for(&store, args.timestamp)?,
    )
    .map_err(|e| e.to_string())?;
    emit(
        cli.json,
        &format!(
            "published {kind} for {company} at height {} as {}\nsupersedes: {}\n{}",
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
        "height {:<6} {}  supersedes {}\n  {}",
        version.height,
        version.tx_id,
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
