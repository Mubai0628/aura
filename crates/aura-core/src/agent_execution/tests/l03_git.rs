//! Only provider output is scripted; permission, resource admission, native Git and durable receipts are real.
use super::*;

#[tokio::test]
async fn all_three_git_read_tools_use_native_broker_in_all_permission_modes() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let f =
            Fixture::with_exposure_setup(mode, &["git.status", "git.diff", "git.log"], |root| {
                let runner = aura_backend_windows_user::git::GitRunner::detect()
                    .expect("isolated default entry")
                    .runner
                    .expect("native Git");
                for args in [
                    vec!["init", "--initial-branch=main"],
                    vec!["add", "tracked"],
                    vec!["commit", "-m", "fixture base"],
                ] {
                    if args[0] == "add" {
                        std::fs::write(root.join("tracked"), b"one\n").expect("tracked file");
                    }
                    let value = runner
                        .run_native(
                            root,
                            &args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                            &std::sync::atomic::AtomicBool::new(false),
                            std::time::Duration::from_secs(5),
                            65536,
                        )
                        .expect("temporary Git setup");
                    assert_eq!(value.exit_code, Some(0), "{value:?}");
                }
                std::fs::write(root.join("tracked"), b"two\n").expect("change");
            })
            .await;
        let batch =
            f.admit(&[("git.status", json!({})), ("git.diff", json!({})), ("git.log", json!({}))]);
        let ids = batch.batch.calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>();
        assert!(batch.calls.values().all(|call| {
            call.operation.content.capabilities.contains(&ResolvedCapability::GitRead)
        }));
        f.execute(batch).await;
        assert_eq!(f.output(ids[0])["branch"], "main");
        assert!(f.output(ids[1])["patch"].as_str().expect("patch").contains("+two"));
        assert_eq!(f.output(ids[2])["commits"][0]["subject"], "fixture base");
        let store = f.service().broker.lock().expect("journal");
        for id in ids {
            let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
                .expect("call");
            assert!(call.attempt_id.is_some(), "durable native attempt");
        }
    }
}
