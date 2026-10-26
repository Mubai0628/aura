//! A branch is sent exactly the history its source would be sent at the branch point,
//! shows the carried turns with their texts, tool calls and results, carries nothing that acts, and
//! stays whole when the conversation it came from is deleted.
use super::*;
use provider_fixture::Scenario;
// A long conversation's branch opens with its newest turns; nested here for its helpers.
#[path = "l10_14_branch_copy.rs"]
mod l10_14_branch_copy;

const FIRST: &str = "AURA-L1011-FIRST: write the result file and finish";
const NEXT: &str = "AURA-L1011-NEXT: say what was written";
const AGAIN: &str = "AURA-L1011-AGAIN: say it once more";

/// One request to Core, boxed: Core's request futures are large, and a test that awaits many of
/// them inline outgrows the test thread's stack in a debug build.
async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

/// Deletes a conversation the way the owner does it: archive it, then delete it.
async fn delete(runtime: &crate::RuntimeState, session: SessionId) {
    call(runtime, "session.archive", json!({ "session_id": session })).await;
    call(runtime, "session.delete", json!({ "session_id": session })).await;
    crate::session_lifecycle::deleted(runtime).await;
}

async fn turn(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    text: &str,
) -> AgentRunProjection {
    let started = call(
        runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: text.to_owned(),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("run projection");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    // Anything but an answer panics with the run's recorded errors.
    Box::pin(waiting::wait(runtime, &request, |v| {
        matches!(v.turn.state, TurnTaskState::Completed { .. })
    }))
    .await
}

async fn branch(
    runtime: &crate::RuntimeState,
    session: SessionId,
    turn: TurnTaskId,
) -> AgentSessionBranchResult {
    let made = call(
        runtime,
        "agent.session.branch",
        json!(AgentSessionBranchRequest { session_id: session, turn_task_id: turn }),
    )
    .await;
    serde_json::from_value(made).expect("branch result")
}

async fn history(runtime: &crate::RuntimeState, session: SessionId) -> AgentSessionProjection {
    serde_json::from_value(
        call(
            runtime,
            "agent.history",
            json!(AgentHistoryRequest {
                session_id: session,
                before_turn_task_id: None,
                within_turn: None,
                limit: 10,
            }),
        )
        .await,
    )
    .expect("history page")
}

async fn whole_text(
    runtime: &crate::RuntimeState,
    session: SessionId,
    source: AgentMessageTextSource,
) -> String {
    let page: AgentMessageTextPage = serde_json::from_value(
        call(
            runtime,
            "agent.message.text",
            json!(AgentMessageTextRequest {
                session_id: session,
                source,
                offset: 0,
                limit: 65_536
            }),
        )
        .await,
    )
    .expect("text page");
    assert_eq!(page.next_offset, None, "one page holds the fixture's text");
    page.text
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services
}

/// What the model is sent before the new turn's own message — the instructions, any compaction
/// summary and the history — and the tools it is offered.
fn history_part(body: &Value) -> (Vec<Value>, Value) {
    let messages = body["messages"].as_array().expect("chat messages");
    let end = messages.iter().rposition(|m| m["role"] == "user").expect("the new turn's message");
    (messages[..end].to_vec(), body["tools"].clone())
}

fn requests(provider: &Provider) -> Vec<Value> {
    provider.requests.lock().expect("provider requests").clone()
}

fn count(runtime: &crate::RuntimeState, sql: &str, session: SessionId) -> i64 {
    let store = services(runtime).broker.lock().expect("V2 store");
    store
        .database
        .connection()
        .query_row(sql, [session.to_string()], |row| row.get(0))
        .expect("count")
}

#[tokio::test]
async fn a_branch_is_sent_its_sources_history_shows_it_and_outlives_it() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Branching).await;
    let (runtime, source, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let first = turn(&runtime, source, model, FIRST).await;
    let first_turn = first.turn.turn_task_id;
    let source_title = services(&runtime)
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .sessions
        .into_iter()
        .find(|s| s.session_id == source)
        .expect("source")
        .title;

    let made = branch(&runtime, source, first_turn).await;
    assert_eq!(made.title, format!("分支：{}", source_title.trim()));
    assert_eq!(made.origin.source_session_id, source);
    assert_eq!(made.origin.source_turn_task_id, first_turn);
    assert_eq!(made.origin.turn_number, 1);
    assert_eq!(
        services(&runtime)
            .persistence
            .session_permission_state(made.session_id)
            .expect("mode")
            .mode,
        PermissionMode::WorkspaceReadWrite,
        "the branch inherits the source's mode"
    );
    // Nothing that acts is carried: no operation, approval, grant, queued message or terminal.
    for (what, sql) in [
        ("turns", "SELECT COUNT(*) FROM turn_tasks WHERE session_id=?1"),
        ("tool batches", "SELECT COUNT(*) FROM tool_batches WHERE session_id=?1"),
        ("terminals", "SELECT COUNT(*) FROM agent_terminals WHERE session_id=?1"),
        ("operations", "SELECT COUNT(*) FROM operations WHERE instr(payload, ?1) > 0"),
        ("approvals", "SELECT COUNT(*) FROM approval_requests WHERE instr(payload, ?1) > 0"),
        ("grants", "SELECT COUNT(*) FROM authorization_grants WHERE instr(payload, ?1) > 0"),
        ("consents", "SELECT COUNT(*) FROM native_session_consents WHERE session_id=?1"),
    ] {
        assert_eq!(count(&runtime, sql, made.session_id), 0, "a branch carries no {what}");
    }

    // The branch and its source continue with the same message: the model is sent the same
    // history, call for call and byte for byte.
    turn(&runtime, made.session_id, model, NEXT).await;
    turn(&runtime, source, model, NEXT).await;
    let sent = requests(&provider);
    assert_eq!(sent.len(), 6, "four requests for the first turn, one for each continuation");
    let carried = history_part(&sent[4]);
    assert_eq!(carried, history_part(&sent[5]), "the branch is sent its source's history");
    assert!(carried.0.iter().any(|m| m["role"] == "tool"), "with the tool results: {carried:?}");
    assert!(carried.0.iter().any(|m| m["content"].as_str().is_some_and(|t| t.contains(FIRST))));

    // The branch shows the carried turn first, marked, with its texts, tool call and result.
    let page = history(&runtime, made.session_id).await;
    let origin = page.branch_origin.clone().expect("branch origin");
    assert_eq!(origin.source_session_id, source);
    assert_eq!(page.turns.len(), 2);
    assert!(page.turns[0].imported && !page.turns[1].imported);
    let shown = &page.turns[0];
    assert_eq!(shown.turn.turn_task_id, first_turn);
    let user = shown.user_messages[0].event_id;
    assert_eq!(
        whole_text(&runtime, made.session_id, AgentMessageTextSource::User { event_id: user })
            .await,
        FIRST
    );
    let write = shown
        .assistant_messages
        .iter()
        .flat_map(|step| step.tool_call_ids.iter().copied())
        .last()
        .expect("the carried turn's last tool call");
    let activity: aura_tool_projection::AgentToolActivityPage = serde_json::from_value(
        call(
            &runtime,
            "agent.tool_activity",
            json!({"session_id": made.session_id, "before_call_id": null, "limit": 1, "call_ids": [write]}),
        )
        .await,
    )
    .expect("activity page");
    assert_eq!(activity.calls.len(), 1);
    assert!(!activity.calls[0].shell_query_available, "a carried call has no live output");
    let result: AgentResultView = serde_json::from_value(
        call(
            &runtime,
            "agent.result",
            json!(AgentResultRequest {
                session_id: made.session_id,
                tool_call_id: write,
                offset: 0,
                limit: 65_536
            }),
        )
        .await,
    )
    .expect("result page");
    assert!(!result.text.is_empty());

    // Deleting the source leaves the branch whole: it still shows and continues.
    delete(&runtime, source).await;
    let page = history(&runtime, made.session_id).await;
    assert_eq!(page.turns.len(), 2);
    assert_eq!(
        whole_text(&runtime, made.session_id, AgentMessageTextSource::User { event_id: user })
            .await,
        FIRST
    );
    turn(&runtime, made.session_id, model, AGAIN).await;
    let after = requests(&provider);
    let continued = history_part(after.last().expect("the branch's third request"));
    assert!(
        continued.0.starts_with(&carried.0),
        "the carried history is still sent first after its source is gone"
    );

    // A branch of the branch at one of its own turns carries both: the turn it carried, shared,
    // and its own turn, read from it. It is sent what the branch was sent after that turn.
    let own_turn = history(&runtime, made.session_id).await.turns[1].turn.turn_task_id;
    let third = branch(&runtime, made.session_id, own_turn).await;
    assert_eq!(third.origin.turn_number, 2);
    turn(&runtime, third.session_id, model, AGAIN).await;
    assert_eq!(history_part(requests(&provider).last().expect("third")), continued);

    // A branch of the branch, at the carried turn, is sent what the first branch was.
    let again = branch(&runtime, made.session_id, first_turn).await;
    assert_eq!(again.origin.source_session_id, made.session_id);
    assert_eq!(again.origin.turn_number, 1);
    turn(&runtime, again.session_id, model, NEXT).await;
    assert_eq!(history_part(requests(&provider).last().expect("again")), carried);
    delete(&runtime, made.session_id).await;
    // Deleting a branch deletes what it kept; what it shared stays with the other.
    for (what, sql) in [
        ("records", "SELECT COUNT(*) FROM events WHERE owner='agent-branch:' || ?1"),
        ("artifacts", "SELECT COUNT(*) FROM artifact_owners WHERE owner=?1"),
        ("compactions", "SELECT COUNT(*) FROM context_compactions WHERE session_id=?1"),
        ("tool groups", "SELECT COUNT(*) FROM history_atomic_groups WHERE session_id=?1"),
    ] {
        assert_eq!(count(&runtime, sql, made.session_id), 0, "a deleted branch keeps no {what}");
    }
    assert!(
        count(&runtime, "SELECT COUNT(*) FROM artifact_owners WHERE owner=?1", again.session_id)
            > 0
    );
    assert_eq!(history(&runtime, again.session_id).await.turns.len(), 2);
    turn(&runtime, again.session_id, model, AGAIN).await;
}

