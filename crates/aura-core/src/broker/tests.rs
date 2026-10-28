use super::*;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

fn request(
    operation: SecuredOperation,
    plan: ExecutionPlan,
    authority: &AuthorizationDecision,
) -> DispatchRequest {
    let binding = DispatchBinding {
        attempt: AttemptRef {
            operation_id: operation.operation_id,
            ordinal: std::num::NonZeroU32::MIN,
        },
        operation_fingerprint: operation.fingerprint.clone(),
        authorization_fingerprint: authority.fingerprint().expect("authority"),
        plan_fingerprint: plan.fingerprint.clone(),
        cancellation_revision: Revision::INITIAL,
    };
    DispatchRequest { operation, plan, binding }
}
fn authority(op: &SecuredOperation, backend: &BackendDescriptor) -> AuthorizationDecision {
    let now = UtcTimestamp::now();
    AuthorizationDecision {
        basis: AuthorizationBasis::AuthenticatedMain {
            principal_id: op.content.identity.principal_id,
            core_runtime_instance_id: backend.runtime_instance_id,
            main_runtime_instance_id: aura_domain_common::RuntimeInstanceId::new(),
            channel_id: aura_domain_common::IpcChannelId::new(),
        },
        scope: AuthorizationScope {
            operation_fingerprint: op.fingerprint.clone(),
            profile: SecurityProfile::Balanced,
            backend_requirement: BackendRequirement {
                allowed_classes: [BackendClass::CoreBroker].into(),
                guarantees: Default::default(),
            },
            lifetime: AuthorizationLifetime::Once { operation_id: op.operation_id },
            consent: None,
            network_consent: None,
            capability_limit: None,
            memory_consent: None,
            mcp_consent: None,
            chrome_consent: None,
            chrome_site_consent: None,
            file_consent: None,
        },
        decided_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::seconds(60)),
    }
}
fn cancellation() -> CancellationState {
    CancellationState { revision: Revision::INITIAL, requested: false }
}

#[tokio::test]
async fn r05_exact_claim_replay_handle_and_unique_result_use_real_sqlite() {
    let root = tempfile::tempdir().expect("isolated");
    let journal =
        std::sync::Arc::new(BrokerJournal::open(&root.path().join("journal")).expect("journal"));
    let file = root.path().join("effect.txt");
    let output = std::sync::Arc::new(Mutex::new(None));
    let descriptor = descriptor(aura_domain_common::RuntimeInstanceId::new());
    let op = operation(
        "test.write",
        &json!({}),
        aura_domain_common::CorrelationId::new(),
        vec![ResolvedCapability::ConfigurationRead],
        vec![ResourceDescriptor::ExternalService { service_id: "test".into(), object_ref: None }],
    )
    .expect("operation");
    let auth = authority(&op, &descriptor);
    let live = || Ok(());
    let evidence_file = file.clone();
    let handler_file = file.clone();
    let backend=CoreBroker::new(descriptor.clone(),port::CallPort {journal:std::sync::Arc::clone(&journal),output:std::sync::Arc::clone(&output),live:&live,
        boundary:crate::sync_boundary::SyncBoundary::open(),subject:crate::sync_boundary::SyncSubject::new(),
        post_execution:crate::broker::completion::PostExecutionOwners::new(aura_domain_common::RuntimeInstanceId::new()),
        settlements:crate::command_settlement::CommandSettlements::new(aura_domain_common::RuntimeInstanceId::new(),std::sync::Arc::new(Mutex::new(crate::command_claims::CommandOutcomeLedger::default()))),
        responsibility:std::sync::Arc::new(Mutex::new(None)),
        inspect:std::sync::Arc::new(||Ok(json!({"scope":"test"}))),
        evidence:std::sync::Arc::new(move |r:&DispatchRequest,_:&Result<Value,IpcErrorEnvelope>|Ok(BrokerEvidence {action_kind:r.operation.content.action_kind.clone(),request_fingerprint:r.operation.fingerprint.0.clone(),before:json!({}),after:json!({"hash":aura_domain_common::ContentHash::blake3(&std::fs::read(&evidence_file)?)}),domain_journal:Value::Null,side_effect:SideEffectStateV1::Unknown})),
        call:Mutex::new(Some(move ||async move {assert!(crate::ipc_session::current_resource_wait_handler().is_some(), "the real Broker owned task must inherit the connection wait ticket");std::fs::write(&handler_file,"once").expect("real effect");Ok(json!({"done":true}))})),
    }).expect("backend");
    let plan = backend
        .prepare(PrepareRequest {
            operation: op.clone(),
            requirement: auth.scope.backend_requirement.clone(),
        })
        .await
        .expect("prepare");
    let dispatch = AuthorizedDispatch::revalidate(
        request(op, plan, &auth),
        &auth,
        &descriptor,
        &cancellation(),
        UtcTimestamp::now(),
    )
    .expect("checked");
    journal.reserve(&dispatch, &auth, "exact-once").expect("reserve");
    let (wait_handler, _wait_signals) = crate::ipc_session::ResourceWaitSignals::channel();
    let handle = crate::ipc_session::RESOURCE_WAIT_HANDLER
        .scope(wait_handler.for_ticket(1), backend.execute(dispatch.clone()))
        .await
        .expect("execute");
    assert!(backend.execute(dispatch).await.is_err());
    let ReconcileResult::Completed { result } =
        backend.reconcile(handle.attempt.clone()).await.expect("reconcile")
    else {
        panic!("completed")
    };
    assert_eq!(result.outcome, ExecutionOutcome::Succeeded);
    assert_eq!(result.side_effect, SideEffectStateV1::Unknown);
    assert_eq!(result.warnings, ["tool.side_effect.unverified"]);
    assert!(matches!(
        backend.cancel(handle.clone()).await.expect("completed cancel").termination,
        TerminationEvidence::Confirmed { .. }
    ));
    let mut forged = handle;
    forged.dispatch_fingerprint = canonical_sha256(&"forged").expect("hash");
    assert!(backend.cancel(forged).await.is_err());
    let store = journal.lock().expect("store");
    assert_eq!(
        store
            .database
            .connection()
            .query_row("SELECT count(*) FROM execution_results", [], |r| r.get::<_, i64>(0))
            .expect("unique"),
        1
    );
}

