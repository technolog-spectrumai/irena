//! Who may sign a record transaction, and the lockout rule.
//!
//! Every record reaches the chain as a Prunella transaction signed by some key. The
//! company's own `<identities>` says whose key that is and its `<authorisation>` says
//! whether that person may sign records of the family in question. Both are read from
//! the company **as it was before the record**: the authorisation in force when the
//! record was written is the one that applies to it, and a record cannot authorise
//! its own signer.
//!
//! The lockout rule: a company must always have at least one `company` signer who
//! holds a key, or nothing could ever be amended again. The reader refuses an
//! authorisation with no `company` row; this module refuses the combination of
//! identities and authorisation in which every `company` signer is keyless.

use crate::error::LedgerError;
use crate::state::CompanyStateV1;
use irena_core::{AuthorisationV1, IdentitiesV1, PersonV1, RecordFamilyV1};
use prunella_core::PublicKey;

/// The person authorised for `family` who currently holds `key`.
///
/// # Errors
///
/// [`LedgerError::UnauthorisedSigner`] if no person in the identities in force holds
/// the key, or the person who does is not a `family` signer in the authorisation in
/// force.
pub fn authorised_signer<'a>(
    state: &'a CompanyStateV1,
    family: RecordFamilyV1,
    key: &PublicKey,
) -> Result<&'a PersonV1, LedgerError> {
    signer_of(
        &state.identities.value,
        &state.authorisation.value,
        family,
        key,
    )
}

/// As [`authorised_signer`], against a pair of parts rather than a whole state.
///
/// # Errors
///
/// As [`authorised_signer`].
pub fn signer_of<'a>(
    identities: &'a IdentitiesV1,
    authorisation: &AuthorisationV1,
    family: RecordFamilyV1,
    key: &PublicKey,
) -> Result<&'a PersonV1, LedgerError> {
    let Some(person) = identities.holder_of(key) else {
        return Err(LedgerError::UnauthorisedSigner {
            family,
            signer: *key,
            detail: "no person in the identities in force holds this key".to_owned(),
        });
    };
    if !authorisation.allows(&person.id, family) {
        return Err(LedgerError::UnauthorisedSigner {
            family,
            signer: *key,
            detail: format!(
                "{} holds this key but is not a {family} signer in the authorisation in force",
                person.id
            ),
        });
    }
    Ok(person)
}

/// Why these parts together would lock the company out, if they would.
///
/// `None` means at least one `company` signer holds a key. `Some(detail)` names the
/// `company` signers that have none, or says the authorisation lists nobody at all.
#[must_use]
pub fn lockout_after(identities: &IdentitiesV1, authorisation: &AuthorisationV1) -> Option<String> {
    let signers: Vec<_> = authorisation.persons_for(RecordFamilyV1::Company).collect();
    if signers
        .iter()
        .any(|person| identities.key_of(person).is_some())
    {
        return None;
    }
    let listed: Vec<String> = signers
        .iter()
        .map(|person| {
            if identities.get(person).is_some() {
                format!("{person} (no key)")
            } else {
                format!("{person} (not in the identities)")
            }
        })
        .collect();
    Some(format!(
        "no company signer holds a key; company signers: {}",
        if listed.is_empty() {
            "none".to_owned()
        } else {
            listed.join(", ")
        }
    ))
}
