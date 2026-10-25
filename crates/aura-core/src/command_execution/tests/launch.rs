use super::*;

#[test]
fn dispatch_claim_and_result_are_unique_and_unknown_is_preserved() {
    let case = Case::new();
    let mut count = 0;
    case.port
        .claim(&case.request, &mut || {
            count += 1;
            Ok(())
        })
        .unwrap();
    assert!(
        case.port
            .claim(&case.request, &mut || {
                count += 1;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(count, 1);
    let termination = case.port.complete(&case.request, None, true, true, None, None).unwrap();
    assert!(matches!(termination, TerminationEvidence::Confirmed { .. }));
    // A late reply is append-only evidence, never a second terminal result.
    case.port
        .complete(
            &case.request,
            None,
            true,
            true,
            Some(aura_backend_windows_user::NotStarted::Cancelled),
            None,
        )
        .unwrap();
    let ReconcileResult::Completed { result } =
        case.port.reconcile(case.request.binding.attempt.clone()).unwrap()
    else {
        panic!("stored result")
    };
    assert_eq!(result.outcome, ExecutionOutcome::Unknown);
    assert_eq!(result.side_effect, SideEffectStateV1::Unknown);
    assert!(result.warnings.is_empty());
}

#[test]
fn launch_failure_is_infrastructure_failure_without_inventing_execution_or_effects() {
    let case = Case::new();
    case.port.claim(&case.request, &mut || Ok(())).unwrap();
    case.port
        .complete(
            &case.request,
            None,
            true,
            true,
            Some(aura_backend_windows_user::NotStarted::Rejected),
            Some("command.mode_changed_before_start"),
        )
        .unwrap();
    let ReconcileResult::Completed { result } =
        case.port.reconcile(case.request.binding.attempt.clone()).unwrap()
    else {
        panic!("terminal")
    };
    assert_eq!(result.outcome, ExecutionOutcome::Failed);
    assert_eq!(result.side_effect, SideEffectStateV1::NoEffect);
    assert!(result.warnings.is_empty());
    // The bounded reason is receipt evidence the model and the audit trail can
    // act on; the generic message alone never named the failed stage.
    let store = case.port.journal.lock().expect("journal");
    let bytes = aura_persistence_core::artifact::read(
        store.database.connection(),
        result.receipt.evidence_ref,
        &case.request.operation.operation_id.to_string(),
    )
    .expect("receipt");
    let evidence: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert_eq!(evidence["failure_code"].as_str(), Some("command.mode_changed_before_start"));
    assert!(
        evidence["model_safe_failure_message"]
            .as_str()
            .expect("message")
            .contains("command.mode_changed_before_start")
    );
}

#[test]
fn stopped_but_unrestored_launch_is_never_a_clean_success() {
    let case = Case::new();
    case.port.claim(&case.request, &mut || Ok(())).unwrap();
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
        exit_code: Some(0),
        stop_reason: RunnerStopReason::Exited,
        process_tree_stopped: true,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    case.port
        .complete(
            &case.request,
            Some(&observation),
            true,
            false,
            None,
            Some("runner.resource_restore_failed"),
        )
        .expect("complete");
    let ReconcileResult::Completed { result } =
        case.port.reconcile(case.request.binding.attempt.clone()).unwrap()
    else {
        panic!("terminal")
    };
    // The script genuinely exited 0, but the granted access was not provably
    // restored: the result must carry the named warning, not read as clean.
    assert_eq!(result.outcome, ExecutionOutcome::Succeeded);
    assert!(result.warnings.iter().any(|warning| warning == "tool.cleanup.incomplete"));
    assert!(
        "tool.cleanup.incomplete".parse::<NormalizedIssueCode>().is_ok(),
        "cleanup warning is a registered model-visible code"
    );
    let store = case.port.journal.lock().expect("journal");
    let bytes = aura_persistence_core::artifact::read(
        store.database.connection(),
        result.receipt.evidence_ref,
        &case.request.operation.operation_id.to_string(),
    )
    .expect("receipt");
    let evidence: serde_json::Value = serde_json::from_slice(&bytes).expect("receipt json");
    assert_eq!(evidence["file_access_restored"].as_bool(), Some(false));
    assert_eq!(evidence["failure_code"].as_str(), Some("runner.resource_restore_failed"));
}

// ---------------------------------------------------------------------------
// R2: the managed background's real lifecycle, observed while it is held
// ---------------------------------------------------------------------------

/// **R2** — a real production callback sequence keeps **at most one** background row and ends at zero.
///
/// # The defect this rejects
///
/// The shipped Shell publisher set `generation = update.sequence`, so `Running(1)`, `Running(2)`,
/// `Unknown(3)` and `Exited(4)` were **four different activities**. The terminal update ended only
/// the fourth, and the first three stayed advertised as `running` forever. The brief's exit criterion
/// is exact: "for a real production callback sequence Running(1)→Running(2)→Unknown(3)→Exited(4), at
/// most one activity at all times, finally zero".
///
/// # Why this goes through the real port
///
/// The sequence below is delivered to `WindowsUserCorePort::background` — the same entry the Shell
/// backend calls when it really observes a managed process. The counts are then read out of the
/// **real index** the port publishes into, so this measures the production path rather than a
/// re-implementation of its rule.
///
/// # The positive control, which the "zero at the end" half depends on
///
/// "Finally zero" is worthless on its own: an implementation that never publishes anything also ends
/// at zero. The assertion that makes it meaningful is the one **during** the hold - a row really
/// exists while the background is running - so the test proves presence first and absence second.
#[test]
fn a_real_background_callback_sequence_keeps_one_row_and_ends_at_zero() {
    use aura_windows_user_contract::{BackgroundState, RunnerBackgroundUpdate};

    let case = Case::new();
    let index = case.port.activity_index.clone();
    let session = case.request.operation.content.identity.session_id;
    let workspace = case.request.operation.content.identity.workspace_id;
    let (Some(session), Some(workspace)) = (session, workspace) else {
        panic!("the fixture's operation carries a real session and workspace")
    };
    let reader = aura_activity_runtime::ActivityVisibility::for_session(
        case.request.plan.prepared.backend.runtime_instance_id,
        session,
        workspace,
    );
    // The fixture's index belongs to this case alone, so a row count is this test's own count. The
    // managed background carries no `ToolCallId` at all - it is not the Agent's call - which is why
    // the rows are counted rather than looked up by one.
    let rows = || index.snapshot(&reader).rows.len();

    let update = |sequence: u64, state: BackgroundState| RunnerBackgroundUpdate {
        attempt: case.request.binding.attempt.clone(),
        sequence,
        observed_at: UtcTimestamp::now(),
        state,
        stdout: Vec::new(),
        stderr: Vec::new(),
        output_truncated: false,
    };

    // Running(1): the real callback, on the real port. THE POSITIVE CONTROL.
    case.port
        .background(&case.request, &update(1, BackgroundState::Running))
        .expect("the real background callback answered");
    assert_eq!(
        rows(),
        1,
        "a running managed background must really be visible, or 'zero at the end' proves nothing"
    );

    // Running(2): a later observation of the SAME instance is still the same one activity.
    case.port
        .background(&case.request, &update(2, BackgroundState::Running))
        .expect("the real background callback answered");
    assert_eq!(
        rows(),
        1,
        "a second observation of one managed process must not mint a second activity"
    );

    // Unknown(3): not "stopped", so the row must survive - presenting a live process as finished is
    // the defect the state rule exists to prevent.
    case.port
        .background(&case.request, &update(3, BackgroundState::Unknown))
        .expect("the real background callback answered");
    assert_eq!(rows(), 1, "Unknown is not stopped: the managed activity must still be present");

    // Exited(4): the real terminal observation really ends it.
    case.port
        .background(&case.request, &update(4, BackgroundState::Exited))
        .expect("the real background callback answered");
    assert_eq!(rows(), 0, "the terminal observation must end the managed activity");

    // A late Running after the terminal state must NOT resurrect the process, and must not create a
    // new row for an instance that really stopped.
    case.port
        .background(&case.request, &update(5, BackgroundState::Running))
        .expect("the real background callback answered");
    assert_eq!(
        rows(),
        0,
        "a late Running after a terminal observation must not resurrect the old process"
    );

    // A repeated, out-of-order older observation changes nothing either.
    case.port
        .background(&case.request, &update(2, BackgroundState::Running))
        .expect("the real background callback answered");
    assert_eq!(
        rows(),
        0,
        "a stale update from before the terminal state must not bring the row back"
    );
}
