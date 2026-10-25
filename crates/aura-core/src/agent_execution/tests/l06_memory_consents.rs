//! Carries the ReadOnly memory answer into session grant management: the real
//! routes list it per session, withdraw only the exact revision of this session's
//! answer, and the next write asks again.
use super::*;
use crate::r03_tests::{request, success};

const TOOLS: &[&str] = &["memory.create"];

fn create(name: &str) -> serde_json::Value {
    json!({"scope":"global","items":[{"path":format!("rules/{name}.md"),"category":"rules","body":format!("# {name}\n")}]})
}

/// Records "本会话允许" for the call the way the approval route does.
fn allow_for_session(
    f: &Fixture,
    call: &CallContext,
) -> aura_security_contract::AuthorizationGrant {
    use aura_security_contract::{ApprovalState, AuthorizationApproval};
    let engine = f.service().agent_execution_required();
    let backend = engine.broker_backend(call).expect("memory backend").descriptor();
    let PolicyAuthorization::Ask { proposal } = aura_permission_engine::policy::authorize(
        &call.policy(UtcTimestamp::now(), engine.runtime),
        &backend,
        None,
    ) else {
        panic!("a ReadOnly memory write asks")
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
    f.service()
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
        .expect("a session answer records a grant")
}

#[tokio::test]
async fn a_memory_answer_is_listed_per_session_and_withdrawn_by_exact_revision() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, TOOLS).await;
    let batch = f.admit(&[("memory.create", create("first"))]);
    let grant = allow_for_session(&f, batch.calls.values().next().expect("real operation"));
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    let session = f.turn.session_id;
    let list =
        |session: SessionId, all: bool| json!({"session_id": session, "include_inactive": all});

    let listed =
        success(&f.runtime, "security.memory_session_consent.list", list(session, false)).await;
    assert_eq!(listed.as_array().map(Vec::len), Some(1), "{listed}");
    assert_eq!(listed[0]["grant_id"], json!(grant.grant_id));
    assert_eq!(listed[0]["grant_revision"], json!(grant.revision));
    assert_eq!(listed[0]["in_force"], json!(true));
    assert_eq!(listed[0]["scope"]["scope"], json!({"kind":"global"}));
    assert_eq!(listed[0]["revoked_at"], json!(null));

    // Another session neither sees nor withdraws this session's answer.
    let other = f.additional_session(PermissionMode::ReadOnly, TOOLS);
    let elsewhere = other.turn.session_id;
    assert_eq!(
        success(&f.runtime, "security.memory_session_consent.list", list(elsewhere, true)).await,
        json!([])
    );
    let revoke = |session: SessionId, revision: serde_json::Value| json!({"session_id": session, "grant_id": grant.grant_id, "expected_revision": revision});
    for refused in [revoke(elsewhere, json!(grant.revision)), revoke(session, json!("7"))] {
        let response = request(&f.runtime, "security.memory_session_consent.revoke", refused).await;
        assert_eq!(
            response["error"]["data"]["code"], "permission.grant_state_changed",
            "{response}"
        );
    }
    assert_eq!(
        success(&f.runtime, "security.memory_session_consent.list", list(session, false)).await,
        listed,
        "a refused withdrawal changes nothing"
    );

    let revoked = success(
        &f.runtime,
        "security.memory_session_consent.revoke",
        revoke(session, json!(grant.revision)),
    )
    .await;
    assert_eq!(revoked["in_force"], json!(false));
    assert!(revoked["revoked_at"].is_string(), "{revoked}");
    assert_ne!(revoked["grant_revision"], json!(grant.revision));
    let again = request(
        &f.runtime,
        "security.memory_session_consent.revoke",
        revoke(session, revoked["grant_revision"].clone()),
    )
    .await;
    assert_eq!(again["error"]["data"]["code"], "permission.grant_state_changed", "{again}");
    assert_eq!(
        success(&f.runtime, "security.memory_session_consent.list", list(session, false)).await,
        json!([])
    );
    let history =
        success(&f.runtime, "security.memory_session_consent.list", list(session, true)).await;
    assert_eq!(history.as_array().map(Vec::len), Some(1), "{history}");
    assert_eq!(history[0]["in_force"], json!(false));

    let next = f.admit(&[("memory.create", create("second"))]);
    assert!(matches!(f.execute_rejecting(next).await, Some(BatchOutcome::AwaitingApproval { .. })));
    let vault = f.service().broker.memory().roots().global.clone();
    assert!(vault.join("rules/first.md").is_file(), "the approved write happened");
    assert!(!vault.join("rules/second.md").exists(), "a withdrawn answer covers nothing");
}
