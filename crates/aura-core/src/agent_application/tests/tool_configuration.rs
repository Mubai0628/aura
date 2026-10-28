//! Exercise the real registry builder/cache against isolated durable session facts.
use super::*;
use aura_persistence_v2::tool::session_tools;
use aura_tool_contract::{SessionDynamicTools, ToolLoadStateV1};

#[tokio::test]
async fn registry_reuses_unchanged_definitions_and_isolates_dynamic_session_membership() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let root = host.root.path().join("tools-workspace");
    std::fs::create_dir(&root).expect("workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Tool registry regression"}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace id");
    let session = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    let other = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { panic!("Core ready") };
    for id in [session, other] {
        s.agent_required().ensure_session(id).await.expect("V2 mirror");
    }
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
    let prepare = |turn: &TurnTask, permission: &aura_security_contract::PermissionSnapshot| {
        let journal = s.broker.lock().expect("journal");
        s.agent_required()
            .prepare_registry(journal.database.connection(), turn, permission, UtcTimestamp::now())
            .expect("registry")
    };
    let (first, empty, settings, _) = prepare(&turn, &permission);
    assert!(empty.is_empty());
    let loaded = |name: &str, registry: &aura_tool_contract::ToolRegistrySnapshotV1| {
        registry.catalog_entries.iter().any(|e| {
            e.canonical_name.as_str() == name
                && matches!(
                    e.load_state,
                    ToolLoadStateV1::Core | ToolLoadStateV1::Pinned | ToolLoadStateV1::Ephemeral
                )
        })
    };
    // An untouched configuration loads the recommended default set: file work,
    // managed Shell, task bookkeeping and this task's context events. Every name
    // is a real built-in that is not mode-required, so the selection is valid.
    // And reading the
    // repository's state, which coding work asks for in most tasks.
    for name in [
        "git.status",
        "git.diff",
        "git.log",
        "fs.find",
        "fs.list",
        "fs.read",
        "fs.search",
        "fs.write",
        "fs.patch",
        "shell.powershell",
        "shell.query",
        "shell.stop",
        "todo.get",
        "todo.replace",
        "todo.update",
        // The conversation's history by round.
        "history.list_turns",
        "history.search",
        "history.read",
        // Deleting into the Recycle Bin.
        "fs.delete",
        // DeepSeek's web search and fetch, as default tools.
        "web.search",
        "web.fetch",
        "web.read",
        "tools.load",
        "normal.complete",
    ] {
        assert!(loaded(name, &first), "{name} must be loaded by default");
    }
    let (same, _, same_settings, _) = prepare(&turn, &permission);
    assert_eq!(first, same, "unchanged local facts reuse the same compiled registry identity");
    assert_eq!(settings, same_settings);
    // A tool outside the recommended set, loaded explicitly (web.search and the HTTP tools are
    // loaded by default).
    let dynamic = first
        .catalog_entries
        .iter()
        .find(|e| e.canonical_name.as_str() == "media.probe")
        .expect("available tool")
        .tool_id;
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
                    tool_ids: vec![dynamic],
                },
                at,
            )
        })
        .expect("explicit load fact");
    let (next, loads, _, _) = prepare(&turn, &permission);
    assert!(loaded("media.probe", &next));
    assert_eq!(loads.len(), 1);
    assert_ne!(first.snapshot_ref, next.snapshot_ref);
    assert!(!loaded("media.probe", &first), "issued snapshot is immutable");
    let another_turn = TurnTask {
        turn_task_id: TurnTaskId::new(),
        agent_run_id: AgentRunId::new(),
        ..turn.clone()
    };
    assert!(
        loaded("media.probe", &prepare(&another_turn, &permission).0),
        "next turn keeps membership"
    );
    let other_permission =
        s.agent_required().execution.freeze_permission_sync(other).expect("other permission");
    let other_turn = TurnTask {
        session_id: other,
        turn_task_id: TurnTaskId::new(),
        agent_run_id: AgentRunId::new(),
        ..turn.clone()
    };
    let other_registry = prepare(&other_turn, &other_permission).0;
    assert!(!loaded("media.probe", &other_registry));
    assert!(loaded("fs.list", &other_registry), "global fixed selection applies across sessions");
    let mut revised = permission.clone();
    revised.session_permission_revision += 1;
    let revised_registry = prepare(&turn, &revised).0;
    assert_ne!(
        revised_registry.eligibility_context_hash, next.eligibility_context_hash,
        "authority revision invalidates cache"
    );
}
