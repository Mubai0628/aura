use super::*;
use aura_core_base::append_core_log;
use aura_execution_orchestrator::{DispatchCompletion, Selection};
use aura_security_contract::BackendClass;

struct StageLog {
    tool_call_id: String,
    tool_batch_id: String,
    agent_run_id: String,
    prepare_ms: u128,
    wait_ms: u128,
    activate_ms: u128,
    execute_ms: u128,
    commit_ms: u128,
    outcome: &'static str,
}

impl StageLog {
    fn begin(call: &CallContext) -> Self {
        Self {
            tool_call_id: call.validated.call.tool_call_id.to_string(),
            tool_batch_id: call.batch.tool_batch_id.to_string(),
            agent_run_id: call.batch.invocation_identity.agent_run_id.to_string(),
            prepare_ms: 0,
            wait_ms: 0,
            activate_ms: 0,
            execute_ms: 0,
            commit_ms: 0,
            outcome: "error",
        }
    }
}

impl StageLog {
    fn fields(&self) -> serde_json::Value {
        serde_json::json!({
            "tool_call_id": self.tool_call_id,
            "tool_batch_id": self.tool_batch_id,
            "agent_run_id": self.agent_run_id,
            "prepare_ms": self.prepare_ms as u64,
            "wait_ms": self.wait_ms as u64,
            "activate_ms": self.activate_ms as u64,
            "execute_ms": self.execute_ms as u64,
            "commit_ms": self.commit_ms as u64,
            "outcome": self.outcome,
        })
    }
}

impl Drop for StageLog {
    fn drop(&mut self) {
        append_core_log("tool.stage", self.fields());
    }
}

/// Writes prepare time on every exit from selection, including a `?` before a backend exists.
struct PrepareClock<'a> {
    stage: &'a mut StageLog,
    started: std::time::Instant,
}

impl Drop for PrepareClock<'_> {
    fn drop(&mut self) {
        self.stage.prepare_ms = self.started.elapsed().as_millis();
    }
}
pub enum CallOutcome {
    Terminal(Box<ToolResultV1>),
    ApprovalRequired {
        proposal: Box<AuthorizationProposal>,
        backend: Box<BackendDescriptor>,
    },
    Denied(PolicyDenyReason),
    /// The call was refused before a runner started. Nothing was written.
    NotStarted {
        message: String,
    },
    Unknown(AttemptRef),
}
/// One bounded record of the decision that produced, or refused, a call's backend.
///
/// The previous shape of this diagnostic was a single `runtime_unavailable` at the
/// end of the pipeline, which could not say which backends were considered, why
/// each was excluded, or whether the user was about to be asked to approve Native.
/// Everything below is already non-secret: backend IDs and classes are published in
/// readiness, reasons are bounded codes, and the operation identity is an
/// identifier, not the script. No path, script, token or credential is recorded.
struct SelectionDiagnostic {
    tool_call_id: String,
    agent_run_id: String,
    mode: String,
    native_offered: bool,
    /// `id:class:readiness`, one entry per candidate that was actually considered.
    candidates: Vec<String>,
    /// `id:class:backend.<kind>.<detail>`, one entry per excluded candidate.
    excluded: Vec<String>,
    /// The backend class that was finally selected, or the policy refusal.
    outcome: String,
    /// `approval`, `native`, or empty. Set before any user-facing prompt exists.
    approval: String,
}

impl SelectionDiagnostic {
    fn new(call: &CallContext, native_offered: bool) -> Self {
        Self {
            tool_call_id: call.validated.call.tool_call_id.to_string(),
            agent_run_id: call.batch.invocation_identity.agent_run_id.to_string(),
            mode: format!("{:?}", call.snapshot.mode),
            native_offered,
            candidates: Vec::new(),
            excluded: Vec::new(),
            outcome: String::new(),
            approval: String::new(),
        }
    }

