//! A turn Core's restart cut off while Aura stayed open is continued by Core itself: the new Core
//! says "继续" once, ahead of the queue, and the queued message follows it. After Aura was
//! closed nothing continues by itself; the user's 继续 does, and the queue stays paused. A turn
//! cut off again right after Aura continued it is left for the user.
use super::*;
use aura_persistence_v2::agent::restart_continuation::Decision;
use aura_projection_contract::AgentTurnView;
use provider_fixture::Scenario;
use std::time::Duration;

/// Runs `work` on a runtime of its own and then drops that runtime without any graceful stop,
/// as a Core that crashed or was ended leaves its stores.
async fn crashed<T: Send + 'static>(
    work: impl FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>
    + Send
    + 'static,
) -> T {
    tokio::task::spawn_blocking(move || {
        let process =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        let value = process.block_on(work());
        process.shutdown_timeout(Duration::from_secs(2));
        value
    })
    .await
    .expect("the crashed runtime's work")
}

async fn requests_reach(provider: &Provider, count: usize) {
    tokio::time::timeout(Duration::from_secs(30), async {
        while provider.requests.lock().expect("requests").len() < count {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the model requests arrive");
}

/// One turn whose model request is held, with one message queued behind it; then the Core
/// "crashes". Returns the session and the cut-off turn.
async fn cut_off_with_a_queued_message(
    host: &Arc<crate::r03_tests::Fixture>,
    provider: &Arc<Provider>,
) -> (SessionId, TurnTaskId) {
    let (host, provider) = (host.clone(), provider.clone());
    crashed(move || {
        Box::pin(async move {
            let (runtime, start) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
            requests_reach(&provider, 1).await;
            crate::r03_tests::success(
                &runtime,
                "agent.submit",
                json!(AgentSubmitRequest {
                    session_id: start.turn.session_id,
                    text: "Then the queued request".into(),
                    destination: AgentInputLane::NextTurn,
                    skill_ids: Vec::new(),
                    reasoning_effort: None,
                    attachments: Vec::new(),
                }),
            )
            .await;
            (start.turn.session_id, start.turn.turn_task_id)
        })
    })
    .await
}

fn agent(runtime: &crate::RuntimeState) -> Arc<AgentApplication> {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services.agent_required()
}

async fn turns(runtime: &crate::RuntimeState, session_id: SessionId) -> Vec<AgentTurnView> {
    agent(runtime)
        .history(AgentHistoryRequest {
            session_id,
            before_turn_task_id: None,
            within_turn: None,
            limit: 10,
        })
        .await
        .expect("history")
        .turns
}

/// Waits until the session has `count` turns and the last of them completed.
async fn completed_turns(
    runtime: &crate::RuntimeState,
    session_id: SessionId,
    count: usize,
) -> Vec<AgentTurnView> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let seen = turns(runtime, session_id).await;
            if seen.len() == count
                && seen
                    .last()
                    .is_some_and(|last| matches!(last.turn.state, TurnTaskState::Completed { .. }))
            {
                return seen;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the turns complete")
}

/// The session's model, as the composer leaves it bound, so the queue can start its message.
async fn bind_model(runtime: &crate::RuntimeState, session_id: SessionId, cut: TurnTaskId) {
    let configured = agent(runtime)
        .store
        .read(crate::agent_store::StoreStep::operation("test.model"), move |db| {
            let run = aura_persistence_v2::agent::turns::load(db, cut)?.agent_run_id;
            aura_persistence_v2::agent::application::configuration(db, run)
        })
        .await
        .expect("the cut-off run's configuration");
    crate::r03_tests::success(
        runtime,
        "models.session.set_binding",
        json!({
            "expected_revision": "0",
            "session_id": session_id,
            "model_profile_id": configured.request.model_profile_id,
        }),
    )
    .await;
}

fn said(view: &AgentTurnView) -> String {
    view.user_messages
        .iter()
        .find(|message| message.event_id == view.turn.root_user_event_id)
        .map(|message| message.text.clone())
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_cut_off_while_aura_stayed_open_is_continued_and_the_queue_follows() {
    let host = Arc::new(crate::r03_tests::Fixture::new());
    let provider = Arc::new(Provider::scenario(Scenario::RecoveredSession).await);
    let (session_id, cut) = cut_off_with_a_queued_message(&host, &provider).await;

    let runtime = host.start().await;
    bind_model(&runtime, session_id, cut).await;
    let decisions = agent(&runtime).continue_after_restart(CoreRestartCause::Unresponsive).await;
    assert_eq!(decisions, vec![(cut, Decision::Continue)]);

    let seen = completed_turns(&runtime, session_id, 3).await;
    assert_eq!(seen[0].turn.turn_task_id, cut);
    assert!(matches!(seen[0].turn.state, TurnTaskState::RecoveryRequired { .. }));
    assert_eq!(said(&seen[1]), "继续", "Aura says only what the user would");
    let note = seen[1].auto_continued.as_ref().expect("the conversation says it continued");
    assert_eq!(note.cause, CoreRestartCause::Unresponsive);
    assert!(matches!(seen[1].turn.state, TurnTaskState::Completed { .. }));
    assert_eq!(said(&seen[2]), "Then the queued request", "the queue follows the continuation");
    assert_eq!(seen[2].auto_continued, None);
    assert_eq!(
        provider.requests.lock().expect("requests").len(),
        3,
        "the cut-off request is never sent again"
    );
    // Once is all: the start's list is spent.
    assert_eq!(agent(&runtime).continue_after_restart(CoreRestartCause::Crash).await, vec![]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_aura_was_closed_only_the_users_continue_continues_and_the_queue_waits() {
    let host = Arc::new(crate::r03_tests::Fixture::new());
    let provider = Arc::new(Provider::scenario(Scenario::RecoveredSession).await);
    let (session_id, cut) = cut_off_with_a_queued_message(&host, &provider).await;

    // Aura was closed: Main passes no cause, so nothing is continued by itself.
    let runtime = host.start().await;
    bind_model(&runtime, session_id, cut).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(turns(&runtime, session_id).await.len(), 1);

    crate::r03_tests::success(
        &runtime,
        "agent.continue",
        json!(AgentContinueRequest { session_id, turn_task_id: cut }),
    )
    .await;
    let seen = completed_turns(&runtime, session_id, 2).await;
    assert_eq!(said(&seen[1]), "继续");
    assert_eq!(seen[1].auto_continued, None, "the user continued it");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(turns(&runtime, session_id).await.len(), 2, "the queue waits for the user");
    let inbox = agent(&runtime).inbox(AgentSessionRequest { session_id }).await.expect("inbox");
    assert!(inbox.paused && inbox.messages.len() == 1, "{inbox:?}");

    // The same 继续 again is stale: the turn was already continued.
    let stale = crate::r03_tests::request(
        &runtime,
        "agent.continue",
        json!(AgentContinueRequest { session_id, turn_task_id: cut }),
    )
    .await;
    assert!(stale.get("error").is_some(), "{stale}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_cut_off_again_right_after_aura_continued_it_is_left_for_the_user() {
    let host = Arc::new(crate::r03_tests::Fixture::new());
    // Every request is held: the continuation is cut off too.
    let provider = Arc::new(Provider::scenario(Scenario::Hold).await);
    let (session_id, cut) = cut_off_with_a_queued_message(&host, &provider).await;

    let (second_host, second_provider) = (host.clone(), provider.clone());
    let continued = crashed(move || {
        Box::pin(async move {
            let runtime = second_host.start().await;
            bind_model(&runtime, session_id, cut).await;
            let decisions = agent(&runtime).continue_after_restart(CoreRestartCause::Crash).await;
            requests_reach(&second_provider, 2).await;
            let latest =
                turns(&runtime, session_id).await.last().map(|view| view.turn.turn_task_id);
            (decisions, latest)
        })
    })
    .await;
    assert_eq!(continued.0, vec![(cut, Decision::Continue)]);
    let continuation = continued.1.expect("the continuation turn");
    assert_ne!(continuation, cut);

    let runtime = host.start().await;
    let decisions = agent(&runtime).continue_after_restart(CoreRestartCause::Crash).await;
    assert_eq!(decisions, vec![(continuation, Decision::RestartLoop)]);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let seen = turns(&runtime, session_id).await;
    assert_eq!(seen.len(), 2, "nothing more is started by itself");
    assert!(matches!(seen[1].turn.state, TurnTaskState::RecoveryRequired { .. }));
    assert_eq!(provider.requests.lock().expect("requests").len(), 2);
}
