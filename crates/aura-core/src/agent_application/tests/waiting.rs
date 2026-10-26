use super::*;
use provider_fixture::Scenario;
// Touching, declining and leaving a question; nested here so the test list keeps its
// ceiling.
#[path = "l10_14_query.rs"]
mod l10_14_query;
// The instructions before the conversation stay the same from step to step.
#[path = "l10_14_prompt_cache.rs"]
mod l10_14_prompt_cache;
// The step prepared after a compaction says so.
#[path = "l10_14_compaction.rs"]
mod l10_14_compaction;
// A smaller window keeps the conversation going.
#[path = "l10_15_compaction.rs"]
mod l10_15_compaction;
// The thinking level the conversation chose goes out in its turn's requests.
#[path = "l10_16_reasoning.rs"]
mod l10_16_reasoning;
// The sidebar's running state.
#[path = "l10_16_activity.rs"]
mod l10_16_activity;
// Room for the output, clearing old tool results, and compaction on the request's prefix.
#[path = "l10_19_compaction.rs"]
mod l10_19_compaction;
// Tool results and provider failures.
#[path = "l10_19_tools.rs"]
mod l10_19_tools;
// The unified entry and a tool list that changes only at a turn start.
#[path = "l10_21_entry.rs"]
mod l10_21_entry;
// A Goal completed with the evidence a model can give.
#[path = "l10_21_goal.rs"]
mod l10_21_goal;
// The built-in browser's tools, against a stand-in for the browser host.
#[path = "l13_browser.rs"]
mod l13_browser;
// `/compact` while nothing runs.
#[path = "l13_1_compaction_turn.rs"]
mod l13_1_compaction_turn;
// Aura's own data asks every time.
#[path = "l13_1_aura_data.rs"]
mod l13_1_aura_data;
// Pictures and files sent with a message.
#[path = "l13_2_attachments.rs"]
mod l13_2_attachments;
#[path = "l13_3_attachments/mod.rs"]
mod l13_3_attachments;
#[path = "l13_3_media/mod.rs"]
mod l13_3_media;
// A message's attachments read again for the 文件 tab.
#[path = "l13_4_attachments.rs"]
mod l13_4_attachments;
// The whole conversation's cache use, in a step's details.
#[path = "l13_4_inspector.rs"]
mod l13_4_inspector;
// Deletions go to the Recycle Bin; for good, or from a Shell, they ask.
#[path = "l13_4_deletion.rs"]
mod l13_4_deletion;
// A provider's default connection and its backups.
#[path = "l13_4_connections.rs"]
mod l13_4_connections;
// An entry of a zip a message carried, opened for the 文件 tab.
#[path = "l13_5_zip_entries.rs"]
mod l13_5_zip_entries;
// A workspace zip's entry, and a file outside the workspace a message names.
#[path = "l13_6_files.rs"]
mod l13_6_files;
// A new conversation's first request measured by another conversation's count.
#[path = "l12_1_borrowed_count.rs"]
mod l12_1_borrowed_count;

/// The runtime, session and model profile a run needs, before anything has been
/// started. Tests that must commit their own configuration first (the context
/// window ladder is calibrated against the size of the request) prepare here,
/// configure, and then call [`start`].
pub(in crate::agent_application::tests) async fn prepare(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, SessionId, ModelProfileId) {
    let runtime = host.start().await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("waiting-workspace");
    std::fs::create_dir(&root).expect("Agent test setup and persisted assertion");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Waiting"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone())
            .expect("Agent test setup and persisted assertion");
    let session = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    (runtime, session, model)
}

/// The same preparation, with the session mode the test needs.
///
/// A test that drives a real `fs.write` through the model loop has to run in a mode that
/// authorizes that write. Under the confirmed semantics the lower modes *ask* instead of
/// refusing, so a write-driving test left on the default would wait for an answer nobody is
/// there to give — which is the honest product behaviour and the wrong fixture for a test that
/// is about something else.
pub(in crate::agent_application::tests) async fn prepare_in_mode(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    mode: PermissionMode,
) -> (crate::RuntimeState, SessionId, ModelProfileId) {
    let runtime = host.start().await;
    let model = profile(&runtime, &provider.url).await;
    let root = host.root.path().join("waiting-workspace");
    std::fs::create_dir(&root).expect("Agent test setup and persisted assertion");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Waiting"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone())
            .expect("Agent test setup and persisted assertion");
    let session = create_session(&runtime, workspace.workspace_id, mode).await;
    (runtime, session, model)
}

