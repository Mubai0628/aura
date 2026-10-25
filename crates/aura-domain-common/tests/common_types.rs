use std::str::FromStr;

use aura_domain_common::{
    AttachmentGrantId, BrowserShareId, ContentHash, ContentHashAlgorithm,
    ContextProjectionSnapshotId, ContractCompatibilityError, ContractVersion, CredentialId,
    ErrorCategory, ErrorCode, ErrorEnvelope, ExpectedRevision, ModelAvailabilityTestId,
    ModelContentBlockId, ModelDiscoveryRunId, ModelExecutionSnapshotId, ModelFailureId,
    PromptBundleSnapshotId, Revision, SessionId, TaskLineageId, TransportAttemptId, UtcTimestamp,
};
use serde_json::{Value, json};

const EARLY_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const LATE_ULID: &str = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

#[test]
fn typed_ids_parse_sort_and_serialize_canonically() {
    let early = SessionId::from_str(&format!("ses_{EARLY_ULID}")).expect("valid early ID");
    let late = SessionId::from_str(&format!("ses_{LATE_ULID}")).expect("valid late ID");
    let mut ids = [late, early];
    ids.sort_unstable();

    assert_eq!(ids, [early, late]);
    assert_eq!(
        serde_json::to_value(early).expect("ID should serialize"),
        json!(format!("ses_{EARLY_ULID}"))
    );
    assert!(SessionId::from_str(&format!("wsp_{EARLY_ULID}")).is_err());
    assert!(SessionId::from_str(&format!("ses_{}", EARLY_ULID.to_lowercase())).is_err());
}

#[test]
fn p05_ids_use_the_registered_three_character_prefixes() {
    macro_rules! assert_registered_id {
        ($type:ty, $prefix:literal) => {{
            let encoded = format!("{}_{}", $prefix, EARLY_ULID);
            let parsed = <$type>::from_str(&encoded).expect("registered P05 ID should parse");
            assert_eq!(parsed.to_string(), encoded);
            assert!(<$type>::from_str(&format!("bad_{EARLY_ULID}")).is_err());
        }};
    }

    assert_registered_id!(ModelExecutionSnapshotId, "mex");
    assert_registered_id!(TransportAttemptId, "tat");
    assert_registered_id!(ModelContentBlockId, "mcb");
    assert_registered_id!(ModelFailureId, "mfl");
    assert_registered_id!(ContextProjectionSnapshotId, "cps");
    assert_registered_id!(PromptBundleSnapshotId, "pbs");
    assert_registered_id!(ModelDiscoveryRunId, "mdr");
    assert_registered_id!(ModelAvailabilityTestId, "mav");
    assert_registered_id!(CredentialId, "crd");
    assert_registered_id!(BrowserShareId, "bsh");
    assert_registered_id!(AttachmentGrantId, "atg");
    assert_registered_id!(TaskLineageId, "lin");
    assert!(
        ModelAvailabilityTestId::from_str(&format!("cor_{EARLY_ULID}")).is_err(),
        "connection tests must never reuse CorrelationId"
    );
}

#[test]
fn revisions_use_decimal_strings_on_the_wire() {
    let revision = Revision::new(u64::MAX);
    let encoded = serde_json::to_value(revision).expect("revision should serialize");
    assert_eq!(encoded, json!(u64::MAX.to_string()));

    let decoded: Revision = serde_json::from_value(encoded).expect("revision should deserialize");
    assert_eq!(decoded, revision);
    assert!(serde_json::from_value::<Revision>(json!(42)).is_err());
    assert!(serde_json::from_value::<ExpectedRevision>(json!("042")).is_err());
}

#[test]
fn timestamps_normalize_to_utc_rfc3339() {
    let timestamp =
        UtcTimestamp::from_str("2026-08-28T08:00:00+08:00").expect("timestamp should parse");
    assert_eq!(timestamp.to_string(), "2026-08-28T00:00:00Z");
    assert_eq!(
        serde_json::to_value(timestamp).expect("timestamp should serialize"),
        json!("2026-08-28T00:00:00Z")
    );
}

#[test]
fn content_hashes_validate_length_case_and_unknown_fields() {
    let hash = ContentHash::new(ContentHashAlgorithm::Blake3, "ab".repeat(32))
        .expect("hash should validate");
    assert_eq!(
        serde_json::from_value::<ContentHash>(
            serde_json::to_value(&hash).expect("hash should serialize")
        )
        .expect("hash should deserialize"),
        hash
    );
    assert!(ContentHash::new(ContentHashAlgorithm::Sha256, "AB".repeat(32)).is_err());
    assert!(
        serde_json::from_value::<ContentHash>(json!({
            "algorithm": "blake3",
            "hex": "ab".repeat(32),
            "unknown": true
        }))
        .is_err()
    );
}

#[test]
fn contract_versions_reject_incompatible_inputs() {
    let supported = ContractVersion::new(1, 2);
    assert!(supported.ensure_supported(ContractVersion::new(1, 0)).is_ok());
    assert_eq!(
        supported.ensure_supported(ContractVersion::new(2, 0)),
        Err(ContractCompatibilityError::MajorVersionMismatch { supported: 1, incoming: 2 })
    );
    assert_eq!(
        supported.ensure_supported(ContractVersion::new(1, 3)),
        Err(ContractCompatibilityError::MinorVersionTooNew { supported: 2, incoming: 3 })
    );
}

#[test]
fn error_envelopes_reject_unknown_fields() {
    let correlation_id = format!("cor_{EARLY_ULID}").parse().expect("correlation ID should parse");
    let envelope = ErrorEnvelope::new(
        ErrorCode::new("contract.invalid_payload").expect("error code should validate"),
        ErrorCategory::Validation,
        "The payload is invalid.",
        false,
        correlation_id,
        None,
    );
    let mut value = serde_json::to_value(envelope).expect("envelope should serialize");
    value
        .as_object_mut()
        .expect("envelope is an object")
        .insert("unknown".to_owned(), Value::Bool(true));

    assert!(serde_json::from_value::<ErrorEnvelope>(value).is_err());
}
