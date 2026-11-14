use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::*;
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, operation};
use aura_persistence_v2::{V2Store, execution, security, tool::execution as tools, workspace};
use aura_tool_contract::*;

struct Fixture {
    registry: ToolRegistrySnapshotV1,
    batch: tools::BatchExecution,
    kernel: aura_contract_schema::V2KernelFixture,
}
impl Fixture {
    fn new() -> Self {
        let mut kernel = aura_contract_schema::v2_kernel_fixture().expect("kernel fixture");
        let identity = CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::new(),
            session_id: SessionId::new(),
            turn_task_id: TurnTaskId::new(),
            agent_run_id: AgentRunId::new(),
            model_step_id: ModelStepId::new(),
        };
        // Registry/schema admission has separate tests; these fixtures exercise
        // the Store transaction boundary after that admission has succeeded.
        let registry = ToolRegistrySnapshotV1::new(
            ToolRegistrySnapshotId::new(),
            Revision::INITIAL,
            ToolRegistryScopeV1::Workspace {
                workspace_id: kernel
                    .dispatch
                    .operation
                    .content
                    .identity
                    .workspace_id
                    .expect("workspace"),
            },
            None,
            None,
            CanonicalHashV1::sha256(b"eligibility"),
            vec![],
            vec![],
            vec![],
            kernel.authority.decided_at,
        )
        .expect("registry");
        let calls = (1..=2)
            .map(|ordinal| NormalizedToolCallV1 {
                tool_call_id: ToolCallId::new(),
                ordinal,
                definition_ref: Some(ToolDefinitionSnapshotRef {
                    tool_id: ToolId::new(),
                    tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
                    registry_snapshot_ref: registry.snapshot_ref.clone(),
                }),
                input: NormalizedToolCallInputStateV1::Ready {
                    purpose: "Read fixture".parse().expect("purpose"),
                    arguments: serde_json::json!({}),
                    arguments_hash: canonical_sha256(&serde_json::json!({})).expect("hash"),
                },
                provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"envelope"),
            })
            .collect::<Vec<_>>();
        let batch = ToolBatchRequestV1::new(
            ToolBatchId::new(),
            identity.clone(),
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"agent checkpoint"),
            registry.snapshot_ref.clone(),
            calls,
        )
        .expect("batch");
        let plan = ToolExecutionPlanV1::new(
            batch.tool_batch_id,
            registry.snapshot_ref.clone(),
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"settings"),
            batch
                .calls
                .iter()
                .map(|call| ToolExecutionPlanCallV1 {
                    tool_call_id: call.tool_call_id,
                    ordinal: call.ordinal,
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
        let mut content = kernel.dispatch.operation.content.clone();
        content.identity.actor = aura_security_contract::InvocationActor::ParentAgent;
        content.identity.session_id = Some(identity.session_id);
        content.identity.turn_task_id = Some(identity.turn_task_id);
        content.identity.agent_run_id = Some(identity.agent_run_id);
        kernel.dispatch.operation =
            SecuredOperation::new(kernel.dispatch.operation.operation_id, content)
                .expect("operation");
        kernel.dispatch.plan.prepared.operation_fingerprint =
            kernel.dispatch.operation.fingerprint.clone();
        kernel.dispatch.plan = ExecutionPlan::new(kernel.dispatch.plan.prepared).expect("plan");
        kernel.authority.scope.operation_fingerprint =
            kernel.dispatch.operation.fingerprint.clone();
        kernel.grant.scope = kernel.authority.scope.clone();
        kernel.dispatch.binding.operation_fingerprint =
            kernel.dispatch.operation.fingerprint.clone();
        kernel.dispatch.binding.plan_fingerprint = kernel.dispatch.plan.fingerprint.clone();
        kernel.dispatch.binding.authorization_fingerprint =
            kernel.authority.fingerprint().expect("authority hash");
        Self {
            registry,
            batch: tools::BatchExecution {
                batch,
                plan,
                admitted_at: kernel.authority.decided_at,
                result: None,
            },
            kernel,
        }
    }
    fn init(&self, store: &mut V2Store) {
        let at = self.batch.admitted_at;
        store
            .database
            .unit_of_work(|tx| {
                let id = self
                    .kernel
                    .dispatch
                    .operation
                    .content
                    .identity
                    .workspace_id
                    .expect("workspace");
                workspace::insert(
                    tx,
                    &aura_workspace_domain::WorkspaceSummary {
                        workspace_id: id,
                        display_name: "Synthetic".into(),
                        selected_root: "C:/synthetic".into(),
                        canonical_root: "C:/synthetic".into(),
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
                        id: self.batch.batch.invocation_identity.session_id,
                        workspace_id: id,
                        title: "Synthetic".into(),
                        created_at: at,
                        updated_at: at,
                    },
                )?;
                tools::admit(tx, &self.batch, &self.registry, &[])
            })
            .expect("atomic admit");
    }
    fn reserve(
        &self,
        tx: &rusqlite::Transaction<'_>,
        binding: &tools::CallReservation,
    ) -> StoreResult<()> {
        let f = &self.kernel;
        operation::begin(
            tx,
            &operation::OperationDraft {
                id: f.dispatch.operation.operation_id,
                domain: "tool",
                kind: "fixture.read",
                request_key: "call-one",
                version: 1,
                payload: &serde_json::to_value(&f.dispatch.operation)?,
                at: f.authority.decided_at,
            },
        )?;
        security::insert_grant(tx, &f.grant)?;
        let dispatch = AuthorizedDispatch::revalidate(
            f.dispatch.clone(),
            &f.authority,
            &f.dispatch.plan.prepared.backend,
            &f.cancellation,
            f.authority.decided_at,
        )
        .expect("dispatch");
        execution::prepare(tx, &dispatch, &f.authority, 0, f.authority.decided_at)?;
        tools::bind_attempt(tx, binding, &dispatch)
    }
}