#[tokio::test]
async fn a_branch_that_cannot_be_made_leaves_no_conversation_behind() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Branching).await;
    let (runtime, source, _model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let before = services(&runtime).persistence.workspace_session_projection().expect("p").sessions;
    let refused = crate::r03_tests::request(
        &runtime,
        "agent.session.branch",
        json!(AgentSessionBranchRequest { session_id: source, turn_task_id: TurnTaskId::new() }),
    )
    .await;
    let message = refused["error"]["message"].as_str().unwrap_or_default();
    assert!(message.contains("agent.branch_turn_unknown"), "{refused}");
    let after = services(&runtime).persistence.workspace_session_projection().expect("p").sessions;
    assert_eq!(after.len(), before.len(), "no half branch is left");
}

/// A compaction made during a carried turn is carried as the branch's own: the branch is sent the
/// same summary its source is, and the model can read it back by the identity the summary names.
#[tokio::test]
async fn a_branch_carries_its_sources_compaction() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::BranchingCompaction).await;
    let (runtime, source, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    // Larger than the retention budget, so the call reading it is what compaction replaces.
    std::fs::write(
        host.root.path().join("waiting-workspace").join(provider_fixture::COMPACTION_SOURCE_FILE),
        "a line of the compacted source file\n".repeat(3_400),
    )
    .expect("compaction source");
    let first = turn(&runtime, source, model, FIRST).await;
    let compacted = {
        let store = services(&runtime).broker.lock().expect("V2 store");
        aura_persistence_v2::context::compaction::latest(store.database.connection(), source)
            .expect("compaction read")
            .expect("the first turn was compacted")
    };

    let made = branch(&runtime, source, first.turn.turn_task_id).await;
    let continued = turn(&runtime, made.session_id, model, NEXT).await;
    turn(&runtime, source, model, NEXT).await;
    let sent = requests(&provider);
    let summary = |body: &Value| body.to_string().contains("Historical-Snapshot-Summary");
    let (branch_request, source_request) = (&sent[sent.len() - 2], &sent[sent.len() - 1]);
    assert!(summary(branch_request), "the branch is sent the summary");
    assert_eq!(history_part(branch_request), history_part(source_request));

    // The model reads the summary back by the identity it was told.
    {
        let store = services(&runtime).broker.lock().expect("V2 store");
        let read = aura_persistence_v2::context::read::compaction(
            store.database.connection(),
            made.session_id,
            continued.run.agent_run_id,
            &aura_context_domain::AgentContextReadCompactionInput {
                source_context_snapshot_id: compacted.id,
                sequence: compacted.sequence,
            },
        )
        .expect("the branch reads its copy");
        assert_eq!(Some(read), compacted.result);
    }

    // Without its source, the branch still sends the summary and continues.
    delete(&runtime, source).await;
    turn(&runtime, made.session_id, model, AGAIN).await;
    assert!(summary(requests(&provider).last().expect("last request")));
}

