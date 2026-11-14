use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn provider_refusal_retains_budget_and_never_persists_or_returns_its_sensitive_body() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("isolated provider");
    let url = format!("http://{}/v1", listener.local_addr().expect("address"));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let mut request = vec![0; 16384];
            let bytes = socket.read(&mut request).await.expect("upstream request");
            let received = std::str::from_utf8(&request[..bytes]).expect("HTTP fixture");
            assert!(
                !received.contains("Bearer synthetic-owner-header"),
                "downstream headers must not be forwarded"
            );
            let body = br#"{"error":{"message":"synthetic-sensitive-provider-body"}}"#;
            socket.write_all(format!("HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await.expect("fixture status");
            socket.write_all(body).await.expect("fixture refusal");
        }
    });
    let root = tempfile::tempdir().expect("isolated ledger");
    let ledger = Arc::new(
        Ledger::open(
            &root.path().join("ledger.db"),
            Limits { cumulative: 50000, task: 20000, request_input: 8192, request_output: 256 },
        )
        .expect("ledger"),
    );
    let meter = Meter::start(MeterConfig {
        api_format: aura_tool_eval::owner::ApiFormat::OpenAiChatCompletions,
        task: "strict-probe".into(),
        output_cap: 256,
        ledger: Arc::clone(&ledger),
        upstream: Upstream::Local(url),
        purpose: MeterPurpose::Probe,
    })
    .await
    .expect("meter");
    let client = reqwest::Client::builder().no_proxy().build().expect("client");
    for _ in 0..2 {
        let response = client
            .post(format!("{}/chat/completions", meter.url))
            .header("authorization", "Bearer synthetic-owner-header")
            .json(&serde_json::json!({"max_tokens":256,"messages":[]}))
            .send()
            .await
            .expect("meter reply");
        assert_eq!(response.status(), 400);
        assert!(
            !response
                .text()
                .await
                .expect("safe refusal")
                .contains("synthetic-sensitive-provider-body")
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2, "a bounded 4xx probe continues to the next case");
    let records = ledger.records().expect("historical metadata");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].provider_status, Some(400));
    assert_eq!(records[0].state, "charged_refusal");
    assert!(ledger.charged().expect("unknown cost remains reserved") >= 4096);
    assert_eq!(ledger.stop_reason().expect("accounting state"), None);
    assert!(!serde_json::to_string(&records).expect("metadata JSON").contains("synthetic"));
    server.abort();
}
