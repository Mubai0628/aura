//! A speed mode's body fields and headers, added to a request the same way in every format. The
//! snapshot carries only a mode its model's catalog entry offers; each field and header is checked
//! against Aura's list again here, as the snapshot may have been written by another build.
use aura_model_domain::{
    ModelExecutionSnapshot, ProviderRequestHeaderClass, SPEED_MODE_BODY_FIELDS, SPEED_MODE_HEADERS,
    is_speed_mode_word,
};
use serde_json::Value;

use crate::NonSecretProviderHeader;

/// The entries of `fields` on `allowed`, each with a plain word for its value.
fn listed<'a>(
    fields: impl IntoIterator<Item = (&'a String, &'a String)>,
    allowed: &'a [&'a str],
) -> impl Iterator<Item = (&'a String, &'a String)> {
    fields
        .into_iter()
        .filter(move |(name, value)| allowed.contains(&name.as_str()) && is_speed_mode_word(value))
}

/// The body fields of the snapshot's speed mode, for the adapter to add to its request body.
#[must_use]
pub fn speed_mode_fields(snapshot: &ModelExecutionSnapshot) -> Vec<(String, Value)> {
    let Some(mode) = &snapshot.request_options.speed_mode else {
        return Vec::new();
    };
    listed(&mode.body, &SPEED_MODE_BODY_FIELDS)
        .map(|(name, value)| (name.clone(), Value::String(value.clone())))
        .collect()
}

/// Adds the headers of the snapshot's speed mode. A header the request already has keeps its own
/// value and gets the mode's after it, as a list; header names are lower case throughout.
pub(crate) fn add_speed_mode_headers(
    snapshot: &ModelExecutionSnapshot,
    headers: &mut Vec<NonSecretProviderHeader>,
) {
    let Some(mode) = &snapshot.request_options.speed_mode else {
        return;
    };
    for (name, value) in listed(&mode.headers, &SPEED_MODE_HEADERS) {
        match headers.iter_mut().find(|header| header.name == *name) {
            Some(own) => own.value = format!("{},{value}", own.value),
            None => headers.push(NonSecretProviderHeader {
                name: name.clone(),
                value: value.clone(),
                class: ProviderRequestHeaderClass::ProtocolFixed,
            }),
        }
    }
}
