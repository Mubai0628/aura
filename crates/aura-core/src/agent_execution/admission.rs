//! The only accepted call mapping comes from persisted P05 request evidence.
//! This application never manufactures Provider identities or a Registry entry.
use super::*;
use aura_domain_common::jcs::canonical_sha256;
use aura_execution_orchestrator::rejection_results;
use aura_persistence_core::PersistenceCoreError as StoreError;
use aura_persistence_v2::{
    agent::{checkpoint, history, turns},
    model, tool,
};
use aura_tool_scheduler::*;
use std::collections::BTreeMap;

pub(crate) struct AdmittedBatch {
    pub batch: ToolBatchRequestV1,
    pub planned: PlannedToolBatchV1,
    pub snapshot: PermissionSnapshot,
    pub calls: BTreeMap<ToolCallId, CallContext>,
}
struct Authority<'a>(&'a crate::broker::BrokerJournal);
impl BatchAdmissionAuthorityV1 for Authority<'_> {
    fn verify_batch_admission(
        &self,
        batch: &ToolBatchRequestV1,
    ) -> Result<(), BatchAdmissionAuthorityIssueV1> {
        // This callback is synchronous and takes the journal directly. It is only correct
        // inside a bounded section; its single caller is `admit_output`, which runs its whole
        // synchronous body on the one Agent Store owner, so the dynamic path that reaches this
        // lock is a real blocking thread and never a reactive task.
        checkpoint::verify_batch(
            self.0
                .lock()
                .map_err(|_| BatchAdmissionAuthorityIssueV1::InvocationBoundaryInvalid)?
                .database
                .connection(),
            batch,
        )
        .map_err(|_| BatchAdmissionAuthorityIssueV1::ControlStateMismatch)
    }
}
impl AgentExecution {
    /// Admits the persisted P05 output into one executable Tool batch.
    ///
    /// Boundary: [`Self::admit_output_sync`] is ONE synchronous critical section - it reads
    /// the output, the snapshot, the evidence, the checkpoint, the registry and the execution call
    /// rows, freezes the permission snapshot, runs the batch admission authority (which itself
    /// verifies the checkpoint against the live database) and commits the admission in one
    /// transaction. It used to run on the reactive task that drives the run. It now runs **whole**
    /// on the one bounded Agent Store owner: the same journal, the same lock order (session
    /// permission authority, then journal), the same statements, and no split of an authority read
    /// from the commit that depends on it. This wrapper is the only production path to it.
    pub async fn admit_output(
        self: &Arc<Self>,
        step_id: ModelStepId,
        settings: ToolSettingsSnapshotV1,
    ) -> anyhow::Result<AdmittedBatch> {
        let service = Arc::clone(self);
        self.store
            .run_critical(
                StoreStep::operation("agent.tool.admission"),
                StoreAccess::Write,
                move || service.admit_output_sync(step_id, settings),
            )
            .await
    }

