//! A compaction asked on the step's request that gave no summary is asked on that request again
//! while it can still give one, so the provider reads it from its cache: an answer that called a
//! tool has the call answered after it - no tool runs - and an answer that broke off is asked for
//! again as it was. No request names a tool choice.
use super::l10_15_compaction::compactions;
use super::l10_19_compaction::{asks_compaction, just_over_the_line};
use super::l12_4_compaction::completed;
use super::*;

fn messages(request: &Value) -> Vec<Value> {
    request["messages"].as_array().expect("messages").clone()
}

/// The step's request after the compaction carries the model's summary, not Core's.
fn summarized_by_the_model(requests: &[Value]) {
    let after = requests
        .iter()
        .rfind(|request| !asks_compaction(request))
        .expect("the step's request after the compaction")
        .to_string();
    assert!(after.contains("The prior response is summarized here"), "the model's summary");
    assert!(!after.contains("without a new summary"), "not Core's");
}

#[tokio::test]
async fn a_compaction_answered_with_a_tool_call_is_asked_again_with_the_call_answered() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactToolOnceOnPrefix).await;
    let (runtime, session) = just_over_the_line(&host, &provider, 64_000).await;
    assert_eq!((compactions(&runtime, session), completed(&runtime, session)), (1, 1));
    let requests = provider.requests.lock().expect("provider requests").clone();
    let asked = requests.iter().filter(|request| asks_compaction(request)).collect::<Vec<_>>();
    assert_eq!(asked.len(), 2, "on the step's request, then with the call answered");
    let (first, again) = (messages(asked[0]), messages(asked[1]));
    assert_eq!(&again[..first.len()], first.as_slice(), "the first request, unchanged");
    let added = &again[first.len()..];
    assert_eq!(added.len(), 2, "the answer and its call's result: {added:?}");
    assert_eq!(added[0]["role"], "assistant");
    assert_eq!(added[0]["tool_calls"][0]["id"], "fixture_compaction_write");
    assert_eq!(added[1]["role"], "tool");
    assert_eq!(added[1]["tool_call_id"], "fixture_compaction_write");
    assert!(
        added[1]["content"].as_str().is_some_and(|text| text.starts_with("No tool runs")),
        "{added:?}"
    );
    assert_eq!(asked[1]["tools"], asked[0]["tools"], "the tools stay for the cache");
    assert!(asked.iter().all(|request| request.get("tool_choice").is_none()));
    let written =
        host.root.path().join("waiting-workspace").join(provider_fixture::COMPACTION_TOOL_FILE);
    assert!(!written.exists(), "the write the compaction answer asked for never ran");
    summarized_by_the_model(&requests);
}

#[tokio::test]
async fn a_compaction_whose_answer_broke_off_is_asked_again_as_it_was() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactInterruptedOnPrefix).await;
    let (runtime, session) = just_over_the_line(&host, &provider, 64_000).await;
    assert_eq!((compactions(&runtime, session), completed(&runtime, session)), (1, 1));
    let requests = provider.requests.lock().expect("provider requests").clone();
    let asked = requests.iter().filter(|request| asks_compaction(request)).collect::<Vec<_>>();
    assert_eq!(asked.len(), 2, "the same request twice");
    assert!(asked[0]["tools"].as_array().is_some_and(|tools| !tools.is_empty()), "the step's");
    assert_eq!(asked[1], asked[0], "asked again as it was");
    summarized_by_the_model(&requests);
}
