//! Default file authority and file/network separation through the production broker.
use super::*;
use aura_domain_common::ContentHash;

#[tokio::test]
async fn s06_a2_c1_workspace_mode_creates_overwrites_moves_and_deletes_without_approval() {
    let f =
        Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, &["fs.write"], |root| {
            std::fs::write(root.join("existing.txt"), "old content")
                .expect("write the fixture content for this permission boundary");
        })
        .await;
    let batch = f.admit(&[
        (
            "fs.write",
            json!({"path":"default.txt","text":"created","expected":{"expected_kind":"absent"}}),
        ),
        (
            "fs.write",
            json!({"path":"existing.txt","text":"overwritten",
            "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"old content")}}),
        ),
    ]);
    let ids = batch.batch.calls.iter().map(|c| c.tool_call_id).collect::<Vec<_>>();
    f.execute(batch).await;
    for id in ids {
        f.output(id);
    }
    assert_eq!(
        std::fs::read(f.root.join("default.txt"))
            .expect("read the actual file effect after dispatch"),
        b"created"
    );
    assert_eq!(
        std::fs::read(f.root.join("existing.txt"))
            .expect("read the actual file effect after dispatch"),
        b"overwritten"
    );
    // Main uses the same physical workspace and production permission/broker chain.
    crate::r03_tests::success(&f.runtime, crate::method::FS_MOVE, json!({
        "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,
        "source":{"path":"existing.txt"},"destination":{"path":"renamed.txt"},
        "source_expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"overwritten")},
        "destination_expected":{"expected_kind":"absent"},
    })).await;
    assert!(!f.root.join("existing.txt").exists());
    assert_eq!(
        std::fs::read(f.root.join("renamed.txt"))
            .expect("read the actual file effect after dispatch"),
        b"overwritten"
    );
    crate::r03_tests::success(
        &f.runtime,
        crate::method::FS_DELETE,
        json!({
            "workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id,
            "target":{"path":"renamed.txt"},"recursive":false,
            "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"overwritten")},
        }),
    )
    .await;
    assert!(!f.root.join("renamed.txt").exists());
}

#[tokio::test]
async fn s06_a4_file_session_approval_does_not_authorize_a_networked_shell() {
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.write", "shell.cmd"]).await;
    let batch = f.admit(&[
        (
            "fs.write",
            json!({"path":"approved.txt","text":"approved file effect",
            "expected":{"expected_kind":"absent"}}),
        ),
        (
            "shell.cmd",
            json!({"script":"echo network required","requires_network":true,
            "working_directory":".","stdin":"","timeout_ms":1000,"max_output_bytes":4096}),
        ),
    ]);
    let id = batch.batch.calls[0].tool_call_id;
    let network_id = batch.batch.calls[1].tool_call_id;
    super::consent_fixture::approve_file(&f, &batch.calls[&id]);
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("approved.txt"))
            .expect("read the actual file effect after dispatch"),
        b"approved file effect"
    );
    let store =
        f.service().broker.lock().expect("lock the existing authority store for inspection");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), network_id)
        .expect("read the durable tool call at this dispatch boundary");
    assert!(call.attempt_id.is_none(), "file approval cannot dispatch a networked process");
    assert!(call.result.is_none(), "a file answer does not answer the pending Shell question");
}
