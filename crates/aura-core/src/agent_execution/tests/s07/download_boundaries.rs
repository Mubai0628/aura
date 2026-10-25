use super::*;

#[tokio::test]
async fn s07_n7_download_to_a_system_parent_asks_in_every_mode_without_any_effect() {
    let server = Server::start().await;
    let system = std::path::PathBuf::from(
        std::env::var_os("SystemRoot")
            .expect("Windows supplies the protected directory for metadata inspection"),
    );
    let target = system.join(format!("aura-s07-no-dispatch-{}.bin", CorrelationId::new()));
    assert!(!target.exists());
    // Inspect existing directory metadata only. Never approve or write a host target.
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let f = fixture(mode, &["http.download"]).await;
        let batch = f.admit(&[("http.download", json!({"url":server.url,"path":target}))]);
        let id = batch.batch.calls[0].tool_call_id;
        assert!(
            batch.calls[&id]
                .operation
                .content
                .resources
                .iter()
                .any(|resource| matches!(resource, ResourceDescriptor::SystemPath { .. }))
        );
        assert!(
            matches!(f.execute_rejecting(batch).await, Some(BatchOutcome::AwaitingApproval { .. })),
            "protected download asks under {mode:?}"
        );
        no_attempt(&f, id);
        assert_eq!(server.count(), 0);
        assert!(!target.exists());
    }
}

#[tokio::test]
async fn s07_n7_download_cannot_replace_the_live_core_database() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::FullAccess, &["http.download"]).await;
    let target = f
        .service()
        .broker
        .lock()
        .expect("isolated Core database")
        .database
        .connection()
        .path()
        .expect("test database path")
        .to_owned();
    let batch =
        f.admit(&[("http.download", json!({"url":server.url,"path":target,"overwrite":true}))]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute_rejecting(batch).await;
    no_attempt(&f, id);
    assert_eq!(server.count(), 0);
    let store = f.service().broker.lock().expect("control database remains readable");
    assert_eq!(
        store
            .database
            .connection()
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .expect("control database was not replaced"),
        "ok"
    );
}

#[tokio::test]
async fn s07_n7_unapproved_external_download_asks_before_connecting() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::WorkspaceReadWrite, &["http.download"]).await;
    let target = f._host.root.path().join("external-download.bin");
    let batch = f.admit(&[("http.download", json!({"url":server.url,"path":target}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    no_attempt(&f, id);
    assert_eq!(server.count(), 0);
    assert!(!target.exists());
}

#[tokio::test]
async fn s07_n7_replacing_an_approved_download_target_never_publishes_over_the_new_object() {
    let server = Server::start().await;
    let f = Fixture::with_exposure_setup(PermissionMode::ReadOnly, &["http.download"], |root| {
        std::fs::write(root.join("target.txt"), "original").expect("original target");
    })
    .await;
    let batch = f.admit(&[(
        "http.download",
        json!({"url":server.url,"path":"target.txt","overwrite":true}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let target = f.root.join("target.txt");
    std::fs::rename(&target, f.root.join("old-object.txt"))
        .expect("replace the directory entry independently");
    std::fs::write(&target, "replacement").expect("unapproved new object");
    f.execute_rejecting(batch).await;
    no_attempt(&f, id);
    assert_eq!(server.count(), 0);
    assert_eq!(std::fs::read_to_string(target).expect("replacement stays intact"), "replacement");
}
