#![allow(clippy::unwrap_used)]
use super::*;
use aura_agent_domain::*;
use aura_domain_common::{RuntimeInstanceId, SessionId};
use aura_ipc::WireFrame;
use aura_windows_platform::{NativeFileAccess, VerifiedFile, open_verified_existing};
use base64::Engine as _;
use std::{sync::Arc, time::Duration};

fn source() -> (tempfile::TempDir, VerifiedFile) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("selected.webm");
    std::fs::write(&path, b"selected").unwrap();
    let file = open_verified_existing(&path, NativeFileAccess::Read).unwrap();
    (root, file)
}

fn command(timeout: u32) -> MediaHostCommandV1 {
    MediaHostCommandV1 {
        command_id: String::new(),
        runtime_instance_id: RuntimeInstanceId::new(),
        session_id: SessionId::new(),
        file_size: 8,
        media_type: "video/webm".into(),
        timeout_ms: timeout,
        action: MediaHostActionV1::Probe,
    }
}

async fn next(queue: &crate::AsyncPriorityQueue) -> crate::JsonRpcNotification {
    let frame = tokio::time::timeout(Duration::from_secs(1), queue.recv())
        .await
        .expect("media command must reach Main")
        .expect("open queue");
    let WireFrame::Control(bytes) = frame.frame else { panic!("control notification") };
    serde_json::from_slice(&bytes).unwrap()
}

async fn active(
    bridge: &Arc<MediaBridge>,
    queue: &crate::AsyncPriorityQueue,
    command: MediaHostCommandV1,
    file: VerifiedFile,
    stop: tokio::sync::watch::Receiver<bool>,
) -> (MediaHostCommandV1, tokio::task::JoinHandle<anyhow::Result<MediaHostResultV1>>) {
    bridge.set_publisher(queue.clone());
    let owned = Arc::clone(bridge);
    let waiting = tokio::spawn(async move { owned.send(command, file, stop).await });
    let notification = next(queue).await;
    assert_eq!(notification.method, "media.command.v1");
    (serde_json::from_value(notification.params).unwrap(), waiting)
}

fn read(command: &MediaHostCommandV1, offset: u64, length: u32) -> MediaHostReadRequestV1 {
    MediaHostReadRequestV1 {
        command_id: command.command_id.clone(),
        runtime_instance_id: command.runtime_instance_id,
        session_id: command.session_id,
        offset,
        length,
    }
}

fn answer(command: &MediaHostCommandV1) -> MediaHostResultV1 {
    MediaHostResultV1 {
        command_id: command.command_id.clone(),
        runtime_instance_id: command.runtime_instance_id,
        session_id: command.session_id,
        outcome: MediaHostOutcomeV1::Refused { message: "fixture done".into() },
    }
}

#[tokio::test]
async fn media_reads_only_the_held_file_and_checks_every_identity_range_and_result() {
    let (root, file) = source();
    let bridge = Arc::new(MediaBridge::default());
    let queue = crate::AsyncPriorityQueue::new();
    let (_stop, stop) = tokio::sync::watch::channel(false);
    let (sent, waiting) = active(&bridge, &queue, command(5_000), file, stop).await;
    assert!(sent.command_id.starts_with("mcm-"));
    let selected = root.path().join("selected.webm");
    std::fs::rename(&selected, root.path().join("original.webm")).unwrap();
    std::fs::write(&selected, b"replaced").unwrap();
    let bytes = bridge.read(&read(&sent, 1, 3)).unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD.decode(bytes.data_base64).unwrap(),
        b"ele"
    );
    let mut wrong = read(&sent, 0, 1);
    wrong.runtime_instance_id = RuntimeInstanceId::new();
    assert!(bridge.read(&wrong).is_err());
    wrong = read(&sent, 0, 1);
    wrong.session_id = SessionId::new();
    assert!(bridge.read(&wrong).is_err());
    wrong = read(&sent, 0, 1);
    wrong.command_id = "mcm-unknown".into();
    assert!(bridge.read(&wrong).is_err());
    for (offset, length) in [(0, 0), (0, MAX_MEDIA_READ_BYTES + 1), (7, 2), (u64::MAX, 2)] {
        assert!(bridge.read(&read(&sent, offset, length)).is_err(), "{offset}+{length}");
    }
    let mut foreign = answer(&sent);
    foreign.session_id = SessionId::new();
    assert!(!bridge.complete(foreign));
    foreign = answer(&sent);
    foreign.runtime_instance_id = RuntimeInstanceId::new();
    assert!(!bridge.complete(foreign));
    assert!(bridge.read(&read(&sent, 0, 1)).is_ok(), "foreign result must not consume command");
    assert!(bridge.complete(answer(&sent)));
    waiting.await.unwrap().unwrap();
    assert!(bridge.read(&read(&sent, 0, 1)).is_err());
    assert!(!bridge.complete(answer(&sent)), "a completed result cannot be replayed");
}

#[tokio::test]
async fn cancel_timeout_and_disconnect_end_the_command_and_its_read_handle() {
    for reason in ["cancel", "timeout", "disconnect"] {
        let (_root, file) = source();
        let bridge = Arc::new(MediaBridge::default());
        let queue = crate::AsyncPriorityQueue::new();
        let (stopper, stop) = tokio::sync::watch::channel(false);
        let timeout = if reason == "timeout" { 50 } else { 5_000 };
        let (sent, waiting) = active(&bridge, &queue, command(timeout), file, stop).await;
        match reason {
            "cancel" => stopper.send(true).unwrap(),
            "disconnect" => queue.close(),
            _ => (),
        }
        let result = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("Core must release the command promptly")
            .unwrap();
        assert!(result.is_err(), "{reason}");
        assert!(bridge.read(&read(&sent, 0, 1)).is_err(), "{reason}: expired command");
        if reason != "disconnect" {
            let cancel = next(&queue).await;
            assert_eq!(cancel.method, "media.cancel.v1");
            let cancel: MediaHostCancelV1 = serde_json::from_value(cancel.params).unwrap();
            assert_eq!(cancel.command_id, sent.command_id);
            assert_eq!(cancel.session_id, sent.session_id);
            assert_eq!(cancel.runtime_instance_id, sent.runtime_instance_id);
        }
    }
}

#[test]
fn media_host_dtos_reject_raw_paths_and_extra_fields() {
    let value = command(30_000);
    let mut raw = serde_json::to_value(&value).unwrap();
    raw["path"] = serde_json::json!("C:/private/other.mp4");
    assert!(serde_json::from_value::<MediaHostCommandV1>(raw).is_err());
    let mut raw = serde_json::to_value(read(&value, 0, 1)).unwrap();
    raw["path"] = serde_json::json!("C:/private/other.mp4");
    assert!(serde_json::from_value::<MediaHostReadRequestV1>(raw).is_err());
    let mut raw = serde_json::to_value(answer(&value)).unwrap();
    raw["outcome"]["extra"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MediaHostResultV1>(raw).is_err());
}
