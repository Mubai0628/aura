use super::*;

impl CommittedAuthorizedToolBuiltinAttemptV1 {
    /// Consumes the exact P04 binding, advances the real Attempt to Running,
    /// and only then dispatches the closed signed handler enum. A failure after
    /// binding consumption remains fail-closed and cannot replay the handler.
    /// The scheduler permit remains held through result persistence.
    pub(crate) fn dispatch(
        self,
        persistence: &PersistenceCoordinator,
        invoked_at: UtcTimestamp,
        // R4: the observation context, when this dispatch was assembled inside the real runtime.
        // It carries the process's one activity index, so the model-executable query reads the same
        // facts every other publisher writes.
        observation: Option<crate::p07_builtin::BuiltinActivityObservationV1>,
    ) -> Result<DispatchedAuthorizedBuiltinAttemptV1, ToolAttemptDispatchErrorV1> {
        let Self {
            tool_batch_id,
            mut attempt,
            binding,
            arguments,
            registry,
            registry_build_request,
            mut projection_state,
            persistence: pre_dispatch_persistence,
            projection_publish_error,
            scheduler_permit,
            result_contract,
            execution_semantics,
            timeout_ms,
            max_retries,
            dispatch_prepared_event_id,
            correlation_id,
            result_context,
        } = self;
        let consumed =
            persistence.consume_authorized_builtin_core_handler_dispatch_v1(binding, invoked_at)?;
        let updated_at = UtcTimestamp::now();
        let mut running_attempt = attempt.clone();
        running_attempt.status = ToolExecutionAttemptStatusV1::Running;
        running_attempt.side_effect_state = SideEffectStateV1::Unknown;
        let mut running_projection_state = projection_state.clone();
        let attempt_projection = running_projection_state.runtime_started(&running_attempt)?;
        let runtime_started_persistence = persistence
            .commit_tool_execution_attempt_runtime_started(
                ToolExecutionAttemptRuntimeStartedRequest {
                    attempt: running_attempt.clone(),
                    attempt_projection,
                    updated_at,
                },
            )?;
        let runtime_started_publish_error = persistence
            .publish_tool_projection_changes(&runtime_started_persistence.projection_changes)
            .err();
        attempt = running_attempt;
        projection_state = running_projection_state;

        let invoked = invoke_builtin_core_handler_v1(
            persistence,
            &consumed,
            &arguments,
            &registry,
            registry_build_request,
            correlation_id,
            invoked_at,
            // R4: the observation context this dispatch was assembled with. It is the process's own
            // activity index and runtime instance, so the query handler reads exactly the index the
            // real executors publish into rather than a second one.
            observation.as_ref(),
        )?;
        let mut projection_publish_errors =
            projection_publish_error.into_iter().collect::<Vec<_>>();
        projection_publish_errors.extend(runtime_started_publish_error);
        Ok(DispatchedAuthorizedBuiltinAttemptV1 {
            dispatched: DispatchedAuthorizedToolAttemptV1 {
                tool_batch_id,
                attempt,
                projection_state,
                evidence: ToolRuntimeDispatchEvidenceV1::BuiltinCoreHandler(invoked.evidence),
                pre_dispatch_persistence,
                runtime_started_persistence: Some(runtime_started_persistence),
                progress_persistence: Vec::new(),
                projection_publish_errors,
                runtime_started_error: None,
                scheduler_permit,
                result_contract,
                execution_semantics,
                timeout_ms,
                max_retries,
                dispatch_prepared_event_id,
                correlation_id,
                result_context,
            },
            attempt_side_effect: invoked.attempt_side_effect,
            call_side_effect_evidence: invoked.call_side_effect_evidence,
        })
    }
}
