use super::*;

pub(super) async fn run(mode: PermissionMode, write: bool) {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::new(write).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("agent-route-workspace");
    std::fs::create_dir(&root).expect("Agent test setup and persisted assertion");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Production Agent route"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone())
            .expect("Agent test setup and persisted assertion");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable");
    };
    let session = create_session(&runtime, workspace.workspace_id, mode).await;
    let started=crate::r03_tests::success(&runtime,"agent.start",json!(AgentStartRequest{ session_id:session,model_profile_id:model,text:"Verify mode enforcement, including the expected file-write refusal in read-only mode, and finish. Synthetic code and build errors are permitted model input.".into(),mode:AgentTaskMode::Normal,goal_budget:None,explicit_backend_id:None, skill_ids: Vec::new(), reasoning_effort: None, attachments: Vec::new() })).await;
    let start: AgentRunProjection =
        serde_json::from_value(started).expect("Agent test setup and persisted assertion");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let result = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let v = crate::r03_tests::success(&runtime, "agent.status", json!(request)).await;
            let v: AgentRunProjection =
                serde_json::from_value(v).expect("Agent test setup and persisted assertion");
            // A mode that does not authorize a write by default now asks instead of refusing, so
            // "the run has stopped making progress on its own" is either a terminal state or a
            // question waiting for the user. Both are honest ends of this wait; only hanging
            // would not be.
            if v.turn.state.terminal() || matches!(v.turn.state, TurnTaskState::AwaitingApproval) {
                break v;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Agent test setup and persisted assertion");
    let store = services.broker.lock().expect("Agent test setup and persisted assertion");
    let db = store.database.connection();
    let diagnostics = {
        let mut q=db.prepare("SELECT payload FROM events WHERE kind IN('agent.application_error','model.failure','tool.dispatch_adapter_error')").expect("Agent test setup and persisted assertion");
        q.query_map([], |r| r.get::<_, String>(0))
            .expect("Agent test setup and persisted assertion")
            .collect::<Result<Vec<_>, _>>()
            .expect("Agent test setup and persisted assertion")
    };
    // ReadOnly asks for the write it is not authorized to make; the other two modes complete.
    // What the mode decides is the *file effect*, asserted below, not whether the turn ends.
    // A text-only turn (no tool call at all) has nothing to approve, so it completes in every
    // mode and is never gated here.
    let asked_for_approval = write && mode == PermissionMode::ReadOnly;
    if asked_for_approval {
        assert!(
            matches!(result.turn.state, TurnTaskState::AwaitingApproval),
            "ReadOnly must ask rather than refuse: {:?}: {diagnostics:?}",
            result.turn.state
        );
        assert!(
            provider.requests.lock().expect("Agent test setup and persisted assertion").iter().all(
                |r| r["messages"]
                    .as_array()
                    .expect("Agent test setup and persisted assertion")
                    .iter()
                    .filter(|m| m["role"] == "user")
                    .count()
                    == 1
            )
        );
        // The one fact this test exists for: nothing was written.
        assert!(
            !root.join("agent-result.txt").exists(),
            "a write that is waiting for its answer has no effect"
        );
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.user_message'", [], |r| {
                r.get(0)
            })
            .expect("Agent test setup and persisted assertion");
        assert_eq!(count, 1, "continuation must not manufacture users");
        return;
    }
    assert!(
        matches!(result.turn.state, TurnTaskState::Completed { .. }),
        "{:?}: {diagnostics:?}",
        result.turn.state
    );
    assert_eq!(
        provider.requests.lock().expect("Agent test setup and persisted assertion").len(),
        // A tool-free final response now ends the turn itself, so the text-only
        // scenario needs no extra completion request; the tool loop is unchanged.
        if write { 4 } else { 1 }
    );
    assert_eq!(root.join("agent-result.txt").exists(), write && mode != PermissionMode::ReadOnly);
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.user_message'", [], |r| r.get(0))
        .expect("Agent test setup and persisted assertion");
    assert_eq!(count, 1, "continuation must not manufacture users");
    let attempts: i64 = db
        .query_row("SELECT COUNT(*) FROM transport_attempts WHERE state='\"completed\"'", [], |r| {
            r.get(0)
        })
        .expect("Agent test setup and persisted assertion");
    assert_eq!(attempts, if write { 4 } else { 1 });
    let final_history = aura_persistence_v2::agent::model_context::history_for_model(
        db,
        result.run.agent_run_id,
        false,
    )
    .expect("Agent test setup and persisted assertion");
    // A natural completion invents no Tool group: the text-only path persists the
    // model's own response as the turn's final answer.
    assert_eq!(final_history.groups.len(), if write { 4 } else { 0 });
    assert_eq!(
        final_history
            .items
            .iter()
            .filter(|i| matches!(i, LogicalConversationItem::User { .. }))
            .count(),
        1
    );
    assert!(
        provider.requests.lock().expect("Agent test setup and persisted assertion").iter().all(
            |r| r["messages"]
                .as_array()
                .expect("Agent test setup and persisted assertion")
                .iter()
                .filter(|m| m["role"] == "user")
                .count()
                == 1
        )
    );
}

