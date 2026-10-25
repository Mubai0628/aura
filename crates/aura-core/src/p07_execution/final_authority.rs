use super::*;

pub(super) fn result_assembly_context(
    admitted_batch: &AdmittedToolBatchV1,
    planned_call: &PlannedToolRuntimeCallV1,
    authority: &ExactToolExecutionAuthorityV1,
) -> Result<ToolResultAssemblyContextV1, ToolAttemptPreDispatchErrorV1> {
    let call = &planned_call.validated_call.call;
    let reference =
        call.definition_ref.as_ref().ok_or(ToolAttemptPreDispatchErrorV1::PlanMismatch)?;
    let NormalizedToolCallInputStateV1::Ready { purpose, arguments_hash, .. } = &call.input else {
        return Err(ToolAttemptPreDispatchErrorV1::PlanMismatch);
    };
    if authority.definition.tool_id != reference.tool_id
        || authority.definition.tool_version != reference.tool_version
        || authority.instance.tool_id != reference.tool_id
        || authority.instance.tool_version != reference.tool_version
    {
        return Err(ToolAttemptPreDispatchErrorV1::PlanMismatch);
    }
    Ok(ToolResultAssemblyContextV1 {
        session_id: admitted_batch.batch.invocation_identity.session_id,
        ordinal: call.ordinal,
        definition_ref: reference.clone(),
        tool_instance_id: authority.instance.tool_instance_id,
        source: authority.instance.source_kind,
        purpose: purpose.clone(),
        arguments_hash: arguments_hash.clone(),
    })
}

pub(super) fn validate_plan_identity(
    admitted_batch: &AdmittedToolBatchV1,
    planned_batch: &PlannedToolBatchV1,
    planned_call: &PlannedToolRuntimeCallV1,
) -> Result<(), ToolAttemptPreDispatchErrorV1> {
    if admitted_batch.batch.tool_batch_id != planned_batch.execution_plan.tool_batch_id
        || admitted_batch.batch.registry_snapshot_ref
            != planned_batch.execution_plan.registry_snapshot_ref
        || planned_call
            .validated_call
            .call
            .definition_ref
            .as_ref()
            .ok_or(ToolAttemptPreDispatchErrorV1::PlanMismatch)?
            .registry_snapshot_ref
            != admitted_batch.batch.registry_snapshot_ref
    {
        return Err(ToolAttemptPreDispatchErrorV1::PlanMismatch);
    }
    Ok(())
}

