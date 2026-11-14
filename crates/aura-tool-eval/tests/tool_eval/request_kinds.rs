use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream},
    owner::ApiFormat,
    replay::{Replay, ReplayStep},
};
use serde_json::{Value, json};
use std::sync::Arc;

async fn start(ledger: &Arc<Ledger>, task: &str, purpose: MeterPurpose) -> Meter {
    Meter::start(MeterConfig {
        task: task.into(),
        output_cap: 512,
        ledger: Arc::clone(ledger),
        upstream: Upstream::Replay(Arc::new(Replay::new(vec![
            ReplayStep::Final {
                text: "offline result".into()
            };
            6
        ]))),
        purpose,
        api_format: ApiFormat::OpenAiChatCompletions,
    })
    .await
    .expect("local replay meter")
}

async fn send(meter: &Meter, body: &Value) {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("bounded local client")
        .post(format!("{}/chat/completions", meter.url))
        .json(body)
        .send()
        .await
        .expect("replay response");
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn coding_requests_are_generations_or_adjacent_identical_retries_per_task() {
    let root = tempfile::tempdir().expect("isolated replay ledger");
    let ledger = Arc::new(
        Ledger::open(
            &root.path().join("ledger.db"),
            Limits {
                cumulative: 50_000_000,
                task: 64_000,
                request_input: 10_000,
                request_output: 512,
            },
        )
        .expect("ledger"),
    );
    let a = start(&ledger, "A", MeterPurpose::Coding).await;
    let first = json!({"messages":[{"role":"user","content":"not-persisted-request-marker"}],"max_tokens":256});
    let changed = json!({"messages":[{"role":"user","content":"another turn"}],"max_tokens":256});
    send(&a, &first).await;
    send(&a, &first).await;
    send(&a, &changed).await;
    send(&a, &first).await;
    let b = start(&ledger, "B", MeterPurpose::Coding).await;
    send(&b, &first).await;
    let probe = start(&ledger, "probe", MeterPurpose::Probe).await;
    send(&probe, &first).await;
    send(&probe, &first).await;
    let records = ledger.records().expect("metadata");
    assert_eq!(
        records.iter().map(|record| record.kind.as_str()).collect::<Vec<_>>(),
        ["generation", "retry", "generation", "generation", "generation", "probe", "probe"]
    );
    assert!(
        !serde_json::to_string(&records)
            .expect("safe metadata")
            .contains("not-persisted-request-marker"),
        "request contents never enter the ledger report"
    );
    a.finish().await.expect("A settled");
    b.finish().await.expect("B settled");
    probe.finish().await.expect("probe settled");
}
