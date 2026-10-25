use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ArtifactId, ToolExecutionAttemptId};
use aura_execution_contract::{
    AttemptSideEffectProofV1, SideEffectEvidenceProducerV1, VerifiedAttemptSideEffectEvidenceV1,
};
use serde_json::{Value, json};

#[test]
fn verified_attempt_side_effect_evidence_is_attempt_scope_producer_and_proof_bound() {
    let evidence = VerifiedAttemptSideEffectEvidenceV1::from_core_evidence(
        ToolExecutionAttemptId::from_bytes([1; 16]),
        CanonicalSha256HashV1::sha256(b"effect scope"),
        SideEffectEvidenceProducerV1::StrictSandbox,
        AttemptSideEffectProofV1::NoEffect { evidence_ref: ArtifactId::from_bytes([2; 16]) },
    )
    .expect("verified evidence");
    evidence.validate().expect("self-validating evidence");

    let wire = serde_json::to_value(&evidence).expect("serialize evidence");
    assert_eq!(
        serde_json::from_value::<VerifiedAttemptSideEffectEvidenceV1>(wire.clone())
            .expect("round trip"),
        evidence
    );
    // Each replacement is valid for its field: rejection must come from the
    // binding, rather than an unknown JSON field or a malformed typed identity.
    for (pointer, replacement) in [
        ("/tool_execution_attempt_id", json!(ToolExecutionAttemptId::from_bytes([3; 16]))),
        ("/effect_scope_hash/hex", Value::String("0".repeat(64))),
        ("/producer", json!("core_transaction")),
        ("/proof/evidence_ref", json!(ArtifactId::from_bytes([4; 16]))),
        ("/proof/evidence_kind", json!("unknown")),
    ] {
        let mut altered = wire.clone();
        *altered.pointer_mut(pointer).expect("existing field") = replacement;
        assert!(
            serde_json::from_value::<VerifiedAttemptSideEffectEvidenceV1>(altered).is_err(),
            "accepted altered evidence at {pointer}"
        );
    }
}
