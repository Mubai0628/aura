use super::*;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::*;
use aura_tool_contract::*;

fn plan(edges: &[&[usize]], groups: &[Option<u32>]) -> ToolExecutionPlanV1 {
    let ids: Vec<_> = edges.iter().map(|_| ToolCallId::new()).collect();
    let registry = ToolRegistrySnapshotV1::new(
        ToolRegistrySnapshotId::new(),
        Revision::INITIAL,
        ToolRegistryScopeV1::Workspace { workspace_id: WorkspaceId::new() },
        None,
        None,
        CanonicalHashV1::sha256(b"eligibility"),
        vec![],
        vec![],
        vec![],
        UtcTimestamp::now(),
    )
    .expect("registry");
    ToolExecutionPlanV1::new(
        ToolBatchId::new(),
        registry.snapshot_ref,
        Revision::INITIAL,
        CanonicalHashV1::sha256(b"settings"),
        edges
            .iter()
            .enumerate()
            .map(|(i, edges)| {
                let mut dependencies: Vec<_> = edges.iter().map(|j| ids[*j]).collect();
                dependencies.sort();
                ToolExecutionPlanCallV1 {
                    tool_call_id: ids[i],
                    ordinal: (i + 1) as u32,
                    dependency_tool_call_ids: dependencies,
                    execution_atomic_group: groups[i],
                    resolved_resource_claims: vec![],
                    timeout_ms: 1_000,
                    max_retries: 0,
                    local_concurrency_limit: Some(1),
                    entity_revision: Revision::INITIAL,
                }
            })
            .collect(),
    )
    .expect("plan")
}
fn result(id: ToolCallId, success: bool, unknown: bool) -> ToolResultV1 {
    let side_effect =
        if unknown { SideEffectStateV1::Unknown } else { SideEffectStateV1::NoEffect };
    let artifact = ArtifactId::new();
    ToolResultV1 {
        tool_call_id: id,
        outcome: if success {
            ToolResultOutcomeV1::Succeeded
        } else {
            ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::UserRequested }
        },
        side_effect_state: side_effect,
        attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
            tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            attempt_number: 1,
            side_effect_state: side_effect,
            side_effect_evidence_ref: Some(artifact),
            dispatched: true,
        }],
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: Some(artifact),
        started_at: None,
        completed_at: UtcTimestamp::now(),
    }
}
#[test]
fn dependencies_wait_for_committed_results_while_independent_calls_are_parallel() {
    let plan = plan(&[&[], &[0], &[]], &[None; 3]);
    let ids: Vec<_> = plan.calls.iter().map(|c| c.tool_call_id).collect();
    let mut progress = BatchProgress::new(plan, &[]).expect("progress");
    let ready = progress.ready(false);
    assert_eq!(ready.len(), 2);
    let UnitReadiness::Ready(first) = &ready[0] else { panic!("first ready") };
    assert_eq!(first.calls, [ids[0]]);
    progress.start(first).expect("start");
    assert_eq!(progress.ready(false).len(), 1, "dependency still waits for commit");
    progress.committed(first, &[result(ids[0], true, false)]).expect("committed");
    assert_eq!(progress.ready(false).len(), 2, "dependent and independent work ready");
}
#[test]
fn failed_or_unverified_dependency_blocks_descendants() {
    for (success, unknown) in [(false, false), (true, true)] {
        let plan = plan(&[&[], &[0]], &[None; 2]);
        let first = plan.calls[0].tool_call_id;
        let progress =
            BatchProgress::new(plan, &[result(first, success, unknown)]).expect("restore");
        assert!(matches!(
            &progress.ready(false)[0],
            UnitReadiness::Rejected { reason: ToolNotExecutedReasonV1::DependencyNotExecuted, .. }
        ));
    }
}
#[test]
fn completion_group_is_ordered_and_cannot_be_restored_or_committed_partially() {
    // Provider order is Complete then Review; the frozen dependency fixes it.
    let plan = plan(&[&[1], &[]], &[Some(1); 2]);
    let complete = plan.calls[0].tool_call_id;
    let review = plan.calls[1].tool_call_id;
    assert!(BatchProgress::new(plan.clone(), &[result(review, true, false)]).is_err());
    let mut progress = BatchProgress::new(plan, &[]).expect("progress");
    let UnitReadiness::Ready(unit) = &progress.ready(false)[0] else { panic!("ready") };
    let unit = unit.clone();
    assert_eq!(unit.calls, [review, complete]);
    progress.start(&unit).expect("start group");
    assert!(progress.committed(&unit, &[result(review, true, false)]).is_err());
    progress
        .committed(&unit, &[result(review, true, false), result(complete, true, false)])
        .expect("whole group commit");
    assert!(progress.finished());
}
#[test]
fn a_group_cannot_form_a_cycle_through_an_external_call() {
    let plan = plan(&[&[], &[0], &[1]], &[Some(1), None, Some(1)]);
    assert!(BatchProgress::new(plan, &[]).is_err());
}
