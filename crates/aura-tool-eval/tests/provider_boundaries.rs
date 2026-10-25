use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream},
    probe::evaluate_cases,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

async fn provider(status: u16, body: String) -> (String, Arc<AtomicUsize>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback provider");
    let url = format!("http://{}/v1", listener.local_addr().expect("address"));
    let count = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    let worker = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut received = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let n = socket.read(&mut buffer).await.expect("request bytes");
                assert!(n > 0);
                received.extend_from_slice(&buffer[..n]);
                if let Some(end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&received[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .expect("length")
                        .trim()
                        .parse()
                        .expect("length value");
                    if received.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            observed.fetch_add(1, Ordering::SeqCst);
            // Status 0: the request arrived, then the connection is lost without a response.
            if status == 0 {
                continue;
            }
            let header = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).await.expect("status");
            socket.write_all(body.as_bytes()).await.expect("body");
        }
    });
    (url, count, worker)
}
fn ledger(root: &tempfile::TempDir) -> Arc<Ledger> {
    Arc::new(Ledger::open(&root.path().join("ledger.db"), limits()).expect("ledger"))
}
fn limits() -> Limits {
    Limits { cumulative: 500_000, task: 100_000, request_input: 8192, request_output: 256 }
}
async fn meter(ledger: &Arc<Ledger>, task: &str, url: String, purpose: MeterPurpose) -> Meter {
    Meter::start(MeterConfig {
        api_format: aura_tool_eval::owner::ApiFormat::OpenAiChatCompletions,
        task: task.into(),
        output_cap: 256,
        ledger: Arc::clone(ledger),
        upstream: Upstream::Local(url),
        purpose,
    })
    .await
    .expect("meter")
}
async fn send(meter: &Meter) -> reqwest::StatusCode {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client")
        .post(format!("{}/chat/completions", meter.url))
        .json(&json!({"max_tokens":256,"messages":[]}))
        .send()
        .await
        .expect("safe meter response")
        .status()
}

#[tokio::test]
async fn all_six_probe_cases_continue_after_4xx_and_only_the_report_has_sanitized_errors() {
    let body=json!({"error":{"message":format!("Unsupported schema sk-synthetic-private-key {}","说明".repeat(320)),"code":"invalid_schema"}}).to_string();
    let (url, count, worker) = provider(400, body).await;
    let root = tempfile::tempdir().expect("isolated ledger");
    let ledger = ledger(&root);
    let meter = meter(&ledger, "probe", url, MeterPurpose::Probe).await;
    let cases =
        evaluate_cases("deepseek-synthetic-owner", &meter, &ledger).await.expect("six probes");
    assert_eq!(cases.len(), 6);
    assert_eq!(count.load(Ordering::SeqCst), 6);
    for case in &cases {
        assert_eq!(case.provider_status, Some(400));
        let error = case.provider_error.as_ref().expect("bounded report-only diagnostic");
        let message = error.message.as_ref().expect("provider message");
        assert!(message.chars().count() <= 300);
        assert!(!message.contains("sk-synthetic"));
        assert_eq!(error.code.as_deref(), Some("invalid_schema"));
    }
    let records = ledger.records().expect("metadata");
    assert!(records.iter().all(|r| r.state == "charged_refusal"));
    let total: u64 = records.iter().map(|r| r.reserved).sum();
    assert_eq!(ledger.charged().expect("all refusals charged"), total);
    assert!(!serde_json::to_string(&records).expect("ledger JSON").contains("Unsupported schema"));
    drop(meter);
    drop(ledger);
    let ledger = Ledger::open(&root.path().join("ledger.db"), limits())
        .expect("reopen fully charged refusals");
    assert_eq!(ledger.stop_reason().expect("no global halt"), None);
    ledger
        .reserve("next-task", 10, 10, "generation")
        .expect("probe refusals do not halt another task after reopening");
    let bytes = std::fs::read(root.path().join("ledger.db")).expect("database bytes");
    assert!(!String::from_utf8_lossy(&bytes).contains("sk-synthetic"));
    worker.abort();
}

#[tokio::test]
async fn a_coding_4xx_stops_only_that_task_and_does_not_retry_the_provider() {
    let (url, count, worker) =
        provider(400, json!({"error":{"message":"unsupported","code":"schema"}}).to_string()).await;
    let root = tempfile::tempdir().expect("ledger");
    let ledger = ledger(&root);
    let a = meter(&ledger, "A", url.clone(), MeterPurpose::Coding).await;
    assert_eq!(send(&a).await, 400);
    assert_eq!(send(&a).await, 429);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(
        ledger.task_stop_reason("A").expect("task state").as_deref(),
        Some("provider_refused")
    );
    assert_eq!(ledger.stop_reason().expect("global state"), None);
    let b = meter(&ledger, "B", url, MeterPurpose::Probe).await;
    assert_eq!(send(&b).await, 400);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    worker.abort();
}

/// A 5xx or a lost exchange confirms no usage. The request is charged at its whole
/// reservation, only its own task stops, and a different task still reaches the Provider.
#[tokio::test]
async fn a_server_error_or_lost_exchange_charges_in_full_and_stops_only_that_task() {
    for (status, body) in [
        (500, json!({"error":{"message":"unknown cost"}}).to_string()),
        (503, String::new()),
        (0, String::new()),
    ] {
        let (url, count, worker) = provider(status, body).await;
        let root = tempfile::tempdir().expect("ledger");
        let ledger = ledger(&root);
        let a = meter(&ledger, "A", url.clone(), MeterPurpose::Coding).await;
        assert_eq!(send(&a).await, 429, "status {status}");
        assert_eq!(send(&a).await, 429, "status {status}: the stopped task sends nothing");
        assert_eq!(count.load(Ordering::SeqCst), 1, "status {status}");
        let records = ledger.records().expect("ledger records");
        assert_eq!(records[0].state, "charged_unknown", "status {status}");
        assert_eq!(ledger.charged().expect("charge"), records[0].reserved, "status {status}");
        assert_eq!(
            ledger.task_stop_reason("A").expect("task state").as_deref(),
            Some("provider_unconfirmed")
        );
        assert_eq!(ledger.stop_reason().expect("global state"), None, "status {status}");
        let b = meter(&ledger, "B", url, MeterPurpose::Coding).await;
        send(&b).await;
        assert_eq!(count.load(Ordering::SeqCst), 2, "status {status}: another task continues");
        worker.abort();
    }
}

#[tokio::test]
async fn empty_refusals_and_missing_or_conflicting_usage_halt_all_sends() {
    for (status, body) in [
        (400, String::new()),
        (200, json!({"choices":[]}).to_string()),
        (
            200,
            json!({"usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":9}}).to_string(),
        ),
    ] {
        let (url, count, worker) = provider(status, body).await;
        let root = tempfile::tempdir().expect("ledger");
        let ledger = ledger(&root);
        let a = meter(&ledger, "A", url.clone(), MeterPurpose::Probe).await;
        assert_eq!(send(&a).await, 429);
        let b = meter(&ledger, "B", url, MeterPurpose::Coding).await;
        assert_eq!(send(&b).await, 429);
        assert_eq!(count.load(Ordering::SeqCst), 1, "global halt blocks a different task");
        assert_eq!(
            ledger.stop_reason().expect("global accounting stop").as_deref(),
            Some("usage_unknown")
        );
        assert_eq!(ledger.records().expect("held reservation")[0].state, "unknown");
        worker.abort();
    }
}
