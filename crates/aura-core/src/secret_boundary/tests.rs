use super::*;
use aura_core_base::ipc_session::{Admission, IpcSession};

fn request(method: &str) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".into(),
        id: CorrelationId::new(),
        command_invocation_id: Some(CommandInvocationId::new()),
        method: method.into(),
        params: json!({}),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s05_secret_timeout_keeps_the_original_late_result_and_does_not_block_control() {
    let fixture = crate::r03_tests::Fixture::new();
    let _route = crate::CoreLogRoute::to(fixture.root.path().join("core.jsonl"));
    let state = Arc::new(fixture.start().await);
    state.sync.set_budget(Duration::from_millis(80));
    let input = request("tools.credential.create");
    let invocation =
        input.command_invocation_id.expect("obtain the actual bounded secret-read fact");
    let (arrive_tx, arrive_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let job_state = state.clone();
    let job_request = input.clone();
    let job = tokio::spawn(async move {
        run(
            &job_state,
            &job_request,
            move || {
                let _ = arrive_tx.send(());
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("obtain the actual bounded secret-read fact");
                Ok(json!({"late":invocation}))
            },
            Value::clone,
        )
        .await
    });
    arrive_rx.await.expect("obtain the actual bounded secret-read fact");
    // The runtime and ordinary control handler continue while the real carrier is parked.
    let control = tokio::time::timeout(
        Duration::from_secs(1),
        crate::r03_tests::request(
            &state,
            "runtime.command_outcome",
            json!({"command_invocation_id":invocation}),
        ),
    )
    .await
    .expect("obtain the actual bounded secret-read fact");
    assert!(control.get("result").is_some());
    let unknown = job
        .await
        .expect("obtain the actual bounded secret-read fact")
        .expect_err("the isolated negative control must be refused");
    assert_eq!(unknown.code.as_str(), "command.outcome_unknown");
    assert_eq!(unknown.details.get("command_invocation_id"), Some(&json!(invocation)));
    let pending = crate::r03_tests::success(
        &state,
        "runtime.command_outcome",
        json!({"command_invocation_id":invocation}),
    )
    .await;
    assert_eq!(pending["outcome"], "unknown");
    release_tx.send(()).expect("obtain the actual bounded secret-read fact");
    let end = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let value = crate::r03_tests::success(
                &state,
                "runtime.command_outcome",
                json!({"command_invocation_id":invocation}),
            )
            .await;
            if value["outcome"] == "confirmed" {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("obtain the actual bounded secret-read fact");
    assert_eq!(end["command_invocation_id"], json!(invocation));
    assert_eq!(end["response"]["result"]["value"]["late"], json!(invocation));
    assert_eq!(
        crate::r03_tests::success(
            &state,
            "runtime.command_outcome",
            json!({"command_invocation_id":invocation})
        )
        .await,
        end
    );
}

#[test]
fn s05_secret_payload_waits_in_the_real_bounded_lane_while_control_is_ready() {
    let mut session = IpcSession::new();
    let secret = request("tools.credential.create");
    let id = secret.id;
    assert_eq!(
        session.admit_secret(secret, SecretBytes::new(b"temporary-synthetic-value".to_vec())),
        Admission::Accepted
    );
    let admitted = session.take_ready();
    assert_eq!(admitted.len(), 1);
    assert_eq!(
        session.take_secret(id).expect("obtain the actual bounded secret-read fact").expose(),
        b"temporary-synthetic-value"
    );
    assert!(session.take_secret(id).is_none(), "the payload has exactly one owner");
    let control = request("runtime.command_outcome");
    let control_id = control.id;
    assert_eq!(session.admit(control), Admission::Accepted);
    assert!(session.take_ready().iter().any(|ready| ready.request_id() == control_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s05_secret_caller_cancellation_keeps_the_worker_and_the_original_receipt() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = Arc::new(fixture.start().await);
    let input = request("tools.credential.rotate");
    let invocation =
        input.command_invocation_id.expect("obtain the actual bounded secret-read fact");
    let (arrive_tx, arrive_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_state = state.clone();
    let task = tokio::spawn(async move {
        run(
            &worker_state,
            &input,
            move || {
                let _ = arrive_tx.send(());
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("obtain the actual bounded secret-read fact");
                Ok(json!({"original":invocation}))
            },
            Value::clone,
        )
        .await
    });
    arrive_rx.await.expect("obtain the actual bounded secret-read fact");
    task.abort();
    assert!(task.await.expect_err("the isolated negative control must be refused").is_cancelled());
    assert!(state.settlements.is_pending(invocation));
    release_tx.send(()).expect("obtain the actual bounded secret-read fact");
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let outcome = crate::r03_tests::success(
                &state,
                "runtime.command_outcome",
                json!({"command_invocation_id":invocation}),
            )
            .await;
            if outcome["outcome"] == "confirmed" {
                break outcome;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("obtain the actual bounded secret-read fact");
    assert_eq!(result["response"]["result"]["value"]["original"], json!(invocation));
    assert!(!state.settlements.is_pending(invocation));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s05_secret_saturation_refuses_before_input_is_consumed() {
    let fixture = crate::r03_tests::Fixture::new();
    let state = Arc::new(fixture.start().await);
    // Keep the hold strictly longer than admission. The product default is 30s;
    // the old 5s hold released all permits before that default could refuse.
    state.sync.set_budget(Duration::from_millis(80));
    let mut releases = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let (arrive_tx, arrive_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task_state = state.clone();
        let input = request("tools.credential.create");
        tasks.push(tokio::spawn(async move {
            run(
                &task_state,
                &input,
                move || {
                    let _ = arrive_tx.send(());
                    release_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("obtain the actual bounded secret-read fact");
                    Ok(json!({"real":true}))
                },
                Value::clone,
            )
            .await
        }));
        arrive_rx.await.expect("obtain the actual bounded secret-read fact");
        releases.push(release_tx);
    }
    assert_eq!(state.sync.available(), 0, "all four real workers are still held");
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = entered.clone();
    let refused = run(
        &state,
        &request("tools.credential.create"),
        move || {
            observed.store(true, std::sync::atomic::Ordering::Release);
            Ok(json!({}))
        },
        Value::clone,
    )
    .await
    .expect_err("the isolated negative control must be refused");
    assert_eq!(refused.code.as_str(), "credential.busy");
    assert!(!entered.load(std::sync::atomic::Ordering::Acquire));
    for release in releases {
        release.send(()).expect("obtain the actual bounded secret-read fact");
    }
    for task in tasks {
        match task.await.expect("obtain the actual bounded secret-read fact") {
            Ok(result) => assert_eq!(result["real"], true),
            Err(error) => assert_eq!(error.code.as_str(), "command.outcome_unknown"),
        }
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        while state.sync.available() != 4 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("obtain the actual bounded secret-read fact");
}
