use super::*;

#[test]
fn nonzero_exit_completes_invocation_and_preserves_actual_output() {
    let case = Case::new();
    case.port.claim(&case.request, &mut || Ok(())).expect("claim");
    let prepared: WindowsUserPreparation =
        serde_json::from_value(case.request.plan.prepared.preparation.clone()).expect("plan");
    let at = UtcTimestamp::now();
    let observation = RunnerObservation {
        submitted_script: String::new(),
        output_truncated: false,
        attempt: case.request.binding.attempt.clone(),
        binding_fingerprint: jcs::canonical_sha256(&case.request.binding).expect("hash"),
        account_sid: "private-account-fixture".into(),
        process_id: 456,
        shell_image: prepared.shell_image,
        started_at: at,
        finished_at: at,
        exit_code: Some(7),
        stop_reason: RunnerStopReason::Exited,
        process_tree_stopped: true,
        stdout: b"partial output".to_vec(),
        stderr: "synthetic failure\n".repeat(1000).into_bytes(),
    };
    case.port
        .complete(&case.request, Some(&observation), true, true, None, None)
        .expect("complete");
    let store = case.port.journal.lock().expect("journal");
    let attempt = aura_persistence_v2::execution::load(
        store.database.connection(),
        case.request.operation.operation_id,
        1,
    )
    .expect("stored execution");
    let AttemptState::Terminal { result } = attempt.state else { panic!("terminal") };
    assert_eq!(result.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(result.side_effect, SideEffectStateV1::Unassessed);
    assert!(result.warnings.is_empty());
    let bytes = aura_persistence_core::artifact::read(
        store.database.connection(),
        result.receipt.evidence_ref,
        &case.request.operation.operation_id.to_string(),
    )
    .expect("receipt");
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(receipt["model_safe_failure_message"].is_null());
    assert_eq!(receipt["observation"]["exit_code"], 7);
    assert_eq!(receipt["observation"], serde_json::to_value(&observation).expect("observation"));
}
