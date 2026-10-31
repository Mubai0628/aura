//! A request is
//! compacted only when it does not fit with room for the output a step can use — at least 32,000
//! tokens, at most the model's limit — and a request that fits is sent with all the room the
//! window has left for its output. A compaction is asked on the request the step would have sent,
//! so the provider's cache of it is used again; a tool call in its answer never runs.
use super::l10_15_compaction::{call, compactions, first_request_tokens, start};
use super::*;

/// What a conversation's request was counted at, as its step's context meter shows it.
pub(super) async fn used(runtime: &crate::RuntimeState, session: SessionId, turn: usize) -> u64 {
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
    page.turns[turn].assistant_messages[0]
        .context_usage
        .as_ref()
        .expect("the step's context meter")
        .used_input_tokens
}

/// Whether a request asks for a compaction rather than for the next step.
pub(super) fn asks_compaction(request: &Value) -> bool {
    request["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["content"]
                .as_str()
                .is_some_and(|text| text.contains("Write the summary as one paragraph"))
        })
    })
}

/// Three turns: a short one, one with a long message, then a short one whose answers are cut off
/// twice, the second time at 20,034 characters. The window is set before the third, so the third
/// step, continuing that answer, misses the 32,000 tokens it must leave for the output by about
/// 6,000 — the earlier turns must be compacted — while its request with the compaction asked in
/// its task state still leaves the summary about 26,000 less the ask. Compacting at a turn's third
/// step compares it with a step that sent the turn's message as the history does: the first step
/// alone sends it with the turn's working set.
pub(super) async fn just_over_the_line(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    output: u64,
) -> (crate::RuntimeState, SessionId) {
    let (runtime, session, model) = before_the_third(host, provider, output).await;
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    let third = start(&runtime, session, model, "AURA-L1019-THIRD: answer at length").await;
    waiting::wait(&runtime, &third, done).await;
    (runtime, session)
}

/// The first two turns of [`just_over_the_line`], with the window set for the third.
pub(super) async fn before_the_third(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    output: u64,
) -> (crate::RuntimeState, SessionId, ModelProfileId) {
    let (runtime, session, model) =
        waiting::prepare_in_mode(host, provider, PermissionMode::WorkspaceReadWrite).await;
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    let first = start(&runtime, session, model, "AURA-L1019-FIRST: answer briefly").await;
    waiting::wait(&runtime, &first, done).await;
    let long = format!("AURA-L1019-LONG: keep this in mind. {}", "a detail kept ".repeat(2_000));
    let second = start(&runtime, session, model, &long).await;
    waiting::wait(&runtime, &second, done).await;
    // The third turn's requests are the second's, its short answer, a short message and the first,
    // short cut-off answer; the third step adds the long one (`provider_fixture`, ordinal 3).
    let continuing = used(&runtime, session, 1).await + 400 + 20_034;
    let usable = continuing + 26_000;
    let window = (usable * 100).div_ceil(95).max(usable + 4_096);
    resize(&runtime, model, window, output).await;
    (runtime, session, model)
}

