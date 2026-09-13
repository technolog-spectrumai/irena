//! Reading and writing signing keys.
//!
//! Prunella has no opinion about key management beyond this: a key file holds a
//! 64-character lowercase hex seed and nothing else. Where it lives, who may read it
//! and how it is backed up are the operator's decisions.

use prunella_crypto::SigningKey;
use std::path::Path;

/// Reads a signing key from a hex seed file.
///
/// # Errors
///
/// Returns a message naming the file if it cannot be read or is not a valid seed.
pub fn read(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read the signing key {}: {error}", path.display()))?;
    let trimmed = text.trim();
    let mut seed = [0u8; 32];
    hex::decode_to_slice(trimmed, &mut seed).map_err(|error| {
        format!(
            "{} does not hold a 64-character hex ed25519 seed: {error}",
            path.display()
        )
    })?;
    Ok(SigningKey::from_seed(seed))
}

/// Writes a signing key as a hex seed file, readable only by its owner where the
/// platform supports it.
///
/// # Errors
///
/// Returns a message naming the file if it cannot be written.
pub fn write(path: &Path, key: &SigningKey) -> Result<(), String> {
    let mut text = hex::encode(key.to_seed());
    text.push('\n');
    std::fs::write(path, &text).map_err(|error| {
        format!(
            "could not write the signing key {}: {error}",
            path.display()
        )
    })?;
    restrict(path)
}

#[cfg(unix)]
fn restrict(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| {
        format!(
            "could not restrict permissions on {}: {error}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<(), String> {
    Ok(())
}
