//! The channel to the browser host. A command goes out on Main's connection and its answer
//! comes back by id; no connection and no answer are reported, never waited on for ever; a
//! command Core stops waiting for is withdrawn on the same connection.
use super::*;
use aura_browser_domain::*;
use aura_ipc::{AsyncPriorityQueue, JsonRpcNotification, WireFrame};

fn command(session: SessionId, timeout_ms: u32) -> BrowserHostCommandV1 {
    BrowserHostCommandV1 {
        command_id: String::new(),
        runtime_instance_id: RuntimeInstanceId::new(),
        session_id: session,
        expected_host: None,
        access: BrowserPageAccessV1 { read_only: false, approved_hosts: vec![], file_root: None },
        action: BrowserHostActionV1::Snapshot { max_chars: 12_000 },
        timeout_ms,
    }
}

/// The next notification on Main's connection; one that never comes fails the test.
async fn next_notification(queue: &AsyncPriorityQueue) -> JsonRpcNotification {
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), queue.recv())
        .await
        .expect("a notification within 5 s")
        .expect("a frame");
    let WireFrame::Control(bytes) = frame.frame else { panic!("a control frame") };
    serde_json::from_slice(&bytes).expect("json-rpc")
}

async fn next_command(queue: &AsyncPriorityQueue) -> BrowserHostCommandV1 {
    let notification = next_notification(queue).await;
    assert_eq!(notification.method, "browser.command.v1");
    serde_json::from_value(notification.params).expect("a browser command")
}

async fn next_withdrawal(queue: &AsyncPriorityQueue) -> BrowserHostCancelV1 {
    let notification = next_notification(queue).await;
    assert_eq!(notification.method, "browser.cancel.v1");
    serde_json::from_value(notification.params).expect("a withdrawal")
}

/// A call nobody stops.
fn running() -> tokio::sync::watch::Receiver<bool> {
    tokio::sync::watch::channel(false).1
}

#[tokio::test]
async fn a_command_goes_out_on_mains_connection_and_its_answer_comes_back_by_id() {
    let bridge = Arc::new(BrowserBridge::default());
    let queue = AsyncPriorityQueue::new();
    bridge.set_publisher(queue.clone());
    let session = SessionId::new();
    let host = {
        let bridge = Arc::clone(&bridge);
        tokio::spawn(async move {
            let sent = next_command(&queue).await;
            assert!(sent.command_id.starts_with("bcm-"), "Core names each command");
            assert!(!bridge.complete(BrowserHostResultV1 {
                command_id: "bcm-not-this-one".into(),
                page_url: "about:blank".into(),
                outcome: BrowserHostOutcomeV1::Refused { message: "x".into() },
            }));
            assert!(bridge.complete(BrowserHostResultV1 {
                command_id: sent.command_id,
                page_url: "http://127.0.0.1:9/game".into(),
                outcome: BrowserHostOutcomeV1::Completed {
                    result: serde_json::json!({}),
                    image: None
                },
            }));
        })
    };
    let answer = bridge.send(command(session, 5_000), running()).await.expect("answered");
    host.await.expect("host");
    assert!(matches!(answer.outcome, BrowserHostOutcomeV1::Completed { .. }));
    assert_eq!(bridge.page_url(session).as_deref(), Some("http://127.0.0.1:9/game"));
}

#[tokio::test]
async fn no_connection_and_no_answer_are_reported_not_waited_on() {
    let bridge = BrowserBridge::default();
    assert!(matches!(
        bridge.send(command(SessionId::new(), 5_000), running()).await,
        Err(bridge::BridgeError::NotConnected)
    ));
    let queue = AsyncPriorityQueue::new();
    bridge.set_publisher(queue.clone());
    let started = std::time::Instant::now();
    let silent = bridge.send(command(SessionId::new(), 200), running()).await;
    assert!(matches!(silent, Err(bridge::BridgeError::TimedOut(1))), "{silent:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    // The late answer finds nothing waiting, and the host was told Core gave up on it.
    let late = next_command(&queue).await;
    assert_eq!(next_withdrawal(&queue).await.command_id, late.command_id);
    assert!(!bridge.complete(BrowserHostResultV1 {
        command_id: late.command_id,
        page_url: "about:blank".into(),
        outcome: BrowserHostOutcomeV1::Refused { message: "late".into() },
    }));
}

#[tokio::test]
async fn a_test_that_plays_the_host_keeps_the_channel() {
    let bridge = BrowserBridge::default();
    let mine = AsyncPriorityQueue::new();
    bridge.pin_publisher(mine.clone());
    bridge.set_publisher(AsyncPriorityQueue::new());
    let sending = bridge.send(command(SessionId::new(), 200), running());
    let (sent, _) = tokio::join!(next_command(&mine), sending);
    assert!(sent.command_id.starts_with("bcm-"));
}

#[tokio::test]
async fn a_stopped_call_withdraws_its_command_at_once() {
    let bridge = BrowserBridge::default();
    let queue = AsyncPriorityQueue::new();
    bridge.set_publisher(queue.clone());
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let mut long = command(SessionId::new(), 300_000);
    long.action =
        BrowserHostActionV1::RequestUser { reason: "请登录".into(), timeout_ms: 285_000 };
    let started = std::time::Instant::now();
    let sending = bridge.send(long, stopping);
    let host = async {
        let sent = next_command(&queue).await;
        stop.send_replace(true);
        let withdrawn = next_withdrawal(&queue).await;
        assert_eq!(withdrawn.command_id, sent.command_id);
        assert_eq!(withdrawn.runtime_instance_id, sent.runtime_instance_id);
        sent
    };
    let (answer, sent) = tokio::join!(sending, host);
    assert!(matches!(answer, Err(bridge::BridgeError::Stopped)), "{answer:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    // The host's answer after the withdrawal finds nothing waiting.
    assert!(!bridge.complete(BrowserHostResultV1 {
        command_id: sent.command_id,
        page_url: "about:blank".into(),
        outcome: BrowserHostOutcomeV1::Refused { message: "late".into() },
    }));
    // A call already stopped never waits.
    let (_, stopped) = tokio::sync::watch::channel(true);
    let at_once = bridge.send(command(SessionId::new(), 300_000), stopped).await;
    assert!(matches!(at_once, Err(bridge::BridgeError::Stopped)), "{at_once:?}");
}
