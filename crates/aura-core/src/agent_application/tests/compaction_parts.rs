//! A compaction asked on the step's request that still calls a tool once its call is answered is
//! not asked on that request again: the same request would be answered the same way. When the
//! history is more than one isolated request can carry, it is taken a part at a time that way, and
//! each part gets the model's summary rather than Core's short one.
use super::l10_15_compaction::{compactions, start};
use super::l10_19_compaction::{asks_compaction, resize, used};
use super::l12_4_compaction::completed;
use super::*;

/// Quotes in each of the two long messages. The step's request carries them as they are; the
/// isolated request carries the history as JSON, where each takes two bytes, so the two messages
/// together are more than it can hold, and each alone fits.
const QUOTES: usize = 120_000;

#[tokio::test]
async fn a_compaction_the_steps_request_cannot_answer_is_taken_a_part_at_a_time() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactToolOnPrefix).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    // Room for both long messages until the window is set for the third turn.
    resize(&runtime, model, 1_000_000, 64_000).await;
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    for label in ["AURA-PARTS-FIRST", "AURA-PARTS-SECOND"] {
        let long = format!("{label}: keep this in mind. {}", "\"".repeat(QUOTES));
        let turn = start(&runtime, session, model, &long).await;
        waiting::wait(&runtime, &turn, done).await;
    }
    // As in `before_the_third`: the third turn's third step misses the room it must leave for the
    // output by about 6,000 tokens, while the step's request with the ask leaves the summary room.
    let continuing = used(&runtime, session, 1).await + 400 + 20_034;
    let usable = continuing + 26_000;
    let window = (usable * 100).div_ceil(95).max(usable + 4_096);
    resize(&runtime, model, window, 64_000).await;
    let third = start(&runtime, session, model, "AURA-PARTS-THIRD: answer at length").await;
    waiting::wait(&runtime, &third, done).await;

    let requests = provider.requests.lock().expect("provider requests").clone();
    let tools = |request: &Value| request["tools"].as_array().map_or(0, Vec::len);
    let asked = requests.iter().filter(|request| asks_compaction(request)).collect::<Vec<_>>();
    let shapes = asked.iter().map(|r| (tools(r), r.to_string().len())).collect::<Vec<_>>();
    assert_eq!(asked.len(), 4, "twice on the step's request, then two parts: {shapes:?}");
    assert!(tools(asked[0]) > 0, "first on the step's request, its tools included: {shapes:?}");
    assert_eq!(tools(asked[1]), tools(asked[0]), "then with the call answered: {shapes:?}");
    assert!(asked[2..].iter().all(|r| tools(r) == 0), "never on it again: {shapes:?}");
    let carries = |request: &Value, label: &str| request.to_string().contains(label);
    assert!(
        carries(asked[2], "AURA-PARTS-FIRST") && !carries(asked[2], "AURA-PARTS-SECOND"),
        "the older message first"
    );
    assert!(carries(asked[3], "AURA-PARTS-SECOND"), "then the newer one");
    assert_eq!(compactions(&runtime, session), 3, "the failed one and the two parts");
    assert_eq!(completed(&runtime, session), 2, "each part with the model's summary");
    let after = requests
        .iter()
        .rfind(|request| !asks_compaction(request))
        .expect("the step's request after the compaction")
        .to_string();
    assert!(after.contains("The prior response is summarized here"), "the model's summary");
    assert!(!after.contains("without a new summary"), "not Core's");
    let written =
        host.root.path().join("waiting-workspace").join(provider_fixture::COMPACTION_TOOL_FILE);
    assert!(!written.exists(), "the write the compaction answer asked for never ran");
}
