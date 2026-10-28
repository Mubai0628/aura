//! A conversation moved to a model with a smaller window keeps going. Crossing the automatic
//! compaction line with nothing left to compact is answered, not failed; a request that cannot
//! fit whatever is compacted waits for the user's decision with its reasons instead of failing.
use super::*;
use aura_context_domain::AgentContextRemediationView;

pub(super) async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

pub(super) async fn start(
    runtime: &crate::RuntimeState,
    session: SessionId,
    model: ModelProfileId,
    text: &str,
) -> AgentRunRequest {
    let started: AgentRunProjection = serde_json::from_value(
        call(
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
        .await,
    )
    .expect("run projection");
    AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id }
}

/// What the first request of the conversation was estimated at, as the window shows it.
pub(super) async fn first_request_tokens(runtime: &crate::RuntimeState, session: SessionId) -> u64 {
    let page: AgentSessionProjection = serde_json::from_value(
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
    .expect("history page");
    page.turns[0].assistant_messages[0]
        .context_usage
        .as_ref()
        .expect("the step's context meter")
        .used_input_tokens
}

/// The model moves to a smaller window, the way choosing a smaller model does: a window of its
/// own and a smaller output reserve, so the next request lands where the test needs it.
async fn shrink(runtime: &crate::RuntimeState, model: ModelProfileId, window: u64) {
    let revision =
        call(runtime, "models.settings.get_snapshot", json!({})).await["models_revision"].clone();
    let updated = call(
        runtime,
        "models.profile.update",
        json!({"expected_revision":revision,"model_profile_id":model,"display_name":"Fixture",
            "notes":null,"enabled":true,"request_defaults":{"max_output_tokens":1024,
            "temperature":null,"top_p":null,"chat_output_token_field":null,
            "chat_instruction_role":null},"historical_snapshot_max_tokens":1024}),
    )
    .await;
    call(
        runtime,
        "models.context_window.correct",
        json!({"expected_revision":updated["revision"],"model_profile_id":model,
            "window_tokens":window}),
    )
    .await;
}

pub(super) fn compactions(runtime: &crate::RuntimeState, session: SessionId) -> usize {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let store = services.broker.lock().expect("V2 store");
    store
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM context_compactions WHERE session_id=?1",
            [session.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .expect("compaction count") as usize
}

#[tokio::test]
async fn a_request_that_fits_is_sent_without_compaction_however_full_the_window() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let first = start(&runtime, session, model, "AURA-L1015-FIRST: answer briefly").await;
    waiting::wait(&runtime, &first, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    // The next request is about the first one plus the first exchange. A window where it is 85%
    // of the window still holds it and the output it may write; a fixed 80% line once
    // compacted it, found nothing to compact, and ended the turn.
    let next = first_request_tokens(&runtime, session).await + 600;
    shrink(&runtime, model, next * 100 / 85).await;

    let second = start(&runtime, session, model, "AURA-L1015-SECOND: answer again").await;
    let done = waiting::wait(&runtime, &second, |v| {
        matches!(v.turn.state, TurnTaskState::Completed { .. })
    })
    .await;
    assert!(matches!(done.turn.state, TurnTaskState::Completed { .. }));
    assert_eq!(compactions(&runtime, session), 0, "it fits, so nothing is compacted");
}

/// The Provider counts each request at a quarter of its bytes. A window the byte count alone
/// would overflow — even its instructions and tools — holds the second request: the parts sent
/// again are counted at the Provider's rate.
#[tokio::test]
async fn a_conversation_is_counted_at_its_providers_rate() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PlainMeasured).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    // The first request, which no count covers, goes byte for byte: the window is set against it.
    count_by_bytes(model);
    let first = start(&runtime, session, model, "AURA-L1015-FIRST: answer briefly").await;
    waiting::wait(&runtime, &first, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let bytes = first_request_tokens(&runtime, session).await;
    shrink(&runtime, model, bytes * 6 / 10).await;

    let second = start(&runtime, session, model, "AURA-L1015-SECOND: answer again").await;
    waiting::wait(&runtime, &second, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(compactions(&runtime, session), 0);
    let page: AgentSessionProjection = serde_json::from_value(
        call(
            &runtime,
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
    .expect("history page");
    let meter = page.turns[1].assistant_messages[0].context_usage.clone().expect("the meter");
    assert!(meter.estimated);
    assert!(
        meter.used_input_tokens < bytes / 2,
        "counted near the Provider's rate: {} of {bytes} bytes",
        meter.used_input_tokens
    );
}

#[tokio::test]
async fn a_request_that_cannot_fit_waits_for_the_user_instead_of_failing() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let first = start(&runtime, session, model, "AURA-L1015-FIRST: answer briefly").await;
    waiting::wait(&runtime, &first, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    // A window smaller than the instructions and tools alone: no compaction can make it fit.
    let fixed = first_request_tokens(&runtime, session).await;
    shrink(&runtime, model, fixed * 9 / 10).await;

    let second = start(&runtime, session, model, "AURA-L1015-SECOND: answer again").await;
    waiting::wait(&runtime, &second, |v| {
        matches!(v.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    let view: AgentContextRemediationView =
        serde_json::from_value(call(&runtime, "agent.context.remediation", json!(second)).await)
            .expect("remediation view");
    let remediation = view.remediation.expect("the reasons and what to do");
    assert_eq!(remediation.plan.overflow.error_code, "context.non_compressible_overflow");
    assert_eq!(remediation.plan.actions, ["reduce_user_managed_context"]);
    assert_eq!(compactions(&runtime, session), 0, "a compaction could not have helped");
}

/// A step the provider refused has no count; the next request is still counted at the rate of the
/// conversation's newest counted request instead of falling back to its bytes.
#[tokio::test]
async fn a_refused_step_does_not_lose_the_providers_rate() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PlainMeasuredRefusingSecond).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    count_by_bytes(model);
    let first = start(&runtime, session, model, "AURA-L1015-FIRST: answer briefly").await;
    waiting::wait(&runtime, &first, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let bytes = first_request_tokens(&runtime, session).await;
    let refused = start(&runtime, session, model, "AURA-L1015-REFUSED: answer again").await;
    waiting::wait(&runtime, &refused, |v| v.turn.state.terminal()).await;
    shrink(&runtime, model, bytes * 6 / 10).await;

    let third = start(&runtime, session, model, "AURA-L1015-THIRD: and once more").await;
    waiting::wait(&runtime, &third, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(compactions(&runtime, session), 0);
}
