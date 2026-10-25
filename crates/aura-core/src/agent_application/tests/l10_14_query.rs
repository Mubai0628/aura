//! Touching a question moves its deadline; the user can answer in their own
//! words or decline; a question nobody touches ends as "no response". Whichever way it ends, the
//! run goes on and the model reads the question, the chosen labels and how it ended.
use super::*;
use aura_persistence_v2::agent::query;

/// A run of the scripted question scenario, waiting on its question, its worker settled.
async fn waiting_question(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, AgentRunRequest, AgentRunProjection) {
    let (runtime, start) = begin(host, provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    let waiting =
        wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::AwaitingQuery { .. }))
            .await;
    if let crate::StartupMode::Ready(s) = &runtime.startup_mode {
        s.agent_required().idle(request.agent_run_id).await.expect("the run's worker settles");
    }
    (runtime, request, waiting)
}

fn answer(
    request: &AgentRunRequest,
    waiting: &AgentRunProjection,
    value: QueryAnswerValue,
) -> AgentQueryAnswerRequest {
    let query = waiting.pending_query.as_ref().expect("a waiting question");
    AgentQueryAnswerRequest {
        session_id: request.session_id,
        agent_run_id: request.agent_run_id,
        query_id: query.query_id,
        expected_revision: query.revision,
        answers: vec![QueryAnswer { question_id: "choice".into(), value }],
    }
}

/// What the model read for the question's end: the newest user message, as JSON.
fn reply(runtime: &crate::RuntimeState) -> serde_json::Value {
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    let store = s.broker.lock().expect("V2 store");
    let payload: String = store
        .database
        .connection()
        .query_row(
            "SELECT payload FROM events WHERE kind='agent.user_message' ORDER BY sequence DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .expect("the question's reply");
    let message: serde_json::Value = serde_json::from_str(&payload).expect("user message");
    serde_json::from_str(message["text"].as_str().expect("text")).expect("the reply is JSON")
}

/// The question's tool record, as the conversation reads it.
async fn record(
    runtime: &crate::RuntimeState,
    request: &AgentRunRequest,
    call: ToolCallId,
) -> serde_json::Value {
    let page = crate::r03_tests::success(
        runtime,
        "agent.tool_activity",
        json!({"session_id": request.session_id, "before_call_id": null, "call_ids": [call], "limit": 10}),
    )
    .await;
    page["calls"][0].clone()
}

fn the_model_read(provider: &Provider, words: &str) -> bool {
    provider
        .requests
        .lock()
        .expect("provider requests")
        .iter()
        .any(|request| request.to_string().contains(words))
}

#[tokio::test]
async fn touching_a_question_moves_its_deadline_and_a_custom_reply_reaches_the_model() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, request, waiting) = waiting_question(&host, &provider).await;
    let question = waiting.pending_query.clone().expect("a waiting question");
    let first = waiting.pending_query_deadline_at.expect("a waiting question has a deadline");
    assert_eq!(first, query_deadline(question.created_at, None));

    let touched: AgentQueryTouchView = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.query.touch",
            json!(AgentQueryTouchRequest {
                session_id: request.session_id,
                agent_run_id: request.agent_run_id,
                query_id: question.query_id,
            }),
        )
        .await,
    )
    .expect("touch view");
    assert!(touched.deadline_at >= first, "a touch never brings the deadline nearer");
    let after: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(&runtime, "agent.status", json!(request)).await,
    )
    .expect("status");
    assert_eq!(after.pending_query_deadline_at, Some(touched.deadline_at));
    assert_eq!(
        after.pending_query.as_ref().map(|q| q.revision),
        Some(question.revision),
        "a touch leaves the question's revision alone, so an answer on its way still applies"
    );

    // While the question waits, its record is not finished: the conversation reads it again until
    // the question ends.
    let waiting_record = record(&runtime, &request, question.tool_call_id).await;
    assert!(waiting_record["summary"]["outcome"].is_null(), "{waiting_record}");
    assert_eq!(waiting_record["display"]["summary"], "等待你的回答");

    // "No response" is Core's to record when the time runs out, never the user's.
    let refused = crate::r03_tests::request(
        &runtime,
        "agent.query.answer",
        json!(answer(&request, &waiting, QueryAnswerValue::NoResponse)),
    )
    .await;
    assert!(refused.get("error").is_some(), "{refused}");

    crate::r03_tests::success(
        &runtime,
        "agent.query.answer",
        json!(answer(&request, &waiting, QueryAnswerValue::Custom { text: "先别动文件".into() })),
    )
    .await;
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let body = &reply(&runtime)["query_reply"];
    assert_eq!(body["outcome"], "answered");
    assert_eq!(body["answers"][0]["question"], "Confirm the fixture result?");
    assert_eq!(body["answers"][0]["reply"]["kind"], "custom");
    assert!(the_model_read(&provider, "先别动文件"));
    let answered = record(&runtime, &request, question.tool_call_id).await;
    assert_eq!(answered["summary"]["outcome"]["type"], "succeeded", "{answered}");
    assert_eq!(answered["display"]["summary"], "已回答");
    assert!(answered["display"]["details"].to_string().contains("（自定义）先别动文件"));
}

#[tokio::test]
async fn a_declined_question_tells_the_model_not_to_ask_again_and_the_run_goes_on() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, request, waiting) = waiting_question(&host, &provider).await;
    crate::r03_tests::success(
        &runtime,
        "agent.query.answer",
        json!(answer(&request, &waiting, QueryAnswerValue::Declined)),
    )
    .await;
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let body = &reply(&runtime)["query_reply"];
    assert_eq!(body["outcome"], "declined");
    assert!(the_model_read(&provider, "chose not to answer"));
}

#[tokio::test]
async fn a_question_nobody_touches_ends_as_no_response_and_the_run_goes_on() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Query).await;
    let (runtime, request, waiting) = waiting_question(&host, &provider).await;
    let crate::StartupMode::Ready(s) = &runtime.startup_mode else { unreachable!() };
    let deadline = waiting.pending_query_deadline_at.expect("deadline");
    let run = request.agent_run_id;
    {
        let mut store = s.broker.lock().expect("V2 store");
        // The run's timer waits for the question as it waits for an approval.
        assert_eq!(
            query::wait_deadline(store.database.connection(), run).expect("deadline"),
            Some(deadline)
        );
        let early = UtcTimestamp::from_value(deadline.value() - std::time::Duration::from_secs(1));
        let ended = store
            .database
            .unit_of_work(|tx| query::expire(tx, run, early))
            .expect("expire before the deadline");
        assert!(!ended, "a question is not ended before its time");
    }
    // Nobody touches it: when its time runs out Core's own timer ends it, and the run goes on.
    let done = tokio::time::timeout(std::time::Duration::from_secs(120), async {
        loop {
            let now: AgentRunProjection = serde_json::from_value(
                crate::r03_tests::success(&runtime, "agent.status", json!(request)).await,
            )
            .expect("status");
            match &now.turn.state {
                TurnTaskState::Completed { .. } => return now,
                state if state.terminal() => panic!("the run ended otherwise: {state:?}"),
                _ => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
            }
        }
    })
    .await
    .expect("the run goes on after the question's time runs out");
    assert!(UtcTimestamp::now() >= deadline, "not before its deadline");
    assert_eq!(done.run.agent_run_id, run);
    let body = &reply(&runtime)["query_reply"];
    assert_eq!(body["outcome"], "no_response");
    assert_eq!(body["answers"][0]["reply"]["kind"], "no_response");
    assert!(the_model_read(&provider, "busy or away"));
}
