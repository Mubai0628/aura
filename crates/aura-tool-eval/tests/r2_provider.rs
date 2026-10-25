use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream},
    owner::{ApiFormat, RequestSettings},
    probe::{arguments_valid, cases_for, evaluate_cases_with_settings},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

fn usage() -> String {
    format!(
        "event: response.created\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
        json!({"type":"response.created","response":{"usage":{"input_tokens":999}}}),
        json!({"type":"response.completed","response":{"usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":400},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":12}}}})
    )
}
async fn provider(
    status: u16,
    body: String,
) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("isolated provider");
    let url = format!("http://{}/v1", listener.local_addr().expect("address"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&requests);
    let worker = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut received = Vec::new();
            let mut buffer = [0; 8192];
            let data = loop {
                let n = socket.read(&mut buffer).await.expect("request bytes");
                assert!(n > 0);
                received.extend_from_slice(&buffer[..n]);
                if let Some(end) = received.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&received[..end]);
                    assert!(headers.starts_with("POST /v1/responses "));
                    let length = headers
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::to_owned)
                        })
                        .expect("length")
                        .trim()
                        .parse::<usize>()
                        .expect("numeric length");
                    if received.len() >= end + 4 + length {
                        break received[end + 4..end + 4 + length].to_vec();
                    }
                }
            };
            let request: Value = serde_json::from_slice(&data).expect("request JSON");
            assert!(
                request.get("stream_options").is_none(),
                "Responses must not inherit Chat-only settings"
            );
            observed.lock().expect("observations").push(request);
            socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.expect("headers");
            socket.write_all(body.as_bytes()).await.expect("body");
        }
    });
    (url, requests, worker)
}
fn ledger(root: &tempfile::TempDir) -> Arc<Ledger> {
    Arc::new(
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
    )
}
async fn meter(ledger: &Arc<Ledger>, key: &str, url: String, purpose: MeterPurpose) -> Meter {
    Meter::start(MeterConfig {
        task: key.into(),
        output_cap: 8192,
        ledger: Arc::clone(ledger),
        upstream: Upstream::Local(url),
        purpose,
        api_format: ApiFormat::OpenAiResponses,
    })
    .await
    .expect("meter")
}
async fn send(meter: &Meter, suffix: &str, output: Value) -> reqwest::StatusCode {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("client")
        .post(format!("{}{suffix}", meter.url))
        .json(&output)
        .send()
        .await
        .expect("meter response")
        .status()
}

#[tokio::test]
async fn r2_responses_route_output_budget_and_terminal_usage_are_enforced() {
    let (url, observed, worker) = provider(200, usage()).await;
    let root = tempfile::tempdir().expect("ledger directory");
    let ledger = ledger(&root);
    let a = meter(&ledger, "A", url, MeterPurpose::Coding).await;
    assert_eq!(
        send(&a, "/responses", json!({"max_output_tokens":8192,"input":"fixture","stream":true}))
            .await,
        200
    );
    let record = &ledger.records().expect("record")[0];
    assert_eq!(record.output, Some(20));
    assert_eq!(record.reasoning_tokens, Some(12));
    assert_eq!(record.cached_input, Some(400));
    assert_eq!(record.state, "settled");
    assert_eq!(
        send(
            &a,
            "/chat/completions",
            json!({"max_output_tokens":8192,"input":"fixture","stream":true})
        )
        .await,
        429,
        "a valid Responses body on the Chat route is refused by the route check alone"
    );
    assert_eq!(
        observed.lock().expect("requests").len(),
        1,
        "wrong-format route never reaches upstream"
    );
    assert_eq!(ledger.records().expect("records").len(), 1);
    worker.abort();
}

