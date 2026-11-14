use super::*;

fn shell(mode: PermissionMode, network: bool) -> Case {
    let mut c = Case::new(true);
    c.snapshot.mode = mode;
    c.op.content.action_kind = "shell.powershell".into();
    c.op.content.network =
        if network { OperationNetwork::BroadProcess } else { OperationNetwork::None };
    c.op.content.capabilities = vec![ResolvedCapability::ProcessExecute];
    if network {
        c.op.content.capabilities.push(ResolvedCapability::OrdinaryNetwork);
    }
    c.backend.class = BackendClass::Native;
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

#[test]
fn s08_readonly_shell_asks_for_running_without_claiming_os_readonly() {
    let c = shell(PermissionMode::ReadOnly, false);
    let assessment = assess(&c.input()).expect("ordinary approved Shell is eligible");
    assert!(!assessment.requirement.guarantees.contains(&EnforcementGuarantee::ReadOnlyUserFiles));
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. }));
}

#[test]
fn s08_readonly_shell_and_terminal_ask_to_run_with_one_session_answer_for_both_shells() {
    for action in ["shell.powershell", "shell.cmd", "terminal.open", "terminal.write"] {
        let mut c = shell(PermissionMode::ReadOnly, false);
        c.op.content.action_kind = action.into();
        c.refresh();
        let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
            panic!("{action} asks before running in read-only mode");
        };
        assert_eq!(
            proposal.requested_capabilities,
            Some(vec![ResolvedCapability::ProcessExecute]),
            "{action}"
        );
        assert!(
            proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession),
            "{action}"
        );
        let consent = proposal.native_session_consent.expect("a session answer is offered");
        assert!(consent.shells.contains(&NativeShell::PowerShell), "{action}");
        assert!(consent.shells.contains(&NativeShell::Cmd), "{action}");
    }
}

#[test]
fn s08_readwrite_shell_and_terminal_run_unasked_network_included() {
    for action in ["shell.powershell", "shell.cmd", "terminal.open", "terminal.write"] {
        for network in [false, true] {
            let mut c = shell(PermissionMode::WorkspaceReadWrite, network);
            c.op.content.action_kind = action.into();
            c.refresh();
            assert!(
                matches!(
                    authorize(&c.input(), &c.backend, None),
                    PolicyAuthorization::Allow { .. }
                ),
                "{action} with network {network} runs unasked in workspace mode"
            );
        }
    }
}

#[test]
fn s08_readonly_shell_network_asks_for_running_and_network() {
    let c = shell(PermissionMode::ReadOnly, true);
    assess(&c.input()).expect("network requires approval rather than refusal");
    let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
        panic!("network approval");
    };
    assert_eq!(
        proposal.requested_capabilities,
        Some(vec![ResolvedCapability::ProcessExecute, ResolvedCapability::OrdinaryNetwork])
    );
}

/// A download asks below 完全访问 and offers a session answer that covers the session's later
/// downloads; in workspace mode only the download is asked about.
#[test]
fn s08_a_download_asks_with_a_session_answer_below_full_access() {
    for (mode, requested) in [
        (PermissionMode::WorkspaceReadWrite, vec![ResolvedCapability::DownloadArtifact]),
        (
            PermissionMode::ReadOnly,
            vec![
                ResolvedCapability::ProcessExecute,
                ResolvedCapability::OrdinaryNetwork,
                ResolvedCapability::DownloadArtifact,
            ],
        ),
    ] {
        // A download uses the network, whatever the script declared.
        let mut c = shell(mode, true);
        c.op.content.capabilities.push(ResolvedCapability::DownloadArtifact);
        c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
        c.refresh();
        let outcome = authorize(&c.input(), &c.backend, None);
        let PolicyAuthorization::Ask { proposal } = outcome else {
            panic!("a download asks in {mode:?}: {outcome:?}");
        };
        assert_eq!(proposal.requested_capabilities, Some(requested), "{mode:?}");
        assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession));
        assert!(proposal.native_session_consent.is_some(), "{mode:?}");
    }
    let mut c = shell(PermissionMode::FullAccess, true);
    c.op.content.capabilities.push(ResolvedCapability::DownloadArtifact);
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.refresh();
    c.allow();
}

/// Writing outside the workspace asks every time below 完全访问, and is never reused.
#[test]
fn s08_a_write_outside_the_workspace_asks_once_below_full_access() {
    for mode in [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite] {
        let mut c = shell(mode, false);
        c.op.content.risks = vec![OperationRiskFlag::OutsideWorkspaceWrite];
        c.refresh();
        let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
            panic!("a write outside asks in {mode:?}");
        };
        assert_eq!(
            proposal.allowed_decisions,
            [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
            "{mode:?}"
        );
    }
    let mut c = shell(PermissionMode::FullAccess, false);
    c.op.content.risks = vec![OperationRiskFlag::OutsideWorkspaceWrite];
    c.refresh();
    c.allow();
}

#[test]
fn s08_full_access_shell_runs_without_asking() {
    for network in [false, true] {
        shell(PermissionMode::FullAccess, network).allow();
    }
}

#[test]
fn s08_native_dispatch_accepts_a_valid_shell_composite_and_refuses_other_actions() {
    let mut c = shell(PermissionMode::FullAccess, false);
    let authority = c.allow();
    dispatch_guard::revalidate(
        c.request(&authority),
        &authority,
        &c.op,
        &c.input(),
        &c.backend,
        &cancel(),
    )
    .expect("Shell mode slice reaches the dispatch contract");
    c.op.content.action_kind = "ordinary.process".into();
    c.refresh();
    let mut forged = authority;
    forged.scope.operation_fingerprint = c.op.fingerprint.clone();
    assert!(
        AuthorizedDispatch::revalidate(
            c.request(&forged),
            &forged,
            &c.backend,
            &cancel(),
            c.snapshot.created_at
        )
        .is_err(),
        "an unrelated Native action cannot borrow the Shell composite exception"
    );
}
