use std::str::FromStr;

use aura_domain_common::{
    CorrelationId, ExternalGrantId, FileIdentity, PermissionGrantId, Revision, RuntimeInstanceId,
    SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_permission_domain::{
    ExternalGrantCapability, ExternalGrantLifecycle, ExternalGrantRecord, ExternalGrantTargetKind,
    ExternalGrantTerm,
};
use aura_persistence::{
    PersistenceCoordinator, PersistenceError, PersistenceOptions, PersistenceStartup,
};
use aura_security_contract::{CanonicalPath, PermissionMode};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use tempfile::TempDir;

fn start(root: &TempDir) -> (PersistenceCoordinator, WorkspaceId, SessionId) {
    let options = PersistenceOptions::new(
        root.path().join("data/aura.db"),
        root.path().join("data/artifacts"),
        RuntimeInstanceId::new(),
    );
    let store = match PersistenceCoordinator::start(options).expect("start fixture store") {
        PersistenceStartup::Ready(store) => store,
        PersistenceStartup::RecoveryRequired(_) => panic!("fixture unexpectedly needs recovery"),
    };
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    store
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id: workspace,
                display_name: "Attachment grants".to_owned(),
                selected_root: r"C:\Attachment Tests\Workspace".to_owned(),
                canonical_root: r"C:\Attachment Tests\Workspace".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id: session,
                workspace_id: workspace,
                title: "Files".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("seed fixture session");
    store
        .ensure_session_permission_state(
            session,
            workspace,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("read-only session");
    (store, workspace, session)
}

fn path(value: &str) -> CanonicalPath {
    CanonicalPath { display: value.to_owned(), utf16: value.encode_utf16().collect() }
}

fn candidate(workspace: WorkspaceId, session: SessionId, location: &str) -> ExternalGrantRecord {
    let id = ExternalGrantId::new();
    let now = UtcTimestamp::now();
    ExternalGrantRecord {
        id,
        session_id: session,
        workspace_id: workspace,
        permission_grant_id: None,
        selected_root: path(location),
        canonical_root: path(location),
        root_identity: FileIdentity {
            volume_serial_number: "1".to_owned(),
            file_id: format!(
                "{:032x}",
                id.to_string().bytes().fold(0_u128, |value, byte| value
                    .wrapping_mul(257)
                    .wrapping_add(u128::from(byte)))
            ),
        },
        target_kind: ExternalGrantTargetKind::File,
        capabilities: vec![ExternalGrantCapability::Read],
        recursive_scope: false,
        lifecycle: ExternalGrantLifecycle::Active,
        revalidation_revision: 1,
        created_at: now,
        updated_at: now,
        suspended_at: None,
        revoked_at: None,
        expires_at: None,
        turn_task_id: None,
        term: ExternalGrantTerm::Session,
    }
}

fn prepare(
    store: &PersistenceCoordinator,
    records: Vec<ExternalGrantRecord>,
) -> Result<Vec<ExternalGrantId>, PersistenceError> {
    store
        .prepare_attachment_grants(records, CorrelationId::new())?
        .blocking_recv()
        .expect("actual writer terminal receipt")
}

#[test]
fn same_attachment_binding_is_idempotent_without_changing_revision_or_issuing_another_event() {
    let root = TempDir::new().expect("fixture root");
    let (store, workspace, session) = start(&root);
    let original = candidate(workspace, session, r"C:\External\video.webm");
    assert_eq!(prepare(&store, vec![original.clone()]).expect("first prepare"), vec![original.id]);
    let events = store.event_count().expect("events after prepare");
    let mut retried = original.clone();
    retried.created_at = UtcTimestamp::from_str("2026-10-24T00:00:00Z").expect("retry time");
    retried.updated_at = retried.created_at;
    assert!(prepare(&store, vec![retried]).expect("verify existing binding").is_empty());
    assert_eq!(store.external_grant(original.id).expect("original grant"), original);
    assert_eq!(store.list_session_external_grants(session).expect("grant list").len(), 1);
    assert_eq!(store.event_count().expect("events after retry"), events);
}

#[test]
fn a_reused_id_cannot_change_any_file_session_or_authority_binding() {
    let root = TempDir::new().expect("fixture root");
    let (store, workspace, session) = start(&root);
    let original = candidate(workspace, session, r"C:\External\video.webm");
    prepare(&store, vec![original.clone()]).expect("initial binding");
    let events = store.event_count().expect("baseline events");
    let mut mutations = Vec::new();
    let mut changed = original.clone();
    changed.session_id = SessionId::new();
    mutations.push(changed);
    let mut changed = original.clone();
    changed.workspace_id = WorkspaceId::new();
    mutations.push(changed);
    let mut changed = original.clone();
    changed.selected_root = path(r"C:\External\other.webm");
    mutations.push(changed);
    let mut changed = original.clone();
    changed.canonical_root = path(r"C:\External\other.webm");
    mutations.push(changed);
    let mut changed = original.clone();
    changed.root_identity.file_id = "22".repeat(16);
    mutations.push(changed);
    let mut changed = original.clone();
    changed.target_kind = ExternalGrantTargetKind::Directory;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.capabilities = vec![ExternalGrantCapability::Read, ExternalGrantCapability::Write];
    mutations.push(changed);
    let mut changed = original.clone();
    changed.recursive_scope = true;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.permission_grant_id = Some(PermissionGrantId::new());
    mutations.push(changed);
    let mut changed = original.clone();
    changed.term = ExternalGrantTerm::Workspace;
    mutations.push(changed);
    let mut changed = original.clone();
    changed.term = ExternalGrantTerm::Turn;
    changed.turn_task_id = Some(TurnTaskId::new());
    mutations.push(changed);
    let mut changed = original.clone();
    changed.expires_at = Some(UtcTimestamp::now());
    mutations.push(changed);
    let mut changed = original.clone();
    changed.revalidation_revision = 2;
    mutations.push(changed);
    for changed in mutations {
        assert!(prepare(&store, vec![changed]).is_err(), "changed authority must be refused");
        assert_eq!(store.external_grant(original.id).expect("unchanged grant"), original);
        assert_eq!(store.event_count().expect("unchanged events"), events);
    }
}

#[test]
fn retrying_the_same_id_never_revives_a_revoked_or_suspended_grant() {
    let root = TempDir::new().expect("fixture root");
    let (store, workspace, session) = start(&root);
    let revoked = candidate(workspace, session, r"C:\External\revoked.webm");
    prepare(&store, vec![revoked.clone()]).expect("first grant");
    let revoked_record =
        store.revoke_external_grant(revoked.id, session, CorrelationId::new()).expect("revoke");
    let suspended_candidate = candidate(workspace, session, r"C:\External\suspended.webm");
    let mut suspended = suspended_candidate.clone();
    suspended.lifecycle = ExternalGrantLifecycle::Suspended;
    suspended.suspended_at = Some(UtcTimestamp::now());
    store
        .create_external_grant(suspended.clone(), CorrelationId::new())
        .expect("seed suspended record");
    let events = store.event_count().expect("inactive baseline");
    for retry in [revoked, suspended_candidate] {
        assert!(matches!(
            prepare(&store, vec![retry]),
            Err(PersistenceError::ExternalGrantStateConflict)
        ));
    }
    assert_eq!(store.external_grant(revoked_record.id).expect("revoked retained"), revoked_record);
    assert_eq!(store.external_grant(suspended.id).expect("suspended retained"), suspended);
    assert_eq!(store.event_count().expect("inactive event count"), events);
}

#[test]
fn a_later_binding_conflict_rolls_back_all_prior_candidates_and_their_events() {
    let root = TempDir::new().expect("fixture root");
    let (store, workspace, session) = start(&root);
    let first = candidate(workspace, session, r"C:\External\new.webm");
    let existing = candidate(workspace, session, r"C:\External\existing.webm");
    prepare(&store, vec![existing.clone()]).expect("existing binding");
    let events = store.event_count().expect("baseline events");
    let mut conflict = existing.clone();
    conflict.root_identity.file_id = "33".repeat(16);
    assert!(matches!(
        prepare(&store, vec![first.clone(), conflict]),
        Err(PersistenceError::ExternalGrantStateConflict)
    ));
    assert!(matches!(store.external_grant(first.id), Err(PersistenceError::ExternalGrantNotFound)));
    assert_eq!(store.external_grant(existing.id).expect("existing retained"), existing);
    assert_eq!(store.list_session_external_grants(session).expect("atomic grant list").len(), 1);
    assert_eq!(store.event_count().expect("rolled back issuance event"), events);
}
