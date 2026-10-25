use super::*;

#[tokio::test]
async fn s07_n6_download_format_does_not_add_approval() {
    for path in ["bytes.exe", "bytes.zip", "bytes.txt", "bytes"] {
        let server = Server::start().await;
        let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.download"]).await;
        let batch = f.admit(&[(
            "http.download",
            json!({"url":format!("{}/binary/64",server.url),"path":path}),
        )]);
        let id = batch.batch.calls[0].tool_call_id;
        f.execute(batch).await;
        f.output(id);
        assert_eq!(
            std::fs::read(f.root.join(path)).expect("published downloaded file"),
            vec![b'Z'; 64]
        );
        assert_eq!(server.count(), 1);
    }
}

#[tokio::test]
async fn s07_n7_readonly_download_waits_for_all_capabilities_before_connecting() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["http.download"]).await;
    let batch = f.admit(&[("http.download", json!({"url":server.url,"path":"pending.txt"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    no_attempt(&f, id);
    assert_eq!(server.count(), 0);
    assert!(!f.root.join("pending.txt").exists());
}

#[tokio::test]
async fn s07_n8_large_download_publishes_complete_bytes_and_hash() {
    let bytes = 200 * 1024 * 1024;
    let server = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.download"]).await;
    let batch = f.admit(&[(
        "http.download",
        json!({"url":format!("{}/binary/{bytes}",server.url),"path":"large.bin"}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let output = f.output(id);
    assert_eq!(
        std::fs::metadata(f.root.join("large.bin")).expect("published large target").len(),
        bytes
    );
    assert_eq!(output["bytes_written"], bytes);
    let mut hasher = blake3::Hasher::new();
    for _ in 0..(bytes / 16384) {
        hasher.update(&[b'Z'; 16384]);
    }
    assert!(
        output.to_string().contains(hasher.finalize().to_hex().as_str()),
        "download reports hash of actual bytes"
    );
}