/// What one real greeting turn persisted and how many Provider requests it took.
pub(super) struct Greeting {
    pub(super) state: TurnTaskState,
    pub(super) requests: usize,
    pub(super) natural: Vec<aura_agent_domain::NaturalCompletionRecord>,
    pub(super) explicit_completions: i64,
    pub(super) reminders_sent: Vec<bool>,
    pub(super) final_text_source: Option<aura_agent_domain::AgentFinalTextSource>,
    pub(super) first_request_body: Option<String>,
    pub(super) second_request_body: Option<String>,
    pub(super) diagnostics: Vec<String>,
}

/// Runs one real Normal greeting turn through the production chain (loopback
/// Provider substitutes only the remote Provider).
pub(super) async fn greeting(scenario: provider_fixture::Scenario) -> Greeting {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let provider = Provider::scenario(scenario).await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("agent-natural-completion-workspace");
    std::fs::create_dir(&root).expect("Agent natural completion workspace");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Natural completion"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone())
            .expect("Agent natural completion workspace");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "你好".into(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection =
        serde_json::from_value(started).expect("Agent natural completion start");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    let result = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let v = crate::r03_tests::success(&runtime, "agent.status", json!(request)).await;
            let v: AgentRunProjection =
                serde_json::from_value(v).expect("Agent natural completion status");
            if v.turn.state.terminal() {
                break v;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Agent natural completion terminal");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable");
    };
    // The journal lock is scoped: nothing guarded by it may outlive this block,
    // so the awaits below never hold it.
    let (natural, explicit_completions, reminders_sent, diagnostics) = {
        let journal = services.broker.lock().expect("Agent natural completion journal");
        let db = journal.database.connection();
        let natural: Vec<aura_agent_domain::NaturalCompletionRecord> = {
            let mut q = db
                .prepare("SELECT payload FROM events WHERE kind='agent.natural_completed'")
                .expect("Agent natural completion query");
            q.query_map([], |r| r.get::<_, String>(0))
                .expect("Agent natural completion query")
                .collect::<Result<Vec<_>, _>>()
                .expect("Agent natural completion query")
        }
        .into_iter()
        .map(|raw| serde_json::from_str(&raw).expect("Agent natural completion record"))
        .collect();
        let explicit_completions: i64 = db
            .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.normal_completed'", [], |r| {
                r.get(0)
            })
            .expect("Agent explicit completion count");
        let reminders_sent: Vec<bool> = {
            let mut q = db
                .prepare("SELECT payload FROM events WHERE kind='agent.turn_guard'")
                .expect("Agent guard query");
            q.query_map([], |r| r.get::<_, String>(0))
                .expect("Agent guard query")
                .collect::<Result<Vec<_>, _>>()
                .expect("Agent guard query")
        }
        .into_iter()
        .map(|raw| {
            let value: serde_json::Value = serde_json::from_str(&raw).expect("Agent guard record");
            value["completion_reminder_sent"].as_bool().unwrap_or(false)
        })
        .collect();
        let users: i64 = db
            .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.user_message'", [], |r| {
                r.get(0)
            })
            .expect("Agent user count");
        assert_eq!(users, 1, "completion must not manufacture a user message");
        let diagnostics: Vec<String> = {
            let mut q = db
                .prepare("SELECT payload FROM events WHERE kind IN('agent.application_error','model.failure')")
                .expect("Agent diagnostics query");
            q.query_map([], |r| r.get::<_, String>(0))
                .expect("Agent diagnostics query")
                .collect::<Result<Vec<_>, _>>()
                .expect("Agent diagnostics query")
        };
        (natural, explicit_completions, reminders_sent, diagnostics)
    };
    let history = crate::r03_tests::success(
        &runtime,
        "agent.history",
        json!(AgentHistoryRequest {
            session_id: session,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10
        }),
    )
    .await;
    let turns: Vec<aura_projection_contract::AgentTurnView> =
        serde_json::from_value(history["turns"].clone()).expect("Agent natural completion history");
    let final_text_source = turns.last().expect("Agent natural completion turn").final_text_source;
    let requests = provider.requests.lock().expect("Agent natural completion requests").clone();
    let first_request_body = requests.first().map(|body| body.to_string());
    let second_request_body = requests.get(1).map(|body| body.to_string());
    Greeting {
        state: result.turn.state,
        requests: requests.len(),
        natural,
        explicit_completions,
        reminders_sent,
        final_text_source,
        first_request_body,
        second_request_body,
        diagnostics,
    }
}