#[test]
fn tool_binding_is_atomic_and_result_history_is_ordered_immutable_and_restartable() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    let mut store = V2Store::create(&root).expect("create");
    let f = Fixture::new();
    f.init(&mut store);
    let first = f.batch.batch.calls[0].tool_call_id;
    let second = f.batch.batch.calls[1].tool_call_id;
    let binding = tools::CallReservation {
        batch: f.batch.batch.clone(),
        call_id: first,
        attempt_id: ToolExecutionAttemptId::new(),
    };
    let interrupted: StoreResult<()> = store.database.unit_of_work(|tx| {
        f.reserve(tx, &binding)?;
        Err(Error::Invalid("interrupted"))
    });
    assert!(interrupted.is_err());
    assert!(tools::call(store.database.connection(), first).expect("call").attempt_id.is_none());
    assert!(
        execution::load(store.database.connection(), f.kernel.dispatch.operation.operation_id, 1)
            .is_err()
    );
    store.database.unit_of_work(|tx| f.reserve(tx, &binding)).expect("reserve and bind");
    let at = f.batch.admitted_at;
    let second_result = ToolResultV1 {
        tool_call_id: second,
        outcome: ToolResultOutcomeV1::NotExecuted {
            reason: ToolNotExecutedReasonV1::PermissionDenied,
            error: None,
        },
        side_effect_state: SideEffectStateV1::NotStarted,
        attempts: vec![],
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: None,
        started_at: None,
        completed_at: at,
    };
    store
        .database
        .unit_of_work(|tx| tools::complete_call(tx, f.batch.batch.tool_batch_id, &second_result))
        .expect("out of order arrival");
    assert!(
        store
            .database
            .unit_of_work(|tx| tools::finish(tx, f.batch.batch.tool_batch_id, at))
            .is_err()
    );
    let evidence = store
        .database
        .unit_of_work(|tx| {
            execution::request_cancel(tx, &f.kernel.dispatch.binding.attempt, at)?;
            execution::reject_prepared(tx, &f.kernel.dispatch, "cancellation", at)
        })
        .expect("real persisted refusal");
    let mut first_result = ToolResultV1 {
        tool_call_id: first,
        outcome: ToolResultOutcomeV1::Cancelled { reason: ToolCancellationReasonV1::UserRequested },
        side_effect_state: evidence.side_effect,
        attempts: vec![ToolExecutionAttemptOutcomeRefV1 {
            tool_execution_attempt_id: binding.attempt_id,
            attempt_number: 1,
            side_effect_state: evidence.side_effect,
            side_effect_evidence_ref: Some(evidence.receipt.evidence_ref),
            dispatched: false,
        }],
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: Some(evidence.receipt.evidence_ref),
        started_at: Some(at),
        completed_at: at,
    };
    let mut forged = first_result.clone();
    forged.outcome = ToolResultOutcomeV1::Succeeded;
    assert!(
        store
            .database
            .unit_of_work(|tx| tools::complete_call(tx, f.batch.batch.tool_batch_id, &forged))
            .is_err()
    );
    let foreign = ArtifactId::new();
    store
        .database
        .unit_of_work(|tx| {
            artifact::put(tx, foreign, "another-call", "text/plain", b"private canary", at)
        })
        .expect("foreign artifact");
    first_result.model_visible_result_ref = Some(foreign);
    assert!(
        store
            .database
            .unit_of_work(|tx| tools::complete_call(tx, f.batch.batch.tool_batch_id, &first_result))
            .is_err()
    );
    first_result.model_visible_result_ref = None;
    let complete = store
        .database
        .unit_of_work(|tx| {
            tools::complete_call(tx, f.batch.batch.tool_batch_id, &first_result)?;
            tools::finish(tx, f.batch.batch.tool_batch_id, at)
        })
        .expect("atomic completed group");
    assert_eq!(
        complete.results.iter().map(|r| r.tool_call_id).collect::<Vec<_>>(),
        [first, second]
    );
    drop(store);
    let mut store = V2Store::open(&root).expect("restart");
    assert!(
        !store
            .database
            .unit_of_work(|tx| tools::admit(tx, &f.batch, &f.registry, &[]))
            .expect("repeat admission")
    );
    assert_eq!(
        store
            .database
            .unit_of_work(|tx| tools::finish(tx, f.batch.batch.tool_batch_id, at))
            .expect("repeat finish"),
        complete
    );
    store
        .database
        .unit_of_work(|tx| tools::complete_call(tx, f.batch.batch.tool_batch_id, &first_result))
        .expect("same result idempotent");
    assert!(
        store
            .database
            .unit_of_work(|tx| tools::complete_call(tx, f.batch.batch.tool_batch_id, &forged))
            .is_err()
    );
}

