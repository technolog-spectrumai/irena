//! The `prunella` command line interface.
//!
//! Every command calls the Prunella libraries and renders what they return. There is
//! no hashing, validation, canonical encoding or storage logic in this crate: a second
//! implementation of any of those would be a second answer to what a chain says, and a
//! ledger can only have one.
//!
//! Exit codes: `0` success, `1` the chain is invalid or the lookup found nothing,
//! `2` the command could not be carried out.

mod args;
mod commands;
mod keyfile;
mod output;

use args::{Cli, Command};
use clap::Parser as _;
use output::{EXIT_ERROR, Format, exit};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format = Format::from_flag(cli.json);
    let chain = cli.chain.as_path();

    let outcome = match &cli.command {
        Command::Init(args) => commands::chain::init(chain, args, format),
        Command::Status => commands::chain::status(chain, format),
        Command::Verify(args) => commands::chain::verify(chain, args, format),
        Command::Block(args) => commands::inspect::block(chain, args, format),
        Command::Tx(args) => commands::inspect::tx(chain, args, format),
        Command::Append(args) => commands::write::append(chain, args, format),
        Command::Export(args) => commands::transfer::export_chain(chain, args, format),
        Command::Import(args) => commands::transfer::import_chain(chain, args, format),
        Command::Keygen(args) => commands::write::keygen(args, format),
    };

    match outcome {
        Ok(code) => exit(code),
        Err(message) => {
            eprintln!("error: {message}");
            exit(EXIT_ERROR)
        }
    }
}
