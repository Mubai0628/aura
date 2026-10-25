use super::consent_fixture::approve_file;
use super::*;

fn no_attempt(f: &Fixture, id: ToolCallId) {
    let store =
        f.service().broker.lock().expect("lock the existing authority store for inspection");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("read the durable tool call at this dispatch boundary");
    assert!(call.attempt_id.is_none());
}

#[tokio::test]
async fn s06_a2_fullaccess_external_write_executes_and_workspace_mode_asks() {
    for mode in [PermissionMode::FullAccess, PermissionMode::WorkspaceReadWrite] {
        let f = Fixture::new(mode).await;
        let target = f._host.root.path().join("ordinary-external.txt");
        let batch = f.admit(&[(
            "fs.write",
            json!({"path":target,"text":"ordinary",
            "expected":{"expected_kind":"absent"}}),
        )]);
        let id = batch.batch.calls[0].tool_call_id;
        if mode == PermissionMode::FullAccess {
            f.execute(batch).await;
            f.output(id);
            assert_eq!(
                std::fs::read(&target).expect("read the actual file effect after dispatch"),
                b"ordinary"
            );
        } else {
            assert!(matches!(f.execute_rejecting(batch).await,
                Some(BatchOutcome::AwaitingApproval { call_id, .. }) if call_id == id));
            no_attempt(&f, id);
            assert!(!target.exists());
        }
    }
}

#[tokio::test]
async fn s06_a3_d2_system_directory_asks_in_every_mode_for_both_native_spellings() {
    let selected = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").expect("Windows supplies the actual SystemRoot fixture"),
    );
    let native = aura_windows_platform::inspect_existing_path(&selected, true)
        .expect("verify the native identity of the fixture target");
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let f = Fixture::with_exposure(mode, &["fs.list"]).await;
        let batch = f.admit(&[
            ("fs.list", json!({"path":selected,"limit":1,"cursor":null})),
            ("fs.list", json!({"path":native.final_path,"limit":1,"cursor":null})),
        ]);
        let ids = batch.batch.calls.iter().map(|c| c.tool_call_id).collect::<Vec<_>>();
        let targets = ids
            .iter()
            .map(|id| {
                let call = &batch.calls[id];
                assert!(matches!(
                    call.operation.content.resources[0],
                    ResourceDescriptor::SystemPath { .. }
                ));
                aura_security_contract::file_consent_target_of(&call.operation.content.resources[0])
                    .expect("the classified system resource carries its verified consent target")
            })
            .collect::<Vec<_>>();
        assert_eq!(targets[0].identity_key, targets[1].identity_key);
        assert!(matches!(
            f.execute_rejecting(batch).await,
            Some(BatchOutcome::AwaitingApproval { .. })
        ));
        for id in ids {
            no_attempt(&f, id);
        }
    }
}

#[tokio::test]
async fn s06_d1_protected_child_approval_does_not_authorize_its_parent() {
    let selected = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").expect("Windows supplies the actual SystemRoot fixture"),
    );
    let parent = selected.parent().expect("the verified fixture path has its parent directory");
    let f = Fixture::with_exposure(PermissionMode::ReadOnly, &["fs.list"]).await;
    // Only directory metadata is requested; no system file content is read.
    let batch = f.admit(&[
        ("fs.list", json!({"path":parent,"limit":1,"cursor":null})),
        ("fs.list", json!({"path":selected,"limit":1,"cursor":null})),
    ]);
    let parent_id = batch.batch.calls[0].tool_call_id;
    let protected_id = batch.batch.calls[1].tool_call_id;
    approve_file(&f, &batch.calls[&protected_id]);
    assert!(matches!(f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { call_id, .. }) if call_id == parent_id));
    no_attempt(&f, parent_id);
}
