//! Installed package tools in each turn's registry through the production
//! preparation: offered as discoverable, loaded by the session's own membership,
//! and gone from the next registry once the package is disabled.
use super::*;
use crate::package_services::test_support::{install, set_enabled};
use aura_package_service::package_program::tests::support::Kind;
use aura_persistence_v2::tool::session_tools;
use aura_tool_contract::{SessionDynamicTools, ToolLoadStateV1};

#[tokio::test]
async fn l04_installed_package_tools_join_the_turn_registry_and_leave_it_when_disabled() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("tools-workspace");
    std::fs::create_dir(&root).expect("workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Package registry"}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace id");
    let session = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { panic!("Core ready") };
    s.agent_required().ensure_session(session).await.expect("V2 mirror");
    let permission =
        s.agent_required().execution.freeze_permission_sync(session).expect("permission");
    let at = UtcTimestamp::now();
    let turn = TurnTask {
        turn_task_id: TurnTaskId::new(),
        session_id: session,
        workspace_id: workspace,
        root_user_event_id: EventId::new(),
        lineage_id: TaskLineageId::new(),
        lineage_resolution_required: false,
        agent_run_id: AgentRunId::new(),
        mode: AgentTaskMode::Normal,
        state: TurnTaskState::Running,
        revision: Revision::INITIAL,
        created_at: at,
        updated_at: at,
    };
    let prepare = || {
        let journal = s.broker.lock().expect("journal");
        s.agent_required()
            .prepare_registry(
                journal.database.connection(),
                &turn,
                &permission,
                UtcTimestamp::now(),
            )
            .expect("registry")
    };
    let (before, _, _, named) = prepare();
    assert!(named.is_empty());

    let installed = install(s, Kind::Command, |_| {});
    let tool_id = installed.summary.installed.as_ref().expect("installed").tools[0].tool_id;
    let (offered, loads, _, named) = prepare();
    assert_ne!(offered.snapshot_ref, before.snapshot_ref, "an install is a new registry");
    let entry = offered
        .catalog_entries
        .iter()
        .find(|entry| entry.tool_id == tool_id)
        .expect("the installed tool is offered");
    assert_eq!(entry.load_state, ToolLoadStateV1::Discoverable);
    assert!(loads.is_empty());
    assert_eq!(named.len(), 1, "its records are stored with the snapshot that names it");
    assert_eq!(named[0].0.tool_id, tool_id);

    s.broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            session_tools::save(
                tx,
                None,
                &SessionDynamicTools {
                    session_id: session,
                    revision: Revision::INITIAL,
                    tool_ids: vec![tool_id],
                },
                at,
            )
        })
        .expect("the session's own load");
    let (loaded, loads, _, _) = prepare();
    assert_eq!(
        loaded.catalog_entries.iter().find(|entry| entry.tool_id == tool_id).map(|e| e.load_state),
        Some(ToolLoadStateV1::Ephemeral)
    );
    assert_eq!(loads.len(), 1);

    set_enabled(s, installed.summary.package_id, false);
    let (after, loads, _, named) = prepare();
    assert!(!after.catalog_entries.iter().any(|entry| entry.tool_id == tool_id));
    assert!(loads.is_empty() && named.is_empty());
}
