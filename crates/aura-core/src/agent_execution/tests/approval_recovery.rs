//! The Agent/Approval Store boundary, with real admitted operations and SQLite.
//! This does not claim that the default Built-ins require Guided approval.
use super::*;
use aura_persistence_v2::agent::{approval, recovery, turns};

async fn pause(f: &Fixture, batch: &AdmittedBatch) -> ApprovalRequestId {
    let call = batch.calls.values().next().expect("persisted recovery fixture and evidence");
    let backend = f
        .service()
        .agent_execution_required()
        .broker_backend(call)
        .expect("persisted recovery fixture and evidence")
        .descriptor();
    let mut policy =
        call.policy(UtcTimestamp::now(), f.service().agent_execution_required().runtime);
    policy.profile = SecurityProfile::Guided;
    let PolicyAuthorization::Ask { proposal } =
        aura_permission_engine::policy::authorize(&policy, &backend, None)
    else {
        panic!("Guided fixture must ask");
    };
    let at = UtcTimestamp::now();
    f.service()
        .broker
        .lock()
        .expect("persisted recovery fixture and evidence")
        .database
        .unit_of_work(|tx| {
            approval::pause(
                tx,
                f.turn.agent_run_id,
                batch.batch.tool_batch_id,
                call.validated.call.tool_call_id,
                proposal,
                backend,
                at,
                UtcTimestamp::from_value(at.value() + time::Duration::minutes(10)),
            )
        })
        .expect("persisted recovery fixture and evidence");
    approval::pending(
        f.service()
            .broker
            .lock()
            .expect("persisted recovery fixture and evidence")
            .database
            .connection(),
        f.turn.agent_run_id,
    )
    .expect("persisted recovery fixture and evidence")[0]
}
#[tokio::test]
async fn denied_approval_restores_the_exact_batch_and_never_dispatches() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"approval.txt","text":"never","expected":{"expected_kind":"absent"}}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let operation = batch.calls[&call].operation.clone();
    let id = pause(&f, &batch).await;
    let request = ExecutionApprovalResolutionRequest {
        session_id: f.turn.session_id,
        approval_id: id,
        expected_revision: Revision::INITIAL,
        decision: ApprovalDecisionKind::Deny,
    };
    let command = CommandInvocationId::new();
    {
        let mut store = f.service().broker.lock().expect("persisted recovery fixture and evidence");
        // Another session's approval is a conflict, not a missing row.
        assert!(matches!(
            approval::view(
                store.database.connection(),
                &ExecutionApprovalRequest { session_id: SessionId::new(), approval_id: id }
            ),
            Err(aura_persistence_core::PersistenceCoreError::Conflict)
        ));
        let rollback: aura_persistence_core::StoreResult<()> = store.database.unit_of_work(|tx| {
            approval::resolve(
                tx,
                command,
                &request,
                f.service()
                    .persistence
                    .session_permission_state(f.turn.session_id)
                    .expect("persisted recovery fixture and evidence")
                    .revision,
                f.service().agent_execution_required().runtime,
                None,
                UtcTimestamp::now(),
            )?;
            Err(aura_persistence_core::PersistenceCoreError::Conflict)
        });
        assert!(rollback.is_err());
        assert_eq!(
            approval::pending(store.database.connection(), f.turn.agent_run_id)
                .expect("persisted recovery fixture and evidence"),
            vec![id]
        );
        store
            .database
            .unit_of_work(|tx| {
                approval::resolve(
                    tx,
                    command,
                    &request,
                    f.service()
                        .persistence
                        .session_permission_state(f.turn.session_id)
                        .expect("persisted recovery fixture and evidence")
                        .revision,
                    f.service().agent_execution_required().runtime,
                    None,
                    UtcTimestamp::now(),
                )
            })
            .expect("persisted recovery fixture and evidence");
        store
            .database
            .unit_of_work(|tx| {
                approval::resolve(
                    tx,
                    command,
                    &request,
                    f.service()
                        .persistence
                        .session_permission_state(f.turn.session_id)
                        .expect("persisted recovery fixture and evidence")
                        .revision,
                    f.service().agent_execution_required().runtime,
                    None,
                    UtcTimestamp::now(),
                )
            })
            .expect("persisted recovery fixture and evidence");
        assert!(
            store
                .database
                .unit_of_work(|tx| approval::resolve(
                    tx,
                    CommandInvocationId::new(),
                    &request,
                    f.service()
                        .persistence
                        .session_permission_state(f.turn.session_id)
                        .expect("persisted recovery fixture and evidence")
                        .revision,
                    f.service().agent_execution_required().runtime,
                    None,
                    UtcTimestamp::now()
                ))
                .is_err()
        );
    }
    let restored = f
        .service()
        .agent_execution_required()
        .resume_batch(batch.batch.tool_batch_id)
        .expect("persisted recovery fixture and evidence");
    assert_eq!(restored.calls[&call].operation, operation);
    f.execute(restored).await;
    let store = f.service().broker.lock().expect("persisted recovery fixture and evidence");
    let saved = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
        .expect("persisted recovery fixture and evidence");
    assert!(saved.attempt_id.is_none());
    assert!(!f.root.join("approval.txt").exists());
    let result = saved.result.expect("persisted recovery fixture and evidence");
    let ToolResultOutcomeV1::NotExecuted {
        reason: ToolNotExecutedReasonV1::PermissionDenied,
        error: Some(error),
    } = &result.outcome
    else {
        panic!("a refusal that says the user refused: {:?}", result.outcome)
    };
    // The model and the user read who refused, that nothing ran, and what to do next.
    assert_eq!(error.code.as_str(), "tool.approval.denied");
    assert!(
        error.message.starts_with("approval.denied:")
            && error.message.contains("用户在批准时拒绝")
            && error.message.contains("不要原样再调用"),
        "{}",
        error.message
    );
}
#[tokio::test]
async fn cancel_waiting_approval_closes_proposal_and_publishes_unattempted_history() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"cancel-waiting.txt","text":"never","expected":{"expected_kind":"absent"}}),
    )]);
    let id = pause(&f, &batch).await;
    let mut store = f.service().broker.lock().expect("persisted recovery fixture and evidence");
    store
        .database
        .unit_of_work(|tx| {
            recovery::interrupt(
                tx,
                f.turn.agent_run_id,
                "agent.cancelled",
                true,
                UtcTimestamp::now(),
            )
        })
        .expect("persisted recovery fixture and evidence");
    assert_eq!(
        turns::load(store.database.connection(), f.turn.turn_task_id)
            .expect("persisted recovery fixture and evidence")
            .state,
        aura_agent_domain::TurnTaskState::Cancelled
    );
    assert_eq!(
        aura_persistence_v2::security_approval::load(store.database.connection(), id)
            .expect("persisted recovery fixture and evidence")
            .state,
        ApprovalState::Cancelled
    );
    assert_eq!(
        aura_persistence_v2::context::history(store.database.connection(), f.turn.session_id)
            .expect("persisted recovery fixture and evidence")
            .len(),
        1
    );
    assert!(
        recovery::episodes(store.database.connection(), f.turn.agent_run_id)
            .expect("persisted recovery fixture and evidence")
            .is_empty()
    );
}
#[tokio::test]
async fn expired_approval_resumes_original_batch_with_a_paired_refusal() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"expired.txt","text":"never","expected":{"expected_kind":"absent"}}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let id = pause(&f, &batch).await;
    let mut store = f.service().broker.lock().expect("persisted recovery fixture and evidence");
    let deadline = approval::deadline(store.database.connection(), f.turn.agent_run_id)
        .expect("persisted recovery fixture and evidence")
        .expect("persisted recovery fixture and evidence");
    assert!(
        !store
            .database
            .unit_of_work(|tx| approval::expire(tx, f.turn.agent_run_id, UtcTimestamp::now()))
            .expect("persisted recovery fixture and evidence")
    );
    let rolled_back: aura_persistence_core::StoreResult<()> = store.database.unit_of_work(|tx| {
        assert!(approval::expire(tx, f.turn.agent_run_id, deadline)?);
        Err(aura_persistence_core::PersistenceCoreError::Conflict)
    });
    assert!(rolled_back.is_err());
    assert_eq!(
        approval::pending(store.database.connection(), f.turn.agent_run_id)
            .expect("persisted recovery fixture and evidence"),
        vec![id]
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| approval::expire(tx, f.turn.agent_run_id, deadline))
            .expect("persisted recovery fixture and evidence")
    );
    assert!(
        !store
            .database
            .unit_of_work(|tx| approval::expire(tx, f.turn.agent_run_id, deadline))
            .expect("persisted recovery fixture and evidence")
    );
    assert_eq!(
        aura_persistence_v2::security_approval::load(store.database.connection(), id)
            .expect("persisted recovery fixture and evidence")
            .state,
        ApprovalState::Expired
    );
    let stored = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
        .expect("persisted recovery fixture and evidence");
    assert!(stored.attempt_id.is_none());
    assert!(stored.result.is_some());
    assert_eq!(
        turns::load_run(store.database.connection(), f.turn.agent_run_id)
            .expect("persisted recovery fixture and evidence")
            .state,
        aura_agent_domain::AgentRunState::ToolsRunning { tool_batch_id: batch.batch.tool_batch_id }
    );
    assert!(!f.root.join("expired.txt").exists());
}

