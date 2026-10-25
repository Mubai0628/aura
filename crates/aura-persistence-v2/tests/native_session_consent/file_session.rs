use super::*;

#[test]
fn s06_b2_store_offers_a_file_consent_to_a_new_operation_in_the_same_session() {
    let mut f = Fixture::new(PermissionMode::ReadOnly);
    let mut content = f.operation("shell.cmd", "first").content;
    content.action_kind = "fs.write".into();
    content.network = OperationNetwork::None;
    content.capabilities = vec![ResolvedCapability::WorkspaceFileWrite];
    content.resources = vec![ResourceDescriptor::WorkspacePath {
        workspace_id: f.workspace,
        canonical_relative_path: aura_domain_common::WorkspacePath::parse("note.txt").unwrap(),
        file_target: Some(aura_security_contract::FileConsentTarget {
            canonical_path: (aura_domain_common::WorkspacePath::parse("note.txt").unwrap())
                .as_str()
                .to_owned(),
            identity_key: format!(
                "fixture:{}",
                (aura_domain_common::WorkspacePath::parse("note.txt").unwrap()).as_str()
            ),
            ..Default::default()
        }),
    }];
    let approved =
        SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content.clone())
            .unwrap();
    // Ordinary file calls use a CoreBroker. Native cannot enforce the ReadOnly
    // ceiling and is deliberately unavailable for a non-process file call.
    let mut backend = f.native_backend();
    backend.class = BackendClass::CoreBroker;
    backend.guarantees =
        [EnforcementGuarantee::AuthorizedReadsOnly, EnforcementGuarantee::AuthorizedWritesOnly]
            .into();
    let approval = AuthorizationApproval {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: AuthorizationScope {
            operation_fingerprint: approved.fingerprint.clone(),
            profile: SecurityProfile::FullAccess,
            backend_requirement: BackendRequirement {
                allowed_classes: [BackendClass::CoreBroker].into(),
                guarantees: backend.guarantees.clone(),
            },
            lifetime: AuthorizationLifetime::Once { operation_id: approved.operation_id },
            consent: None,
            network_consent: None,
            capability_limit: None,
            memory_consent: None,
            mcp_consent: None,
            file_consent: None,
        },
        allowed_decisions: vec![ApprovalDecisionKind::AllowSession],
        state: ApprovalState::Pending,
        requested_at: f.at,
        expires_at: UtcTimestamp::from_value(f.at.value() + time::Duration::minutes(10)),
    };
    let grant = f
        .store
        .database
        .unit_of_work(|tx| {
            security::insert_approval(tx, &approval)?;
            security_approval::resolve(
                tx,
                approval.approval_id,
                approval.revision,
                &approved,
                ApprovalDecisionKind::AllowSession,
                backend.class,
                f.snapshot.session_permission_revision,
                f.runtime,
                f.at,
            )
        })
        .unwrap()
        .unwrap();
    assert!(grant.scope.file_consent.is_some());
    content.action = serde_json::json!({"text":"second"});
    let next =
        SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content).unwrap();
    let offered = security::grants_for_call(
        f.store.database.connection(),
        &next.fingerprint,
        Some(f.session),
    )
    .unwrap();
    assert!(
        offered.iter().any(|g| g.grant_id == grant.grant_id),
        "file consents must be queried by session"
    );
    assert!(
        security::grants_for_call(
            f.store.database.connection(),
            &next.fingerprint,
            Some(SessionId::new())
        )
        .unwrap()
        .is_empty()
    );
    let decision = AuthorizationDecision::from_grant_for_operation(
        &grant,
        &next.fingerprint,
        f.at,
        UtcTimestamp::from_value(f.at.value() + time::Duration::minutes(5)),
    )
    .unwrap();
    f.reserve(&next, &backend, &decision, f.snapshot.session_permission_revision)
        .expect("a new operation reserves under its session file consent");
    let request = reserved_request(&f, next.operation_id);
    assert_eq!(
        aura_persistence_v2::execution::dispatch_authority(
            f.store.database.connection(),
            &request,
            f.at,
        )
        .expect("a reserved reusable file consent must remain valid at actual dispatch"),
        decision
    );
    f.store
        .database
        .unit_of_work(|tx| security::revoke(tx, grant.grant_id, grant.revision, None, f.at))
        .unwrap();
    assert!(
        aura_persistence_v2::execution::dispatch_authority(
            f.store.database.connection(),
            &request,
            f.at,
        )
        .is_err(),
        "revocation after reservation still blocks actual dispatch"
    );
}

#[test]
fn s06_b2_native_consent_uses_the_same_derived_scope_at_actual_dispatch() {
    let mut f = Fixture::new(PermissionMode::FullAccess);
    let first = f.operation("shell.cmd", "echo first");
    f.approve(&first, ApprovalDecisionKind::AllowSession);
    let next = f.operation("shell.powershell", "Write-Output second");
    let (_, decision) = f.derive(&next).unwrap();
    let backend = f.native_backend();
    f.reserve(&next, &backend, &decision, f.snapshot.session_permission_revision)
        .expect("Native consent reservation");
    let request = reserved_request(&f, next.operation_id);
    assert_eq!(
        aura_persistence_v2::execution::dispatch_authority(
            f.store.database.connection(),
            &request,
            f.at,
        )
        .expect("the dispatch boundary also honors a derived Native scope"),
        decision
    );
}

fn reserved_request(
    f: &Fixture,
    operation: aura_domain_common::SecurityOperationId,
) -> DispatchRequest {
    let payload: String = f.store.database.connection().query_row(
        "SELECT payload FROM operation_journal_entries WHERE operation_id=?1 AND kind='dispatch_prepared'",
        [operation.to_string()], |row| row.get(0),
    ).unwrap();
    serde_json::from_str(&payload).unwrap()
}