pub(super) fn validate_final_authority_and_state(
    persistence: &PersistenceCoordinator,
    admitted_batch: &AdmittedToolBatchV1,
    planned_call: &PlannedToolRuntimeCallV1,
    evaluated_authority: &ExactToolExecutionAuthorityV1,
    max_retries: u8,
) -> Result<aura_persistence::ToolExecutionEntityStateV1, ToolAttemptPreDispatchErrorV1> {
    let call_id = planned_call.validated_call.call.tool_call_id;
    // The **live** enablement authority, read at the final dispatch
    // guard and **before** the authority comparison.
    //
    // # Why this check belongs here
    //
    // A tool's `enabled` flag was previously consulted only while **building a registry
    // snapshot** (`aura_tool_registry::builder::ineligibility_reason`). That is a point-in-time
    // judgement over a frozen record: a registry built while a package tool was enabled keeps
    // admitting it after the user disables it, because the frozen snapshot still lists it as
    // eligible. Registry eligibility is therefore necessary but not sufficient, and the
    // coordinator's current switch has to be re-read at the moment a real attempt is created.
    //
    // This is the same authority the Agent path consults
    // (`agent_execution::operation::CallContext::verify_source`), read through the same port, so
    // the two dispatch paths cannot disagree about whether a tool may run. It reads the
    // **current** committed state rather than any snapshot, which is what makes a disable take
    // effect for an attempt that has not yet been prepared.
    //
    // # Why it is checked before the authority comparison
    //
    // A disable also moves the entity revision the execution authority is keyed by, so the
    // existing `AuthorityChanged` guard would refuse the attempt too — but it would report "the
    // authority changed", which is a statement about a record rather than about the user's
    // decision. The user disabled a tool; that is the fact they need to see, and it is the fact
    // that decides whether the attempt is legal. Checking it first makes the refusal name the
    // real reason while leaving the authority guard intact for the cases it really covers.
    //
    // The scope is the one the batch's own Registry Snapshot names: the call was admitted against
    // that snapshot, and its `scope` is the authoritative workspace/session/turn/run tuple. It is
    // read from the frozen snapshot rather than resolved by guessing, so the enablement question
    // is asked against the very scope the call belongs to.
    //
    // An unresolvable scope is a refusal (`EnablementUnavailable`) rather than a permissive
    // default: failing to ask is never evidence that a tool is enabled.
    let dispatched_tool = planned_call.validated_call.binding.tool_id;
    let corpus = persistence
        .read_tool_registry_search_corpus(&admitted_batch.batch.registry_snapshot_ref)
        .map_err(|_| ToolAttemptPreDispatchErrorV1::EnablementUnavailable)?
        .ok_or(ToolAttemptPreDispatchErrorV1::EnablementUnavailable)?;
    let disabled = persistence
        .tools_not_enabled_under_scope(
            &corpus.snapshot.scope,
            &std::collections::BTreeSet::from([dispatched_tool]),
        )
        .map_err(|_| ToolAttemptPreDispatchErrorV1::EnablementUnavailable)?;
    if !disabled.is_empty() {
        return Err(ToolAttemptPreDispatchErrorV1::ToolDisabled { tool_id: dispatched_tool });
    }
    // The pre-existing authority guard, unchanged and still enforced: a call whose exact
    // authority moved for any reason other than enablement is refused here.
    let current_authority = persistence
        .load_exact_tool_execution_authority(
            planned_call
                .validated_call
                .call
                .definition_ref
                .as_ref()
                .ok_or(ToolAttemptPreDispatchErrorV1::PlanMismatch)?,
            Some(admitted_batch.batch.tool_batch_id),
        )?
        .ok_or(ToolAttemptPreDispatchErrorV1::AuthorityUnavailable)?;
    if &current_authority != evaluated_authority {
        return Err(ToolAttemptPreDispatchErrorV1::AuthorityChanged);
    }
    let entity = persistence
        .load_tool_execution_entity_state_v1(admitted_batch.batch.tool_batch_id, call_id)?
        .ok_or(ToolAttemptPreDispatchErrorV1::ExecutionStateUnavailable)?;
    let expected_call_status = if entity.next_attempt_number == 1 {
        ToolCallStatusV1::Queued
    } else {
        ToolCallStatusV1::Running
    };
    if !matches!(entity.batch_status, ToolBatchStatusV1::Admitted | ToolBatchStatusV1::Running)
        || entity.call_status != expected_call_status
        || entity.next_attempt_number > max_retries.saturating_add(1)
    {
        return Err(ToolAttemptPreDispatchErrorV1::ExecutionStateUnavailable);
    }
    Ok(entity)
}

pub(super) fn dispatch_prepared_attempt(
    tool_call_id: aura_domain_common::ToolCallId,
    attempt_number: u8,
    prepared_at: UtcTimestamp,
) -> Result<ToolExecutionAttemptRecordV1, ToolAttemptPreDispatchErrorV1> {
    let attempt = ToolExecutionAttemptRecordV1 {
        tool_execution_attempt_id: ToolExecutionAttemptId::new(),
        tool_call_id,
        attempt_number,
        status: ToolExecutionAttemptStatusV1::DispatchPrepared,
        side_effect_state: SideEffectStateV1::NotStarted,
        evidence_ref: None,
        started_at: prepared_at,
        dispatch_prepared_at: Some(prepared_at),
        completed_at: None,
        last_progress_sequence: 0,
    };
    attempt.validate().map_err(|_| ToolAttemptPreDispatchErrorV1::PlanMismatch)?;
    Ok(attempt)
}
