use super::*;
use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_persistence::ToolCredentialScopeSource;
use aura_tool_domain::{ToolCredentialCreateRequestV1, ToolCredentialScopeV1};

#[test]
fn s05_tool_credential_scope_uses_the_real_agent_store_without_a_second_session_row() {
    let log_root = tempfile::tempdir().expect("obtain the actual tool-credential scope fact");
    let _route = crate::CoreLogRoute::to(log_root.path().join("core.jsonl"));
    let fixture = crate::p07_test_support::BuiltinHttpCredentialFixtureV1::new()
        .expect("obtain the actual tool-credential scope fact");
    let root = tempfile::tempdir().expect("obtain the actual tool-credential scope fact");
    let journal = Arc::new(
        crate::broker::BrokerJournal::open(&root.path().join("journal"))
            .expect("obtain the actual tool-credential scope fact"),
    );
    let workspace = fixture
        .persistence
        .workspace_session_projection()
        .expect("obtain the actual tool-credential scope fact")
        .workspaces[0]
        .clone();
    let session = SessionId::new();
    let now = UtcTimestamp::now();
    journal
        .lock()
        .expect("obtain the actual tool-credential scope fact")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::workspace::insert(tx, &workspace)?;
            aura_persistence_v2::workspace::insert_session(
                tx,
                &aura_persistence_v2::workspace::SessionMetadata {
                    id: session,
                    workspace_id: workspace.workspace_id,
                    title: "V2-only ownership".into(),
                    created_at: now,
                    updated_at: now,
                },
            )
        })
        .expect("obtain the actual tool-credential scope fact");
    let store = Arc::new(crate::agent_store::AgentStore::new(
        journal.clone(),
        crate::sync_boundary::SyncBoundary::open(),
    ));
    fixture
        .persistence
        .bind_tool_credential_scope_owner(store.clone() as Arc<dyn ToolCredentialScopeSource>)
        .expect("obtain the actual tool-credential scope fact");
    let db = rusqlite::Connection::open(fixture.persistence.database_path())
        .expect("obtain the actual tool-credential scope fact");
    let duplicate: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM sessions WHERE session_id=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .expect("obtain the actual tool-credential scope fact");
    assert_eq!(duplicate, 0, "never populate the wrong DB to satisfy scope lookup");
    assert!(
        store
            .session_belongs_to(workspace.workspace_id, session)
            .expect("obtain the actual tool-credential scope fact")
    );
    let target = ToolCredentialManagementTargetV1 {
        tool_instance_id: fixture.instance.tool_instance_id,
        expected_instance_revision: fixture.instance.entity_revision,
        credential_slot_key: aura_tool_contract::CredentialSlotKey::new("api_key")
            .expect("obtain the actual tool-credential scope fact"),
        placement: aura_tool_domain::ToolCredentialPlacementV1::HttpBearerAuthorization,
        workspace_id: Some(workspace.workspace_id),
        session_id: Some(session),
    };
    struct Cleanup(Option<aura_domain_common::CredentialRef>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(reference) = &self.0 {
                let _ = WindowsCredentialStore.delete(reference);
            }
        }
    }
    let mut cleanup = Cleanup(None);
    let service = ToolCredentialService::new(Arc::new(fixture.persistence.clone()));
    let secret = SecretBytes::new(b"s05-scope-only-synthetic-secret".to_vec());
    let created = service
        .mutate(
            CommandInvocationId::new(),
            ToolCredentialMutationIntentV1::Create {
                request: ToolCredentialCreateRequestV1 {
                    allowed_origins: Vec::new(),
                    target: target.clone(),
                    display_name: "session-only".into(),
                    scope: ToolCredentialScopeV1::Session {
                        workspace_id: workspace.workspace_id,
                        session_id: session,
                    },
                },
            },
            Some(&secret),
        )
        .expect("obtain the actual tool-credential scope fact")
        .credential
        .expect("obtain the actual tool-credential scope fact");
    cleanup.0 = Some(
        fixture
            .persistence
            .authorized_tool_secret_binding_v1(
                target.clone(),
                created.credential_id,
                created.credential_revision,
            )
            .expect("obtain the actual tool-credential scope fact")
            .1,
    );
    assert_eq!(
        service
            .read_authorized(target.clone(), created.credential_id, created.credential_revision)
            .expect("obtain the actual tool-credential scope fact")
            .expose(),
        secret.expose()
    );
    let mut wrong = target.clone();
    wrong.workspace_id = Some(WorkspaceId::new());
    assert!(
        service.read_authorized(wrong, created.credential_id, created.credential_revision).is_err()
    );
    let mut wrong = target.clone();
    wrong.session_id = Some(SessionId::new());
    assert!(
        service.read_authorized(wrong, created.credential_id, created.credential_revision).is_err()
    );
    service
        .mutate(
            CommandInvocationId::new(),
            ToolCredentialMutationIntentV1::Clear {
                request: aura_tool_domain::ToolCredentialChangeRequestV1 {
                    target,
                    credential_id: created.credential_id,
                    expected_row_revision: created.row_revision,
                },
            },
            None,
        )
        .expect("obtain the actual tool-credential scope fact");
    // No request manufactured a Model credential or a second task/session record.
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM sessions WHERE session_id=?1",
            [session.to_string()],
            |row| row.get::<_, i64>(0)
        )
        .expect("obtain the actual tool-credential scope fact"),
        0
    );
}
