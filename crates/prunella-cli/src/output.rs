//! Rendering results as text or JSON, and the exit codes that go with them.

use std::process::ExitCode;

/// Exit code for a successful operation.
pub const EXIT_OK: u8 = 0;
/// Exit code for a chain that is invalid, or a lookup that found nothing.
///
/// Separate from [`EXIT_ERROR`] so that a script can tell "the chain is bad" apart
/// from "the command could not run".
pub const EXIT_FINDING: u8 = 1;
/// Exit code for an operation that could not be carried out.
pub const EXIT_ERROR: u8 = 2;

/// How results should be rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Plain text for a person.
    Text,
    /// JSON for a program.
    Json,
}

impl Format {
    /// Chooses a format from the `--json` flag.
    #[must_use]
    pub const fn from_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Text }
    }

    /// Prints a value in the chosen format.
    pub fn emit(self, text: &str, json: &serde_json::Value) {
        match self {
            Self::Text => println!("{text}"),
            Self::Json => println!("{}", serde_json::to_string_pretty(json).unwrap_or_default()),
        }
    }
}

/// Turns a code into a process exit status.
#[must_use]
pub fn exit(code: u8) -> ExitCode {
    ExitCode::from(code)
}
