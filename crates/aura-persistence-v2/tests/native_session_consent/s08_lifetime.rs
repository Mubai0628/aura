use super::*;

#[test]
fn s08_a_legacy_shell_answer_cannot_be_revived_by_a_new_core_full_access_default() {
    let mut f = Fixture::new(PermissionMode::FullAccess);
    let first = f.operation("shell.powershell", "historical approval");
    let issued = f.approve(&first, ApprovalDecisionKind::AllowSession);
    legacy::legacy(&mut f, issued);
    f.runtime = RuntimeInstanceId::new();
    let second = f.operation("shell.cmd", "a new Core must ask for its own answer");
    let (_, historical) = f.derive(&second).expect("legacy records remain visible as history");
    let backend = f.native_backend();
    assert!(
        f.reserve(&second, &backend, &historical, f.snapshot.session_permission_revision).is_err(),
        "the dispatch guard must validate the original legacy Grant, even when the new mode covers running"
    );
}

#[test]
fn s08_new_shell_consent_survives_a_real_store_reopen_and_core_restart() {
    let mut f = Fixture::new(PermissionMode::FullAccess);
    let first = f.operation("shell.powershell", "first");
    f.approve(&first, ApprovalDecisionKind::AllowSession);
    let directory = f._root.path().join("v2");
    drop(f.store);
    f.store = V2Store::open(&directory).expect("reopen persisted store");
    f.runtime = RuntimeInstanceId::new();
    let second = f.operation("shell.cmd", "second");
    let (_, decision) = f.derive(&second).expect("persisted session consent");
    let backend = f.native_backend();
    assert!(
        f.reserve(&second, &backend, &decision, f.snapshot.session_permission_revision).is_ok(),
        "a new operation may use the answer after Core restart"
    );
}

#[test]
fn s08_new_shell_consent_survives_tightening_and_loosening() {
    let mut f = Fixture::new(PermissionMode::FullAccess);
    let first = f.operation("shell.powershell", "first");
    f.approve(&first, ApprovalDecisionKind::AllowSession);
    for mode in
        [PermissionMode::WorkspaceReadWrite, PermissionMode::ReadOnly, PermissionMode::FullAccess]
    {
        f.snapshot.mode = mode;
        f.snapshot.session_permission_revision += 1;
        let second = f.operation("shell.cmd", "fresh operation");
        let (_, decision) = f.derive(&second).expect("session consent");
        let backend = f.native_backend();
        assert!(
            f.reserve(&second, &backend, &decision, f.snapshot.session_permission_revision).is_ok(),
            "a mode change does not withdraw the user's answer"
        );
    }
}
