use super::*;

// UNWIRED: no production caller dispatches work to the sandbox host; only tests reach this.
impl ToolAttemptPreDispatchServiceV1 {
    /// Acquires deterministic locks and the selected Host's actual capacity,
    /// rechecks exact Registry/Package authority, then atomically persists
    /// `tea_`, Started, DispatchPrepared, projections, and the one-shot Host
    /// authorization binding. No Runtime boundary is crossed here.
    pub async fn begin(
        self,
        persistence: &PersistenceCoordinator,
        scheduler: &ToolExecutionSchedulerV1,
        sandbox: &ToolSandboxExecutionService,
        cancellation: &mut watch::Receiver<bool>,
        input: BeginAuthorizedToolAttemptInputV1<'_>,
    ) -> Result<CommittedAuthorizedToolAttemptV1, ToolAttemptPreDispatchErrorV1> {
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
        sandbox.ensure_available().await?;
        let host_permit = sandbox.acquire_host_capacity().await?;
        #[cfg(test)]
        let scheduler_permit =
            capacity_observer::activate(call_id, readiness, cancellation).await?;
        #[cfg(not(test))]
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
        let execution = input.authorized_operation.materialize_dispatch(
            attempt.tool_execution_attempt_id,
            &input.attempt_temp_directory,
            prepared_at,
        )?;
        let attempt_output_root = execution
            .binding
            .attempt_output_root
            .as_ref()
            .map(|path| std::path::PathBuf::from(&path.display));
        if execution.binding.tool_batch_id != input.admitted_batch.batch.tool_batch_id
            || execution.binding.tool_call_id != attempt.tool_call_id
            || execution.binding.tool_execution_attempt_id != attempt.tool_execution_attempt_id
            || execution.binding.execution_plan_hash
                != input.planned_batch.execution_plan.execution_plan_hash
        {
            return Err(ToolAttemptPreDispatchErrorV1::PlanMismatch);
        }
        let commit = persistence.commit_authorized_tool_execution_attempt_dispatch_prepared_v1(
            AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1 {
                attempt_commit: ToolExecutionAttemptDispatchPreparedCommitRequest {
                    attempt: attempt.clone(),
                    execution_plan_hash: input
                        .planned_batch
                        .execution_plan
                        .execution_plan_hash
                        .clone(),
                    dispatch_evidence_hash: execution.binding.binding_hash.clone(),
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
                sandbox_dispatch_binding: execution.binding.clone(),
            },
        )?;
        let dispatch_prepared_event_id = *commit
            .event_ids
            .last()
            .ok_or(ToolAttemptPreDispatchErrorV1::PersistenceEvidenceMissing)?;
        let projection_publish_error =
            persistence.publish_tool_projection_changes(&commit.projection_changes).err();
        Ok(CommittedAuthorizedToolAttemptV1 {
            tool_batch_id: input.admitted_batch.batch.tool_batch_id,
            attempt,
            execution,
            projection_state,
            persistence: commit,
            projection_publish_error,
            scheduler_permit,
            host_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id: input.correlation_id,
            result_context,
            attempt_output_root,
        })
    }
}