pub(super) async fn submit_input(
    runtime: &crate::RuntimeState,
    session: SessionId,
    _turn: TurnTaskId,
    text: &str,
    destination: AgentInputLane,
) -> EventId {
    let inbox: AgentInboxProjection = serde_json::from_value(
        crate::r03_tests::success(
            runtime,
            "agent.submit",
            json!(AgentSubmitRequest {
                session_id: session,
                text: text.into(),
                destination,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            }),
        )
        .await,
    )
    .expect("submitted input");
    inbox
        .messages
        .iter()
        .find(|message| message.text == text)
        .expect("submitted message stays addressable")
        .message_id
}

pub(super) async fn wait_for_terminal(
    runtime: &crate::RuntimeState,
    session: SessionId,
    run: AgentRunId,
) -> AgentRunProjection {
    let request = AgentRunRequest { session_id: session, agent_run_id: run };
    // The budget is for a real turn — provider round trips, backend selection, persistence —
    // so it has to stay generous enough that a loaded machine does not turn a passing test
    // into a flaky one. What is asserted is that the turn reaches a terminal state, not that
    // it does so within any particular deadline.
    tokio::time::timeout(std::time::Duration::from_secs(180), async {
        loop {
            let view: AgentRunProjection = serde_json::from_value(
                crate::r03_tests::success(runtime, "agent.status", json!(request)).await,
            )
            .expect("addition status");
            if view.turn.state.terminal() {
                break view;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("addition turn reaches a terminal state")
}

pub(super) fn completion_counts(runtime: &crate::RuntimeState) -> (i64, i64) {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
        panic!("Core unavailable");
    };
    let journal = services.broker.lock().expect("addition journal");
    let db = journal.database.connection();
    let natural = db
        .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.natural_completed'", [], |row| {
            row.get(0)
        })
        .expect("natural count");
    let explicit = db
        .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.normal_completed'", [], |row| {
            row.get(0)
        })
        .expect("explicit count");
    (natural, explicit)
}
