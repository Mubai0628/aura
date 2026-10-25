use super::support::{Fixture, Kind};
use crate::package_program::{ProgramInvocationIdentity, run_case};
use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
use aura_tool_domain::PackageConformanceReasonV1;
use std::sync::atomic::AtomicBool;

#[test]
fn stdio_keeps_the_core_call_and_attempt_identity_in_bootstrap_frames_and_evidence() {
    let fixture = Fixture::new(Kind::Stdio, |raw| {
        raw["tools"][0]["runtime"]["arguments"] =
            serde_json::json!([{"type":"literal","value":"--report-identity"}]);
    });
    let mut context = fixture.context();
    let expected = ProgramInvocationIdentity {
        tool_call_id: ToolCallId::new(),
        tool_execution_attempt_id: ToolExecutionAttemptId::new(),
    };
    context.identity = expected.clone();
    let key = &fixture.staged.canonical_manifest().tools[0].tool_key;
    let first = run_case(
        &fixture.staged,
        key,
        &serde_json::json!({}),
        &context,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("observed first attempt");
    assert_eq!(first.reason, PackageConformanceReasonV1::Completed);
    assert_eq!(first.identity, expected);
    let body: serde_json::Value = serde_json::from_slice(&first.body).expect("fixture report");
    assert_eq!(body["tool_call_id"], expected.tool_call_id.to_string());
    assert_eq!(body["tool_execution_attempt_id"], expected.tool_execution_attempt_id.to_string());
    context.identity.tool_execution_attempt_id = ToolExecutionAttemptId::new();
    let second = run_case(
        &fixture.staged,
        key,
        &serde_json::json!({}),
        &context,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("observed different attempt");
    assert_eq!(second.identity, context.identity);
    assert_ne!(first.runtime_evidence_hash, second.runtime_evidence_hash);
    fixture.assert_scratch_clean();
}
