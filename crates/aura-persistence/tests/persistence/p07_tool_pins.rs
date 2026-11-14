//! The four V1 Pin commands that stay routed. The test that covered them pinned a P07
//! package tool, which can no longer be created now that path is retired, so the Pins are
//! exercised on a materialized signed Built-in definition.
use aura_domain_common::{
    CorrelationId, Revision, RuntimeInstanceId, SessionId, UtcTimestamp, WorkspaceId,
};
use aura_persistence::{
    PersistenceCoordinator, PersistenceError, PersistenceOptions, PersistenceStartup,
    SessionToolPinChangeRequest, ToolPinTargetV1, WorkspaceToolPinChangeRequest,
};
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::ToolPinResolutionStateV1;
use aura_tool_domain::ToolDefinitionRecordV1;
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};

fn start(root: &std::path::Path) -> PersistenceCoordinator {
    let options = PersistenceOptions::new(
        root.join("aura.db"),
        root.join("artifacts"),
        RuntimeInstanceId::new(),
    );
    match PersistenceCoordinator::start(options).expect("start persistence") {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected migration recovery: {}", value.reason)
        }
    }
}

/// A persisted workspace with one session, and one signed Built-in definition to pin.
fn fixture(
    persistence: &PersistenceCoordinator,
) -> (WorkspaceId, SessionId, ToolDefinitionRecordV1) {
    persistence
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("materialize the signed Built-in release");
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "Pins".to_owned(),
                selected_root: r"C:\Aura Tests\Pins".to_owned(),
                canonical_root: r"C:\Aura Tests\Pins".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: "Pins".to_owned(),
                state: "running".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("workspace and session");
    let authority = persistence.tool_authority_snapshot(None, None, None).expect("authority");
    let definition = authority
        .definitions
        .into_iter()
        .find(|definition| definition.tool_package_id.is_none())
        .expect("a Built-in definition");
    (workspace_id, session_id, definition)
}

fn target(definition: &ToolDefinitionRecordV1) -> Option<ToolPinTargetV1> {
    Some(ToolPinTargetV1 {
        tool_version: definition.tool_version.clone(),
        state: ToolPinResolutionStateV1::Resolved,
        safe_reason: None,
    })
}

#[test]
fn session_pin_set_repeat_stale_and_clear_keep_exact_revisions() {
    let root = tempfile::tempdir().expect("root");
    let persistence = start(root.path());
    let (workspace_id, session_id, definition) = fixture(&persistence);
    let change = |expected_revision, target| SessionToolPinChangeRequest {
        session_id,
        workspace_id,
        tool_id: definition.tool_id,
        expected_revision,
        target,
        changed_at: UtcTimestamp::now(),
        correlation_id: CorrelationId::new(),
    };

    let set =
        persistence.commit_session_tool_pin_change(change(None, target(&definition))).expect("set");
    assert!(set.changed);
    assert!(matches!(
        set.projection_changes.as_slice(),
        [ProjectionChange::SessionToolPins { revision, payload, .. }]
            if *revision == Revision::INITIAL
                && payload.session_id == session_id
                && payload.pins.len() == 1
                && payload.pins[0].tool_id == definition.tool_id
                && payload.pins[0].tool_version == definition.tool_version
    ));

    let repeated = persistence
        .commit_session_tool_pin_change(change(Some(Revision::INITIAL), target(&definition)))
        .expect("the same target is a no-op");
    assert!(!repeated.changed);
    assert!(repeated.projection_changes.is_empty());

    let stale = persistence.commit_session_tool_pin_change(change(None, None));
    assert!(matches!(stale, Err(PersistenceError::ToolEntityRevisionConflict)), "{stale:?}");

    let cleared = persistence
        .commit_session_tool_pin_change(change(Some(Revision::INITIAL), None))
        .expect("clear");
    assert!(cleared.changed);
    assert!(matches!(
        cleared.projection_changes.as_slice(),
        [ProjectionChange::SessionToolPins { revision, payload, .. }]
            if *revision > Revision::INITIAL && payload.pins.is_empty()
    ));
}

#[test]
fn workspace_pin_set_and_clear_and_unknown_session_is_refused() {
    let root = tempfile::tempdir().expect("root");
    let persistence = start(root.path());
    let (workspace_id, _, definition) = fixture(&persistence);
    let change = |expected_revision, target| WorkspaceToolPinChangeRequest {
        workspace_id,
        tool_id: definition.tool_id,
        expected_revision,
        target,
        changed_at: UtcTimestamp::now(),
        correlation_id: CorrelationId::new(),
    };

    let set = persistence
        .commit_workspace_tool_pin_change(change(None, target(&definition)))
        .expect("set");
    assert!(set.changed);
    assert!(matches!(
        set.projection_changes.as_slice(),
        [ProjectionChange::WorkspaceToolPins { revision, payload, .. }]
            if *revision == Revision::INITIAL
                && payload.workspace_id == workspace_id
                && payload.pins.len() == 1
    ));
    let cleared = persistence
        .commit_workspace_tool_pin_change(change(Some(Revision::INITIAL), None))
        .expect("clear");
    assert!(cleared.changed);
    assert!(matches!(
        cleared.projection_changes.as_slice(),
        [ProjectionChange::WorkspaceToolPins { payload, .. }] if payload.pins.is_empty()
    ));

    let unknown = persistence.commit_session_tool_pin_change(SessionToolPinChangeRequest {
        session_id: SessionId::new(),
        workspace_id,
        tool_id: definition.tool_id,
        expected_revision: None,
        target: target(&definition),
        changed_at: UtcTimestamp::now(),
        correlation_id: CorrelationId::new(),
    });
    assert!(unknown.is_err(), "a Pin needs the session that owns it: {unknown:?}");
}