#[tokio::test]
async fn r2_responses_probe_refusals_continue_and_preserve_sanitized_statuses() {
    let (url, observed, worker) = provider(400, json!({"error":{"message":"Forced choice refused sk-synthetic-private-key","code":"unsupported_choice"}}).to_string()).await;
    let root = tempfile::tempdir().expect("directory");
    let ledger = ledger(&root);
    let a = meter(&ledger, "probe-unique-run", url, MeterPurpose::Probe).await;
    let settings = RequestSettings { api_format: ApiFormat::OpenAiResponses, ..Default::default() };
    let results = evaluate_cases_with_settings("deepseek-flash", &settings, &a, &ledger)
        .await
        .expect("six cases");
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|r| r.provider_status == Some(400) && !r.produced_function_call));
    assert!(results.iter().all(|r| {
        !r.provider_error
            .as_ref()
            .expect("error")
            .message
            .as_ref()
            .expect("message")
            .contains("sk-synthetic")
    }));
    assert_eq!(observed.lock().expect("requests").len(), 6);
    assert!(ledger.records().expect("ledger").iter().all(|r| r.state == "charged_refusal"));
    assert_eq!(ledger.stop_reason().expect("global state"), None);
    worker.abort();
}

/// A refusal (4xx) and a server error (5xx) stop only their own task; a 2xx whose usage is
/// missing or contradicts itself stops every task until it is reconciled.
#[tokio::test]
async fn r2_responses_refusals_and_server_errors_stop_one_task_but_unknown_usage_stops_all() {
    // (status, body, what each task sees, Provider requests, global stop)
    for (status, body, seen, sent, global) in [
        (400, json!({"error":{"message":"refused"}}).to_string(), 400, 2, false),
        (500, json!({"error":{"message":"unknown"}}).to_string(), 429, 2, false),
        (200, "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n".into(), 429, 1, true),
        (200, usage()+&usage().replace("\"output_tokens\":20", "\"output_tokens\":21"), 429, 1, true),
    ] {
        let (url, observed, worker) = provider(status, body).await;
        let root = tempfile::tempdir().expect("directory");
        let ledger = ledger(&root);
        let a = meter(&ledger, "A", url.clone(), MeterPurpose::Coding).await;
        let request = json!({"input":"sample","max_output_tokens":8192,"stream":true});
        assert_eq!(send(&a, "/responses", request.clone()).await, seen, "status {status}");
        let b = meter(&ledger, "B", url, MeterPurpose::Probe).await;
        assert_eq!(send(&b, "/responses", request).await, seen, "status {status}");
        assert_eq!(observed.lock().expect("requests").len(), sent, "status {status}");
        assert_eq!(ledger.stop_reason().expect("global state").is_some(), global, "status {status}");
        worker.abort();
    }
}

#[test]
fn r2_flat_probe_and_function_done_or_delta_parameters_are_supported() {
    let settings = RequestSettings { api_format: ApiFormat::OpenAiResponses, ..Default::default() };
    for case in cases_for("deepseek-flash", &settings) {
        assert_eq!(case.request["max_output_tokens"], 8192);
        assert_eq!(case.request["tools"][0]["strict"], true);
        assert!(case.request.get("tool_choice").is_none(), "thinking mode refuses forced choices");
        assert!(case.request.get("reasoning").is_none() && case.request.get("thinking").is_none());
    }
    let schema = json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false});
    for event in [
        json!({"type":"response.output_item.done","item":{"type":"function_call","id":"f1","name":"schema_probe","arguments":"{\"value\":\"sample\"}"}}),
        json!({"type":"response.function_call_arguments.delta","item_id":"f1","delta":"{\"value\":\"sample\"}"}),
    ] {
        assert!(
            arguments_valid(format!("data: {event}\n\n").as_bytes(), &schema).expect("parameters")
        );
    }
    const {
        assert!(aura_tool_eval::meter::UPSTREAM_TIMEOUT_SECONDS >= 360);
        assert!(
            aura_tool_eval::meter::CORE_FIRST_EVENT_MS
                > aura_tool_eval::meter::UPSTREAM_TIMEOUT_SECONDS * 1000
        );
        assert!(
            aura_tool_eval::meter::CONNECTION_TIMEOUT_SECONDS * 1000
                > aura_tool_eval::meter::CORE_TOTAL_MS
        );
    }
}

