//! Editor receipts follow existing file approvals across sessions, without expanding authority.
use super::*;
use aura_domain_common::ApprovalRequestId;
use aura_persistence_v2::file_consents;

fn file_grant(
    f: &mut Fixture,
    session: SessionId,
    target: FileConsentTarget,
) -> AuthorizationGrant {
    let operation = f.operation("shell.cmd", "approval audit identity");
    let principal = PrincipalId::new();
    AuthorizationGrant {
        grant_id: PermissionGrantId::new(),
        revision: Revision::INITIAL,
        issued_at: UtcTimestamp::from_value(f.at.value() - time::Duration::minutes(1)),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
        scope: AuthorizationScope {
            operation_fingerprint: operation.fingerprint.clone(),
            profile: SecurityProfile::FullAccess,
            backend_requirement: BackendRequirement {
                allowed_classes: [BackendClass::CoreBroker].into(),
                guarantees: Default::default(),
            },
            lifetime: AuthorizationLifetime::Session { session_id: session },
            consent: None,
            network_consent: None,
            capability_limit: None,
            memory_consent: None,
            mcp_consent: None,
            chrome_consent: None,
            chrome_site_consent: None,
            file_consent: Some(FileSessionConsentRef {
                approval_id: ApprovalRequestId::new(),
                session_id: session,
                principal_id: principal,
                capabilities: vec![ResolvedCapability::WorkspaceFileWrite],
                targets: vec![target],
                approved_operation_fingerprint: operation.fingerprint,
                grant_revision: Revision::INITIAL,
                revoked_grant_revision: None,
            }),
        },
    }
}

#[test]
fn editor_publication_follows_all_matching_workspace_sessions_and_preserves_other_grants() {
    let mut f = Fixture::new(PermissionMode::ReadOnly);
    let other_session = SessionId::new();
    let foreign_session = SessionId::new();
    let foreign_workspace = WorkspaceId::new();
    let mut foreign = workspace::get(f.store.database.connection(), f.workspace)
        .expect("read the workspace fixture")
        .expect("the fixture workspace exists");
    foreign.workspace_id = foreign_workspace;
    foreign.canonical_root = "C:\\fixture-other-workspace".into();
    f.store
        .database
        .unit_of_work(|tx| {
            workspace::insert(tx, &foreign)?;
            for (id, workspace_id) in
                [(other_session, f.workspace), (foreign_session, foreign_workspace)]
            {
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id,
                        workspace_id,
                        title: "editor follow-up".into(),
                        created_at: f.at,
                        updated_at: f.at,
                    },
                )?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(())
        })
        .expect("seed a second session and an unrelated workspace");
    let before = "object:fixture-volume:original-file";
    let after = "object:fixture-volume:published-file";
    let entry = "entry:fixture-volume:parent:NOTE.TXT";
    let target = FileConsentTarget {
        canonical_path: "C:/fixture/note.txt".into(),
        recursive: false,
        identity_key: before.into(),
        entry_key: entry.into(),
        ancestors: vec!["object:fixture-volume:parent".into()],
        protection: FileConsentProtection::Ordinary,
    };
    let own_session = f.session;
    let mut grants = vec![
        file_grant(&mut f, own_session, target.clone()),
        file_grant(
            &mut f,
            other_session,
            FileConsentTarget { protection: FileConsentProtection::System, ..target.clone() },
        ),
        file_grant(&mut f, foreign_session, target.clone()),
        file_grant(
            &mut f,
            own_session,
            FileConsentTarget { entry_key: "another-hardlink-entry".into(), ..target.clone() },
        ),
        file_grant(&mut f, own_session, target.clone()),
        file_grant(&mut f, other_session, target.clone()),
        file_grant(
            &mut f,
            own_session,
            FileConsentTarget { identity_key: "another-object".into(), ..target },
        ),
    ];
    grants[4].revoked_at = Some(f.at);
    grants[5].expires_at = Some(f.at);
    f.store
        .database
        .unit_of_work(|tx| {
            for grant in &grants {
                security::insert_grant(tx, grant)?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(())
        })
        .expect("seed existing grants with different subjects, entries and liveness");
    f.store
        .database
        .unit_of_work(|tx| {
            file_consents::published_in_workspace(tx, f.workspace, before, after, entry, f.at)
        })
        .expect("consume the real publication facts in the workspace authority store");
    let mut expected = grants.clone();
    for grant in &mut expected[..2] {
        grant.revision = Revision::new(2);
        let scope = grant.scope.file_consent.as_mut().expect("an existing file approval");
        scope.grant_revision = grant.revision;
        scope.targets[0].identity_key = after.into();
    }
    for grant in &expected {
        assert_eq!(
            security::grant(f.store.database.connection(), grant.grant_id)
                .expect("read the published grant")
                .as_ref(),
            Some(grant),
            "only matching live targets follow; capability, scope, protection and audit identity stay exact"
        );
    }
    let count: i64 = f
        .store
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM authorization_grants", [], |row| row.get(0))
        .expect("count the existing approval records");
    assert_eq!(count, grants.len() as i64, "publication cannot create approval");
    f.store
        .database
        .unit_of_work(|tx| {
            file_consents::published_in_workspace(tx, f.workspace, before, after, entry, f.at)
        })
        .expect("re-observing an already applied receipt is idempotent");
    for grant in &expected {
        assert_eq!(
            security::grant(f.store.database.connection(), grant.grant_id)
                .expect("read the idempotent grant")
                .as_ref(),
            Some(grant)
        );
    }
}
