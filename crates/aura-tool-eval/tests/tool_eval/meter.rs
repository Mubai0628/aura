use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream, read_usage},
    replay::{Replay, ReplayStep},
};
use serde_json::json;
use std::sync::Arc;

fn limits() -> Limits {
    Limits { cumulative: 50_000_000, task: 20_000, request_input: 10_000, request_output: 512 }
}
#[tokio::test]
async fn every_auxiliary_send_is_metered_and_budget_refusal_never_reaches_upstream() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let ledger = Arc::new(Ledger::open(&root.path().join("ledger.db"), limits()).expect("ledger"));
    let replay = Arc::new(Replay::new(vec![ReplayStep::Final { text: "replay".into() }; 5]));
    let meter = Meter::start(MeterConfig {
        api_format: aura_tool_eval::owner::ApiFormat::OpenAiChatCompletions,
        task: "probe-retry-compaction".into(),
        output_cap: 512,
        ledger: Arc::clone(&ledger),
        upstream: Upstream::Replay(Arc::clone(&replay)),
        purpose: MeterPurpose::Coding,
    })
    .await
    .expect("local meter");
    let client = reqwest::Client::builder().no_proxy().build().expect("client");
    for phase in ["probe", "retry", "compaction", "auxiliary"] {
        let response=client.post(format!("{}/chat/completions",meter.url)).json(&json!({"messages":[{"role":"user","content":phase}],"max_tokens":256,"stream":true})).send().await.expect("local request");
        assert_eq!(response.status(), 200);
    }
    assert_eq!(replay.calls(), 4);
    assert_eq!(ledger.records().expect("usage records").len(), 4);
    assert_eq!(ledger.charged().expect("actual tokens including cache"), 4400);
    let response = client
        .post(format!("{}/chat/completions", meter.url))
        .json(&json!({"max_tokens":513}))
        .send()
        .await
        .expect("refused request");
    assert_eq!(response.status(), 429);
    assert_eq!(replay.calls(), 4, "no upstream send after cap refusal");
    assert_eq!(
        ledger
            .task_stop_reason("probe-retry-compaction")
            .expect("task stop classification")
            .as_deref(),
        Some("budget_exhausted"),
        "budget termination is not a task failure even with all earlier requests settled"
    );
    assert_eq!(ledger.stop_reason().expect("other tasks remain available"), None);
    let serialized =
        serde_json::to_string(&ledger.records().expect("metadata only")).expect("records JSON");
    assert!(!serialized.contains("messages"));
    assert!(!serialized.contains("content"));
}
#[tokio::test]
async fn corrupt_ledger_stops_before_any_remote_or_replay_request() {
    let root = tempfile::tempdir().expect("isolated ledger");
    let path = root.path().join("ledger.db");
    let ledger = Arc::new(Ledger::open(&path, limits()).expect("ledger"));
    rusqlite::Connection::open(&path)
        .expect("fault injection connection")
        .execute("DROP TABLE requests", [])
        .expect("inject write failure");
    let replay = Arc::new(Replay::new(vec![ReplayStep::Final { text: "never sent".into() }]));
    let meter = Meter::start(MeterConfig {
        api_format: aura_tool_eval::owner::ApiFormat::OpenAiChatCompletions,
        task: "fault".into(),
        output_cap: 512,
        ledger,
        upstream: Upstream::Replay(Arc::clone(&replay)),
        purpose: MeterPurpose::Coding,
    })
    .await
    .expect("meter");
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client")
        .post(format!("{}/chat/completions", meter.url))
        .json(&json!({"max_tokens":256}))
        .send()
        .await
        .expect("refused");
    assert_eq!(response.status(), 429);
    assert_eq!(replay.calls(), 0);
}
#[test]
fn usage_missing_or_conflicting_is_never_zero_cost_and_cache_is_separate() {
    assert!(read_usage(b"data: [DONE]\n\n").is_err());
    let first = "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"prompt_cache_hit_tokens\":7}}\n\n";
    assert_eq!(
        read_usage(format!("{first}{first}data: [DONE]\n").as_bytes())
            .expect("identical repeated usage")
            .cached_input,
        7
    );
    assert!(
        read_usage(
            format!(
                "{first}data: {{\"usage\":{{\"prompt_tokens\":11,\"completion_tokens\":2}}}}\n"
            )
            .as_bytes()
        )
        .is_err()
    );
}

#[test]
fn s09_conflicting_total_usage_is_refused_and_optional_telemetry_is_preserved() {
    assert!(
        read_usage(br#"{"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":99}}"#)
            .is_err(),
        "reported totals cannot conceal contradictory accounting"
    );
    let usage = read_usage(br#"{"usage":{"prompt_tokens":10,"completion_tokens":4,"total_tokens":14,"prompt_cache_hit_tokens":6,"completion_tokens_details":{"reasoning_tokens":3}}}"#).expect("consistent reported usage");
    let metadata = serde_json::to_value(usage).expect("metadata only");
    assert_eq!(metadata.get("reasoning_tokens"), Some(&json!(3)));
}
