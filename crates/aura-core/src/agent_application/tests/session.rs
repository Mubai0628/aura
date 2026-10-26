use super::*;
use aura_projection_contract::AgentSessionProjection;
// Deleting a conversation; nested here so the test list keeps its ceiling.
#[path = "l10_7_session_delete.rs"]
mod l10_7_session_delete;
// Branching a conversation; nested here for the same reason.
#[path = "l10_11_branch.rs"]
mod l10_11_branch;
// Reading a conversation writes nothing that did not change and waits on no write.
#[path = "l10_13_reads.rs"]
mod l10_13_reads;
// Deleting a long conversation lets other writes in between its steps.
#[path = "l12_2_purge_steps.rs"]
mod l12_2_purge_steps;
// A failed file index is walked again on request.
#[path = "l12_2_rebuild_index.rs"]
mod l12_2_rebuild_index;

#[tokio::test]
async fn session_creation_binds_authority_and_rejects_changed_replay_without_workspace_mutation() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("session-workspace");
    std::fs::create_dir(&root).expect("session route and authority evidence");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Session authority"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone())
            .expect("session route and authority evidence");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable")
    };
    let command = CommandInvocationId::new();
    let input = aura_workspace_domain::SessionCreateRequest {
        workspace_id: workspace.workspace_id,
        title: "Session".into(),
    };
    let original = services
        .persistence
        .create_session(command, input.clone(), PermissionMode::ReadOnly, CorrelationId::new())
        .expect("session route and authority evidence");
    let repeated = services
        .persistence
        .create_session(command, input.clone(), PermissionMode::FullAccess, CorrelationId::new())
        .expect("session route and authority evidence");
    assert_eq!(original, repeated);
    assert_eq!(
        services
            .persistence
            .session_permission_state(original.session_id)
            .expect("session route and authority evidence")
            .mode,
        PermissionMode::ReadOnly
    );
    let mut changed = input.clone();
    changed.title = "changed invocation".into();
    assert!(
        services
            .persistence
            .create_session(command, changed, PermissionMode::FullAccess, CorrelationId::new())
            .is_err()
    );
    let invalid = crate::r03_tests::request(
        &runtime,
        "session.create",
        json!({"workspace_id":WorkspaceId::new(),"title":"Missing"}),
    )
    .await;
    assert!(invalid.get("error").is_some());
    let forbidden = crate::r03_tests::request(&runtime,"session.create",json!({"workspace_id":workspace.workspace_id,"title":"Attempt supplied authority","mode":"full_access"})).await;
    assert!(forbidden.get("error").is_some());
    assert_eq!(
        services
            .persistence
            .workspace(workspace.workspace_id)
            .expect("session route and authority evidence"),
        workspace
    );
    assert_eq!(
        services
            .persistence
            .workspace_session_projection()
            .expect("session route and authority evidence")
            .sessions
            .len(),
        1
    );
    let restarted = host.restart(runtime).await;
    let crate::StartupMode::Ready(services) = &restarted.startup_mode else {
        panic!("Core unavailable")
    };
    assert_eq!(
        services
            .persistence
            .create_session(command, input, PermissionMode::FullAccess, CorrelationId::new())
            .expect("session route and authority evidence"),
        original
    );
    assert_eq!(
        services
            .persistence
            .session_permission_state(original.session_id)
            .expect("session route and authority evidence")
            .mode,
        PermissionMode::ReadOnly
    );
}

/// An unnamed draft Session is a real product state: the primary registry
/// persists it with an empty title and names it only after the first accepted
/// message. Its first send must reach the model through the real queue path,
/// and the V2 mirror must follow the naming instead of blocking it.
#[tokio::test]
async fn unnamed_session_first_send_starts_and_mirrors_the_first_title() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::new(false).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("unnamed-session-workspace");
    std::fs::create_dir(&root).expect("unnamed session workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Unnamed session"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace projection");
    let created = crate::r03_tests::success(
        &runtime,
        "session.create",
        json!({"workspace_id":workspace.workspace_id,"title":""}),
    )
    .await;
    let session: SessionId =
        serde_json::from_value(created["session_id"].clone()).expect("session identity");
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!(SessionModelBindingSetRequest {
            expected_revision: ExpectedRevision::new(0),
            session_id: session,
            model_profile_id: Some(model),
        }),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        json!(AgentSubmitRequest {
            session_id: session,
            text: "你目前都能做什么".into(),
            destination: AgentInputLane::NextTurn,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let history = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let value = crate::r03_tests::success(
                &runtime,
                "agent.history",
                json!(AgentHistoryRequest {
                    session_id: session,
                    before_turn_task_id: None,
                    within_turn: None,
                    limit: 10,
                }),
            )
            .await;
            let history: AgentSessionProjection =
                serde_json::from_value(value).expect("history projection");
            if history.turns.last().is_some_and(|turn| turn.turn.state.terminal()) {
                break history;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the first send of an unnamed Session reaches a terminal state");
    let turn = history.turns.last().expect("turn history");
    assert!(matches!(turn.turn.state, TurnTaskState::Completed { .. }), "{:?}", turn.turn.state);
    assert_eq!(turn.user_messages.len(), 1, "one accepted message, one user identity");

    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable")
    };
    // Each read takes and releases the store lock so no guard is held across an
    // await of the real routes below.
    let mirrored_title = || -> String {
        services
            .broker
            .lock()
            .expect("V2 store")
            .database
            .connection()
            .query_row("SELECT title FROM sessions WHERE id=?1", [session.to_string()], |row| {
                row.get(0)
            })
            .expect("the mirror holds the Session that owns its agent facts")
    };
    let queue_facts = || -> i64 {
        services
            .broker
            .lock()
            .expect("V2 store")
            .database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='agent.inbox' AND owner=?1",
                [format!("agent-inbox:{session}")],
                |row| row.get(0),
            )
            .expect("queue facts")
    };
    assert_eq!(mirrored_title(), "", "an unnamed Session stays unnamed in the mirror");
    assert!(queue_facts() > 0, "the queue path committed its consumed facts");

    // First-message naming uses the primary CAS rename; the mirror converges on
    // the next agent call instead of keeping a stale or invented title.
    crate::r03_tests::success(
        &runtime,
        "session.rename",
        json!({"session_id":session,"expected_title":"","title":"你目前都能"}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.inbox",
        json!(AgentSessionRequest { session_id: session }),
    )
    .await;
    assert_eq!(mirrored_title(), "你目前都能", "the mirror follows the first-message naming");
}
