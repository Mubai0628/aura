use aura_agent_domain::*;
use aura_domain_common::{
    jcs::{CanonicalHashV1, canonical_sha256},
    *,
};
use aura_execution_contract::*;
use aura_model_domain::{
    NormalizedModelOutput, NormalizedToolCall, normalize_provider_tool_call_input_v1,
};
use aura_persistence_core::{StoreResult, artifact, operation};
use aura_persistence_v2::{
    V2Store,
    agent::{lifecycle, turns},
    context, execution, model, security,
    tool::execution as tools,
    workspace,
};
use aura_tool_contract::*;
use rusqlite::Transaction;

pub struct Fixture {
    pub turn: TurnTask,
    pub step: ModelStep,
    pub at: UtcTimestamp,
    pub template: NormalizedModelOutput,
}
impl Fixture {
    pub fn new(store: &mut V2Store, mode: AgentTaskMode) -> Self {
        Self::start(store, mode, true, "C:/synthetic", false)
    }
    // Each integration-test binary imports only the fixture paths it exercises.
    /// A second running Session in the same store: its own Workspace and execution snapshot.
    #[allow(dead_code)]
    pub fn new_in_workspace(store: &mut V2Store, mode: AgentTaskMode, root: &str) -> Self {
        Self::start(store, mode, true, root, true)
    }
    #[allow(dead_code)]
    pub fn preflight(store: &mut V2Store, mode: AgentTaskMode) -> Self {
        Self::start(store, mode, false, "C:/synthetic", false)
    }
    #[allow(dead_code)]
    pub fn preflight_in_workspace(store: &mut V2Store, mode: AgentTaskMode, root: &str) -> Self {
        Self::start(store, mode, false, root, false)
    }
    fn start(
        store: &mut V2Store,
        mode: AgentTaskMode,
        start_model: bool,
        root: &str,
        own_snapshot: bool,
    ) -> Self {
        let canonical = aura_contract_schema::canonical_fixture().expect("contract fixtures");
        let p06 = aura_context_fixtures::p06_prepared_context_fixture().expect("P06 fixture");
        let at = "2026-09-07T00:00:00Z".parse().expect("time");
        let session = SessionId::new();
        let wid = WorkspaceId::new();
        let user = OriginalUserMessage::new(
            EventId::new(),
            session,
            "Verify the requested work".into(),
            UserMessageOrigin::UserInput,
            at,
        )
        .expect("user");
        let (turn, step) = store
            .database
            .unit_of_work(|tx| -> StoreResult<_> {
                workspace::insert(
                    tx,
                    &aura_workspace_domain::WorkspaceSummary {
                        workspace_id: wid,
                        display_name: "Synthetic".into(),
                        selected_root: root.into(),
                        canonical_root: root.into(),
                        root_identity: None,
                        state: aura_workspace_domain::WorkspaceState::Offline,
                        revision: Revision::INITIAL,
                        catalog_freshness: aura_workspace_domain::CatalogFreshness::Unavailable,
                        authorization_epoch: Revision::INITIAL,
                    },
                )?;
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id: session,
                        workspace_id: wid,
                        title: "Synthetic".into(),
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                let bundle = &p06.prompt_bundle_snapshot;
                context::insert_bundle(tx, session, bundle)?;
                let budget = (mode == AgentTaskMode::Goal)
                    .then_some(GoalBudget { max_tokens: None, max_turns: None });
                let turn = lifecycle::start(tx, &user, mode, budget, bundle.id)?;
                let step = lifecycle::prepare_step(
                    tx,
                    turn.agent_run_id,
                    AgentContinuation::Initial { user_event_id: user.event_id },
                    at,
                )?;
                if !start_model {
                    return Ok((turns::load(tx, turn.turn_task_id)?, step));
                }
                let mut snapshot = canonical.models.execution_snapshot.clone().expect("snapshot");
                if own_snapshot {
                    snapshot.id = ModelExecutionSnapshotId::new();
                }
                snapshot.model_step_id = step.model_step_id;
                snapshot.prompt_bundle_snapshot_id = bundle.id;
                model::insert_snapshot(tx, session, &snapshot)?;
                let mut started = step.clone();
                started.state = ModelStepState::Running { execution_snapshot_id: snapshot.id };
                turns::save_step(tx, &step, &started)?;
                let run = turns::load_run(tx, turn.agent_run_id)?;
                let mut running = run.clone();
                running.state = AgentRunState::ModelRunning { model_step_id: step.model_step_id };
                running.revision = Revision::new(2);
                turns::save_run(tx, &run, &running)?;
                Ok((turns::load(tx, turn.turn_task_id)?, started))
            })
            .expect("real SQLite Agent start");
        Self { turn, step, at, template: canonical.models.output.expect("model output fixture") }
    }
    #[allow(dead_code)]
    pub fn admit(
        &self,
        store: &mut V2Store,
        calls: Vec<(&str, serde_json::Value)>,
    ) -> tools::BatchExecution {
        let mut output = self.template.clone();
        output.model_step_id = self.step.model_step_id;
        output.tool_calls = calls.iter().enumerate().map(|(i, (name, args))| NormalizedToolCall {
            tool_call_id: ToolCallId::new(), provider_call_id: format!("call_{i}").parse().expect("provider ID"), tool_name: (*name).into(), position: i as u32,
            arguments_json: serde_json::json!({"_aura":{"purpose":"Verify requested work"},"arguments":args}).to_string(),
        }).collect();
        // Synthetic catalog tests Store authority/atomicity, not signed-release
        // admission or production Provider transport (covered at those owners).
        let entries = calls
            .iter()
            .map(|(name, _)| ToolRegistryCatalogEntryV1 {
                tool_id: ToolId::new(),
                tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
                tool_instance_id: ToolInstanceId::new(),
                canonical_name: (*name).parse().expect("name"),
                display_name: (*name).into(),
                summary: "Synthetic".into(),
                source: ToolSourceKindV1::BuiltIn,
                definition_fingerprint: CanonicalHashV1::sha256(name.as_bytes()),
                source_contract_fingerprint: ToolSourceContractFingerprintV1::BuiltIn {
                    signed_builtin_tool_contract_fingerprint:
                        SignedBuiltinToolContractFingerprintV1::new(CanonicalHashV1::sha256(
                            name.as_bytes(),
                        )),
                },
                load_state: ToolLoadStateV1::Discoverable,
            })
            .collect::<Vec<_>>();
        let registry = ToolRegistrySnapshotV1::new(
            ToolRegistrySnapshotId::new(),
            Revision::INITIAL,
            ToolRegistryScopeV1::Workspace { workspace_id: self.turn.workspace_id },
            None,
            None,
            CanonicalHashV1::sha256(b"synthetic eligibility"),
            vec![],
            {
                let mut catalog = entries.clone();
                catalog.sort_by_key(|entry| entry.tool_id);
                catalog
            },
            vec![],
            self.at,
        )
        .expect("registry");
        for (call, entry) in output.tool_calls.iter_mut().zip(&entries) {
            call.tool_name = aura_model_domain::provider_wire_tool_name(
                entry.canonical_name.as_str(),
                entry.tool_id,
            );
        }
        let snapshot = model::step_snapshot(store.database.connection(), self.step.model_step_id)
            .expect("snapshot");
        let mut request = aura_contract_schema::canonical_fixture()
            .expect("fixture")
            .models
            .provider_request_evidence
            .expect("request");
        request.adapter.api_format = snapshot.api_format;
        request.compatibility_removals.clear();
        request.historical_tool_call_wire_bindings.clear();
        request.operation_context =
            aura_model_domain::TransportOperationContext::AgentModelAttempt {
                model_step_id: snapshot.model_step_id,
                model_execution_snapshot_id: snapshot.id,
                context_snapshot_id: snapshot.context_snapshot_id,
                prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
            };
        request.tool_registry_snapshot_ref = Some(registry.snapshot_ref.clone());
        request.tool_name_bindings = entries
            .iter()
            .map(|entry| aura_model_domain::ProviderToolNameBinding {
                tool_definition: ToolDefinitionSnapshotRef {
                    tool_id: entry.tool_id,
                    tool_version: entry.tool_version.clone(),
                    registry_snapshot_ref: registry.snapshot_ref.clone(),
                },
                provider_wire_name: aura_model_domain::provider_wire_tool_name(
                    entry.canonical_name.as_str(),
                    entry.tool_id,
                ),
                canonical_name: entry.canonical_name.to_string(),
                package_schema_hash: CanonicalHashV1::sha256(b"synthetic schema"),
                provider_wrapped_schema_hash: CanonicalHashV1::sha256(b"synthetic wrapped schema"),
                provider_arguments_envelope_version:
                    aura_model_domain::PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V1,
            })
            .collect();
        request.tool_name_bindings.sort_by_key(|v| v.tool_definition.tool_id);
        store
            .database
            .unit_of_work(|tx| model::record_request(tx, snapshot.id, &request, self.at))
            .expect("compiled request evidence");
        let batch = ToolBatchRequestV1::new(
            ToolBatchId::new(),
            CoreInvocationIdentityV1 {
                command_invocation_id: CommandInvocationId::new(),
                session_id: self.turn.session_id,
                turn_task_id: self.turn.turn_task_id,
                agent_run_id: self.turn.agent_run_id,
                model_step_id: self.step.model_step_id,
            },
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"checkpoint"),
            registry.snapshot_ref.clone(),
            output
                .tool_calls
                .iter()
                .zip(&entries)
                .map(|(p, e)| {
                    let input = normalize_provider_tool_call_input_v1(&p.arguments_json);
                    NormalizedToolCallV1 {
                        tool_call_id: p.tool_call_id,
                        ordinal: p.position,
                        definition_ref: Some(ToolDefinitionSnapshotRef {
                            tool_id: e.tool_id,
                            tool_version: e.tool_version.clone(),
                            registry_snapshot_ref: registry.snapshot_ref.clone(),
                        }),
                        input: input.input,
                        provider_arguments_envelope_hash: input.provider_arguments_envelope_hash,
                    }
                })
                .collect(),
        )
        .expect("batch");
        let settings = Self::settings();
        let plan = ToolExecutionPlanV1::new(
            batch.tool_batch_id,
            registry.snapshot_ref.clone(),
            settings.revision,
            settings.content_hash.clone(),
            batch
                .calls
                .iter()
                .map(|c| ToolExecutionPlanCallV1 {
                    tool_call_id: c.tool_call_id,
                    ordinal: c.ordinal,
                    dependency_tool_call_ids: vec![],
                    execution_atomic_group: None,
                    resolved_resource_claims: vec![],
                    timeout_ms: 10_000,
                    max_retries: 0,
                    local_concurrency_limit: Some(1),
                    entity_revision: Revision::INITIAL,
                })
                .collect(),
        )
        .expect("plan");
        let stored = tools::BatchExecution { batch, plan, admitted_at: self.at, result: None };
        store
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::agent::history::record_output(
                    tx,
                    &output,
                    Some(stored.batch.tool_batch_id),
                    self.at,
                )?;
                tools::admit(tx, &stored, &registry, &[])?;
                let run = turns::load_run(tx, self.turn.agent_run_id)?;
                let mut next = run.clone();
                next.state =
                    AgentRunState::ToolsRunning { tool_batch_id: stored.batch.tool_batch_id };
                next.revision = Revision::new(run.revision.value() + 1);
                turns::save_run(tx, &run, &next)
            })
            .expect("durable model output and admission");
        stored
    }
    /// The tool settings every fixture batch is planned and admitted against.
    ///
    /// A settings snapshot is self-validating: its content hash is computed from its own
    /// fields, so a batch planned against a made-up hash produces an admission record that
    /// cannot be read back. One definition keeps the plan and the record consistent.
    fn settings() -> ToolSettingsSnapshotV1 {
        ToolSettingsSnapshotV1::new(Revision::INITIAL, 8, 1, 10_000, 0)
            .expect("fixture tool settings")
    }

    /// Records one call's admission, with the operation it was admitted as.
    ///
    /// A real admission stores every call of the batch at once and names the operation each
    /// one became, plus the permission snapshot and tool settings the plan was built against.
    /// The projection resolves a claim through that record, so writing it is what makes a
    /// claimed call visible as running.
    #[allow(dead_code)]
    fn record_admission(
        &self,
        tx: &Transaction<'_>,
        stored: &tools::BatchExecution,
        call_id: &ToolCallId,
        operation: &SecuredOperation,
    ) -> StoreResult<()> {
        if aura_persistence_v2::tool::admission::load(tx, stored.batch.tool_batch_id)?.is_some() {
            return Ok(());
        }
        // The stored snapshots carry their revision inside the record, so the next
        // revision is read from the records themselves rather than from a column.
        let mut highest = 0_u64;
        {
            let mut statement =
                tx.prepare("SELECT payload FROM permission_snapshots WHERE session_id=?1")?;
            let rows = statement
                .query_map([self.turn.session_id.to_string()], |row| row.get::<_, String>(0))?;
            for row in rows {
                let stored: aura_security_contract::PermissionSnapshot =
                    aura_persistence_core::record::decode(&row?)?;
                highest = highest.max(stored.session_permission_revision);
            }
        }
        let snapshot = aura_security_contract::PermissionSnapshot {
            id: PermissionSnapshotId::new(),
            session_id: self.turn.session_id,
            workspace_id: self.turn.workspace_id,
            mode: aura_security_contract::PermissionMode::WorkspaceReadWrite,
            session_permission_revision: highest.saturating_add(1),
            active_grant_ids: vec![],
            created_at: self.at,
        };
        security::insert_snapshot(tx, &snapshot)?;
        let settings = Self::settings();
        // An admitted operation names the permission snapshot it was authorized under, so the
        // record carries the operation as it really was — rebuilt with that snapshot, keeping
        // its identity so the Attempt stays the same call.
        let mut content = operation.content.clone();
        content.identity.permission_snapshot_id = Some(snapshot.id);
        let admitted_operation = SecuredOperation::new(operation.operation_id, content)
            .map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)?;
        let mut operations = std::collections::BTreeMap::new();
        for call in &stored.plan.calls {
            if &call.tool_call_id == call_id {
                operations.insert(call.tool_call_id, admitted_operation.clone());
            }
        }
        if operations.len() != stored.plan.calls.len() {
            // A multi-call batch needs every operation at once; this fixture builds one per
            // call, so it records the admission when the single call it knows is the batch.
            return Ok(());
        }
        aura_persistence_v2::tool::admission::record(
            tx,
            &stored.batch,
            &snapshot,
            &settings,
            operations,
        )
    }

    #[allow(dead_code)]
    pub fn begin_execution(
        &self,
        tx: &Transaction<'_>,
        stored: &tools::BatchExecution,
        index: usize,
        action: Option<(&str, serde_json::Value)>,
    ) -> StoreResult<DispatchRequest> {
        let request = self.reserve_execution(tx, stored, index, action)?;
        execution::mark_dispatched(tx, request.binding.attempt.operation_id, 1, self.at)?;
        Ok(request)
    }
    /// The call's attempt, reserved and bound but not yet started.
    #[allow(dead_code)]
    pub fn reserve_execution(
        &self,
        tx: &Transaction<'_>,
        stored: &tools::BatchExecution,
        index: usize,
        action: Option<(&str, serde_json::Value)>,
    ) -> StoreResult<DispatchRequest> {
        let call = &stored.batch.calls[index];
        let mut f = aura_contract_schema::v2_kernel_fixture().expect("kernel fixture");
        let op = SecurityOperationId::new();
        let mut content = f.dispatch.operation.content.clone();
        content.identity.actor = aura_security_contract::InvocationActor::ParentAgent;
        content.identity.session_id = Some(self.turn.session_id);
        content.identity.turn_task_id = Some(self.turn.turn_task_id);
        content.identity.agent_run_id = Some(self.turn.agent_run_id);
        content.identity.workspace_id = Some(self.turn.workspace_id);
        if let Some((kind, action)) = action {
            content.action_kind = kind.into();
            content.action = action;
        }
        f.dispatch.operation = SecuredOperation::new(op, content).expect("operation");
        f.dispatch.plan.prepared.operation_fingerprint = f.dispatch.operation.fingerprint.clone();
        f.dispatch.plan = ExecutionPlan::new(f.dispatch.plan.prepared).expect("plan");
        f.grant.grant_id = PermissionGrantId::new();
        f.grant.scope.operation_fingerprint = f.dispatch.operation.fingerprint.clone();
        f.grant.scope.lifetime =
            aura_security_contract::AuthorizationLifetime::Once { operation_id: op };
        f.authority = aura_security_contract::AuthorizationDecision::from_grant(
            &f.grant,
            self.at,
            f.authority.expires_at,
        )
        .expect("authority");
        f.dispatch.binding.attempt.operation_id = op;
        f.dispatch.binding.operation_fingerprint = f.dispatch.operation.fingerprint.clone();
        f.dispatch.binding.authorization_fingerprint =
            f.authority.fingerprint().expect("fingerprint");
        f.dispatch.binding.plan_fingerprint = f.dispatch.plan.fingerprint.clone();
        let authorized = AuthorizedDispatch::revalidate(
            f.dispatch.clone(),
            &f.authority,
            &f.dispatch.plan.prepared.backend,
            &f.cancellation,
            self.at,
        )
        .expect("dispatch");
        operation::begin(
            tx,
            &operation::OperationDraft {
                id: op,
                domain: "tool",
                kind: "synthetic.control",
                request_key: &call.tool_call_id.to_string(),
                version: 1,
                payload: &serde_json::to_value(&f.dispatch.operation)?,
                at: self.at,
            },
        )?;
        security::insert_grant(tx, &f.grant)?;
        // These fixtures carry an exact-operation Grant and no reusable Native
        // session consent, so there is no session permission revision for the
        // reservation to compare against.
        execution::prepare(tx, &authorized, &f.authority, 0, self.at)?;
        // A real admission records which operation each call was admitted as. The
        // projection resolves the Attempt through that record, so a batch that is stored
        // without it cannot show a claimed call as running — which is exactly the gap the
        // ignored Running test described. It is written here, once per batch, because an
        // admission covers every call of the batch at the same time.
        self.record_admission(tx, stored, &call.tool_call_id, &f.dispatch.operation)?;
        let attempt_id = ToolExecutionAttemptId::new();
        tools::bind_attempt(
            tx,
            &tools::CallReservation {
                batch: stored.batch.clone(),
                call_id: call.tool_call_id,
                attempt_id,
            },
            &authorized,
        )?;
        Ok(f.dispatch)
    }
    #[allow(dead_code)]
    pub fn success(
        &self,
        tx: &Transaction<'_>,
        stored: &tools::BatchExecution,
        index: usize,
    ) -> StoreResult<()> {
        let dispatch = self.begin_execution(tx, stored, index, None)?;
        let op = dispatch.operation.operation_id;
        let call = &stored.batch.calls[index];
        let attempt_id = tools::call(tx, call.tool_call_id)?.attempt_id.expect("bound attempt");
        let evidence = ArtifactId::new();
        artifact::put(
            tx,
            evidence,
            &op.to_string(),
            "application/json",
            b"{\"synthetic_result\":true}",
            self.at,
        )?;
        let result = ExecutionResult {
            outcome: ExecutionOutcome::Succeeded,
            side_effect: SideEffectStateV1::NoEffect,
            receipt: ExecutionReceipt {
                attempt: dispatch.binding.attempt,
                evidence_ref: evidence,
                postcondition_fingerprint: Some(
                    canonical_sha256(&call.input).expect("postcondition"),
                ),
                rollback: RollbackEvidence::NotApplicable,
            },
            termination: TerminationEvidence::Confirmed { evidence_ref: evidence },
            completed_at: self.at,
            warnings: vec![],
        };
        execution::complete(tx, op, 1, &result)?;
        tools::complete_call(
            tx,
            stored.batch.tool_batch_id,
            &ToolResultV1 {
                tool_call_id: call.tool_call_id,
                outcome: ToolResultOutcomeV1::Succeeded,
                side_effect_state: result.side_effect,
                attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
                    tool_execution_attempt_id: attempt_id,
                    attempt_number: 1,
                    side_effect_state: result.side_effect,
                    side_effect_evidence_ref: Some(evidence),
                    dispatched: true,
                }],
                model_visible_result_ref: None,
                user_audit_result_ref: None,
                internal_diagnostic_ref: None,
                aggregate_side_effect_evidence_ref: Some(evidence),
                started_at: Some(self.at),
                completed_at: self.at,
            },
        )
    }
}
