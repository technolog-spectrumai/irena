//! The company commands: founding, amending, showing, history, verification.

use crate::{
    Cli, Command, EXIT_FINDING, EXIT_OK, PublishArgs, company_of, emit, height_or_head, open,
    parse_supersedes, read, read_key, render_tx, timestamp_for,
};
use irena_core::{ChannelModeV1, NotarisationV1, RecordKindV1};
use irena_decision::{actors_of_register, resolve_channel};
use irena_ledger::{
    InForceV1, LedgerError, RecordRefV1, company_now, genesis_with_company, history, publish,
    reconstruct,
};
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
        Command::PublishChannels(args) => publish_kind(cli, args, RecordKindV1::DecisionChannels),
        Command::PublishIdentities(args) => publish_kind(cli, args, RecordKindV1::Identities),
        Command::PublishAuthorisation(args) => publish_kind(cli, args, RecordKindV1::Authorisation),
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
                 {}\n\
                 {}\n\
                 {} record(s) applied",
                state.company,
                state.genesis_height,
                state.genesis_tx_id,
                identity.name,
                identity.jurisdiction.as_deref().unwrap_or("-"),
                identity.registered_number.as_deref().unwrap_or("-"),
                describe_record("identity     ", &state.identity),
                describe_record("shares       ", &state.shares),
                describe_record("channels     ", &state.channels),
                describe_record("identities   ", &state.identities),
                describe_record("authorisation", &state.authorisation),
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
            let actors =
                actors_of_register(register, &state.identities.value).map_err(|e| e.to_string())?;
            let mut lines = vec![
                format!("share register of {} at height {at}", state.company),
                describe_record("record", found),
                format!(
                    "{} holder(s), {} share(s) in issue; one share, one vote; total weight {}; {} can sign",
                    register.len(),
                    register.total_shares(),
                    actors.total_weight,
                    actors.signing_actors
                ),
            ];
            for (actor, holder) in actors.actors.iter().zip(register.holders()) {
                lines.push(format!(
                    "  {:<24} shares {:>12}  weight {:>12}  {}",
                    actor.id.as_str(),
                    holder.shares,
                    actor.weight.value(),
                    if actor.can_sign {
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
                    "total_weight": actors.total_weight,
                    "signing_holders": actors.signing_actors,
                    "holders": actors.actors.iter().zip(register.holders()).map(|(a, h)| json!({
                        "id": a.id,
                        "name": h.name,
                        "shares": h.shares,
                        "weight": a.weight,
                        "key": a.key,
                        "can_sign": a.can_sign,
                    })).collect::<Vec<_>>(),
                }),
            );
            Ok(EXIT_OK)
        }
        Command::Channels { at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let state = reconstruct(&store, at).map_err(|e| e.to_string())?;
            let found = &state.channels;
            let mut lines = vec![
                format!("decision channels of {} at height {at}", state.company),
                describe_record("record", found),
                format!("{} channel(s)", found.value.len()),
            ];
            let mut channels = Vec::new();
            for channel in found.value.channels() {
                let mode = match &channel.mode {
                    ChannelModeV1::Individual => "individual",
                    ChannelModeV1::Collective { .. } => "collective",
                };
                let source = channel.actors.as_str();
                match resolve_channel(&state, &channel.id) {
                    Ok(resolved) => {
                        lines.push(format!(
                            "  {:<20} {mode:<10} {source:<14} {} actor(s), total weight {}, {} can sign{}",
                            channel.id.as_str(),
                            resolved.actors.len(),
                            resolved.actors.total_weight,
                            resolved.actors.signing_actors,
                            if channel.mode.is_individual() {
                                "  — decides alone; may amend the channel set only downwards (self-demotion rule)"
                            } else {
                                ""
                            }
                        ));
                        for actor in &resolved.actors.actors {
                            lines.push(format!(
                                "      {:<24} weight {:>12}  {}",
                                actor.id.as_str(),
                                actor.weight.value(),
                                if actor.can_sign {
                                    "can sign"
                                } else {
                                    "no key: cannot sign"
                                }
                            ));
                        }
                        if let Some(rules) = resolved.rules() {
                            lines.push(format!(
                                "      rules: {}",
                                serde_json::to_string(rules).unwrap_or_default()
                            ));
                        }
                        channels.push(json!({
                            "id": channel.id,
                            "mode": mode,
                            "source": source,
                            "resolves": true,
                            "actors": resolved.actors.actors,
                            "total_weight": resolved.actors.total_weight,
                            "signing_actors": resolved.actors.signing_actors,
                            "rules": resolved.rules(),
                        }));
                    }
                    Err(error) => {
                        lines.push(format!(
                            "  {:<20} {mode:<10} {source:<14} DOES NOT RESOLVE: {error}",
                            channel.id.as_str()
                        ));
                        channels.push(json!({
                            "id": channel.id,
                            "mode": mode,
                            "source": source,
                            "resolves": false,
                            "error": error.to_string(),
                        }));
                    }
                }
            }
            emit(
                cli.json,
                &lines.join("\n"),
                &json!({
                    "company": state.company,
                    "at": at,
                    "record": record_json(found),
                    "channels": channels,
                }),
            );
            Ok(EXIT_OK)
        }
        Command::Identities { at } => {
            let store = open(&cli.chain)?;
            let at = height_or_head(&store, *at)?;
            let state = reconstruct(&store, at).map_err(|e| e.to_string())?;
            let identities = &state.identities.value;
            let authorisation = &state.authorisation.value;
            let mut lines = vec![
                format!("identities of {} at height {at}", state.company),
                describe_record("identities   ", &state.identities),
                describe_record("authorisation", &state.authorisation),
                format!(
                    "{} person(s); {} hold a key; a person's key is their voice in every channel they sit on",
                    identities.len(),
                    identities
                        .persons()
                        .iter()
                        .filter(|p| p.key.is_some())
                        .count()
                ),
            ];
            for person in identities.persons() {
                let families: Vec<String> = authorisation
                    .families_of(&person.id)
                    .map(|family| family.to_string())
                    .collect();
                lines.push(format!(
                    "  {:<24} {:<30} {:<20} may sign: {}",
                    person.id.as_str(),
                    person.name.as_deref().unwrap_or("-"),
                    person.key.map_or_else(
                        || "no key: cannot sign".to_owned(),
                        |key| format!("{key:.16}")
                    ),
                    if families.is_empty() {
                        "nothing".to_owned()
                    } else {
                        families.join(", ")
                    }
                ));
            }
            let unknown: Vec<String> = authorisation
                .signers()
                .iter()
                .filter(|signer| identities.get(&signer.person).is_none())
                .map(|signer| format!("{} ({})", signer.person, signer.family))
                .collect();
            if !unknown.is_empty() {
                lines.push(format!(
                    "  authorised but not in the identities, so unable to sign: {}",
                    unknown.join(", ")
                ));
            }
            emit(
                cli.json,
                &lines.join("\n"),
                &json!({
                    "company": state.company,
                    "at": at,
                    "identities_record": record_json(&state.identities),
                    "authorisation_record": record_json(&state.authorisation),
                    "persons": identities.persons().iter().map(|person| json!({
                        "id": person.id,
                        "name": person.name,
                        "document_id": person.document_id,
                        "key": person.key,
                        "can_sign": person.key.is_some(),
                        "may_sign": authorisation.families_of(&person.id).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                    "signers": authorisation.signers(),
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
                        "--kind must be identity, share-structure, decision-channels, identities or authorisation, not {kind:?}"
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
        Command::Vote(_) | Command::Decision(_) | Command::Meeting(_) | Command::Resolution(_) => {
            unreachable!("dispatched in main")
        }
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