#[tokio::test]
async fn a_command_whose_end_is_unproven_is_recorded_as_unknown_not_as_a_failure() {
    // The Q1 requirement this covers: an executed handler whose OUTCOME the Core itself declared
    // unknown must not become a definite failure in the execution ledger merely because its result
    // was an `Err`. The row is written to the same real SQLite the product uses and read back.
    let root = tempfile::tempdir().expect("isolated");
    let journal =
        std::sync::Arc::new(BrokerJournal::open(&root.path().join("journal")).expect("journal"));
    let descriptor = descriptor(aura_domain_common::RuntimeInstanceId::new());
    let op = operation(
        "test.write",
        &json!({}),
        aura_domain_common::CorrelationId::new(),
        vec![ResolvedCapability::ConfigurationRead],
        vec![ResourceDescriptor::ExternalService { service_id: "test".into(), object_ref: None }],
    )
    .expect("operation");
    let auth = authority(&op, &descriptor);
    let plan = ExecutionPlan::new(PreparedExecution {
        operation_fingerprint: op.fingerprint.clone(),
        backend: descriptor.clone(),
        preparation: json!({}),
    })
    .expect("plan");
    let dispatch = AuthorizedDispatch::revalidate(
        request(op, plan, &auth),
        &auth,
        &descriptor,
        &cancellation(),
        UtcTimestamp::now(),
    )
    .expect("checked");
    journal.reserve(&dispatch, &auth, "unsettled-end").expect("reserve");
    // The ledger only holds a completion for an attempt that was really claimed and dispatched.
    journal.claim(dispatch.request()).expect("claim");
    journal
        .complete(
            dispatch.request(),
            BrokerEvidence {
                action_kind: dispatch.request().operation.content.action_kind.clone(),
                request_fingerprint: dispatch.request().operation.fingerprint.0.clone(),
                before: json!({}),
                after: json!({"written": true}),
                domain_journal: Value::Null,
                side_effect: SideEffectStateV1::FullyApplied,
            },
            CompletionVerdict::Unknown,
        )
        .expect("the unknown end is recorded");
    let store = journal.lock().expect("store");
    let stored = aura_persistence_v2::execution::result(
        store.database.connection(),
        dispatch.request().binding.attempt.operation_id,
        dispatch.request().binding.attempt.ordinal.get(),
    )
    .expect("read the recorded result")
    .expect("the attempt has a recorded result");
    assert_eq!(
        stored.outcome,
        ExecutionOutcome::Unknown,
        "an unproven end must not be recorded as a definite failure"
    );
    assert_eq!(
        stored.termination,
        TerminationEvidence::Unknown,
        "the termination is what an unproven end leaves unproven"
    );
    assert_eq!(
        stored.side_effect,
        SideEffectStateV1::FullyApplied,
        "the real side effect the evidence measured is preserved"
    );
    assert_eq!(stored.warnings, ["tool.outcome.unsettled"]);
}

