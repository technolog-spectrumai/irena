//! export and import.

use crate::args::{ExportArgs, ImportArgs};
use crate::output::{EXIT_OK, Format};
use prunella_core::{BlockHeight, Namespace};
use prunella_store::LocalChainStore;
use prunella_xml::{
    ExportRequest, export, import, plan_import, read_document, restore, write_document,
};
use serde_json::json;
use std::io::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Writes the chain, or part of it, as XML.
pub fn export_chain(path: &Path, args: &ExportArgs, format: Format) -> Result<u8, String> {
    let store = LocalChainStore::open(path).map_err(|error| error.to_string())?;

    let namespace = args
        .namespace
        .as_ref()
        .map(|text| Namespace::new(text.clone()))
        .transpose()
        .map_err(|error| error.to_string())?;

    let request = ExportRequest {
        from: args.from.map(BlockHeight),
        to: args.to.map(BlockHeight),
        namespace,
        exported_at_millis: now_millis(),
    };

    let document = export(&store, &request).map_err(|error| error.to_string())?;
    let xml = write_document(&document).map_err(|error| error.to_string())?;

    if args.out == Path::new("-") {
        std::io::stdout()
            .write_all(xml.as_bytes())
            .map_err(|error| format!("could not write to standard output: {error}"))?;
        return Ok(EXIT_OK);
    }
    std::fs::write(&args.out, &xml)
        .map_err(|error| format!("could not write {}: {error}", args.out.display()))?;

    if document.projection.is_some() {
        eprintln!(
            "warning: this is a namespace projection, not a chain backup. It omits \
             transactions, so its blocks cannot reproduce their own transaction root and \
             it cannot be imported. Export without --namespace for a backup."
        );
    }

    format.emit(
        &format!(
            "wrote {}\nkind: {}\nheights: {}..={}\nblocks: {}",
            args.out.display(),
            document.kind,
            document.range_start,
            document.range_end,
            document.block_count()
        ),
        &json!({
            "path": args.out.display().to_string(),
            "kind": document.kind.as_str(),
            "range_start": document.range_start,
            "range_end": document.range_end,
            "block_count": document.block_count(),
            "is_backup": document.is_importable(),
        }),
    );
    Ok(EXIT_OK)
}

/// Applies an XML document to a chain.
pub fn import_chain(path: &Path, args: &ImportArgs, format: Format) -> Result<u8, String> {
    let xml = std::fs::read_to_string(&args.r#in)
        .map_err(|error| format!("could not read {}: {error}", args.r#in.display()))?;
    let document = read_document(&xml).map_err(|error| error.to_string())?;

    if args.create {
        if args.dry_run {
            return Err(
                "--dry-run and --create cannot be combined: there is no chain to \
                        check the document against until it is created"
                    .into(),
            );
        }
        let (store, outcome) = restore(path, &document).map_err(|error| error.to_string())?;
        format.emit(
            &format!(
                "created {} from {}\nappended: {}\nalready present: {}\nhead: {}",
                path.display(),
                args.r#in.display(),
                outcome.appended,
                outcome.already_present,
                outcome.head
            ),
            &json!({
                "created": true,
                "path": path.display().to_string(),
                "appended": outcome.appended,
                "already_present": outcome.already_present,
                "head": outcome.head,
                "genesis_hash": store.genesis_hash(),
            }),
        );
        return Ok(EXIT_OK);
    }

    let store = LocalChainStore::open(path).map_err(|error| error.to_string())?;

    if args.dry_run {
        let plan = plan_import(&store, &document).map_err(|error| error.to_string())?;
        format.emit(
            &format!("{plan}\n(dry run: nothing was written)"),
            &json!({
                "dry_run": true,
                "kind": plan.kind.as_str(),
                "range_start": plan.range_start,
                "range_end": plan.range_end,
                "blocks_in_document": plan.blocks_in_document,
                "blocks_already_present": plan.blocks_already_present,
                "blocks_to_append": plan.blocks_to_append,
                "resulting_head": plan.resulting_head,
            }),
        );
        return Ok(EXIT_OK);
    }

    let outcome = import(&store, &document).map_err(|error| error.to_string())?;
    format.emit(
        &format!(
            "imported {}\nappended: {}\nalready present: {}\nhead: {}",
            args.r#in.display(),
            outcome.appended,
            outcome.already_present,
            outcome.head
        ),
        &json!({
            "dry_run": false,
            "appended": outcome.appended,
            "already_present": outcome.already_present,
            "head": outcome.head,
        }),
    );
    Ok(EXIT_OK)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
