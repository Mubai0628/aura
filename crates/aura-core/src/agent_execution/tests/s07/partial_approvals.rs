use super::*;

#[tokio::test]
async fn s07_n7_file_session_approval_and_a_network_once_answer_combine_without_reuse() {
    let server = Server::start().await;
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::ReadOnly,
        &["fs.write", "http.download"],
        |root| {
            std::fs::write(root.join("approved.txt"), "before").expect("file target");
        },
    )
    .await;
    let write=f.admit(&[("fs.write",json!({"path":"approved.txt","text":"approved file","expected":{"expected_kind":"file_hash","hash":aura_domain_common::ContentHash::blake3(b"before")}}))]);
    let id = write.batch.calls[0].tool_call_id;
    approve(&f, &write.calls[&id], ApprovalDecisionKind::AllowSession);
    let batch_id = write.batch.tool_batch_id;
    let group = f.execute(write).await;
    f.output(id);
    f.continue_after_tools(batch_id, &group);

    let download = f.admit(&[(
        "http.download",
        json!({"url":format!("{}/binary/10",server.url),"path":"approved.txt","overwrite":true}),
    )]);
    let id = download.batch.calls[0].tool_call_id;
    let call = &download.calls[&id];
    let engine = f.service().agent_execution_required();
    let grants = aura_persistence_v2::security::grants_for_call(
        f.service().broker.lock().expect("file consent candidates").database.connection(),
        &call.operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("live authorities");
    let mut policy = call.policy(UtcTimestamp::now(), engine.runtime);
    policy.grants = &grants;
    let PolicyAuthorization::Ask { proposal } = aura_permission_engine::policy::authorize(
        &policy,
        &broker::descriptor(engine.runtime),
        None,
    ) else {
        panic!("only network remains missing")
    };
    assert_eq!(proposal.requested_capabilities, Some(vec![ResolvedCapability::OrdinaryNetwork]));
    assert_eq!(server.count(), 0);
    approve(&f, call, ApprovalDecisionKind::AllowOnce);
    let batch_id = download.batch.tool_batch_id;
    let group = f.execute(download).await;
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("approved.txt")).expect("combined-authority effect"),
        vec![b'Z'; 10]
    );
    f.continue_after_tools(batch_id, &group);
    let next = f.admit(&[(
        "http.download",
        json!({"url":server.url,"path":"approved.txt","overwrite":true}),
    )]);
    let id = next.batch.calls[0].tool_call_id;
    assert!(matches!(f.execute_rejecting(next).await, Some(BatchOutcome::AwaitingApproval { .. })));
    no_attempt(&f, id);
    assert_eq!(server.count(), 1, "network Once is not reused by the next operation");
}