#[test]
fn failed_command_summary_is_bound_bounded_and_never_overrides_unknown_or_retryability() {
    for (message, schema, matching_binding, outcome, accepted) in [
        (
            Some("Command failed: exit code 7\nstdout: partial\nstderr: failure".into()),
            "windows-user-execution-evidence-v1",
            true,
            ExecutionOutcome::Failed,
            true,
        ),
        (None, "windows-user-execution-evidence-v1", true, ExecutionOutcome::Failed, false),
        (
            Some("x".repeat(8193)),
            "windows-user-execution-evidence-v1",
            true,
            ExecutionOutcome::Failed,
            false,
        ),
        (
            Some("unsafe\u{1b}".into()),
            "windows-user-execution-evidence-v1",
            true,
            ExecutionOutcome::Failed,
            false,
        ),
        (Some("foreign".into()), "unrecognized", true, ExecutionOutcome::Failed, false),
        (
            Some("wrong binding".into()),
            "windows-user-execution-evidence-v1",
            false,
            ExecutionOutcome::Failed,
            false,
        ),
        (
            Some("must remain unknown".into()),
            "windows-user-execution-evidence-v1",
            true,
            ExecutionOutcome::Unknown,
            false,
        ),
    ] {
        let dir = tempfile::tempdir().expect("temp");
        let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
        let f = Fixture::new();
        f.init(&mut store);
        let call = f.batch.batch.calls[0].tool_call_id;
        let at = f.batch.admitted_at;
        let operation = f.kernel.dispatch.operation.operation_id;
        let receipt_id = ArtifactId::new();
        let result = store
            .database
            .unit_of_work(|tx| {
                f.reserve(
                    tx,
                    &tools::CallReservation {
                        batch: f.batch.batch.clone(),
                        call_id: call,
                        attempt_id: ToolExecutionAttemptId::new(),
                    },
                )?;
                execution::mark_dispatched(tx, operation, 1, at)?;
                let binding = if matching_binding {
                    canonical_sha256(&f.kernel.dispatch.binding).expect("hash")
                } else {
                    jcs::CanonicalSha256HashV1::sha256(b"foreign")
                };
                artifact::put(
                    tx,
                    receipt_id,
                    &operation.to_string(),
                    "application/json",
                    &serde_json::to_vec(&serde_json::json!({
                        "schema":schema, "binding":binding, "observation":null,
                        "model_safe_failure_message":message,
                    }))?,
                    at,
                )?;
                execution::complete(
                    tx,
                    operation,
                    1,
                    &ExecutionResult {
                        outcome,
                        side_effect: SideEffectStateV1::Unknown,
                        termination: TerminationEvidence::Confirmed { evidence_ref: receipt_id },
                        completed_at: at,
                        warnings: vec!["tool.side_effect.unverified".into()],
                        receipt: ExecutionReceipt {
                            attempt: f.kernel.dispatch.binding.attempt.clone(),
                            evidence_ref: receipt_id,
                            postcondition_fingerprint: None,
                            rollback: RollbackEvidence::Unavailable,
                        },
                    },
                )?;
                aura_persistence_v2::tool::result::materialize(
                    tx,
                    f.batch.batch.tool_batch_id,
                    call,
                )
            })
            .expect("materialize terminal receipt");
        assert_eq!(result.side_effect_state, SideEffectStateV1::Unknown);
        assert_eq!(result.aggregate_side_effect_evidence_ref, Some(receipt_id));
        let visible: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(
            &artifact::read(
                store.database.connection(),
                result.model_visible_result_ref.expect("visible"),
                &call.to_string(),
            )
            .expect("artifact"),
        )
        .expect("visible envelope");
        let error = match visible.outcome {
            ModelVisibleToolResultOutcomeV1::Failed { error } => {
                assert_eq!(outcome, ExecutionOutcome::Failed);
                error
            }
            ModelVisibleToolResultOutcomeV1::Indeterminate { error, .. } => {
                assert_eq!(outcome, ExecutionOutcome::Unknown);
                error
            }
            _ => panic!("failure or indeterminate must remain unchanged"),
        };
        assert!(!error.retryable);
        assert_eq!(error.message == message.unwrap_or_default(), accepted);
        assert!(error.message.len() <= 8192);
        let repeated = store
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::tool::result::materialize(
                    tx,
                    f.batch.batch.tool_batch_id,
                    call,
                )
            })
            .expect("idempotent publication");
        assert_eq!(result, repeated);
    }
}

