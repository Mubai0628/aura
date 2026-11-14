use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{
        CONNECTION_TIMEOUT_SECONDS, Meter, MeterConfig, MeterPurpose, UPSTREAM_TIMEOUT_SECONDS,
        Upstream, core_timeouts,
    },
    owner::ApiFormat,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[test]
fn core_waits_longer_for_the_meter_head_than_the_meter_waits_upstream() {
    // The meter answers only after the whole upstream body; Core's Connect phase waits for that
    // head, so a shorter Core timeout abandons and retries every long generation.
    let timeouts = core_timeouts();
    let upstream = UPSTREAM_TIMEOUT_SECONDS * 1000;
    for field in ["connect_ms", "first_event_ms", "stream_idle_ms"] {
        assert!(timeouts[field].as_u64().expect(field) > upstream, "{field} outlasts the meter");
    }
    let total = timeouts["total_ms"].as_u64().expect("total");
    assert!(total >= timeouts["connect_ms"].as_u64().expect("connect"));
    assert!(CONNECTION_TIMEOUT_SECONDS * 1000 > total);
}

#[tokio::test]
async fn a_request_core_abandons_is_still_settled_before_the_report() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback provider");
    let url = format!("http://{}/v1", listener.local_addr().expect("address"));
    let body = format!(
        "event: response.completed\ndata: {}\n\n",
        json!({"type":"response.completed","response":{"usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":400},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":12}}}})
    );
    let worker = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request");
        let mut request = [0; 8192];
        assert!(socket.read(&mut request).await.expect("request bytes") > 0);
        // A long generation: the client below stops waiting before it ends.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        socket
            .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
            .await
            .expect("complete response");
    });
    let root = tempfile::tempdir().expect("directory");
    let ledger = Arc::new(
        Ledger::open(
            &root.path().join("ledger.db"),
            Limits {
                cumulative: 50_000_000,
                task: 600_000,
                request_input: 262_144,
                request_output: 8192,
            },
        )
        .expect("ledger"),
    );
    let meter = Meter::start(MeterConfig {
        task: "abandoned".into(),
        output_cap: 8192,
        ledger: Arc::clone(&ledger),
        upstream: Upstream::Local(url),
        purpose: MeterPurpose::Coding,
        api_format: ApiFormat::OpenAiResponses,
    })
    .await
    .expect("meter");
    let address = format!("{}/responses", meter.url);
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(300))
        .build()
        .expect("client");
    let request = json!({"max_output_tokens":8192,"input":"fixture","stream":true});
    let gave_up = client.post(&address).json(&request).send().await;
    assert!(gave_up.is_err_and(|e| e.is_timeout()), "the client stops waiting first");
    meter.finish().await.expect("every received request ends");
    let record = &ledger.records().expect("records")[0];
    assert_eq!(record.state, "settled", "the billed usage is known, not left pending");
    assert_eq!(record.output, Some(20));
    worker.await.expect("provider answered");
    assert!(client.post(&address).json(&request).send().await.is_err(), "no request after finish");
    assert_eq!(ledger.records().expect("records").len(), 1);
}