/// Start a real write-driving run in a mode that authorizes the write.
pub(in crate::agent_application::tests) async fn begin_writing(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    mode: AgentTaskMode,
) -> (crate::RuntimeState, AgentRunProjection) {
    let (runtime, session, model) =
        prepare_in_mode(host, provider, PermissionMode::WorkspaceReadWrite).await;
    let run = start(&runtime, session, model, mode).await;
    (runtime, run)
}

/// Start the run on a prepared runtime and return its first projection.
pub(in crate::agent_application::tests) async fn start(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    mode: AgentTaskMode,
) -> AgentRunProjection {
    let v = crate::r03_tests::success(
        runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: "Check the answer and finish".into(),
            mode,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    serde_json::from_value(v).expect("Agent test setup and persisted assertion")
}

pub(in crate::agent_application::tests) async fn begin(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    mode: AgentTaskMode,
) -> (crate::RuntimeState, AgentRunProjection) {
    let (runtime, session, model) = prepare(host, provider).await;
    let run = start(&runtime, session, model, mode).await;
    (runtime, run)
}
pub(in crate::agent_application::tests) async fn wait(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
    predicate: impl Fn(&AgentRunProjection) -> bool,
) -> AgentRunProjection {
    tokio::time::timeout(std::time::Duration::from_secs(45),async {
        loop {
            let v:AgentRunProjection=serde_json::from_value(crate::r03_tests::success(runtime,"agent.status",json!(request)).await).expect("Agent test setup and persisted assertion");
            if predicate(&v){return v;}
            if v.turn.state.terminal(){
                let diagnostics=if let crate::StartupMode::Ready(s)=&runtime.startup_mode {
                    let j=s.broker.lock().expect("Agent test setup and persisted assertion");let mut q=j.database.connection().prepare("SELECT payload FROM events WHERE kind IN('agent.application_error','model.failure','tool.dispatch_adapter_error')").expect("Agent test setup and persisted assertion");
                    q.query_map([],|r|r.get::<_,String>(0)).expect("Agent test setup and persisted assertion").collect::<Result<Vec<_>,_>>().expect("Agent test setup and persisted assertion")
                }else{vec![]};
                panic!("unexpected termination: {:?}: {diagnostics:?}",v.turn.state);
            }tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    }).await.expect("Agent test setup and persisted assertion")
}
#[tokio::test]
async fn actual_query_answer_survives_core_restart_and_resumes_same_turn() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let waiting =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. }))
            .await;
    // Let the actual worker settle; the API answer also waits for this boundary.
    if let crate::StartupMode::Ready(s) = &runtime.startup_mode {
        s.agent_required()
            .idle(request.agent_run_id)
            .await
            .expect("Agent test setup and persisted assertion");
    }
    let runtime = host.restart(runtime).await;
    let restored =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. }))
            .await;
    assert_eq!(waiting.pending_query, restored.pending_query);
    assert_eq!(
        provider.requests.lock().expect("Agent test setup and persisted assertion").len(),
        3
    );
    let q = restored.pending_query.expect("Agent test setup and persisted assertion");
    let answer = AgentQueryAnswerRequest {
        session_id: request.session_id,
        agent_run_id: request.agent_run_id,
        query_id: q.query_id,
        expected_revision: q.revision,
        answers: vec![QueryAnswer {
            question_id: "choice".into(),
            value: QueryAnswerValue::Confirm { accepted: true },
        }],
    };
    crate::r03_tests::success(&runtime, "agent.query.answer", json!(answer)).await;
    let done =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert_eq!(done.turn.turn_task_id, start.turn.turn_task_id);
    assert_eq!(done.run.agent_run_id, start.run.agent_run_id);
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    {
        let store = s.broker.lock().expect("Agent test setup and persisted assertion");
        let db = store.database.connection();
        let users: i64 = db
            .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.user_message'", [], |r| {
                r.get(0)
            })
            .expect("Agent test setup and persisted assertion");
        assert_eq!(users, 2);
    }
    let before = done.clone();
    assert!(
        s.agent_required().answer(CommandInvocationId::new(), answer).await.is_err(),
        "stale answer cannot append another user event"
    );
    assert_eq!(
        s.agent_required()
            .status(&request)
            .await
            .expect("Agent test setup and persisted assertion"),
        before
    );
}
#[tokio::test]
async fn cancellation_stops_real_provider_and_goal_budget_is_optional() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Hold).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Goal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while provider.requests.lock().expect("Agent test setup and persisted assertion").is_empty()
        {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Agent test setup and persisted assertion");
    let cancelled: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(&runtime, "agent.cancel", json!(request)).await,
    )
    .expect("Agent test setup and persisted assertion");
    assert_eq!(cancelled.turn.state, TurnTaskState::Cancelled);
    assert!(cancelled.model_steps.iter().all(|s| s.state == ModelStepState::Cancelled));
    let runtime = host.restart(runtime).await;
    let after: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(&runtime, "agent.status", json!(request)).await,
    )
    .expect("Agent test setup and persisted assertion");
    assert_eq!(after, cancelled);
    assert_eq!(
        provider.requests.lock().expect("Agent test setup and persisted assertion").len(),
        1,
        "restart never repeats the request"
    );
}
#[tokio::test]
async fn plan_and_goal_complete_through_real_model_steps_and_metered_usage() {
    for (mode, scenario, requests) in
        [(AgentTaskMode::Plan, Scenario::Plan, 1), (AgentTaskMode::Goal, Scenario::Goal, 2)]
    {
        let host = crate::r03_tests::Fixture::new();
        let provider = Provider::scenario(scenario).await;
        let (runtime, start) = begin(&host, &provider, mode).await;
        let request = AgentRunRequest {
            session_id: start.turn.session_id,
            agent_run_id: start.run.agent_run_id,
        };
        let done =
            wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. }))
                .await;
        assert_eq!(
            provider.requests.lock().expect("Agent test setup and persisted assertion").len(),
            requests
        );
        let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
        let store = s.broker.lock().expect("Agent test setup and persisted assertion");
        let db = store.database.connection();
        if mode == AgentTaskMode::Goal {
            let goal = aura_persistence_v2::agent::work::goal(db, done.turn.turn_task_id)
                .expect("Agent test setup and persisted assertion");
            assert!(goal.completed);
            assert_eq!(goal.budget, GoalBudget::default());
            assert_eq!(goal.consumed_tokens, 80);
            assert_eq!(goal.consumed_turns, 2);
        }
        assert_eq!(
            aura_persistence_v2::context::history(db, done.turn.session_id)
                .expect("Agent test setup and persisted assertion")
                .len(),
            requests
        );
    }
}
/// Compacting a span that holds a Tool call. The summary replaces the call, so the next
/// request no longer carries it, and its snapshot must not keep the call's wire binding: the
/// adapter checks that both name exactly the same calls. Keeping it interrupted the first step
/// after every such compaction.
#[tokio::test]
async fn compaction_of_a_span_with_a_tool_call_continues_the_turn() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactAfterTool).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    // Larger than the 30% retention budget of the fixture's 256000-token window, so the
    // reading call cannot be kept and is the part compaction replaces.
    std::fs::write(
        host.root.path().join("waiting-workspace").join(provider_fixture::COMPACTION_SOURCE_FILE),
        "a line of the compacted source file
"
        .repeat(3_400),
    )
    .expect("Agent test setup and persisted assertion");
    let start = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("Agent test setup and persisted assertion");
    let compaction = |body: &serde_json::Value| {
        body.to_string().contains("Return exactly these headings in this order:")
    };
    assert_eq!(requests.iter().filter(|b| compaction(b)).count(), 2, "bounded compaction");
    let first_call = requests[1]["messages"]
        .as_array()
        .expect("second request messages")
        .iter()
        .find_map(|m| m["tool_calls"][0]["id"].as_str())
        .expect("the Tool call is in the history before compaction")
        .to_owned();
    let after = requests.last().expect("the answer request after compaction");
    assert!(!compaction(after));
    assert!(
        !after.to_string().contains(&first_call),
        "the compacted call is no longer in the request"
    );
}

