use super::support::*;
use aura_backend_windows_user::compat::CompatJobs;
use aura_windows_user_contract::ManagedShell;
use std::{sync::atomic::AtomicBool, time::Duration};

#[test]
fn root_identity_is_retained_and_cancel_confirms_the_owned_job_is_empty() {
    let root = tempfile::tempdir().expect("owned compat workspace");
    let backend = backend();
    let mut execution = backend
        .launch(&command(ManagedShell::PowerShell, root.path(), "Start-Sleep -Seconds 30"), None)
        .expect("launch waiting root");
    let root_identity = execution.root_identity().clone();
    let observed = execution.observe().expect("query owned Job");
    assert!(observed.members.iter().any(|member| member == &root_identity));
    let result = execution.run_direct(&AtomicBool::new(true)).expect("cancelled terminal");
    assert_eq!(result.active_processes, 0);
    assert!(execution.stop(Duration::from_secs(2)).confirmed);
}

#[test]
fn root_exit_retains_a_real_grandchild_and_stop_ends_its_heartbeat_for_both_shells() {
    for shell in [ManagedShell::PowerShell, ManagedShell::Cmd] {
        let root = tempfile::tempdir().expect("owned tree workspace");
        let backend = backend();
        let mut execution = backend
            .launch(&command(shell, root.path(), &fixture_script(shell, "tree", root.path())), None)
            .expect("launch real tree");
        wait_file(&root.path().join("tree.ready"));
        let observed = execution.observe().expect("observe root child and grandchild");
        assert!(observed.active_processes >= 3);
        assert!(observed.members.len() >= 3);
        let identities = observed.members;
        std::fs::write(root.path().join("release"), b"release").expect("allow direct root exit");
        let direct = execution
            .run_direct(&AtomicBool::new(false))
            .expect("direct result with retained background");
        assert_eq!(direct.exit_code, Some(0));
        assert!(direct.active_processes > 0);
        let stop = execution.stop(Duration::from_secs(3));
        assert!(stop.confirmed, "{stop:?}");
        assert_eq!(stop.active_processes, Some(0));
        let heartbeat =
            std::fs::read(root.path().join("heartbeat")).expect("heartbeat before stop");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            std::fs::read(root.path().join("heartbeat")).expect("heartbeat after stop"),
            heartbeat
        );
        for identity in identities {
            if let Ok(retained) = aura_windows_platform::retain_process(identity.process_id)
                && retained.identity() == &identity
            {
                assert!(!retained.is_running().expect("identity stop fact"));
            }
        }
    }
}

#[test]
fn explicit_breakaway_is_refused_while_detached_and_new_group_children_remain_owned() {
    for mode in ["breakaway", "detached"] {
        let root = tempfile::tempdir().expect("owned flag workspace");
        let backend = backend();
        let mut execution = backend
            .launch(
                &command(
                    ManagedShell::Cmd,
                    root.path(),
                    &fixture_script(ManagedShell::Cmd, mode, root.path()),
                ),
                None,
            )
            .expect("launch flag fixture");
        wait_file(&root.path().join("flags.json"));
        let facts: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.path().join("flags.json")).expect("flag facts"),
        )
        .expect("parse flag facts");
        if mode == "breakaway" {
            assert_eq!(facts["breakaway_refused"], true);
        } else {
            assert!(execution.observe().expect("detached membership").active_processes >= 3);
        }
        assert!(execution.stop(Duration::from_secs(3)).confirmed);
    }
}

#[test]
fn a_child_can_create_a_nested_job_without_escaping_the_parent_job() {
    let root = tempfile::tempdir().expect("nested job workspace");
    let backend = backend();
    let mut execution = backend
        .launch(
            &command(
                ManagedShell::Cmd,
                root.path(),
                &fixture_script(ManagedShell::Cmd, "nested", root.path()),
            ),
            None,
        )
        .expect("launch nested Job fixture");
    wait_file(&root.path().join("nested.ready"));
    assert!(execution.observe().expect("outer includes nested child").members.len() >= 3);
    assert!(execution.stop(Duration::from_secs(3)).confirmed);
}

#[test]
fn background_registry_keeps_one_owned_row_until_confirmed_stop() {
    let root = tempfile::tempdir().expect("owned registry workspace");
    let backend = backend();
    let mut execution = backend
        .launch(
            &command(
                ManagedShell::Cmd,
                root.path(),
                &fixture_script(ManagedShell::Cmd, "tree", root.path()),
            ),
            None,
        )
        .expect("registered tree");
    wait_file(&root.path().join("tree.ready"));
    std::fs::write(root.path().join("release"), b"release").expect("release root");
    assert!(
        execution.run_direct(&AtomicBool::new(false)).expect("direct root result").active_processes
            > 0
    );
    let mut jobs = CompatJobs::default();
    let id = jobs.register(execution).expect("transfer Job owner to background registry");
    assert_eq!(jobs.len(), 1);
    assert!(jobs.observe(&id).expect("live registry row").active_processes > 0);
    assert!(jobs.observe("another-owner-job").is_err());
    assert!(jobs.stop(&id, Duration::from_secs(3)).expect("owned bounded stop").confirmed);
    assert!(jobs.is_empty());
}