#[test]
fn native_isolation_warning_materializes_into_the_visible_envelope() {
    let dir = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = Fixture::new();
    f.init(&mut store);
    let call = f.batch.batch.calls[0].tool_call_id;
    let at = f.batch.admitted_at;
    let operation = f.kernel.dispatch.operation.operation_id;
    let receipt_id = ArtifactId::new();
    let result = store
        .database
        .unit_of_work(|tx| {
            f.reserve(
                tx,
                &tools::CallReservation {
                    batch: f.batch.batch.clone(),
                    call_id: call,
                    attempt_id: ToolExecutionAttemptId::new(),
                },
            )?;
            execution::mark_dispatched(tx, operation, 1, at)?;
            let binding = canonical_sha256(&f.kernel.dispatch.binding).expect("hash");
            artifact::put(
                tx,
                receipt_id,
                &operation.to_string(),
                "application/json",
                &serde_json::to_vec(&serde_json::json!({
                    "schema":"windows-user-execution-evidence-v1",
                    "binding":binding,
                    "observation":null,
                }))?,
                at,
            )?;
            execution::complete(
                tx,
                operation,
                1,
                &ExecutionResult {
                    outcome: ExecutionOutcome::Failed,
                    side_effect: SideEffectStateV1::Unknown,
                    termination: TerminationEvidence::Confirmed { evidence_ref: receipt_id },
                    completed_at: at,
                    warnings: vec![
                        "execution.isolation_reduced".into(),
                        "tool.side_effect.unverified".into(),
                    ],
                    receipt: ExecutionReceipt {
                        attempt: f.kernel.dispatch.binding.attempt.clone(),
                        evidence_ref: receipt_id,
                        postcondition_fingerprint: None,
                        rollback: RollbackEvidence::Unavailable,
                    },
                },
            )?;
            aura_persistence_v2::tool::result::materialize(tx, f.batch.batch.tool_batch_id, call)
        })
        .expect("Native isolation warning must not become Conflict");
    let visible: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(
        &artifact::read(
            store.database.connection(),
            result.model_visible_result_ref.expect("visible"),
            &call.to_string(),
        )
        .expect("artifact"),
    )
    .expect("visible envelope");
    assert!(
        visible.warnings.iter().any(|warning| warning.as_str() == "execution.isolation_reduced"),
        "{visible:?}"
    );
}