#[tokio::test]
async fn compaction_uses_real_provider_then_original_source_receipt_without_fake_user() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Compact).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let done =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert_eq!(
        provider.requests.lock().expect("Agent test setup and persisted assertion").len(),
        6,
        "four normal calls and two bounded compaction attempts"
    );
    // The fixture profile allows 4096 output tokens and a 1024-token snapshot. A reasoning
    // model spends output tokens before it writes, so the call keeps the step's cap, and the
    // snapshot size is a stated character limit of the document (about four per token).
    let compaction_requests = provider
        .requests
        .lock()
        .expect("Agent test setup and persisted assertion")
        .iter()
        .filter(|body| body.to_string().contains("Return exactly these headings in this order:"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(compaction_requests.len(), 2);
    for body in &compaction_requests {
        let cap = body.get("max_completion_tokens").or_else(|| body.get("max_tokens"));
        assert_eq!(cap, Some(&serde_json::json!(4096)), "compaction output cap in {body}");
        assert!(
            body.to_string().contains("under 4096 characters"),
            "the snapshot limit is stated to the model"
        );
        // The parser takes a Truth-Index line only as the bare anchor. "Each anchor exactly
        // once as '- a000001'" next to "retain the source and anchor mapping" let a real model
        // write "- a000001 -> evt_...; note" twice, and the turn was interrupted.
        assert!(
            body.to_string().contains("each line is exactly '- a000001' and nothing else"),
            "the Truth-Index line format the parser accepts is stated to the model"
        );
    }
    assert_eq!(
        done.model_steps.len(),
        4,
        "auxiliary requests cannot masquerade as assistant responses"
    );
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    let journal = s.broker.lock().expect("Agent test setup and persisted assertion");
    let db = journal.database.connection();
    let compaction = aura_persistence_v2::context::compaction::latest(db, request.session_id)
        .expect("Agent test setup and persisted assertion")
        .expect("Agent test setup and persisted assertion");
    assert_eq!(compaction.attempts.len(), 2);
    assert_eq!(
        compaction.attempts[0].state,
        aura_context_domain::CompactionModelAttemptState::Failed
    );
    assert_eq!(
        compaction.attempts[1].state,
        aura_context_domain::CompactionModelAttemptState::Completed
    );
    let read = aura_persistence_v2::context::read::compaction(
        db,
        request.session_id,
        request.agent_run_id,
        &aura_context_domain::AgentContextReadCompactionInput {
            source_context_snapshot_id: compaction.id,
            sequence: compaction.sequence,
        },
    )
    .expect("Agent test setup and persisted assertion");
    assert_eq!(read, compaction.result.expect("Agent test setup and persisted assertion"));
    assert!(
        aura_persistence_v2::context::read::compaction(
            db,
            SessionId::new(),
            request.agent_run_id,
            &aura_context_domain::AgentContextReadCompactionInput {
                source_context_snapshot_id: compaction.id,
                sequence: compaction.sequence
            }
        )
        .is_err()
    );
    let users: i64 = db
        .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.user_message'", [], |r| r.get(0))
        .expect("Agent test setup and persisted assertion");
    assert_eq!(users, 1);
    let all = aura_persistence_v2::agent::model_context::history_for_model(
        db,
        request.agent_run_id,
        false,
    )
    .expect("Agent test setup and persisted assertion");
    assert!(all.items.iter().any(|v|matches!(v,LogicalConversationItem::Assistant{content:Some(text),..} if text.len()>100000)),"compaction never deletes original history");
}
#[tokio::test]
async fn drive_failure_during_wait_does_not_poison_status() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, start) = begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let waiting =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. }))
            .await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    s.agent_required()
        .idle(request.agent_run_id)
        .await
        .expect("Agent test setup and persisted assertion");
    s.agent_required()
        .persist_drive_failure_for_test(
            request.agent_run_id,
            "persistent revision or identity conflict",
        )
        .await;
    let after = s
        .agent_required()
        .status(&request)
        .await
        .expect("Agent test setup and persisted assertion");
    assert!(
        matches!(after.turn.state, TurnTaskState::AwaitingQuery { .. }),
        "{:?}",
        after.turn.state
    );
    assert_eq!(after.pending_query, waiting.pending_query);
    let store = s.broker.lock().expect("Agent test setup and persisted assertion");
    let db = store.database.connection();
    let errors: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind='agent.application_error' AND owner=?1",
            [request.agent_run_id.to_string()],
            |r| r.get(0),
        )
        .expect("Agent test setup and persisted assertion");
    assert_eq!(errors, 1);
}
