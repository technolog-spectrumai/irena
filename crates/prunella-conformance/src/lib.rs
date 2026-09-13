//! Conformance for Prunella Protocol V1.
//!
//! Two things live here:
//!
//! * [`oracle`] — an independent implementation of the protocol written from
//!   `PROTOCOL_V1.md` alone, with no shared code with the real one.
//! * [`vectors`] — the golden vector format, the fixed cases, and the builders that
//!   derive expected values from the real implementation.
//!
//! The tests in this crate hold three things to the same set of bytes: the committed
//! files under `test-vectors/v1/`, the implementation, and the oracle. Any drift in any
//! one of them fails a test that names the vector and the field.

pub mod oracle;
pub mod vectors;

use std::path::PathBuf;

/// The directory the committed V1 vectors live in.
#[must_use]
pub fn vector_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-vectors/v1")
}
