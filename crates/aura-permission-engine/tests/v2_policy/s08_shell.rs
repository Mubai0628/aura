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
fn s08_readwrite_shell_network_asks_only_for_network() {
    let c = shell(PermissionMode::WorkspaceReadWrite, true);
    assess(&c.input()).expect("network requires approval rather than refusal");
    let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
        panic!("network approval");
    };
    assert_eq!(proposal.requested_capabilities, Some(vec![ResolvedCapability::OrdinaryNetwork]));
}

#[test]
fn s08_native_dispatch_accepts_a_valid_shell_composite_and_refuses_other_actions() {
    let mut c = shell(PermissionMode::WorkspaceReadWrite, false);
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
