use super::*;

pub(super) fn persist_web_execution(
    cancelled: bool,
    stop_confirmed: bool,
    ok: bool,
    attempt: aura_execution_contract::AttemptRef,
    evidence_ref: ArtifactId,
    completed_at: UtcTimestamp,
    postcondition_fingerprint: Option<aura_domain_common::jcs::CanonicalSha256HashV1>,
) -> aura_execution_contract::ExecutionResult {
    use aura_execution_contract::*;
    let (outcome, confirmed) = web_result_states(cancelled, stop_confirmed, ok);
    ExecutionResult {
        outcome,
        side_effect: SideEffectStateV1::NoEffect,
        termination: if confirmed {
            TerminationEvidence::Confirmed { evidence_ref }
        } else {
            TerminationEvidence::Unknown
        },
        completed_at,
        warnings: vec![],
        receipt: ExecutionReceipt {
            attempt,
            evidence_ref,
            postcondition_fingerprint,
            rollback: RollbackEvidence::NotApplicable,
        },
    }
}

pub(super) fn web_result_states(
    cancelled: bool,
    stop_confirmed: bool,
    ok: bool,
) -> (aura_execution_contract::ExecutionOutcome, bool) {
    use aura_execution_contract::ExecutionOutcome;
    if !stop_confirmed {
        (ExecutionOutcome::Unknown, false)
    } else if cancelled {
        (ExecutionOutcome::Cancelled, true)
    } else if ok {
        (ExecutionOutcome::Succeeded, true)
    } else {
        (ExecutionOutcome::Failed, true)
    }
}

impl ControlledWeb {
    pub async fn execute(
        &self,
        dispatch: &AuthorizedDispatch,
        service: &Arc<crate::agent_execution::AgentExecution>,
        call: &crate::agent_execution::CallContext,
    ) -> Result<serde_json::Value> {
        let request = dispatch.request();
        let identity = &request.operation.content.identity;
        let session = identity.session_id.ok_or_else(|| anyhow::anyhow!("web.session_required"))?;
        let run =
            identity.agent_run_id.ok_or_else(|| anyhow::anyhow!("web.agent_authority_required"))?;
        // Boundary: the claim is ONE indivisible ordered section - the session
        // permission authority first, then the journal, with the prepared-preparation
        // re-check, the dispatch authority, the dispatched mark and the activity
        // registration - and it runs whole on the bounded owner.
        let claim = {
            let web = self.clone();
            let request = request.clone();
            let identity = identity.clone();
            self.section(StoreStep::run(run, "agent.web.claim"), StoreAccess::Write, move || {
                web.persistence.with_session_permission_authority(
                    session,
                    |mode| -> Result<crate::activity::ActivityGuard> {
                        use aura_persistence_core::PersistenceCoreError as Error;
                        ensure!(
                            web.inspect(&request.operation)? == request.plan.prepared.preparation,
                            "web.preparation_changed"
                        );
                        web.journal.lock()?.database.unit_of_work(|tx| {
                            let now = UtcTimestamp::now();
                            let snapshot = aura_persistence_v2::security::snapshot(
                                tx,
                                identity.permission_snapshot_id.ok_or(Error::Conflict)?,
                            )?
                            .ok_or(Error::Conflict)?;
                            if snapshot.mode != mode.mode
                                || snapshot.session_permission_revision != mode.revision
                            {
                                return Err(Error::Conflict);
                            }
                            aura_persistence_v2::agent::checkpoint::verify_bound_operation(
                                tx,
                                request.operation.operation_id,
                            )?;
                            let _ = aura_persistence_v2::execution::dispatch_authority(
                                tx, &request, now,
                            )?;
                            let current = aura_persistence_v2::execution::load(
                                tx,
                                request.operation.operation_id,
                                request.binding.attempt.ordinal.get(),
                            )?;
                            if current.binding != request.binding {
                                return Err(Error::Conflict);
                            }
                            aura_persistence_v2::execution::mark_dispatched(
                                tx,
                                request.operation.operation_id,
                                request.binding.attempt.ordinal.get(),
                                now,
                            )
                        })?;
                        web.activities.register(
                            request.operation.operation_id,
                            session,
                            ActivityCapability::ControlledWeb,
                            mode.mode,
                        )
                    },
                )?
            })
            .await?
        };
        let guard = claim;
        let _execution_activity = crate::agent_activity::execution::observe_current_execution();
        // Reap the request, including any render Job, before this guard
        // acknowledges stop. The Provider has no entry in this registry.
        let finished = if request.operation.content.action_kind == "http.download" {
            self.invoke_download(dispatch, service, call, guard.cancel.clone()).await
        } else if request.operation.content.action_kind == "http.request" {
            self.invoke_http_with_policy(dispatch, call, guard.cancel.clone()).await
        } else if super::package::is_package_http(&request.operation.content) {
            self.invoke_package_http(dispatch, service, call, guard.cancel.clone()).await
        } else {
            self.invoke(dispatch, session, guard.cancel.clone()).await
        };
        let value = self
            .complete_section(
                run,
                request,
                finished.value,
                finished.cancelled,
                finished.stop_confirmed,
            )
            .await?;
        drop(guard);
        value
    }

