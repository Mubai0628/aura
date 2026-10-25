use aura_tool_eval::{
    budget::{Ledger, Limits},
    meter::{Meter, MeterConfig, MeterPurpose, Upstream},
    owner::ApiFormat,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[tokio::test]
async fn a_settled_reply_to_a_disconnected_core_has_its_own_task_stop() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback provider");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        // Exceed the socket send buffer: a small reply can appear to succeed after disconnect.
        let body = format!(
            ": {}\n\ndata: {}\n\n",
            "x".repeat(4 * 1024 * 1024),
            json!({"type":"response.completed","response":{"usage":{"input_tokens":1000,"output_tokens":20}}})
        );
        let (accepted, saw_request) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("request");
            let mut request = [0; 8192];
            assert!(socket.read(&mut request).await.expect("request bytes") > 0);
            accepted.send(()).expect("client is waiting");
            tokio::time::sleep(Duration::from_millis(150)).await;
            socket
                .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
                .await
                .expect("upstream is collected despite the Core disconnect");
        });
        let root = tempfile::tempdir().expect("isolated ledger");
        let ledger = Arc::new(
            Ledger::open(
                &root.path().join("ledger.db"),
                Limits { cumulative: 50_000_000, task: 600_000, request_input: 262_144, request_output: 8192 },
            )
            .expect("ledger"),
        );
        let meter = Meter::start(MeterConfig {
            task: "disconnected".into(),
            output_cap: 8192,
            ledger: Arc::clone(&ledger),
            upstream: Upstream::Local(url),
            purpose: MeterPurpose::Coding,
            api_format: ApiFormat::OpenAiResponses,
        }).await.expect("local meter");
        let address = meter.url.strip_prefix("http://").expect("local URL").trim_end_matches("/v1");
        let mut core = TcpStream::connect(address).await.expect("Core socket");
        let request = json!({"input":"fixture","max_output_tokens":8192}).to_string();
        core.write_all(format!("POST /v1/responses HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{request}", request.len()).as_bytes()).await.expect("request");
        saw_request.await.expect("upstream received the billed request");
        drop(core);
        meter.finish().await.expect("settlement and disconnected reply finish");
        worker.await.expect("provider finished");
        assert_eq!(ledger.records().expect("records")[0].state, "settled");
        assert_eq!(ledger.charged().expect("known actual cost"), 1020);
        assert_eq!(ledger.stop_reason().expect("no global halt"), None);
        assert_eq!(ledger.task_stop_reason("disconnected").expect("classified task stop").as_deref(), Some("core_disconnected"));
        ledger.reserve("other-task", 100, 100, "generation").expect("unaffected task may reserve");
    }).await.expect("a disconnected reply test must finish in ten seconds");
}
