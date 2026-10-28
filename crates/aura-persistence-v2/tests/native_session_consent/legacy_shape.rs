use super::*;

/// A row written by the predecessor single-Shell shape must not poison the table.
///
/// The predecessor recorded `{principal_id, shell}`. Every query that touches
/// `native_session_consents` decodes the row before it can judge it, so a shape the
/// reader refuses turns one inert old row into a failure of the whole call lookup and
/// the whole management list — including for a restarted Core, whose new runtime is
/// supposed to make the old row *irrelevant* rather than fatal.
#[test]
fn a_predecessor_single_shell_row_does_not_poison_calls_or_the_management_list() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.cmd", "echo old");
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let grant = legacy::legacy(&mut fixture, grants).remove(0);
    let consent =
        security::native_session_consent(fixture.store.database.connection(), grant.grant_id)
            .expect("consent")
            .expect("present");
    // The predecessor wire shape, written into both rows that carry it.
    let predecessor = serde_json::json!({ "principal_id": fixture.principal, "shell": "cmd" });
    let mut grant_json = serde_json::to_value(&grant).expect("grant json");
    grant_json["scope"]["consent"]["approved"] = predecessor.clone();
    let mut consent_json = serde_json::to_value(&consent).expect("consent json");
    consent_json["approved"] = predecessor;
    let connection = fixture.store.database.connection();
    connection
        .execute(
            "UPDATE authorization_grants SET payload=?1 WHERE id=?2",
            rusqlite::params![grant_json.to_string(), grant.grant_id.to_string()],
        )
        .expect("rewrite grant");
    connection
        .execute(
            "UPDATE native_session_consents SET payload=?1 WHERE grant_id=?2",
            rusqlite::params![consent_json.to_string(), grant.grant_id.to_string()],
        )
        .expect("rewrite consent");
    // A restarted Core: the row is inert either way, and reading it must succeed.
    fixture.runtime = RuntimeInstanceId::new();
    let new_call = fixture.operation("shell.powershell", "Write-Output new");
    let lookup = security::grants_for_call(
        fixture.store.database.connection(),
        &new_call.fingerprint,
        Some(fixture.session),
    );
    assert!(
        lookup.is_ok(),
        "an inert predecessor row must not break new calls: {:?}",
        lookup.as_ref().err()
    );
    let listing =
        security::native_session_consents(fixture.store.database.connection(), fixture.session);
    assert!(
        listing.is_ok(),
        "an inert predecessor row must not break the management list: {:?}",
        listing.as_ref().err()
    );
    // Read back, the predecessor row still means exactly what it meant: one Shell.
    let stored =
        security::native_session_consent(fixture.store.database.connection(), grant.grant_id)
            .expect("consent")
            .expect("present");
    assert_eq!(stored.approved.approved_shell, NativeShell::Cmd);
    assert!(!stored.approved.covers(NativeShell::PowerShell));
    assert_eq!(
        fixture.consent_view(grant.grant_id).expect("the record still projects").availability,
        NativeSessionConsentAvailability::Superseded,
        "the restarted Core supersedes it, and reading it did not fail"
    );
}

#[test]
fn a_session_approval_of_a_restricted_backend_does_not_mint_a_consent() {
    // The consent describes the reduced-isolation Native path. A session approval of
    // a restricted backend is a different answer, and the class travels with the
    // decision so the store cannot infer it from the tool name.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let requested_at = fixture.at;
    let approval = AuthorizationApproval {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: AuthorizationScope {
            operation_fingerprint: first.fingerprint.clone(),
            profile: SecurityProfile::FullAccess,
            backend_requirement: BackendRequirement {
                allowed_classes: [BackendClass::RestrictedUserOnline].into(),
                guarantees: Default::default(),
            },
            lifetime: AuthorizationLifetime::Once { operation_id: first.operation_id },
            consent: None,
            network_consent: None,
            capability_limit: None,
            memory_consent: None,
            mcp_consent: None,
            chrome_consent: None,
            chrome_site_consent: None,
            file_consent: None,
        },
        allowed_decisions: vec![
            ApprovalDecisionKind::AllowOnce,
            ApprovalDecisionKind::AllowSession,
        ],
        state: ApprovalState::Pending,
        requested_at,
        expires_at: UtcTimestamp::from_value(requested_at.value() + time::Duration::minutes(10)),
    };
    let at = fixture.at;
    let revision = fixture.snapshot.session_permission_revision;
    let runtime = fixture.runtime;
    let grants = fixture
        .store
        .database
        .unit_of_work(|tx| {
            security::insert_approval(tx, &approval)?;
            security_approval::resolve(
                tx,
                approval.approval_id,
                Revision::INITIAL,
                &first,
                ApprovalDecisionKind::AllowSession,
                BackendClass::RestrictedUserOnline,
                revision,
                runtime,
                at,
            )
        })
        .expect("resolution")
        .expect("a grant");
    assert!(
        grants.scope.consent.is_none(),
        "a restricted session approval keeps its exact-operation meaning"
    );
    assert!(
        security::native_session_consents(fixture.store.database.connection(), fixture.session)
            .expect("consents")
            .is_empty()
    );
}
