//! Writes the V1 golden vectors to `test-vectors/v1/`.
//!
//! Run once to create them. After that the files are frozen: a test asserts that
//! regenerating reproduces them byte for byte, so this binary can never quietly change
//! a committed vector. If it ever disagrees with the files, the implementation has
//! changed and that is a protocol break to be reverted, not a file to be regenerated.

use prunella_conformance::{vector_dir, vectors};
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = vector_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("could not create {}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    for vector in vectors::build_all() {
        let path = dir.join(format!("{}.json", vector.name));
        if let Err(error) = std::fs::write(&path, vectors::render(&vector)) {
            eprintln!("could not write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}