/// The branch's own compaction takes in the history it carried: carried items have their place
/// and their tool groups in the branch, whatever became of the conversation they came from.
#[tokio::test]
async fn a_branch_compacts_the_history_it_carried() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::BranchingThenCompact).await;
    let (runtime, source, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let first = turn(&runtime, source, model, FIRST).await;
    let made = branch(&runtime, source, first.turn.turn_task_id).await;
    delete(&runtime, source).await;
    turn(&runtime, made.session_id, model, NEXT).await;
    // Core finishes the turn's own writes before the store is read here.
    if let crate::StartupMode::Ready(s) = &runtime.startup_mode {
        let run = waiting_run(&runtime, made.session_id).await;
        s.agent_required().idle(run).await.expect("the turn's worker settles");
    }
    let store = services(&runtime).broker.lock().expect("V2 store");
    let own = aura_persistence_v2::context::compaction::latest(
        store.database.connection(),
        made.session_id,
    )
    .expect("compaction read")
    .expect("the branch compacted its history");
    assert!(!own.attempts.is_empty(), "the branch's own compaction, not a copy");
    // The carried history was planned with the branch's own records: its source is gone, so
    // none of its events is left to give a carried item its place or its tool group.
    let db = store.database.connection();
    let carried =
        aura_persistence_v2::agent::branch::history_sequences(db, made.session_id).expect("places");
    assert!(carried.contains_key(&first.turn.root_user_event_id));
    for event in carried.keys() {
        let left: i64 = db
            .query_row("SELECT COUNT(*) FROM events WHERE event_id=?1", [event.to_string()], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(left, 0, "the source's event {event} is gone");
    }
}

/// The branch's latest run, so a test can wait for its worker to settle.
async fn waiting_run(runtime: &crate::RuntimeState, session: SessionId) -> AgentRunId {
    history(runtime, session).await.turns.last().expect("a turn").turn.agent_run_id
}