fn materialize_failed_web(error: &str) -> ModelSafeToolErrorV1 {
    let dir = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = Fixture::new();
    f.init(&mut store);
    let call = f.batch.batch.calls[0].tool_call_id;
    let at = f.batch.admitted_at;
    let operation = f.kernel.dispatch.operation.operation_id;
    let receipt_id = ArtifactId::new();
    let result = store
        .database
        .unit_of_work(|tx| {
            f.reserve(
                tx,
                &tools::CallReservation {
                    batch: f.batch.batch.clone(),
                    call_id: call,
                    attempt_id: ToolExecutionAttemptId::new(),
                },
            )?;
            execution::mark_dispatched(tx, operation, 1, at)?;
            artifact::put(
                tx,
                receipt_id,
                &operation.to_string(),
                "application/json",
                &serde_json::to_vec(&serde_json::json!({
                    "schema":"controlled-web-result-v1",
                    "action":"web.fetch",
                    "output":null,
                    "error":error,
                    "cancelled":false,
                    "request_future_dropped":true,
                    "content_trust":"external_untrusted",
                }))?,
                at,
            )?;
            execution::complete(
                tx,
                operation,
                1,
                &ExecutionResult {
                    outcome: ExecutionOutcome::Failed,
                    side_effect: SideEffectStateV1::NoEffect,
                    termination: TerminationEvidence::Confirmed { evidence_ref: receipt_id },
                    completed_at: at,
                    warnings: vec![],
                    receipt: ExecutionReceipt {
                        attempt: f.kernel.dispatch.binding.attempt.clone(),
                        evidence_ref: receipt_id,
                        postcondition_fingerprint: None,
                        rollback: RollbackEvidence::NotApplicable,
                    },
                },
            )?;
            aura_persistence_v2::tool::result::materialize(tx, f.batch.batch.tool_batch_id, call)
        })
        .expect("web failure must materialize");
    let visible: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(
        &artifact::read(
            store.database.connection(),
            result.model_visible_result_ref.expect("visible"),
            &call.to_string(),
        )
        .expect("artifact"),
    )
    .expect("visible envelope");
    match visible.outcome {
        ModelVisibleToolResultOutcomeV1::Failed { error } => error,
        other => panic!("web failure must stay Failed: {other:?}"),
    }
}

#[test]
fn controlled_web_limit_is_named_instead_of_generic_dispatch_failed() {
    let error = materialize_failed_web("network.download_limit_exceeded");
    assert_eq!(error.code.as_str(), "tool.runtime.io_limit_exceeded");
    assert!(error.message.contains("network.download_limit_exceeded"), "{error:?}");
}

#[test]
fn controlled_web_redirect_is_named_instead_of_generic_dispatch_failed() {
    let error = materialize_failed_web("network.redirect_blocked");
    assert_eq!(error.code.as_str(), "tool.runtime.http_redirect_denied");
    assert!(error.message.contains("network.redirect_blocked"), "{error:?}");
}

#[test]
fn controlled_web_url_credentials_reject_is_named_instead_of_generic_dispatch_failed() {
    let error = materialize_failed_web("network.url_credentials_forbidden");
    assert_eq!(error.code.as_str(), "tool.runtime.http_request_invalid");
    assert!(error.message.contains("network.url_credentials_forbidden"), "{error:?}");
}

#[test]
fn controlled_web_wrapped_transport_error_keeps_the_named_prefix() {
    let error = materialize_failed_web("network.transport_failed: response body read failed");
    assert_eq!(error.code.as_str(), "tool.runtime.http_gateway_failed");
    assert!(error.message.contains("network.transport_failed"), "{error:?}");
    assert!(!error.message.contains("response body read failed"), "{error:?}");
}
