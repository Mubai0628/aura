use super::*;

fn policy(f: &Fixture, call: &CallContext) -> PolicyAuthorization {
    let engine = f.service().agent_execution_required();
    let grants = aura_persistence_v2::security::grants_for_call(
        f.service().broker.lock().expect("grant candidates").database.connection(),
        &call.operation.fingerprint,
        Some(f.turn.session_id),
    )
    .expect("grants");
    let mut input = call.policy(UtcTimestamp::now(), engine.runtime);
    input.grants = &grants;
    aura_permission_engine::policy::authorize(
        &input,
        &engine.broker_backend(call).expect("Git backend").descriptor(),
        None,
    )
}

#[tokio::test]
async fn normal_push_session_scope_reuses_only_the_same_physical_repository_url_and_branch() {
    let (_temporary, url) = remote();
    let mut f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["git.push"], |root| {
            initialize(root);
            git(root, &["remote", "add", "origin", &url]);
        })
        .await;
    let args = json!({"remote":"origin","remote_branch":"main"});
    let first = f.admit(&[("git.push", args.clone())]);
    let id = first.batch.calls[0].tool_call_id;
    let batch_id = first.batch.tool_batch_id;
    let grant = approve(&f, &first.calls[&id], ApprovalDecisionKind::AllowSession);
    assert_eq!(
        grant.scope.network_consent.as_ref().expect("existing network consent class").caller,
        aura_security_contract::NetworkConsentCaller::Git
    );
    let group = f.execute(first).await;
    f.output(id);
    for mode in [PermissionMode::FullAccess, PermissionMode::ReadOnly] {
        let current = f
            .service()
            .persistence
            .session_permission_state(f.turn.session_id)
            .expect("mode state");
        crate::r03_tests::success(&f.runtime, "permission.set_session_mode", json!({"session_id":f.turn.session_id,"expected_revision":current.revision,"mode":mode})).await;
    }
    Arc::get_mut(&mut f._host).expect("fixture owns its temporary host").renew_runtime_instance();
    f.runtime = Arc::new(f._host.start().await);
    f.continue_after_tools(batch_id, &group);
    let second = f.admit(&[("git.push", args)]);
    let id = second.batch.calls[0].tool_call_id;
    let batch_id = second.batch.tool_batch_id;
    let call = &second.calls[&id];
    assert!(matches!(policy(&f, call), PolicyAuthorization::Allow { .. }));
    for change in ["branch", "repository", "url", "force"] {
        let mut changed = call.clone();
        if let ResourceDescriptor::GitRemoteDestination { facts } =
            &mut changed.operation.content.resources[1]
        {
            match change {
                "branch" => facts.target.remote_branch = "other".into(),
                "repository" => {
                    facts.target.repository =
                        aura_domain_common::jcs::canonical_sha256(&"other repo").expect("hash")
                }
                "url" => facts.target.remote_url = "https://other.invalid/repo".into(),
                _ => {
                    facts.force_with_lease = Some("a".repeat(40));
                    changed.operation.content.capabilities.push(ResolvedCapability::GitDestructive);
                }
            }
        }
        changed.operation =
            SecuredOperation::new(changed.operation.operation_id, changed.operation.content)
                .expect("changed operation");
        assert!(
            matches!(policy(&f, &changed), PolicyAuthorization::Ask { .. }),
            "{change} cannot borrow the consent"
        );
    }
    let group = f.execute(second).await;
    f.output(id);
    {
        let mut store = f.service().broker.lock().expect("revoke store");
        assert_eq!(
            aura_persistence_v2::network_consents::list(
                store.database.connection(),
                f.turn.session_id
            )
            .expect("list")
            .len(),
            1
        );
        store
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::network_consents::revoke(
                    tx,
                    f.turn.session_id,
                    grant.grant_id,
                    grant.revision,
                    UtcTimestamp::now(),
                )
            })
            .expect("revoke existing class");
    }
    f.continue_after_tools(batch_id, &group);
    let third = f.admit(&[("git.push", json!({"remote":"origin","remote_branch":"main"}))]);
    assert!(matches!(
        f.execute_rejecting(third).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
}

#[tokio::test]
async fn an_exact_lease_asks_each_time_and_once_approval_can_actually_dispatch() {
    let (temporary, url) = remote();
    let mut f = Fixture::with_exposure_setup(PermissionMode::FullAccess, &["git.push"], |root| {
        initialize(root);
        git(root, &["remote", "add", "origin", &url]);
        git(root, &["push", "origin", "main"]);
    })
    .await;
    let expected = oid(&f.root, "HEAD");
    let args = json!({"remote":"origin","remote_branch":"main","force_with_lease":expected});
    let first = f.admit(&[("git.push", args.clone())]);
    let id = first.batch.calls[0].tool_call_id;
    let batch_id = first.batch.tool_batch_id;
    let PolicyAuthorization::Ask { proposal } = policy(&f, &first.calls[&id]) else {
        panic!("force always asks");
    };
    assert_eq!(
        proposal.allowed_decisions,
        [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );
    approve(&f, &first.calls[&id], ApprovalDecisionKind::AllowOnce);
    let group = f.execute(first).await;
    f.output(id);
    assert_eq!(oid(&temporary.path().join("remote.git"), "refs/heads/main"), expected);
    f.continue_after_tools(batch_id, &group);
    let next = f.admit(&[("git.push", args)]);
    assert!(matches!(f.execute_rejecting(next).await, Some(BatchOutcome::AwaitingApproval { .. })));
}
