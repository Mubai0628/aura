//! Ordinary external reads use the same production default/approval boundary as writes.
use super::*;

#[tokio::test]
async fn s06_a2_a3_external_read_asks_in_workspace_mode_and_executes_in_full_access() {
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
        let f = Fixture::with_exposure(mode, &["fs.read"]).await;
        let target = f._host.root.path().join("ordinary-external-read.txt");
        std::fs::write(&target, "s06-external-read-marker")
            .expect("write the fixture content for this permission boundary");
        let batch = f.admit(&[(
            "fs.read",
            json!({
                "path":target, "range":{"range_kind":"full"}, "explicit_encoding":null,
            }),
        )]);
        let id = batch.batch.calls[0].tool_call_id;
        if mode == PermissionMode::FullAccess {
            f.execute(batch).await;
            assert!(f.output(id).to_string().contains("s06-external-read-marker"));
        } else {
            assert!(matches!(f.execute_rejecting(batch).await,
                Some(BatchOutcome::AwaitingApproval { call_id, .. }) if call_id == id));
            let store = f
                .service()
                .broker
                .lock()
                .expect("lock the existing authority store for inspection");
            assert!(
                aura_persistence_v2::tool::execution::call(store.database.connection(), id,)
                    .expect("read the durable tool call at this dispatch boundary")
                    .attempt_id
                    .is_none()
            );
        }
        assert_eq!(
            std::fs::read(target).expect("read the actual file effect after dispatch"),
            b"s06-external-read-marker"
        );
    }
}