/// An approval nobody answered in time is not a denial: nothing ran, the model reads that the user
/// may be away and is told to say what needs approving before asking again once they are back,
/// and the stored reason stays the one older Cores read.
#[tokio::test]
async fn an_expired_approval_tells_the_model_the_user_never_answered() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"unanswered.txt","text":"never","expected":{"expected_kind":"absent"}}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    pause(&f, &batch).await;
    let mut store = f.service().broker.lock().expect("store");
    let deadline = approval::deadline(store.database.connection(), f.turn.agent_run_id)
        .expect("deadline")
        .expect("a pending approval");
    assert!(
        store
            .database
            .unit_of_work(|tx| approval::expire(tx, f.turn.agent_run_id, deadline))
            .expect("expired")
    );
    let result = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
        .expect("call")
        .result
        .expect("recorded");
    let ToolResultOutcomeV1::NotExecuted { reason, error: Some(error) } = &result.outcome else {
        panic!("an explained refusal: {:?}", result.outcome)
    };
    assert_eq!(*reason, ToolNotExecutedReasonV1::PermissionDenied, "the reason older Cores read");
    assert_eq!(error.code.as_str(), "tool.approval.expired");
    let visible = aura_persistence_core::artifact::read(
        store.database.connection(),
        result.model_visible_result_ref.expect("model-visible result"),
        &call.to_string(),
    )
    .expect("model-visible result");
    let visible: serde_json::Value = serde_json::from_slice(&visible).expect("result json");
    let told = visible["outcome"]["error"]["message"].as_str().unwrap_or_default();
    assert!(told.starts_with("approval.expired:") && told.contains("不是拒绝"), "{visible}");
    // Telling the user comes first; asking again waits until they are back.
    let tell = told.find("告诉用户").unwrap_or(usize::MAX);
    let again = told.find("再调用").unwrap_or(0);
    assert!(told.contains("等用户回来") && tell < again, "{visible}");
    assert!(!f.root.join("unanswered.txt").exists());
}
