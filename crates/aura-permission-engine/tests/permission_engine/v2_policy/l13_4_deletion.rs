//! A deletion that skips the Recycle Bin is asked every time and never
//! reused — a Shell command that deletes in every mode but 完全访问, `fs.delete` for good in every mode.
use super::*;

fn shell(mode: PermissionMode, deletes: bool) -> Case {
    let mut c = Case::new(true);
    c.snapshot.mode = mode;
    c.op.content.action_kind = "shell.powershell".into();
    c.op.content.network = OperationNetwork::None;
    c.op.content.capabilities = vec![ResolvedCapability::ProcessExecute];
    c.op.content.risks =
        if deletes { vec![OperationRiskFlag::PermanentFileDeletion] } else { vec![] };
    c.backend.class = BackendClass::Native;
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

fn delete(mode: PermissionMode, permanent: bool) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.action_kind = "fs.delete".into();
    c.op.content.action = serde_json::json!({"path":"old.txt","permanent":permanent});
    c.op.content.risks =
        if permanent { vec![OperationRiskFlag::PermanentFileDeletion] } else { vec![] };
    c.op.content.network = OperationNetwork::None;
    c.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileDelete];
    c.op.content.resources = vec![ResourceDescriptor::WorkspacePath {
        workspace_id: c.snapshot.workspace_id,
        canonical_relative_path: aura_domain_common::WorkspacePath::parse("old.txt").expect("path"),
        file_target: None,
    }];
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees = [
        EnforcementGuarantee::AuthorizedReadsOnly,
        EnforcementGuarantee::AuthorizedWritesOnly,
        EnforcementGuarantee::BoundedOutput,
        EnforcementGuarantee::CredentialIsolation,
    ]
    .into();
    c.refresh();
    c
}

fn once_only(c: &Case, what: &str) {
    match authorize(&c.input(), &c.backend, None) {
        PolicyAuthorization::Ask { proposal } => assert_eq!(
            proposal.allowed_decisions,
            [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
            "{what}"
        ),
        other => panic!("{what} must ask once: {other:?}"),
    }
}

#[test]
fn a_shell_deletion_asks_once_outside_full_access() {
    for mode in [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite] {
        once_only(&shell(mode, true), &format!("a deleting Shell in {mode:?}"));
    }
    // Without a deletion read-only mode asks and offers this session's approval; workspace
    // mode runs it.
    let ordinary = shell(PermissionMode::ReadOnly, false);
    let PolicyAuthorization::Ask { proposal } =
        authorize(&ordinary.input(), &ordinary.backend, None)
    else {
        panic!("an ordinary Shell asks in read-only mode");
    };
    assert!(proposal.allowed_decisions.len() > 2, "{proposal:?}");
    let ordinary = shell(PermissionMode::WorkspaceReadWrite, false);
    assert!(matches!(
        authorize(&ordinary.input(), &ordinary.backend, None),
        PolicyAuthorization::Allow { .. }
    ));
}

#[test]
fn a_shell_deletion_runs_unasked_in_full_access_as_its_user_was_told() {
    let c = shell(PermissionMode::FullAccess, true);
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }));
}

#[test]
fn deleting_for_good_asks_once_in_every_mode_and_the_recycle_bin_follows_the_mode() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        once_only(&delete(mode, true), &format!("fs.delete permanent in {mode:?}"));
    }
    let recycle = delete(PermissionMode::WorkspaceReadWrite, false);
    assert!(matches!(
        authorize(&recycle.input(), &recycle.backend, None),
        PolicyAuthorization::Allow { .. }
    ));
}
