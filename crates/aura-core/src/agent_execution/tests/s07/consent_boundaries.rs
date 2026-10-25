use super::*;

#[tokio::test]
async fn s07_n7_network_approval_asks_only_for_the_missing_file_capability() {
    let server = Server::start().await;
    let mut f = fixture(PermissionMode::ReadOnly, &["http.request", "http.download"]).await;
    let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.output(id);
    f.continue_after_tools(batch_id, &group);
    let batch = f.admit(&[(
        "http.download",
        json!({"url":format!("{}/binary/10",server.url),"path":"approved.bin"}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let call = &batch.calls[&id];
    let engine = f.service().agent_execution_required();
    let grants = aura_persistence_v2::security::grants_for_call(
        f.service().broker.lock().expect("read approvals").database.connection(),
        &call.operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("live approval candidates");
    let mut policy = call.policy(UtcTimestamp::now(), engine.runtime);
    policy.grants = &grants;
    let PolicyAuthorization::Ask { proposal } = aura_permission_engine::policy::authorize(
        &policy,
        &broker::descriptor(engine.runtime),
        None,
    ) else {
        panic!("file capability remains missing")
    };
    assert_eq!(
        proposal.requested_capabilities,
        Some(vec![ResolvedCapability::WorkspaceFileCreate])
    );
    assert_eq!(server.count(), 1);
    approve(&f, call, ApprovalDecisionKind::AllowOnce);
    f.execute(batch).await;
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("approved.bin")).expect("approved download"),
        vec![b'Z'; 10]
    );
}

#[tokio::test]
async fn s07_n13_revocation_expiry_and_old_scope_cannot_authorize_a_new_network_request() {
    for change in ["revoked", "expired", "legacy"] {
        let server = Server::start().await;
        let f = fixture(PermissionMode::ReadOnly, &["http.request"]).await;
        let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
        let id = batch.batch.calls[0].tool_call_id;
        let grant = approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
        let mut grant = grant;
        match change {
            "revoked" => {
                grant.revoked_at = Some(UtcTimestamp::now());
            }
            "expired" => {
                grant.expires_at = Some(UtcTimestamp::from_value(
                    UtcTimestamp::now().value() - time::Duration::seconds(1),
                ));
            }
            _ => {
                grant.scope.network_consent = None;
                grant.scope.capability_limit = Some(vec![ResolvedCapability::PublicNetworkRead]);
            }
        }
        // Pass the historical authority to the real policy. Reusable network matching must
        // independently reject it, even if a caller offers it as a candidate.
        let call = &batch.calls[&id];
        let engine = f.service().agent_execution_required();
        let candidates = [grant];
        let mut policy = call.policy(UtcTimestamp::now(), engine.runtime);
        policy.grants = &candidates;
        assert!(
            matches!(
                aura_permission_engine::policy::authorize(
                    &policy,
                    &broker::descriptor(engine.runtime),
                    None
                ),
                PolicyAuthorization::Ask { .. }
            ),
            "{change} is not current network authority"
        );
        assert_eq!(server.count(), 0);
        no_attempt(&f, id);
    }
}

#[tokio::test]
async fn s07_n7_combined_session_approval_creates_independently_revocable_file_and_network_records()
{
    let server = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["http.download"]).await;
    let batch = f.admit(&[("http.download", json!({"url":server.url,"path":"independent.bin"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let mut store = f.service().broker.lock().expect("approval store");
    let network =
        aura_persistence_v2::network_consents::list(store.database.connection(), f.turn.session_id)
            .expect("network records");
    assert_eq!(network.len(), 1);
    assert!(network[0].scope.file_consent.is_none());
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::network_consents::revoke(
                tx,
                f.turn.session_id,
                network[0].grant_id,
                network[0].revision,
                UtcTimestamp::now(),
            )
        })
        .expect("network-only revoke");
    let grants = aura_persistence_v2::security::grants_for_call(
        store.database.connection(),
        &batch.calls[&id].operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("remaining file authority");
    assert!(
        grants.iter().any(
            |grant| grant.scope.file_consent.is_some() && grant.is_live_at(UtcTimestamp::now())
        )
    );
}
