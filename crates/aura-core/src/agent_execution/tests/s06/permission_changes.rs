use super::consent_fixture::{approve_file, set_mode};
use super::*;

#[tokio::test]
async fn s06_e1_withdrawn_prepared_approval_never_dispatches_or_creates_an_attempt() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"withdrawn.txt","text":"must not execute",
        "expected":{"expected_kind":"absent"}}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let grant = approve_file(&f, &batch.calls[&id]);
    crate::r03_tests::success(&f.runtime, "security.file_session_consent.revoke", json!({
        "session_id":f.turn.session_id,"grant_id":grant.grant_id,"expected_revision":grant.revision,
    })).await;
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    let store =
        f.service().broker.lock().expect("lock the existing authority store for inspection");
    assert!(
        aura_persistence_v2::tool::execution::call(store.database.connection(), id)
            .expect("read the durable tool call at this dispatch boundary")
            .attempt_id
            .is_none()
    );
    assert!(!f.root.join("withdrawn.txt").exists());
}

#[tokio::test]
async fn s06_e1_revoke_cancels_only_calls_that_need_the_withdrawn_file_approval() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"wait","expected":{"expected_kind":"absent"}}),
    )]);
    let call = batch.calls.values().next().expect("bound file call");
    let grant = approve_file(&f, call);
    let engine = f.service().agent_execution_required();
    let (_parent, global) = tokio::sync::watch::channel(false);
    let (lifetime, mut cancelled) =
        engine.active_calls.register(call, global.clone()).expect("prepared managed call");
    let other = Fixture::new(PermissionMode::ReadOnly).await;
    let other_batch = other.admit(&[(
        "fs.write",
        json!({"path":"other.txt","text":"wait","expected":{"expected_kind":"absent"}}),
    )]);
    let (other_lifetime, other_cancel) = engine
        .active_calls
        .register(
            other_batch
                .calls
                .values()
                .next()
                .expect("the admitted batch contains the requested call"),
            global.clone(),
        )
        .expect("register the managed execution with its cancellation owner");
    let ended = tokio::spawn(async move {
        cancelled.changed().await.expect("selective withdrawal signal");
        drop(lifetime);
    });
    let response = crate::r03_tests::request(&f.runtime, "security.file_session_consent.revoke", json!({
        "session_id": f.turn.session_id, "grant_id": grant.grant_id, "expected_revision": grant.revision,
    })).await;
    assert!(response.get("error").is_none(), "withdrawal and bounded stop: {response:?}");
    assert!(!*global.borrow(), "the model run cancellation token is not withdrawn");
    assert!(!*other_cancel.borrow(), "another session remains active");
    let ended_ok = tokio::time::timeout(std::time::Duration::from_millis(500), ended).await;
    assert!(
        ended_ok.is_ok(),
        "revoking explicit file authority must signal affected prepared calls"
    );
    assert!(!f.root.join("approved.txt").exists());
    drop(other_lifetime);
}

#[tokio::test]
async fn s06_e2_tightening_keeps_explicit_file_approval_and_loosening_cancels_nothing() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[
        (
            "fs.write",
            json!({"path":"explicit.txt","text":"wait","expected":{"expected_kind":"absent"}}),
        ),
        (
            "fs.write",
            json!({"path":"default.txt","text":"wait","expected":{"expected_kind":"absent"}}),
        ),
    ]);
    let explicit = &batch.calls[&batch.batch.calls[0].tool_call_id];
    let default = &batch.calls[&batch.batch.calls[1].tool_call_id];
    approve_file(&f, explicit);
    let engine = f.service().agent_execution_required();
    let (_parent, global) = tokio::sync::watch::channel(false);
    let (explicit_lifetime, explicit_cancel) = engine
        .active_calls
        .register(explicit, global.clone())
        .expect("register the managed execution with its cancellation owner");
    let (default_lifetime, mut default_cancel) = engine
        .active_calls
        .register(default, global.clone())
        .expect("register the managed execution with its cancellation owner");
    set_mode(&f, PermissionMode::FullAccess).await;
    assert!(
        !*explicit_cancel.borrow() && !*default_cancel.borrow(),
        "loosening preserves all prepared calls"
    );
    let stopped = tokio::spawn(async move {
        default_cancel
            .changed()
            .await
            .expect("the withdrawn default permission signals cancellation");
        drop(default_lifetime);
    });
    set_mode(&f, PermissionMode::ReadOnly).await;
    tokio::time::timeout(std::time::Duration::from_secs(1), stopped)
        .await
        .expect("the managed-stop observation completes within its budget")
        .expect("the cancelled default execution releases its lifetime without panicking");
    assert!(!*explicit_cancel.borrow(), "approved writes remain covered after tightening");
    assert!(!*global.borrow(), "no whole-run cancellation");
    drop(explicit_lifetime);
}

#[tokio::test]
async fn s06_e2_an_unconfirmed_end_is_reported_with_a_deadline() {
    let f = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"held.txt","text":"wait","expected":{"expected_kind":"absent"}}),
    )]);
    let engine = f.service().agent_execution_required();
    let (_parent, global) = tokio::sync::watch::channel(false);
    let (held, cancel) = engine
        .active_calls
        .register(
            batch.calls.values().next().expect("the admitted batch contains the requested call"),
            global,
        )
        .expect("register the managed execution with its cancellation owner");
    f.service()
        .persistence
        .set_session_permission_mode(
            f.turn.session_id,
            1,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("tightened mode");
    let result = engine
        .stop_permissions_removed(
            f.turn.session_id,
            PermissionMode::WorkspaceReadWrite,
            std::time::Instant::now() + std::time::Duration::from_millis(50),
        )
        .await;
    assert!(*cancel.borrow());
    assert!(
        result
            .expect_err("an unconfirmed managed stop must report stop_pending")
            .to_string()
            .contains("stop_pending"),
        "no claimed termination while the executor is held"
    );
    drop(held);
}
