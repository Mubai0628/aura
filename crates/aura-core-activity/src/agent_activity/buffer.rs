//! Execution facts for the buffer publisher.
use super::*;

/// The record for one real buffer commit.
///
/// The buffer store is the executor that really wrote the bytes, so it publishes under its own
/// reservation identity and **no Agent run**. The target uses the identity the store already
/// verified, which is what associates two commits of one file across workspaces and sessions.
pub(crate) fn buffer_commit_record(
    fact: &aura_workspace::BufferCommitFact<'_>,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
) -> aura_activity_domain::ActivityRecord {
    use aura_activity_domain::*;
    let execution = ActivityExecutionRef::BufferCommit {
        reservation_id: fact.reservation_id,
        buffer_revision: fact.buffer_revision,
    };
    // The generation is the buffer revision this commit was prepared from, so a later commit of
    // the same file is a distinct activity and an old commit's end cannot remove it.
    let generation = u32::try_from(fact.buffer_revision.value()).unwrap_or(u32::MAX).max(1);
    let id = ActivityId::derive(ActivityKind::BufferCommit, &execution, generation);
    ActivityRecord {
        tool_name: "buffer.save".into(),
        private_source: aura_activity_domain::ActivityPrivateSource::WorkspaceMetadata,
        id,
        kind: ActivityKind::BufferCommit,
        execution,
        generation,
        tool_id: aura_domain_common::ToolId::from_stable_name("editor.save"),
        tool_group: ActivityToolGroup::FileWrite,
        owner: ActivityOwnerScope {
            session_id: fact.session_id,
            workspace_id: fact.workspace_id,
            // A buffer commit is not an Agent tool call. Nothing invents an Agent run for it.
            agent_run_id: None,
            turn_task_id: None,
            model_step_id: None,
            runtime_instance_id,
        },
        resources: resources::project(fact.resources),
        parameters: vec![],
        started_at: aura_domain_common::UtcTimestamp::now(),
        runtime_instance_id,
        published_at_revision: aura_domain_common::Revision::INITIAL,
    }
}

/// Publishes one real buffer commit moment and ends it at the real settlement.
///
/// The store calls this twice under the **same** identity: at `Start`, before the write window
/// opens, and at a terminal phase once the write really settled or failed. Two facts follow from
/// that shape and both are the ones R2 requires:
///
/// * a save that is really in flight occupies exactly one `running` row - it is observable while
///   the bytes are being written, which a publish-after-the-fact can never be, and
/// * a save that finished leaves **zero** rows, because the terminal call ends the same identity it
///   published. There is no `end` on a phase the store never reaches.
pub fn publish_buffer_commit_activity(
    fact: &aura_workspace::BufferCommitFact<'_>,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    index: &aura_activity_runtime::ActivityIndex,
    observer: &aura_activity_runtime::ActivityObserverHandle,
) {
    let record = buffer_commit_record(fact, runtime_instance_id);
    let id = record.id;
    let generation = record.generation;
    match fact.phase {
        aura_workspace::BufferCommitPhase::Start => {
            let outcome = index.publish(record);
            observer.observe(outcome);
        }
        // A terminal moment ends the exact identity the start published. `end` is identity- and
        // generation-bound and idempotent, so a repeated terminal report is safe and a stale one
        // from a later buffer revision cannot remove a newer commit.
        aura_workspace::BufferCommitPhase::Settled
        | aura_workspace::BufferCommitPhase::Failed
        | aura_workspace::BufferCommitPhase::Terminated => {
            index.end(id, generation);
        }
    }
}