    fn observe(&mut self, descriptor: &BackendDescriptor) {
        let readiness = match classify_readiness(&descriptor.readiness, true) {
            Some(failure) => failure.code(),
            None => "ready".to_owned(),
        };
        self.candidates.push(format!(
            "{}:{}:{}",
            descriptor.id.0,
            class_key(descriptor.class),
            readiness
        ));
        if let Some(failure) = classify_readiness(&descriptor.readiness, true) {
            self.excluded.push(format!(
                "{}:{}:{}",
                descriptor.id.0,
                class_key(descriptor.class),
                failure
            ));
        }
    }

    fn fields(&self) -> serde_json::Value {
        serde_json::json!({
            "tool_call_id": self.tool_call_id,
            "agent_run_id": self.agent_run_id,
            "mode": self.mode,
            "native_offered": self.native_offered,
            "candidates": self.candidates,
            "excluded": self.excluded,
            "outcome": self.outcome,
            "approval": self.approval,
        })
    }
}

impl Drop for SelectionDiagnostic {
    fn drop(&mut self) {
        append_core_log("tool.backend_selection", self.fields());
    }
}

fn class_key(class: BackendClass) -> &'static str {
    match class {
        BackendClass::CoreBroker => "core_broker",
        BackendClass::RestrictedUserOffline => "restricted_offline",
        BackendClass::RestrictedUserOnline => "restricted_online",
        BackendClass::Native => "native",
        BackendClass::AppContainer => "app_container",
        BackendClass::Container => "container",
    }
}

