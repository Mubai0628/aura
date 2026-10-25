//! Batch C: the tool credential authority record is renderer-safe.
//!
//! The two domain credential authorities in this stage are the Provider
//! authority (`aura-core::p05_credential`, whose secret bytes live in the Windows
//! Credential Manager) and the tool authority represented by
//! [`ToolCredentialAuthorityRecordV1`]. The tool record is deliberately a
//! *metadata* record: it names an owner, a scope, a placement policy, a usage
//! policy and a display name, and it holds **no** secret bytes and **no** WinCred
//! locator.
//!
//! That claim is structural, so it is checked against the type's real serde
//! output rather than asserted in prose. If a future change adds a secret-bearing
//! field, this test fails.
use aura_tool_domain::ToolCredentialAuthorityRecordV1;

/// Field names that must never appear in this record's serialized shape.
///
/// The check is on **property names**, not on prose. The record's own doc
/// comment deliberately says that secret bytes and WinCred locators are held by
/// the P02 storage binding and never by this renderer-safe record — matching that
/// sentence would flag the design statement itself rather than a leak.
const FORBIDDEN_FIELDS: &[&str] = &[
    "secret",
    "secret_bytes",
    "secret_value",
    "password",
    "passphrase",
    "token",
    "token_value",
    "access_token",
    "api_key",
    "api_key_value",
    "value",
    "wincred",
    "windows_target",
    "target_name",
    "credential_ref",
    "private_key",
    "plaintext",
];

/// Fields the product genuinely needs, so a fix cannot pass by emptying the type.
const REQUIRED_FIELDS: &[&str] = &[
    "credential_id",
    "credential_revision",
    "authenticated_owner",
    "scope",
    "credential_kind",
    "state",
    "placement_policy",
    "usage_policy",
    "display_name",
    "row_revision",
];

/// Every property name in the record's schema, plus every `$defs` property name,
/// because a nested placement or scope type is part of the same serialized shape.
fn property_names() -> Vec<String> {
    let schema = serde_json::to_value(schemars::schema_for!(ToolCredentialAuthorityRecordV1))
        .expect("the tool credential record schema renders");
    let mut names = Vec::new();
    collect_property_names(&schema, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_property_names(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "properties"
                    && let serde_json::Value::Object(properties) = child
                {
                    into.extend(properties.keys().cloned());
                }
                collect_property_names(child, into);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_property_names(item, into);
            }
        }
        _ => {}
    }
}

#[test]
fn the_tool_credential_authority_record_carries_no_secret_field() {
    let names = property_names();
    assert!(!names.is_empty(), "the schema really has properties");
    for forbidden in FORBIDDEN_FIELDS {
        assert!(
            !names.iter().any(|name| name == forbidden),
            "the renderer-safe tool credential record must not carry a `{forbidden}` field; \
             it has {names:?}"
        );
    }
}

#[test]
fn the_tool_credential_authority_record_still_carries_its_real_fields() {
    let names = property_names();
    for required in REQUIRED_FIELDS {
        assert!(
            names.iter().any(|name| name == required),
            "the record must still carry `{required}`, otherwise the guard above \
             would pass by emptying the type; it has {names:?}"
        );
    }
}

/// The record round-trips, and a real serialized body carries no secret field.
#[test]
fn a_serialized_tool_credential_record_contains_no_secret_field() {
    let reference = serde_json::json!({
        "credential_id": "crd_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "credential_revision": "1",
        "authenticated_owner": "pri_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "scope": { "scope": "user" },
        "credential_kind": "api_key",
        "state": "configured",
        "placement_policy": { "allowed_placements": [{ "placement": "http_bearer_authorization" }] },
        "usage_policy": {
            "policy": "explicit",
            "tool_package_ids": [],
            "tool_ids": ["tol_01ARZ3NDEKTSV4RRFFQ69G5FAV"]
        },
        "display_name": "S05 tool credential",
        "row_revision": "1",
        "created_at": "2026-10-08T00:00:00Z",
        "updated_at": "2026-10-08T00:00:00Z"
    });
    // The shape is real: it parses into the authority record.
    let record: ToolCredentialAuthorityRecordV1 =
        serde_json::from_value(reference).expect("the tool credential record parses");
    record.validate().expect("the record is valid");

    // And its own serialization carries no secret-bearing key.
    let serialized: serde_json::Value =
        serde_json::to_value(&record).expect("the record serializes");
    let keys = serialized
        .as_object()
        .expect("the record serializes to an object")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for forbidden in FORBIDDEN_FIELDS {
        assert!(
            !keys.iter().any(|key| key == forbidden),
            "a serialized tool credential record must not carry `{forbidden}`; it has {keys:?}"
        );
    }
}
