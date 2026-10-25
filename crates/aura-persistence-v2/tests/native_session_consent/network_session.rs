//! Included by the Store consent suite to reuse its real SQLite/dispatch fixture.
use super::*;
#[path = "network_store_authority.rs"]
mod store_authority;
use aura_permission_engine::policy::{PolicyInput, authorize};

fn operation(f: &mut Fixture) -> SecuredOperation {
    let mut content = f.operation("shell.cmd", "unused").content;
    content.action_kind = "http.request".into();
    content.action = serde_json::json!({"url":"http://localhost:43210/"});
    content.network = OperationNetwork::ExactOrigins;
    content.capabilities = vec![ResolvedCapability::OrdinaryNetwork];
    content.resources = vec![ResourceDescriptor::NetworkOrigin {
        scheme: "http".into(),
        host: "localhost".into(),
        port: 43210,
    }];
    SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content)
        .expect("ordinary network operation")
}
fn backend(f: &Fixture) -> BackendDescriptor {
    let mut backend = f.native_backend();
    backend.class = BackendClass::CoreBroker;
    backend.capabilities = [ResolvedCapability::OrdinaryNetwork].into();
    backend.guarantees = [
        EnforcementGuarantee::AuthorizedReadsOnly,
        EnforcementGuarantee::AuthorizedWritesOnly,
        EnforcementGuarantee::ExactNetworkOrigins,
    ]
    .into();
    backend
}
fn decision(f: &Fixture, op: &SecuredOperation) -> PolicyAuthorization {
    let snapshot = security::snapshot(
        f.store.database.connection(),
        op.content.identity.permission_snapshot_id.expect("snapshot id"),
    )
    .expect("read snapshot")
    .expect("stored snapshot");
    let grants =
        security::grants_for_call(f.store.database.connection(), &op.fingerprint, Some(f.session))
            .expect("live candidates");
    authorize(
        &PolicyInput {
            network_caller: NetworkCallerKind::AuraTool,
            operation: op,
            snapshot: &snapshot,
            profile: SecurityProfile::FullAccess,
            source_enabled: true,
            trust: &TrustState::SystemTrusted,
            source_fingerprint: None,
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &grants,
            verified_external_capabilities: &[],
            verified_external_grants: &[],
            now: f.at,
            expires_at: UtcTimestamp::from_value(f.at.value() + time::Duration::seconds(30)),
            core_runtime_instance_id: Some(f.runtime),
        },
        &backend(f),
        None,
    )
}
fn approve(
    f: &mut Fixture,
    op: &SecuredOperation,
    choice: ApprovalDecisionKind,
) -> AuthorizationGrant {
    let PolicyAuthorization::Ask { proposal } = decision(f, op) else {
        panic!("missing network authority")
    };
    let request = AuthorizationApproval {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: f.at,
        expires_at: UtcTimestamp::from_value(f.at.value() + time::Duration::minutes(1)),
    };
    f.store
        .database
        .unit_of_work(|tx| {
            security::insert_approval(tx, &request)?;
            security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                op,
                choice,
                BackendClass::CoreBroker,
                f.snapshot.session_permission_revision,
                f.runtime,
                f.at,
            )
        })
        .expect("resolve actual approval")
        .expect("issued grant")
}

#[test]
fn s07_n13_persisted_network_consent_survives_runtime_and_mode_revisions_but_not_withdrawal() {
    let mut f = Fixture::new(PermissionMode::ReadOnly);
    let first = operation(&mut f);
    let grant = approve(&mut f, &first, ApprovalDecisionKind::AllowSession);
    assert!(grant.scope.network_consent.is_some());
    assert!(grant.expires_at.is_none());
    for mode in [PermissionMode::FullAccess, PermissionMode::ReadOnly] {
        f.runtime = RuntimeInstanceId::new();
        f.snapshot.mode = mode;
        f.snapshot.session_permission_revision += 1;
        let next = operation(&mut f);
        let PolicyAuthorization::Allow { authorization } = decision(&f, &next) else {
            panic!("durable consent covers both mode directions")
        };
        let backend = backend(&f);
        let revision = f.snapshot.session_permission_revision;
        f.reserve(&next, &backend, &authorization, revision).expect("real attempt reservation");
    }
    f.store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::network_consents::revoke(
                tx,
                f.session,
                grant.grant_id,
                grant.revision,
                f.at,
            )
        })
        .expect("withdrawal");
    let next = operation(&mut f);
    assert!(matches!(decision(&f, &next), PolicyAuthorization::Ask { .. }));
}

#[test]
fn s07_n13_foreign_principal_or_session_and_once_reuse_do_not_inherit_network_authority() {
    for once in [false, true] {
        let mut f = Fixture::new(PermissionMode::ReadOnly);
        let first = operation(&mut f);
        approve(
            &mut f,
            &first,
            if once { ApprovalDecisionKind::AllowOnce } else { ApprovalDecisionKind::AllowSession },
        );
        let next = operation(&mut f);
        if once {
            assert!(matches!(decision(&f, &next), PolicyAuthorization::Ask { .. }));
        } else {
            let mut content = next.content;
            content.identity.principal_id = PrincipalId::new();
            let foreign =
                SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content)
                    .expect("foreign identity");
            assert!(matches!(decision(&f, &foreign), PolicyAuthorization::Ask { .. }));
        }
        // A real second session of the same workspace, querying with its own operation: an
        // unknown session id returns before the consent query runs, and the first session's
        // fingerprint names that session's own exact approval.
        let other = SessionId::new();
        f.store
            .database
            .unit_of_work(|tx| {
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id: other,
                        workspace_id: f.workspace,
                        title: "other session".into(),
                        created_at: f.at,
                        updated_at: f.at,
                    },
                )
            })
            .expect("second session in the same workspace");
        let mut content = operation(&mut f).content;
        content.identity.session_id = Some(other);
        let in_other =
            SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content)
                .expect("operation of the second session");
        assert!(
            security::grants_for_call(
                f.store.database.connection(),
                &in_other.fingerprint,
                Some(other)
            )
            .expect("foreign session query")
            .iter()
            .all(|g| g.scope.network_consent.is_none()),
            "session consent is never offered to a different session"
        );
    }
}

#[test]
fn s07_n7_store_rejects_a_composite_decision_missing_or_forging_a_slice() {
    for mutation in ["missing", "caller", "resource", "grant"] {
        let mut f = Fixture::new(PermissionMode::ReadOnly);
        let op = operation(&mut f);
        approve(&mut f, &op, ApprovalDecisionKind::AllowSession);
        let PolicyAuthorization::Allow { mut authorization } = decision(&f, &op) else {
            panic!("approved operation")
        };
        let AuthorizationBasis::Composite { slices } = &mut authorization.basis else {
            panic!("per-capability authority")
        };
        match mutation {
            "missing" => slices.clear(),
            "caller" => slices[0].caller = NetworkCallerKind::ModelTransport,
            "resource" => slices[0].resources.clear(),
            _ => {
                if let CapabilityAuthority::Grant { revision, .. } = &mut slices[0].authority {
                    *revision = Revision::new(99);
                }
            }
        }
        let backend = backend(&f);
        let revision = f.snapshot.session_permission_revision;
        assert!(
            f.reserve(&op, &backend, &authorization, revision).is_err(),
            "{mutation} cannot reserve an attempt"
        );
        assert_eq!(
            f.store
                .database
                .connection()
                .query_row("SELECT count(*) FROM execution_attempts", [], |r| r.get::<_, i64>(0))
                .expect("attempt count"),
            0
        );
    }
}