    /// [`Self::complete`] on the bounded path: the terminal result and its evidence artifact
    /// are one write section, so the receipt is committed off the reactor. The caller's own
    /// result is handed back unchanged - the section does not reinterpret it.
    pub(super) async fn complete_section(
        &self,
        run: AgentRunId,
        request: &DispatchRequest,
        output: Result<serde_json::Value>,
        cancelled: bool,
        stop_confirmed: bool,
    ) -> Result<Result<serde_json::Value>> {
        let web = self.clone();
        let request = request.clone();
        self.section(StoreStep::run(run, "agent.web.complete"), StoreAccess::Write, move || {
            web.complete(&request, &output, cancelled, stop_confirmed)?;
            Ok(output)
        })
        .await
    }
    pub(super) fn complete(
        &self,
        request: &DispatchRequest,
        output: &Result<serde_json::Value>,
        cancelled: bool,
        stop_confirmed: bool,
    ) -> Result<()> {
        let id = ArtifactId::new();
        let now = UtcTimestamp::now();
        let fingerprint =
            output.as_ref().ok().map(aura_domain_common::jcs::canonical_sha256).transpose()?;
        let mut result = persist_web_execution(
            cancelled,
            stop_confirmed,
            output.is_ok(),
            request.binding.attempt.clone(),
            id,
            now,
            fingerprint,
        );
        let action = &request.operation.content;
        if stop_confirmed
            && output
                .as_ref()
                .err()
                .is_some_and(|error| error.to_string() == "tool.execution_timeout")
        {
            result.outcome = aura_execution_contract::ExecutionOutcome::TimedOut;
        }
        let package = super::package::is_package_http(action);
        let method = if package {
            action.action["request"]["method"].as_str()
        } else {
            action.action.get("method").and_then(serde_json::Value::as_str)
        };
        if (action.action_kind == "http.request" || package)
            && !(stop_confirmed
                && output.as_ref().err().is_some_and(super::proxy::request_not_started))
            && !matches!(
                method.unwrap_or("GET").to_ascii_uppercase().as_str(),
                "GET" | "HEAD" | "OPTIONS"
            )
        {
            // A dispatched mutating HTTP request cannot prove a remote application's effects.
            // In particular, a disconnect must never become NoEffect or automatic replay.
            result.side_effect = aura_execution_contract::SideEffectStateV1::Unknown;
            if output.is_err() {
                result.outcome = aura_execution_contract::ExecutionOutcome::Unknown;
            } else {
                result.warnings.push("tool.side_effect.unverified".into());
            }
        }
        if package
            && !(stop_confirmed
                && output.as_ref().err().is_some_and(super::proxy::request_not_started))
            && crate::agent_execution::package_tools::changed_during_call(
                self.journal.lock()?.database.connection(),
                action,
            )
        {
            result.warnings.push(crate::agent_execution::package_tools::CHANGED_DURING_CALL.into());
        }
        if action.action_kind == "http.download" {
            if !stop_confirmed {
                // A bounded Store wait can end while its filesystem worker still owns
                // publication. A completed caller wait is not proof of a stopped write.
                result.side_effect = aura_execution_contract::SideEffectStateV1::Unknown;
                result.outcome = aura_execution_contract::ExecutionOutcome::Unknown;
            } else if output.is_ok() {
                result.side_effect = aura_execution_contract::SideEffectStateV1::FullyApplied;
            } else if output
                .as_ref()
                .err()
                .is_some_and(|e| e.to_string() == "http.publication_rolled_back")
            {
                result.side_effect = aura_execution_contract::SideEffectStateV1::Compensated;
                result.receipt.rollback =
                    aura_execution_contract::RollbackEvidence::Completed { evidence_ref: id };
            } else if output
                .as_ref()
                .err()
                .is_some_and(|e| e.to_string().contains("http.publication_unconfirmed"))
            {
                result.side_effect = aura_execution_contract::SideEffectStateV1::Unknown;
                result.outcome = aura_execution_contract::ExecutionOutcome::Unknown;
            }
        }
        let mut evidence = serde_json::json!({
            "schema":"controlled-web-result-v1",
            "action":request.operation.content.action_kind,
            "output":output.as_ref().ok(),
            "error":output.as_ref().err().map(ToString::to_string),
            "cancelled":cancelled,
            "stop_confirmed":stop_confirmed,
            "request_future_dropped":false,
            "content_trust":"external_untrusted"
        });
        if let Some(route) = self.take_route(request.operation.operation_id) {
            evidence["network_route"] = route;
        }
        if let Some(rejection) = output
            .as_ref()
            .err()
            .and_then(|error| super::web_service::rejection(&error.to_string()))
        {
            evidence["operation_rejection"] = serde_json::to_value(rejection)?;
        }
        self.journal.lock()?.database.unit_of_work(|tx| {
            aura_persistence_core::artifact::put(
                tx,
                id,
                &request.operation.operation_id.to_string(),
                "application/vnd.aura.web-execution+json",
                &serde_json::to_vec(&evidence)?,
                now,
            )?;
            aura_persistence_v2::execution::complete(
                tx,
                request.operation.operation_id,
                request.binding.attempt.ordinal.get(),
                &result,
            )
        })?;
        Ok(())
    }
}