/// A body cut off mid-stream confirms no usage: the reservation is charged in full and stays
/// charged, only that task stops, and a different task still reserves its own request.
#[tokio::test]
async fn r2_disconnected_responses_body_charges_in_full_and_stops_only_that_task() {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback provider");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request");
            let mut request = [0; 8192];
            assert!(
                socket.read(&mut request).await.expect("request bytes") > 0,
                "a real request precedes the disconnect"
            );
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\npartial",
                )
                .await
                .expect("truncated body");
        });
        let root = tempfile::tempdir().expect("directory");
        let ledger = ledger(&root);
        let a = meter(&ledger, "A", url.clone(), MeterPurpose::Coding).await;
        assert_eq!(
            send(
                &a,
                "/responses",
                json!({"max_output_tokens":8192,"input":"fixture","stream":true})
            )
            .await,
            429
        );
        worker.await.expect("provider ended without completing the body");
        let record = &ledger.records().expect("charged reservation")[0];
        assert_eq!(record.state, "charged_unknown");
        assert!(record.reserved >= 8192 + 4096);
        assert_eq!(ledger.charged().expect("charge"), record.reserved, "never released");
        assert_eq!(ledger.stop_reason().expect("global stop"), None);
        assert_eq!(
            ledger.task_stop_reason(&record.task).expect("task stop").as_deref(),
            Some("provider_unconfirmed")
        );
        let b = meter(&ledger, "B", url, MeterPurpose::Coding).await;
        assert_eq!(
            send(&b, "/responses", json!({"max_output_tokens":8192,"input":"next task"})).await,
            429,
            "the Provider is gone, so B's own request is unconfirmed too"
        );
        assert_eq!(ledger.records().expect("B reserved").len(), 2, "a global halt would refuse B");
    })
    .await
    .expect("a disconnected upstream body must fail closed within ten seconds");
}

#[tokio::test]
async fn r2_probe_reports_function_output_reasoning_and_truncation_without_persisting_the_body() {
    let function = json!({"type":"response.output_item.done","item":{"type":"function_call","id":"fc","name":"schema_probe","arguments":"{\"value\":\"sample\"}"}});
    let terminal = json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":400},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":12}}}});
    let (url, _, worker) = provider(200, format!("data: {function}\n\ndata: {terminal}\n\n")).await;
    let root = tempfile::tempdir().expect("directory");
    let ledger = ledger(&root);
    let a = meter(&ledger, "probe-r2-truncation", url, MeterPurpose::Probe).await;
    let settings = RequestSettings { api_format: ApiFormat::OpenAiResponses, ..Default::default() };
    let results = evaluate_cases_with_settings("deepseek-flash", &settings, &a, &ledger)
        .await
        .expect("six bounded replies");
    assert!(results.iter().all(|r| r.produced_function_call
        && r.output_tokens == Some(20)
        && r.reasoning_tokens == Some(12)
        && r.incomplete_reason.as_deref() == Some("max_output_tokens")));
    assert!(ledger.records().expect("ledger").iter().all(|r| r.state == "settled"));
    assert!(
        !serde_json::to_string(&ledger.records().expect("ledger"))
            .expect("JSON")
            .contains("max_output_tokens")
    );
    worker.abort();
}

#[tokio::test]
async fn r2_failed_response_keeps_only_a_sanitized_probe_error_and_halts_on_unknown_usage() {
    let failed = json!({"type":"response.failed","response":{"error":{"code":"failed_fixture","message":"Provider failure sk-synthetic-private-key"}}});
    let (url, _, worker) = provider(200, format!("data: {failed}\n\n")).await;
    let root = tempfile::tempdir().expect("directory");
    let ledger = ledger(&root);
    let a = meter(&ledger, "failed-r2-probe", url, MeterPurpose::Probe).await;
    let settings = RequestSettings { api_format: ApiFormat::OpenAiResponses, ..Default::default() };
    let results = evaluate_cases_with_settings("deepseek-flash", &settings, &a, &ledger)
        .await
        .expect("first failure report");
    assert_eq!(results.len(), 1, "global stop prevents remaining probe sends");
    let error =
        results[0].provider_error.as_ref().expect("sanitized SSE error remains diagnosable");
    assert_eq!(error.code.as_deref(), Some("failed_fixture"));
    assert!(!error.message.as_ref().expect("message").contains("sk-synthetic"));
    assert_eq!(ledger.stop_reason().expect("stop").as_deref(), Some("usage_unknown"));
    assert!(
        !serde_json::to_string(&ledger.records().expect("ledger"))
            .expect("JSON")
            .contains("Provider failure")
    );
    worker.abort();
}
