use super::*;

/// An addition accepted while the first model request is frozen stays out of
/// that request. The next safe request consumes it, and a later complete Normal
/// answer that has seen it finishes naturally.
#[tokio::test]
async fn an_addition_accepted_mid_turn_is_consumed_before_the_later_answer_completes() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::SearchThenText).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("agent-addition-workspace");
    std::fs::create_dir(&root).expect("Agent addition workspace");
    std::fs::write(root.join("search-fixture.txt"), "unique_fs_search_agent_token\n")
        .expect("search fixture");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Addition"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("Agent addition workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "Search the fixture token and report it".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("Agent addition start");
    provider.parked.notified().await;
    let first = "Additionally record where the token was found";
    let second = "Also name the matching file";
    let queued = "Leave this for a later turn";
    let first_id = submit_input(
        &runtime,
        session,
        start.turn.turn_task_id,
        first,
        AgentInputLane::Addition { turn_task_id: start.turn.turn_task_id },
    )
    .await;
    let second_id = submit_input(
        &runtime,
        session,
        start.turn.turn_task_id,
        second,
        AgentInputLane::Addition { turn_task_id: start.turn.turn_task_id },
    )
    .await;
    let queued_id =
        submit_input(&runtime, session, start.turn.turn_task_id, queued, AgentInputLane::NextTurn)
            .await;
    provider.release_window.notify_one();
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    let bodies = provider.requests.lock().expect("Agent addition requests").clone();
    assert!(
        bodies.first().is_some_and(|body| !body.to_string().contains(first)),
        "the frozen first request does not contain the addition"
    );
    assert!(
        bodies
            .iter()
            .skip(1)
            .any(|body| body.to_string().contains(first) && body.to_string().contains(second)),
        "a later request consumes both additions"
    );
    assert_eq!(done.turn.mode, AgentTaskMode::Normal);
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    let (natural, explicit) = completion_counts(&runtime);
    assert_eq!(natural, 1, "the later answer that saw the additions completes naturally");
    assert_eq!(explicit, 0, "the explicit completion tool is not required");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable");
    };
    let lineage = {
        let journal = services.broker.lock().expect("addition journal");
        aura_persistence_v2::agent::lineage::load(
            journal.database.connection(),
            done.turn.lineage_id,
        )
        .expect("completed lineage")
    };
    let sources = lineage.sources.iter().map(|source| source.user_event_id).collect::<Vec<_>>();
    assert!(sources.contains(&first_id) && sources.contains(&second_id));
    assert!(!sources.contains(&queued_id), "NextTurn is not consumed into this turn");
}

#[tokio::test]
async fn a_frozen_text_answer_does_not_swallow_an_addition_the_next_answer_has_seen() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::AdditionText).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("agent-addition-text");
    std::fs::create_dir(&root).expect("Agent addition text workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"AdditionText"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "请只用一句中文问候我。".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("start");
    provider.parked.notified().await;
    let addition = "补充：问候之后说明你已看到这句。";
    submit_input(
        &runtime,
        session,
        start.turn.turn_task_id,
        addition,
        AgentInputLane::Addition { turn_task_id: start.turn.turn_task_id },
    )
    .await;
    provider.release_window.notify_one();
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    let bodies = provider.requests.lock().expect("requests").clone();
    assert!(bodies.first().is_some_and(|body| !body.to_string().contains(addition)));
    assert!(bodies.iter().skip(1).any(|body| body.to_string().contains(addition)));
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    let (natural, explicit) = completion_counts(&runtime);
    assert_eq!(natural, 1);
    assert_eq!(explicit, 0);
}

/// A consumed addition that production compaction removes from the model
/// context still lets the following text continuation finish naturally.
#[tokio::test]
async fn a_compacted_addition_continues_and_completes_after_its_text_is_removed() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(provider_fixture::Scenario::CompactedAddition).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("agent-compacted-addition");
    std::fs::create_dir(&root).expect("compacted addition workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"CompactedAddition"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).expect("workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "请只用一句中文问候我。".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("start");
    provider.parked.notified().await;
    let marker = "AURA_COMPACTED_ADDITION_BODY";
    let visible = "AURA_VISIBLE_ADDITION";
    // Past the lineage preview (first 512 and last 256 characters) and the
    // 128-character anchor preview, while the whole text still crosses the
    // automatic compaction threshold and fits in the compaction request.
    let huge = format!("{}{marker}{}", "x".repeat(600), "x".repeat(200_000 - 600 - marker.len()));
    assert!(huge.len() > 180_000 && huge.len() <= 256 * 1024);
    assert!(!huge[..512].contains(marker) && !huge[huge.len() - 256..].contains(marker));
    let huge_id = submit_input(
        &runtime,
        session,
        start.turn.turn_task_id,
        &huge,
        AgentInputLane::Addition { turn_task_id: start.turn.turn_task_id },
    )
    .await;
    submit_input(
        &runtime,
        session,
        start.turn.turn_task_id,
        visible,
        AgentInputLane::Addition { turn_task_id: start.turn.turn_task_id },
    )
    .await;
    provider.release_window.notify_one();
    let done = wait_for_terminal(&runtime, session, start.run.agent_run_id).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable");
    };
    let bodies = provider.requests.lock().expect("requests").clone();
    let auxiliary = "Write the summary as one paragraph";
    let normal = bodies
        .iter()
        .filter(|body| !body.to_string().contains(auxiliary))
        .map(|body| body.to_string())
        .collect::<Vec<_>>();
    let described = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| {
            let text = body.to_string();
            format!(
                "{index}: auxiliary={} marker={} visible={} bytes={}",
                text.contains(auxiliary),
                text.contains(marker),
                text.contains(visible),
                text.len()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        normal.first().is_some_and(|body| !body.contains(marker) && !body.contains(visible)),
        "frozen request absorbed an addition\n{described}"
    );
    assert!(
        normal.iter().skip(1).any(|body| body.contains(visible) && !body.contains(marker)),
        "the continuation keeps the tail and drops the compacted addition\n{described}\nstate={:?}",
        done.turn.state
    );
    assert!(
        bodies
            .iter()
            .any(|body| body.to_string().contains(auxiliary) && body.to_string().contains(marker)),
        "compaction itself still receives the stored addition"
    );
    let excluded = {
        let journal = services.broker.lock().expect("compaction journal");
        let db = journal.database.connection();
        let mut statement =
            db.prepare("SELECT payload FROM context_compactions").expect("compactions");
        let rows = statement.query_map([], |row| row.get::<_, String>(0)).expect("rows");
        rows.filter_map(Result::ok).any(|payload| payload.contains(&huge_id.to_string()))
    };
    assert!(excluded, "the applied compaction excludes the compacted addition");
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    let (natural, explicit) = completion_counts(&runtime);
    assert_eq!(natural, 1);
    assert_eq!(explicit, 0);
}