impl AgentExecution {
    pub async fn execute_call(
        self: &Arc<Self>,
        call: CallContext,
        plan_call: &ToolExecutionPlanCallV1,
        generation: aura_tool_scheduler::ToolInstanceExecutionGenerationKeyV1,
        candidates: &[Arc<dyn aura_backend_api::ExecutionBackend>],
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<CallOutcome> {
        // Global executing capacity is consumed inside the scheduler, at the one
        // point where a real runtime slot is activated. Preparation, approval and
        // resource waits therefore never occupy an execution slot.
        let (_lifetime, cancel) = self.active_calls.register(&call, cancel)?;
        let mut stage = StageLog::begin(&call);
        let prepare = PrepareClock { stage: &mut stage, started: std::time::Instant::now() };
        {
            // Boundary: the frozen-source re-derivation. `verify_source` takes the journal
            // lock directly, so its section runs whole on the one bounded Agent Store owner.
            let service = Arc::clone(self);
            let verified = call.clone();
            self.section(
                StoreStep::run(
                    call.batch.invocation_identity.agent_run_id,
                    "agent.dispatch.verify_source",
                ),
                StoreAccess::Read,
                move || service.verify_source(&verified),
            )
            .await?;
        }
        if let Some(message) = self.network_preflight_notice(&call).await {
            return Ok(CallOutcome::NotStarted { message });
        }
        let grants = {
            // Boundary: the grant read for this call's operation fingerprint.
            let service = Arc::clone(self);
            let fingerprint = call.operation.fingerprint.clone();
            let session_id = call.operation.content.identity.session_id;
            self.section(
                StoreStep::run(
                    call.batch.invocation_identity.agent_run_id,
                    "agent.dispatch.grants",
                ),
                StoreAccess::Read,
                move || {
                    Ok(aura_persistence_v2::security::grants_for_call(
                        service.journal.lock()?.database.connection(),
                        &fingerprint,
                        session_id,
                    )?)
                },
            )
            .await?
        };
        let observed_candidates = candidates
            .iter()
            .map(|backend| {
                Arc::new(crate::agent_activity::execution::ObservedBackend {
                    delegate: Arc::clone(backend),
                    service: Arc::clone(self),
                    call: call.clone(),
                }) as Arc<dyn aura_backend_api::ExecutionBackend>
            })
            .collect::<Vec<_>>();
        let candidates = observed_candidates.as_slice();
        let external_capabilities = self.external_authority(&call.operation);
        let external_grants = self.external_authority_slices(&call.operation);
        // A ReadOnly denial of an MCP server holds for the rest of the turn.
        let mcp_denied = if mcp_tools::is_mcp(&call.operation.content) {
            let service = Arc::clone(self);
            let operation = call.operation.clone();
            self.section(
                StoreStep::run(
                    call.batch.invocation_identity.agent_run_id,
                    "agent.dispatch.mcp_denied",
                ),
                StoreAccess::Read,
                move || mcp_tools::denied_for_turn(&service, &operation),
            )
            .await?
        } else {
            false
        };
        let mut policy = call.policy(UtcTimestamp::now(), self.runtime);
        policy.denied_for_turn = mcp_denied;
        policy.grants = &grants;
        policy.verified_external_capabilities = &external_capabilities;
        policy.verified_external_grants = &external_grants;
        let explicit = {
            // Boundary: the explicit-backend configuration read.
            let service = Arc::clone(self);
            let run_id = call.batch.invocation_identity.agent_run_id;
            self.section(
                StoreStep::run(run_id, "agent.dispatch.explicit_backend"),
                StoreAccess::Read,
                move || {
                    Ok(aura_persistence_v2::agent::application::configuration(
                        service.journal.lock()?.database.connection(),
                        run_id,
                    )
                    .ok()
                    .and_then(|config| config.request.explicit_backend_id))
                },
            )
            .await?
        }
        .filter(|_| {
            matches!(call.operation.content.action_kind.as_str(), "shell.powershell" | "shell.cmd")
        });
        // Preparation begins here, and it can be slow — backend selection and readiness
        // read and verify the installation — so the start is recorded per call before any
        // of that work. It is a timestamp, not an Attempt: no execution capacity is
        // claimed and nothing is implied to have been dispatched.
        // Boundary: the preparing mark, one bounded write.
        {
            let service = Arc::clone(self);
            let batch_id = call.batch.tool_batch_id;
            let call_id = call.validated.call.tool_call_id;
            let run_id = call.batch.invocation_identity.agent_run_id;
            self.section(
                StoreStep::run(run_id, "agent.dispatch.preparing_mark"),
                StoreAccess::Write,
                move || {
                    Ok(service.journal.lock()?.database.unit_of_work(|tx| {
                        aura_persistence_v2::tool::preparing::mark(
                            tx,
                            batch_id,
                            call_id,
                            UtcTimestamp::now(),
                        )
                    })?)
                },
            )
            .await?;
        }
        let native_present =
            candidates.iter().any(|backend| backend.descriptor().class == BackendClass::Native);
        let mut diagnostic = SelectionDiagnostic::new(&call, native_present);
        for backend in candidates {
            diagnostic.observe(&backend.descriptor());
        }
        let selected = match self.select_backend(&policy, candidates, explicit.as_deref()).await {
            Ok(value) => value,
            // An identity boundary is not "this backend is unavailable". Any of
            // the binding facts — a mismatch, or an identity that cannot be read
            // at all — means this Core is not the process the installation is
            // bound to, so the call stops with the existing explanation instead
            // of continuing into a broader backend. The classification below is
            // the same one the readiness card uses; the prefix test it replaces
            // silently let `core_binding_unreadable` fall through.
            Err(BackendError::PreparationRejected { reason })
                if classify_runner_code(&reason).kind.is_identity_boundary() =>
            {
                diagnostic.outcome = format!("denied:{}", classify_runner_code(&reason));
                prepare.stage.outcome = "denied";
                return Ok(CallOutcome::NotStarted {
                    message: aura_backend_windows_user::installation::InstallationBinding::core_binding_user_text(
                        &reason,
                    ),
                });
            }
            Err(error) => {
                diagnostic.outcome = format!("error:{error:?}");
                return Err(anyhow::anyhow!("{error:?}"));
            }
        };
        let selected = match selected {
            Selection::Ready(v) => {
                diagnostic.outcome =
                    format!("selected:{}:{}", v.descriptor().id.0, class_key(v.descriptor().class));
                if v.descriptor().class == BackendClass::Native {
                    diagnostic.approval = "native".into();
                }
                *v
            }
            Selection::ApprovalRequired { proposal, backend } => {
                // Whether the user is about to be asked matters for the
                // "why did this not just run" question, and it is known here
                // rather than after the Run pauses.
                diagnostic.outcome = format!("approval_required:{}", backend.id.0);
                diagnostic.approval =
                    if backend.class == BackendClass::Native { "native" } else { "restricted" }
                        .into();
                prepare.stage.outcome = "approval";
                return Ok(CallOutcome::ApprovalRequired { proposal, backend });
            }
            Selection::Denied { reason } => {
                diagnostic.outcome = format!("denied:{reason:?}");
                prepare.stage.outcome = "denied";
                return Ok(CallOutcome::Denied(reason));
            }
        };
        drop(diagnostic);
        drop(prepare);
        // Preparation, approval, scheduling and Attempt reservation create no running row.
        // ObservedBackend hands an empty lease to the actual file/Web/task worker. That
        // worker publishes after admission and keeps the lease through caller cancellation.
        // Shell observation is handed to the native/managed owner through CorePort instead.
        // The callback here is only the existing diagnostic reservation probe.
        let probe = self.activity_start_probe.try_lock().ok().and_then(|probe| probe.clone());
        let publish_on_start = move |attempt: &aura_execution_contract::AttemptRef| {
            if let Some(probe) = &probe {
                probe(attempt);
            }
        };
        let mut dispatch_stages = aura_execution_orchestrator::DispatchStages::default();
        let store = reservation::Reservation {
            service: self.clone(),
            call: call.clone(),
            cancel: cancel.clone(),
        };
        let finished = self
            .orchestrator
            .execute_observed(
                selected,
                aura_tool_scheduler::ToolScheduleRequestV1 {
                    plan_call,
                    generation,
                    source_lock_key: None,
                },
                &store,
                cancel,
                &mut dispatch_stages,
                &publish_on_start,
            )
            .await;
        // Copy the clocks before converting the error. An early return still
        // has to log the wait and activation it already measured.
        stage.wait_ms = dispatch_stages.wait_ms;
        stage.activate_ms = dispatch_stages.activate_ms;
        stage.execute_ms = dispatch_stages.execute_ms;
        let finished = finished.map_err(|e| anyhow::anyhow!("{e:?}"))?;
        // Every way this call can finish ends the activity it started. The end
        // names the exact identity it published, so a late duplicate or a stale message
        // from another generation can never remove a newer activity.
        let outcome = match finished {
            DispatchCompletion::Unknown { attempt }
                if matches!(
                    call.operation.content.action_kind.as_str(),
                    "shell.powershell" | "shell.cmd"
                ) =>
            {
                let commit_at = std::time::Instant::now();
                let committed: anyhow::Result<_> = {
                    // Boundary: the lost-shell terminal commit is one bounded write; the
                    // scheduler slot is released inside the same section, exactly as before.
                    let service = Arc::clone(self);
                    let attempt_for_release = attempt.clone();
                    let batch_id = call.batch.tool_batch_id;
                    let call_id = call.validated.call.tool_call_id;
                    let run_id = call.batch.invocation_identity.agent_run_id;
                    self.section(
                        StoreStep::run(run_id, "agent.dispatch.lost_shell"),
                        StoreAccess::Write,
                        move || {
                            let result = service.journal.lock()?.database.unit_of_work(|tx| {
                                aura_persistence_v2::execution::shell::lost(
                                    tx,
                                    attempt.operation_id,
                                    UtcTimestamp::now(),
                                )?;
                                aura_persistence_v2::tool::result::materialize(
                                    tx, batch_id, call_id,
                                )
                            })?;
                            service
                                .orchestrator
                                .release_managed_call(&attempt_for_release)
                                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                            Ok(result)
                        },
                    )
                    .await
                };
                stage.commit_ms = commit_at.elapsed().as_millis();
                let result = committed?;
                stage.outcome = "terminal";
                Ok(CallOutcome::Terminal(Box::new(result)))
            }
            DispatchCompletion::Unknown { attempt } => {
                stage.outcome = "unknown";
                Ok(CallOutcome::Unknown(attempt))
            }
            DispatchCompletion::Terminal(_) => {
                let commit_at = std::time::Instant::now();
                let committed: anyhow::Result<_> = {
                    // Boundary: materialising the committed terminal result is one bounded
                    // write over the same statement.
                    let service = Arc::clone(self);
                    let batch_id = call.batch.tool_batch_id;
                    let call_id = call.validated.call.tool_call_id;
                    let run_id = call.batch.invocation_identity.agent_run_id;
                    self.section(
                        StoreStep::run(run_id, "agent.dispatch.materialize"),
                        StoreAccess::Write,
                        move || {
                            let mut journal = service.journal.lock()?;
                            journal
                                .database
                                .unit_of_work(|tx| {
                                    aura_persistence_v2::tool::result::materialize(
                                        tx, batch_id, call_id,
                                    )
                                })
                                .map_err(|error| anyhow::anyhow!("{error:?}"))
                        },
                    )
                    .await
                };
                stage.commit_ms = commit_at.elapsed().as_millis();
                let result = committed?;
                stage.outcome = "terminal";
                Ok(CallOutcome::Terminal(Box::new(result)))
            }
        };
        // A caller cancel or timeout, and a foreground `Completed`, end **this call's**
        // activity and nothing else. A managed background process publishes its own
        // activity under its own owner and keeps it until that owner really stops. This
        // explicit end is the normal path; the guard's `Drop` covers the `?` returns above it
        // and any added later, so "no terminated call is left running" does not rest on a
        // future edit remembering to repeat this line.

        outcome
    }
}

#[cfg(test)]
mod stage_log_tests {
    use super::StageLog;

