//! Only model responses are scripted; admission, policy, Vault I/O and receipts are real.
use super::*;

const TOOLS: &[&str] = &[
    "memory.create",
    "memory.read",
    "memory.search",
    "memory.update",
    "memory.delete",
    "memory.repair",
];

#[tokio::test]
async fn readonly_session_answer_is_loaded_for_a_new_call_and_revocation_stops_reuse() {
    use aura_security_contract::{ApprovalState, AuthorizationApproval};
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, TOOLS).await;
    let create = |name: &str| json!({"scope":"global","items":[{"path":format!("rules/{name}.md"),"category":"rules","body":format!("# {name}\n")} ]});
    let batch = f.admit(&[("memory.create", create("first"))]);
    let call = batch.calls.values().next().expect("real operation");
    let engine = f.service().agent_execution_required();
    let backend = engine.broker_backend(call).expect("memory backend").descriptor();
    let PolicyAuthorization::Ask { proposal } = aura_permission_engine::policy::authorize(
        &call.policy(UtcTimestamp::now(), engine.runtime),
        &backend,
        None,
    ) else {
        panic!("readonly asks")
    };
    let now = UtcTimestamp::now();
    let approval = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    let grant = f
        .service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &approval)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                approval.approval_id,
                approval.revision,
                &call.operation,
                ApprovalDecisionKind::AllowSession,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("approve")
        .expect("grant");
    assert!(grant.scope.memory_consent.is_some());
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    let next = f.admit(&[("memory.create", create("second"))]);
    assert_ne!(
        next.calls.values().next().expect("call").operation.fingerprint,
        grant.scope.operation_fingerprint
    );
    let batch_id = next.batch.tool_batch_id;
    let group = f.execute(next).await;
    f.continue_after_tools(batch_id, &group);
    f.service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::revoke(
                tx,
                grant.grant_id,
                grant.revision,
                None,
                UtcTimestamp::now(),
            )
        })
        .expect("revoke");
    let refused = f.admit(&[("memory.create", create("third"))]);
    assert!(matches!(
        f.execute_rejecting(refused).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    assert!(!f.service().broker.memory().roots().global.join("rules/third.md").exists());
}

#[tokio::test]
async fn a_model_writes_memory_and_the_next_real_session_finds_it() {
    let first = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let batch = first.admit(&[("memory.create",json!({"scope":"project","items":[{"path":"commands/build.md","category":"commands","body":"# 构建经验\n项目使用离线测试。\n"}]}))]);
    let call = batch.batch.calls[0].tool_call_id;
    first.execute(batch).await;
    let receipt = first.output(call);
    let id = receipt["entries"][0]["id"].clone();
    assert!(id.is_string(), "{receipt}");
    assert!(first.root.join(".memories/commands/build.md").is_file());
    {
        let store = first.service().broker.lock().expect("activity");
        let page = aura_persistence_v2::tool::activity::page(
            store.database.connection(),
            &aura_tool_projection::AgentToolActivityRequest {
                session_id: first.turn.session_id,
                before_call_id: None,
                limit: 10,
                call_ids: None,
            },
        )
        .expect("real activity");
        let display = &page
            .calls
            .iter()
            .find(|c| c.summary.tool_call_id == call)
            .expect("memory row")
            .display;
        assert_eq!(display.title, "创建记忆");
        assert_eq!(display.target.as_deref(), Some("当前项目记忆"));
        assert!(display.details.iter().any(|line| line.contains("构建经验")));
    }
    let next = first.additional_session(PermissionMode::ReadOnly, TOOLS);
    let batch = next.admit(&[("memory.read", json!({"target":{"kind":"id","id":id}}))]);
    let call = batch.batch.calls[0].tool_call_id;
    next.execute(batch).await;
    let read = next.output(call);
    assert_eq!(read["note"]["metadata"]["source_sessions"][0], first.turn.session_id.to_string());
    assert!(read["note"]["body"].as_str().expect("body").contains("离线测试"));
    let search = first.additional_session(PermissionMode::ReadOnly, TOOLS);
    let batch = search.admit(&[("memory.search", json!({"scope":"project","query":"经验"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    search.execute(batch).await;
    assert_eq!(search.output(call)["hits"][0]["metadata"]["id"], id);
}

#[tokio::test]
async fn readonly_write_and_full_access_delete_wait_for_precise_approval() {
    let readonly = Fixture::with_exposure(PermissionMode::ReadOnly, TOOLS).await;
    let batch = readonly.admit(&[("memory.create",json!({"scope":"global","items":[{"path":"rules/safe.md","category":"rules","body":"# 临时测试记忆\n"}]}))]);
    assert!(matches!(
        readonly.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    assert!(
        !readonly.service().broker.memory().roots().global.exists(),
        "approval must precede the first Vault write"
    );
    let writer = readonly.additional_session(PermissionMode::FullAccess, TOOLS);
    let batch=writer.admit(&[("memory.create",json!({"scope":"global","items":[{"path":"rules/safe.md","category":"rules","body":"# 临时测试记忆\n"}]}))]);
    let call = batch.batch.calls[0].tool_call_id;
    writer.execute(batch).await;
    let receipt = writer.output(call);
    let delete = writer.additional_session(PermissionMode::FullAccess, TOOLS);
    let batch=delete.admit(&[("memory.delete",json!({"scope":"global","items":[{"target":{"kind":"id","id":receipt["entries"][0]["id"]},"expected_hash":receipt["entries"][0]["hash"]}]}))]);
    assert!(matches!(
        delete.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    let vault = delete.service().broker.memory().roots().global.clone();
    assert!(vault.join("rules/safe.md").is_file());
}

/// DeepSeek once wrote `confidence: 1.0` and an `importance` outside 1–5.
/// The `1.0` made admission refuse the call as an identity conflict and interrupted the whole
/// turn; the wrong importance then came back as "check the arguments". Now the call is
/// admitted, the model reads which rule it broke, and the corrected call writes the note.
#[tokio::test]
async fn an_integral_decimal_is_admitted_and_a_wrong_importance_says_what_to_fix() {
    let f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let note = |importance: u32| json!({"scope":"project","items":[{"path":"conventions/test-command.md","category":"convention","body":"# 测试命令\n\n`pnpm test`\n","importance":importance,"confidence":1.0,"recall_weight":1.0}]});
    let batch = f
        .admit_contending(&[("memory.create", note(128))])
        .expect("admitted, not refused as an identity conflict");
    let call = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    // The schema states the range, so the call is refused before it runs, naming the field.
    let refused = failure_message(&f, call);
    assert!(refused.contains("/items/0/importance") && refused.contains("<= 5"), "{refused}");
    assert!(!f.root.join(".memories/conventions/test-command.md").exists());
    let again = f.additional_session(PermissionMode::WorkspaceReadWrite, TOOLS);
    let batch = again.admit(&[("memory.create", note(4))]);
    let call = batch.batch.calls[0].tool_call_id;
    again.execute(batch).await;
    assert!(again.output(call)["entries"][0]["id"].is_string());
    assert!(f.root.join(".memories/conventions/test-command.md").is_file());
}

/// The model-visible message of a call that failed or was refused before it ran.
fn failure_message(f: &Fixture, id: ToolCallId) -> String {
    let store = f.service().broker.lock().expect("journal");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("the call");
    match call.result.expect("a terminal result").outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } => error.message.to_string(),
        other => panic!("the call did not fail: {other:?}"),
    }
}

/// A rule the schema cannot state (a category's spelling) comes back from the Vault itself,
/// named for the model instead of "check the arguments".
#[tokio::test]
async fn a_rule_the_schema_cannot_state_is_named_for_the_model() {
    let f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let batch = f.admit(&[(
        "memory.create",
        json!({"scope":"project","items":[{"path":"rules/a.md","category":"Rules With Spaces","body":"# 规则\n"}]}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let refused = failure_message(&f, call);
    assert!(refused.contains("category must be 1 to 64 lowercase"), "{refused}");
    assert!(!refused.contains('\\') && !refused.contains("C:"), "no local path: {refused}");
}