#[tokio::test]
async fn a_compaction_is_asked_on_the_request_the_step_would_have_sent() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let (runtime, session) = just_over_the_line(&host, &provider, 64_000).await;
    assert_eq!(compactions(&runtime, session), 1);
    let requests = provider.requests.lock().expect("provider requests").clone();
    let asked = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| asks_compaction(request))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(asked.len(), 1, "answered the first time, so asked once");
    let (previous, compaction) = (&requests[asked[0] - 1], &requests[asked[0]]);
    // Everything the previous step was sent before its task state, the tools included, starts the
    // compaction request: the part a provider has cached.
    assert_eq!(compaction["tools"], previous["tools"]);
    let messages = |request: &Value| request["messages"].as_array().expect("messages").clone();
    let (before, now) = (messages(previous), messages(compaction));
    assert!(now.len() > before.len());
    // Each message's role and opening, to read a failure by.
    let layout = |messages: &[Value]| {
        messages
            .iter()
            .map(|m| {
                let text = m["content"].as_str().unwrap_or("").chars().take(40).collect::<String>();
                format!("{}: {text}", m["role"].as_str().unwrap_or("?"))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    for (index, (was, is)) in before[..before.len() - 1].iter().zip(&now).enumerate() {
        let (was, is) = (was.to_string(), is.to_string());
        let at = was.chars().zip(is.chars()).take_while(|(a, b)| a == b).count();
        let near =
            |text: &str| text.chars().skip(at.saturating_sub(80)).take(200).collect::<String>();
        assert!(
            was == is,
            "message {index} differs at {at}:\nwas …{}\nis  …{}\nprevious:\n{}\ncompaction:\n{}",
            near(&was),
            near(&is),
            layout(&before),
            layout(&now)
        );
    }
    // The step's task state stays where the step sends it, and the ask follows it as a user
    // message of its own; no tool choice is named, so the request's tools stay as they were.
    let state = &now[now.len() - 2];
    assert_eq!(state["role"], before.last().expect("the step's task state")["role"]);
    assert!(state["content"].as_str().is_some_and(|text| text.contains("\"turn\"")), "{state}");
    let ask = now.last().expect("the ask");
    assert_eq!(ask["role"], "user", "{}", layout(&now));
    let ask = ask["content"].as_str().expect("text").to_owned();
    assert!(
        ask.contains("Compaction request:")
            && ask.contains("from its first message through the one that begins"),
        "{ask}"
    );
    assert!(compaction.get("tool_choice").is_none());
}

#[tokio::test]
async fn a_tool_call_in_a_compaction_answer_never_runs() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactToolOnPrefix).await;
    let (runtime, session) = just_over_the_line(&host, &provider, 64_000).await;
    assert_eq!(compactions(&runtime, session), 1, "the third attempt summarized");
    let written =
        host.root.path().join("waiting-workspace").join(provider_fixture::COMPACTION_TOOL_FILE);
    assert!(!written.exists(), "the writes compaction answers asked for never ran");
    let requests = provider.requests.lock().expect("provider requests").clone();
    let asked = requests.iter().filter(|request| asks_compaction(request)).collect::<Vec<_>>();
    assert_eq!(asked.len(), 3);
    let tools = |request: &Value| request["tools"].as_array().map_or(0, Vec::len);
    assert!(tools(asked[0]) > 0, "first on the step's prefix, its tools included");
    assert_eq!(tools(asked[1]), tools(asked[0]), "then on it again with the call answered");
    assert_eq!(tools(asked[2]), 0, "then, called again, the isolated way, without tools");
}

