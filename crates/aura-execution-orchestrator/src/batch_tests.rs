use super::*;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::*;
use aura_tool_contract::*;

fn plan(edges: &[&[usize]], groups: &[Option<u32>]) -> ToolExecutionPlanV1 {
    planned(edges, groups, &vec![Vec::new(); edges.len()])
}
fn planned(
    edges: &[&[usize]],
    groups: &[Option<u32>],
    claims: &[Vec<ResolvedResourceClaimV1>],
) -> ToolExecutionPlanV1 {
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
                    resolved_resource_claims: claims[i].clone(),
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

fn claim(key: &str, access: ResolvedResourceAccessV1) -> ResolvedResourceClaimV1 {
    ResolvedResourceClaimV1 {
        kind: ResolvedResourceKindV1::WorkspacePath,
        access,
        canonical_resource_identity: key.to_owned(),
        canonical_lock_key: key.to_owned(),
        resolution_evidence_hash: CanonicalHashV1::sha256(key.as_bytes()),
        core_derived: true,
    }
}
/// Calls in the order written, each with the one resource it uses and how.
fn using(calls: &[(&str, ResolvedResourceAccessV1)], edges: &[&[usize]]) -> ToolExecutionPlanV1 {
    let claims = calls.iter().map(|(key, access)| vec![claim(key, *access)]).collect::<Vec<_>>();
    planned(edges, &vec![None; calls.len()], &claims)
}
fn ready_calls(progress: &BatchProgress) -> Vec<ToolCallId> {
    progress
        .ready(false)
        .into_iter()
        .map(|readiness| match readiness {
            UnitReadiness::Ready(unit) => unit.calls[0],
            UnitReadiness::Rejected { unit, reason } => {
                panic!("{:?} rejected: {reason:?}", unit.calls)
            }
        })
        .collect()
}
fn run(progress: &mut BatchProgress, id: ToolCallId, success: bool) {
    let unit = DispatchUnit { calls: vec![id], atomic_group: None };
    progress.start(&unit).expect("start");
    progress.committed(&unit, &[result(id, success, false)]).expect("committed");
}
#[test]
fn calls_on_one_file_start_in_the_order_written() {
    use ResolvedResourceAccessV1::{Read, Write};
    // A patch, a read and another patch of one file, and a read of another file.
    let plan =
        using(&[("a", Write), ("a", Read), ("a", Write), ("b", Read)], &[&[], &[], &[], &[]]);
    let ids: Vec<_> = plan.calls.iter().map(|c| c.tool_call_id).collect();
    let mut progress = BatchProgress::new(plan, &[]).expect("progress");
    assert_eq!(ready_calls(&progress), [ids[0], ids[3]], "the other file does not wait");
    let first = DispatchUnit { calls: vec![ids[0]], atomic_group: None };
    progress.start(&first).expect("start");
    assert_eq!(ready_calls(&progress), [ids[3]], "the read waits while the patch runs");
    progress.committed(&first, &[result(ids[0], true, false)]).expect("committed");
    assert_eq!(ready_calls(&progress), [ids[1], ids[3]], "the read after the patch");
    run(&mut progress, ids[1], true);
    assert_eq!(ready_calls(&progress), [ids[2], ids[3]], "the second patch after the read");
}
#[test]
fn reads_of_one_file_start_together() {
    use ResolvedResourceAccessV1::Read;
    let plan = using(&[("a", Read), ("a", Read)], &[&[], &[]]);
    let ids: Vec<_> = plan.calls.iter().map(|c| c.tool_call_id).collect();
    let progress = BatchProgress::new(plan, &[]).expect("progress");
    assert_eq!(ready_calls(&progress), ids);
}
#[test]
fn a_call_written_after_one_that_failed_on_the_file_still_runs() {
    use ResolvedResourceAccessV1::{Read, Write};
    let plan = using(&[("a", Write), ("a", Read)], &[&[], &[]]);
    let ids: Vec<_> = plan.calls.iter().map(|c| c.tool_call_id).collect();
    let mut progress = BatchProgress::new(plan.clone(), &[]).expect("progress");
    run(&mut progress, ids[0], false);
    assert_eq!(ready_calls(&progress), [ids[1]]);
    // The same when the batch is taken up again after the first call was committed.
    let restored = BatchProgress::new(plan, &[result(ids[0], false, false)]).expect("restore");
    assert_eq!(ready_calls(&restored), [ids[1]]);
}
#[test]
fn calls_with_dependencies_keep_the_order_their_dependencies_give() {
    use ResolvedResourceAccessV1::Write;
    // The first call depends on the second; both use one resource. The dependency decides.
    let plan = using(&[("state", Write), ("state", Write)], &[&[1], &[]]);
    let ids: Vec<_> = plan.calls.iter().map(|c| c.tool_call_id).collect();
    let mut progress = BatchProgress::new(plan, &[]).expect("progress");
    assert_eq!(ready_calls(&progress), [ids[1]]);
    run(&mut progress, ids[1], true);
    assert_eq!(ready_calls(&progress), [ids[0]]);
    run(&mut progress, ids[0], true);
    assert!(progress.finished());
}
