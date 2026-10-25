use super::*;

impl ToolAttemptPreDispatchServiceV1 {
    /// HTTP counterpart of `begin`. It holds resource/generation/global
    /// permits, reloads the exact Registry authority and atomically persists
    /// Started + DispatchPrepared before the Gateway can observe the request.
    pub async fn begin_http(
        self,
        persistence: &PersistenceCoordinator,
        scheduler: &ToolExecutionSchedulerV1,
        cancellation: &mut watch::Receiver<bool>,
        input: BeginAuthorizedToolHttpAttemptInputV1<'_>,
    ) -> Result<CommittedAuthorizedToolHttpAttemptV1, ToolAttemptPreDispatchErrorV1> {
        let call_id = input.planned_call.validated_call.call.tool_call_id;
        let plan_call = input
            .planned_batch
            .execution_plan
            .calls
            .iter()
            .find(|call| call.tool_call_id == call_id)
            .ok_or(ToolAttemptPreDispatchErrorV1::PlanMismatch)?;
        let result_contract = input.evaluated_authority.definition.result_contract.clone();
        let execution_semantics = input.evaluated_authority.definition.execution_semantics;
        let timeout_ms = plan_call.timeout_ms;
        let max_retries = plan_call.max_retries;
        let result_context = result_assembly_context(
            input.admitted_batch,
            input.planned_call,
            input.evaluated_authority,
        )?;
        validate_plan_identity(input.admitted_batch, input.planned_batch, input.planned_call)?;

        let readiness = scheduler
            .acquire_dispatch_readiness(
                ToolScheduleRequestV1 {
                    plan_call,
                    generation: input.planned_call.generation.clone(),
                    source_lock_key: None,
                },
                cancellation,
            )
            .await?;
        let scheduler_permit = readiness.activate_runtime_slot(cancellation).await?;
        if *cancellation.borrow() {
            return Err(ToolAttemptPreDispatchErrorV1::CancelledBeforeDispatch);
        }

        let entity = validate_final_authority_and_state(
            persistence,
            input.admitted_batch,
            input.planned_call,
            input.evaluated_authority,
            plan_call.max_retries,
        )?;
        let prepared_at = UtcTimestamp::now();
        let attempt = dispatch_prepared_attempt(call_id, entity.next_attempt_number, prepared_at)?;
        let mut projection_state = input.projection_state;
        let projections = projection_state.dispatch_prepared(&attempt)?;
        let materialized = input
            .authorized_operation
            .materialize_dispatch(attempt.tool_execution_attempt_id, prepared_at)?;
        let commit = persistence.commit_tool_execution_attempt_dispatch_prepared(
            ToolExecutionAttemptDispatchPreparedCommitRequest {
                attempt: attempt.clone(),
                execution_plan_hash: input.planned_batch.execution_plan.execution_plan_hash.clone(),
                dispatch_evidence_hash: materialized.dispatch_evidence_hash,
                expected_batch_revision: entity.batch_revision,
                expected_call_revision: entity.call_revision,
                admitted_event_id: input.admitted_event_id,
                causation_id: CausationId::from_ulid(input.admitted_event_id.ulid()),
                batch_projection: projections.batch,
                call_projection: projections.call,
                attempt_projection: projections.attempt,
                session_activity_projection: projections.session,
                correlation_id: input.correlation_id,
            },
        )?;
        let dispatch_prepared_event_id = *commit
            .event_ids
            .last()
            .ok_or(ToolAttemptPreDispatchErrorV1::PersistenceEvidenceMissing)?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&commit.projection_changes).err();
        Ok(CommittedAuthorizedToolHttpAttemptV1 {
            tool_batch_id: input.admitted_batch.batch.tool_batch_id,
            attempt,
            gateway_request: materialized.gateway_request,
            success_status_ranges: materialized.success_status_ranges,
            projection_state,
            persistence: commit,
            projection_publish_error,
            scheduler_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id: input.correlation_id,
            result_context,
        })
    }
}