/// The model's window and output limit, set through the routes the settings page uses.
pub(super) async fn resize(
    runtime: &crate::RuntimeState,
    model: ModelProfileId,
    window: u64,
    output: u64,
) {
    let revision =
        call(runtime, "models.settings.get_snapshot", json!({})).await["models_revision"].clone();
    let updated = call(
        runtime,
        "models.profile.update",
        json!({"expected_revision":revision,"model_profile_id":model,"display_name":"Fixture",
            "notes":null,"enabled":true,"request_defaults":{"max_output_tokens":output,
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

/// The output limit the provider was asked for in a chat request.
fn requested_output(request: &Value) -> u64 {
    request["max_tokens"]
        .as_u64()
        .or_else(|| request["max_completion_tokens"].as_u64())
        .expect("the request's output limit")
}

#[tokio::test]
async fn a_request_that_fits_with_the_floor_reserved_is_sent_with_the_room_left_for_its_output() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    // Counted byte for byte, a bound: the output gets all the room left. A request counted by the
    // rule of thumb keeps a margin for its text being longer (`with_room_for_output`).
    count_by_bytes(model);
    let first = start(&runtime, session, model, "AURA-L1019-FIRST: answer briefly").await;
    waiting::wait(&runtime, &first, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    // Room for the next request and 40,000 tokens of output, under a 64,000-token limit. Before
    // The whole limit was held back for the output, so this request could not be sent.
    let window = first_request_tokens(&runtime, session).await + 600 + 40_000;
    resize(&runtime, model, window, 64_000).await;

    let second = start(&runtime, session, model, "AURA-L1019-SECOND: answer again").await;
    waiting::wait(&runtime, &second, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    assert_eq!(compactions(&runtime, session), 0, "it fits with the floor kept for the output");
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
    let used = page.turns[1].assistant_messages[0]
        .context_usage
        .as_ref()
        .expect("the step's context meter")
        .used_input_tokens;
    let safety = 4_096_u64.max((window * 5).div_ceil(100));
    let room = window - safety - used;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let sent = requested_output(requests.last().expect("the second request"));
    assert_eq!(sent, room.min(64_000), "the output gets the room the window has left");
    assert!((32_000..64_000).contains(&sent), "{sent} lies between the floor and the limit");
}

/// What the latest turn's last two steps were counted at, as their context meters show them.
pub(super) async fn last_two_steps(
    runtime: &crate::RuntimeState,
    session: SessionId,
) -> (u64, u64) {
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
    let meters = page
        .turns
        .last()
        .expect("the turn")
        .assistant_messages
        .iter()
        .filter_map(|step| step.context_usage.as_ref().map(|usage| usage.used_input_tokens))
        .collect::<Vec<_>>();
    let [.., before, last] = meters.as_slice() else { panic!("two steps: {meters:?}") };
    (*before, *last)
}

/// The tool results a chat request shows cleared, in order.
pub(super) fn cleared(request: &Value) -> Vec<String> {
    request["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .filter_map(|message| message["content"].as_str())
        .filter(|text| text.contains("Earlier tool result cleared to save context"))
        .map(ToOwned::to_owned)
        .collect()
}

/// How long ten steps of reads may take to reach the parked request. Alone they take seconds;
/// beside a full parallel test run, well over half a minute. The wait ends once it is parked.
pub(super) const PARKED_WITHIN: std::time::Duration = std::time::Duration::from_secs(120);

#[tokio::test]
async fn a_request_over_the_line_is_compacted_deeply_before_any_old_result_is_cleared() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::ClearOldResults).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    // Eleven files of 6,993 characters each: a result under the 8,192 that would cut it to a preview.
    for index in 0..=10 {
        let file = host.root.path().join("waiting-workspace").join(format!("read-{index}.txt"));
        let text = format!("file {index:02} ") + &"read one by one ".repeat(436);
        std::fs::write(file, text).expect("a file a step reads");
    }
    let run = start(&runtime, session, model, "AURA-L1019-READS: read the files in order").await;
    if tokio::time::timeout(PARKED_WITHIN, provider.parked.notified()).await.is_err() {
        let status = call(&runtime, "agent.status", json!(run)).await;
        let count = provider.requests.lock().expect("provider requests").len();
        let crate::StartupMode::Ready(services) = &runtime.startup_mode else {
            panic!("Core ready")
        };
        let store = services.broker.lock().expect("V2 store");
        let mut query = store
            .database
            .connection()
            .prepare("SELECT kind, payload FROM events WHERE kind IN ('agent.application_error','model.failure','tool.dispatch_adapter_error','agent.run_interrupted') ORDER BY sequence")
            .expect("failure query");
        let failures = query
            .query_map([], |row| {
                Ok(format!("{}: {}", row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("failures")
            .collect::<Result<Vec<_>, _>>()
            .expect("failures");
        panic!(
            "the tenth request was not parked: {count} requests; {}; {failures:#?}",
            status["turn"]["state"]
        );
    }
    // The eleventh request misses the 32,000 tokens it must leave for the output by about half a
    // step. The history older than the recent part is summarized, all of it at once, and no result is
    // cleared — clearing a batch would change the request in its middle and lose the provider's
    // cache for all that follows.
    let (ninth, tenth) = last_two_steps(&runtime, session).await;
    let usable = tenth + (tenth - ninth) / 2 + 32_000;
    let window = (usable * 100).div_ceil(95).max(usable + 4_096);
    resize(&runtime, model, window, 64_000).await;
    provider.release_window.notify_one();
    tokio::time::timeout(PARKED_WITHIN, provider.parked.notified())
        .await
        .expect("the eleventh request is parked");
    provider.release_window.notify_one();
    waiting::wait(&runtime, &run, |v| matches!(v.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let asked = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| asks_compaction(request))
        .map(|(index, request)| {
            let text = request.to_string();
            let on_prefix = request["tools"].as_array().is_some_and(|tools| !tools.is_empty());
            (index, on_prefix, text.matches("\\\"anchor\\\":").count(), text.len())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        compactions(&runtime, session),
        1,
        "one compaction took all the older history: {asked:?}"
    );
    assert!(requests.iter().all(|request| cleared(request).is_empty()), "nothing was cleared");
    let normal = requests.iter().filter(|request| !asks_compaction(request)).collect::<Vec<_>>();
    assert_eq!(normal.len(), 12, "eleven reads and the answer");
    let results = |request: &Value| {
        request["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .filter(|m| m["role"] == "tool")
            .count()
    };
    assert_eq!(results(normal[9]), 9, "the tenth request carried every read so far");
    // What stays as it was is the recent part, 8%-10% of the window (a read or two), and the
    // step's own latest read, which is not yet sealed.
    assert!(results(normal[10]) <= 3, "{} results stayed", results(normal[10]));
    let steps = all_steps(&runtime, session).await;
    assert!(steps.iter().all(|step| step.context_cleared.is_none()));
    let compacted =
        steps.iter().filter_map(|step| step.context_compacted.as_ref()).collect::<Vec<_>>();
    let [note] = compacted.as_slice() else { panic!("one compaction, one line") };
    assert!(!note.manual);
}

/// Every step of the conversation's one turn, oldest first: a page holds the newest steps, and its
/// cursor leads to the older ones.
async fn all_steps(
    runtime: &crate::RuntimeState,
    session: SessionId,
) -> Vec<aura_projection_contract::AgentAssistantView> {
    let mut steps = Vec::new();
    let mut within_turn = None;
    loop {
        let page: AgentSessionProjection = serde_json::from_value(
            call(
                runtime,
                "agent.history",
                json!(AgentHistoryRequest {
                    session_id: session,
                    before_turn_task_id: None,
                    within_turn,
                    limit: 1,
                }),
            )
            .await,
        )
        .expect("history page");
        let turn = page.turns.last().expect("the turn");
        steps.splice(0..0, turn.assistant_messages.iter().cloned());
        let Some(cursor) = turn.next_messages.clone() else { return steps };
        within_turn = Some(cursor);
    }
}

/// From a real DeepSeek run: a model whose output limit is larger than its window less
/// the safety margin. Its steps fit with the floor kept for the output; its compaction sized its
/// room with the whole limit, found none, and ended the turn ("context.compaction_output_budget").
/// The compaction keeps the floor too, on the prefix and, after its tool calls, the isolated way.
#[tokio::test]
async fn a_model_whose_output_limit_passes_its_window_still_compacts() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactToolOnPrefix).await;
    let (runtime, session) = just_over_the_line(&host, &provider, 400_000).await;
    assert_eq!(compactions(&runtime, session), 1, "compacted the isolated way after the tool call");
    let requests = provider.requests.lock().expect("provider requests").clone();
    let isolated = requests.iter().rfind(|request| asks_compaction(request)).expect("an ask");
    let last = requests.last().map(requested_output).expect("a request");
    assert!(requested_output(isolated) < 400_000 && last < 400_000, "outputs fit the window");
}