    /// The synchronous body of [`Self::admit_output`].
    ///
    /// Contract: this takes `journal.lock()` directly, so it is only correct on a real
    /// blocking thread. Production reaches it **only** through `admit_output`, which runs it inside
    /// one bounded section on the one Agent Store owner. The `agent_execution` test fixtures call it
    /// directly, on their own test thread, which is a real blocking thread and not a reactor worker;
    /// that is the only other caller and it does not exist in a product build.
    pub(super) fn admit_output_sync(
        &self,
        step_id: ModelStepId,
        settings: ToolSettingsSnapshotV1,
    ) -> anyhow::Result<AdmittedBatch> {
        let hook_blocks;
        let (batch, registry, mut safe_feedback) = {
            let store = self.journal.lock()?;
            let db = store.database.connection();
            let output = history::output(db, step_id)?;
            let snapshot = model::step_snapshot(db, step_id)?;
            let evidence = model::request_evidence(db, snapshot.id)?
                .ok_or_else(|| anyhow::anyhow!("agent.request_evidence_missing"))?;
            let boundary = checkpoint::load(db, step_id)?;
            let step = turns::step(db, step_id)?;
            let aura_agent_domain::ModelStepState::Completed {
                tool_batch_id: Some(batch_id), ..
            } = step.state
            else {
                anyhow::bail!("agent.no_tool_batch");
            };
            let registry =
                tool::registry(db, boundary.registry_snapshot_ref.tool_registry_snapshot_id)?
                    .ok_or(StoreError::Conflict)?;
            let mut calls = aura_model_domain::derive_p07_normalized_tool_calls_v1(
                &snapshot, &evidence, &output,
            )?;
            // Each `tools.call` becomes the call of the tool it names, so everything below
            // - Hooks, schema, permission, approval - is that tool's own.
            aura_persistence_v2::tool::entry::expand(db, &evidence, &mut calls)?;
            let mut feedback = unknown_tool_feedback(&evidence, &output);
            feedback.extend(super::unknown_tool::entry_feedback(&registry, &calls));
            // The kept `tool.before_call` decisions. Changed arguments are checked from the
            // start below; a blocked call is refused before anything about it is resolved.
            hook_blocks = aura_persistence_v2::hooks::apply_tool_call_decisions(db, &mut calls)?;
            (
                ToolBatchRequestV1::new(
                    batch_id,
                    boundary.identity,
                    boundary.state_revision,
                    boundary.state_hash,
                    registry.snapshot_ref.clone(),
                    calls,
                )?,
                registry,
                feedback,
            )
        };
        if tool::admission::load(self.journal.lock()?.database.connection(), batch.tool_batch_id)?
            .is_some()
        {
            return self.resume_batch(batch.tool_batch_id);
        }
        let snapshot = self.freeze_permission_sync(batch.invocation_identity.session_id)?;
        let release = aura_tool_registry::agent_release::release()?;
        // The installed package tools, derived now from the live install. A call
        // whose frozen definition no longer derives the same way is refused here.
        let package_tools =
            super::package_tools::installed_tools(self.journal.lock()?.database.connection())?;
        // The tools of usable MCP servers, derived now from the live configuration.
        let mcp_tools = match super::mcp_tools::runtime(self) {
            Ok(runtime) => {
                super::mcp_tools::usable_tools(&runtime, self.journal.lock()?.database.connection())
            }
            Err(_) => Vec::new(),
        };
        let definitions = release
            .manifest()
            .entries
            .iter()
            .map(|e| e.definition.clone())
            .chain(package_tools.iter().map(|tool| tool.definition.clone()))
            .chain(mcp_tools.iter().map(|tool| tool.definition.clone()))
            .collect();
        let admitted = match admit_tool_batch(
            batch.clone(),
            &registry,
            definitions,
            &Authority(&self.journal),
        )? {
            ToolBatchAdmissionOutcomeV1::IdentityIntegrityFault { code } => {
                anyhow::bail!("agent.batch_identity_fault:{code:?}")
            }
            ToolBatchAdmissionOutcomeV1::BatchRejected { calls, .. } => AdmittedToolBatchV1 {
                batch: batch.clone(),
                validated_calls: vec![],
                not_executed: calls,
            },
            ToolBatchAdmissionOutcomeV1::Admitted(value) => *value,
        };
        let mut contexts = BTreeMap::new();
        let mut planning = vec![];
        let authority = self.persistence.tool_authority_snapshot(
            Some(snapshot.workspace_id),
            Some(batch.invocation_identity.session_id),
            Some(batch.invocation_identity.turn_task_id),
        )?;
        for call in &admitted.validated_calls {
            // A package's or an MCP server's tool: its instance comes with it.
            let extension = package_tools
                .iter()
                .find(|tool| tool.definition.tool_id == call.definition.tool_id)
                .map(|tool| &tool.instance)
                .or_else(|| {
                    mcp_tools
                        .iter()
                        .find(|tool| tool.definition.tool_id == call.definition.tool_id)
                        .map(|tool| &tool.instance)
                });
            let (instance, handler) = match extension {
                Some(instance) => (instance, call.definition.canonical_name.as_str()),
                None => {
                    let entry = operation::entry(call)?;
                    (
                        authority
                            .instances
                            .iter()
                            .find(|instance| {
                                instance.tool_instance_id == call.binding.tool_instance_id
                                    && instance.tool_version == call.definition.tool_version
                                    && instance.source_contract_fingerprint
                                        == call.binding.source_contract_fingerprint
                            })
                            .unwrap_or(&entry.instance),
                        entry.handler.name(),
                    )
                }
            };
            if let Some((hook, reason)) = hook_blocks.get(&call.call.tool_call_id) {
                safe_feedback
                    .insert(call.call.tool_call_id, format!("被 Hook {hook} 阻止：{reason}"));
                planning.push(FrozenToolCallPlanningInputV1 {
                    tool_call_id: call.call.tool_call_id,
                    instance: instance.clone(),
                    resources: planning::hook_rejection(),
                    runtime_required_scopes: vec![],
                    resolver_additional_scopes: vec![],
                });
                continue;
            }
            let resolved = self.resolve_call(&batch, &registry, call, &snapshot);
            if matches!(handler, "git.sync" | "git.push")
                && let Err(error) = &resolved
                && let Some(message) = super::git::admission_feedback(error)
            {
                safe_feedback.insert(call.call.tool_call_id, message.into());
            }
            if handler == super::workspace_init::ACTION
                && let Err(error) = &resolved
                && let Some(message) = super::workspace_init::admission_feedback(error)
            {
                safe_feedback.insert(call.call.tool_call_id, message);
            }
            if super::command_tools::is_command_tool(handler)
                && let Err(error) = &resolved
                && let Some(message) = super::command_tools::admission_feedback(error)
            {
                safe_feedback.insert(call.call.tool_call_id, message);
            }
            if handler.starts_with("browser.")
                && let Err(error) = &resolved
                && let Some(message) = super::browser::admission_feedback(error)
            {
                safe_feedback.insert(call.call.tool_call_id, message);
            }
            let resources = match resolved {
                Ok(context) => match planning::resources(self, &context) {
                    Ok(resources) => {
                        contexts.insert(call.call.tool_call_id, context);
                        ToolCallResourcePreflightV1::Resolved(resources)
                    }
                    // A planning failure is about the call's resources, so the
                    // model is told which class of problem it has to correct
                    // instead of receiving one generic refusal.
                    Err(error) => {
                        // A Git call says what is wrong with the repository.
                        if handler.starts_with("git.")
                            && let Some(message) = super::git::admission_feedback(&error)
                        {
                            safe_feedback.insert(call.call.tool_call_id, message.into());
                        }
                        planning::rejection_with(
                            ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
                            planning::resource_error_code(&error),
                        )
                    }
                },
                Err(error) => planning::rejection_with(
                    ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
                    planning::resource_error_code(&error),
                ),
            };
            planning.push(FrozenToolCallPlanningInputV1 {
                tool_call_id: call.call.tool_call_id,
                instance: instance.clone(),
                resources,
                runtime_required_scopes: vec![],
                resolver_additional_scopes: vec![],
            });
        }
        let planned = build_tool_execution_plan_v1(&admitted, &registry, &settings, planning)?;
        contexts.retain(|id, _| planned.execution_plan.calls.iter().any(|c| c.tool_call_id == *id));
        let mut preflight = rejection_results(
            &planned.not_executed,
            UtcTimestamp::now(),
            CorrelationId::new(),
            None,
        )?;
        for result in &mut preflight {
            if let Some(message) = safe_feedback.get(&result.tool_call_id)
                && let ToolResultOutcomeV1::NotExecuted {
                    reason:
                        ToolNotExecutedReasonV1::ToolNotFound
                        | ToolNotExecutedReasonV1::ResourceClaimResolutionFailed
                        | ToolNotExecutedReasonV1::PermissionDenied,
                    error: Some(error),
                } = &mut result.outcome
            {
                error.message = message.clone();
            }
        }
        let value = tool::execution::BatchExecution {
            batch: batch.clone(),
            plan: planned.execution_plan.clone(),
            admitted_at: UtcTimestamp::now(),
            result: None,
        };
        self.journal.lock()?.database.unit_of_work(|tx| {
            // A changed checkpoint becomes a paired rejected batch only. A
            // successfully planned batch rechecks at the atomic commit point.
            if !planned.execution_plan.calls.is_empty() {
                checkpoint::verify_batch(tx, &batch)?;
            }
            for entry in &release.manifest().entries {
                tool::definitions::put(tx, &entry.definition, &entry.instance)?;
            }
            for tool in package_tools.iter().filter(|tool| {
                admitted
                    .validated_calls
                    .iter()
                    .any(|call| call.definition.tool_id == tool.definition.tool_id)
            }) {
                tool::definitions::put(tx, &tool.definition, &tool.instance)?;
            }
            for tool in mcp_tools.iter().filter(|tool| {
                admitted
                    .validated_calls
                    .iter()
                    .any(|call| call.definition.tool_id == tool.definition.tool_id)
            }) {
                tool::definitions::put(tx, &tool.definition, &tool.instance)?;
            }
            tool::execution::admit(tx, &value, &registry, &preflight)?;
            tool::admission::record(
                tx,
                &batch,
                &snapshot,
                &settings,
                contexts.iter().map(|(id, v)| (*id, v.operation.clone())).collect(),
            )
        })?;
        // R3: the resource references this task really named, recorded from the **verified**
        // descriptors on the operations A just admitted. Nothing is re-resolved, and this happens
        // at admission so a later model step can associate another writer of a file this task
        // operated on even after this task's own activity row has ended.
        for call in contexts.values() {
            self.record_task_resources(call);
        }
        Ok(AdmittedBatch { batch, planned, snapshot, calls: contexts })
    }
}
// This hash is used only for scheduler resource evidence, never as permission.
pub(super) fn evidence(
    value: impl serde::Serialize,
) -> anyhow::Result<aura_domain_common::jcs::CanonicalHashV1> {
    Ok(canonical_sha256(&value)?)
}