#[tokio::test]
async fn r05_changed_preparation_and_replaced_main_channel_never_invoke() {
    let root = tempfile::tempdir().expect("isolated");
    let journal =
        std::sync::Arc::new(BrokerJournal::open(&root.path().join("journal")).expect("journal"));
    let output = std::sync::Arc::new(Mutex::new(None));
    let count = std::sync::Arc::new(AtomicUsize::new(0));
    let inspect_count = std::sync::Arc::clone(&count);
    let descriptor = descriptor(aura_domain_common::RuntimeInstanceId::new());
    let live = || Ok(());
    let op = operation(
        "test.read",
        &json!({}),
        aura_domain_common::CorrelationId::new(),
        vec![ResolvedCapability::ConfigurationRead],
        vec![ResourceDescriptor::ExternalService { service_id: "test".into(), object_ref: None }],
    )
    .expect("operation");
    let auth = authority(&op, &descriptor);
    let backend = CoreBroker::new(
        descriptor.clone(),
        port::CallPort {
            journal: std::sync::Arc::clone(&journal),
            output: std::sync::Arc::clone(&output),
            live: &live,
            boundary: crate::sync_boundary::SyncBoundary::open(),
            subject: crate::sync_boundary::SyncSubject::new(),
            post_execution: crate::broker::completion::PostExecutionOwners::new(
                aura_domain_common::RuntimeInstanceId::new(),
            ),
            settlements: crate::command_settlement::CommandSettlements::new(
                aura_domain_common::RuntimeInstanceId::new(),
                std::sync::Arc::new(Mutex::new(
                    crate::command_claims::CommandOutcomeLedger::default(),
                )),
            ),
            responsibility: std::sync::Arc::new(Mutex::new(None)),
            inspect: std::sync::Arc::new(move || {
                Ok(json!({"revision":inspect_count.fetch_add(1,Ordering::SeqCst)}))
            }),
            evidence: std::sync::Arc::new(
                |_: &DispatchRequest,
                 _: &Result<Value, IpcErrorEnvelope>|
                 -> anyhow::Result<BrokerEvidence> {
                    panic!("must not create receipt")
                },
            ),
            call: Mutex::new(Some(|| async { panic!("must not invoke changed resource") })),
        },
    )
    .expect("backend");
    let plan = backend
        .prepare(PrepareRequest {
            operation: op.clone(),
            requirement: auth.scope.backend_requirement.clone(),
        })
        .await
        .expect("prepare");
    let dispatch = AuthorizedDispatch::revalidate(
        request(op, plan, &auth),
        &auth,
        &descriptor,
        &cancellation(),
        UtcTimestamp::now(),
    )
    .expect("checked");
    journal.reserve(&dispatch, &auth, "changed").expect("reserve");
    assert!(backend.execute(dispatch).await.is_err());
    let fixture = crate::r03_tests::Fixture::new();
    let state = std::sync::Arc::new(fixture.start().await);
    let op = operation(
        "test.read",
        &json!({}),
        aura_domain_common::CorrelationId::new(),
        vec![ResolvedCapability::ConfigurationRead],
        vec![ResourceDescriptor::ExternalService { service_id: "test".into(), object_ref: None }],
    )
    .expect("operation");
    let result = run(
        &state,
        &journal,
        op,
        "channel".into(),
        None,
        std::sync::Arc::new({
            let state = state.clone();
            move || {
                state.authenticated_channel.lock().expect("channel").active_channel_id = None;
                Ok(json!({}))
            }
        }),
        std::sync::Arc::new(
            |_, _: &Result<Value, IpcErrorEnvelope>| -> anyhow::Result<BrokerEvidence> {
                panic!("no receipt")
            },
        ),
        || async { panic!("no invocation") },
    )
    .await;
    assert!(result.is_err());
}

#[test]
fn r05_main_basis_rejects_expiry_cancellation_and_agent_identity() {
    let descriptor = descriptor(aura_domain_common::RuntimeInstanceId::new());
    let original = operation(
        "read",
        &json!({}),
        aura_domain_common::CorrelationId::new(),
        vec![ResolvedCapability::ConfigurationRead],
        vec![ResourceDescriptor::ExternalService { service_id: "test".into(), object_ref: None }],
    )
    .expect("operation");
    for kind in ["valid", "expired", "cancelled", "agent", "parent_agent", "native"] {
        let mut op = original.clone();
        let mut backend = descriptor.clone();
        if kind == "agent" {
            op.content.identity.agent_run_id = Some(aura_domain_common::AgentRunId::new());
        }
        if kind == "parent_agent" {
            op.content.identity.parent_agent_run_id = Some(aura_domain_common::AgentRunId::new());
        }
        if kind == "native" {
            backend.class = BackendClass::Native;
        }
        op = SecuredOperation::new(op.operation_id, op.content).expect("content");
        let mut auth = authority(&op, &backend);
        if kind == "expired" {
            auth.expires_at = auth.decided_at;
        }
        let plan = ExecutionPlan::new(PreparedExecution {
            operation_fingerprint: op.fingerprint.clone(),
            backend: backend.clone(),
            preparation: json!({}),
        })
        .expect("plan");
        let mut cancel = cancellation();
        cancel.requested = kind == "cancelled";
        let result = AuthorizedDispatch::revalidate(
            request(op, plan, &auth),
            &auth,
            &backend,
            &cancel,
            UtcTimestamp::now(),
        );
        assert_eq!(result.is_ok(), kind == "valid", "{kind}");
    }
}