    #[test]
    fn a_failed_call_log_keeps_wait_apart_from_execution() {
        let root = tempfile::tempdir().expect("log");
        let path = root.path().join("core.jsonl");
        let stage = StageLog {
            tool_call_id: "call".into(),
            tool_batch_id: "batch".into(),
            agent_run_id: "run".into(),
            prepare_ms: 12,
            wait_ms: 4_000,
            activate_ms: 3,
            execute_ms: 0,
            commit_ms: 0,
            outcome: "error",
        };
        let stage = std::mem::ManuallyDrop::new(stage);
        let fields = stage.fields();
        aura_core_base::core_log::write_core_log_line(&path, "tool.stage", fields).expect("line");
        let text = std::fs::read_to_string(&path).expect("log");
        let entry: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
        assert_eq!(entry["component"], "core");
        assert_eq!(entry["event"], "tool.stage");
        let fields = &entry["fields"];
        assert_eq!(fields["outcome"], "error");
        assert_eq!(fields["prepare_ms"], 12);
        assert_eq!(fields["wait_ms"], 4_000);
        assert_eq!(fields["activate_ms"], 3);
        assert_eq!(fields["execute_ms"], 0);
        assert_eq!(fields["commit_ms"], 0);
        assert_eq!(fields["tool_call_id"], "call");
        assert_eq!(fields["tool_batch_id"], "batch");
        assert_eq!(fields["agent_run_id"], "run");
    }

    #[test]
    fn prepare_time_is_recorded_when_selection_returns() {
        let mut stage = std::mem::ManuallyDrop::new(StageLog {
            tool_call_id: "call".into(),
            tool_batch_id: "batch".into(),
            agent_run_id: "run".into(),
            prepare_ms: 0,
            wait_ms: 0,
            activate_ms: 0,
            execute_ms: 0,
            commit_ms: 0,
            outcome: "error",
        });
        let started = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(12))
            .expect("clock");
        {
            let _prepare = super::PrepareClock { stage: &mut stage, started };
        }
        assert!(stage.prepare_ms >= 12);
    }
}
