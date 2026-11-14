//! Trusted package tools: the program exception asks per package per session in
//! Workspace, every time in ReadOnly, never in FullAccess; nothing else may borrow it.
use super::*;
use aura_security_contract::{
    NetworkConsentCaller, NetworkConsentDestinations, NetworkSessionConsentRef, PackageRuntimeClass,
};

fn package_resource(fingerprint: &str, runtime: PackageRuntimeClass) -> ResourceDescriptor {
    ResourceDescriptor::ToolPackage {
        package_id: "tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().expect("package"),
        package_fingerprint: aura_domain_common::jcs::canonical_sha256(&fingerprint).expect("hash"),
        runtime,
    }
}

fn program(mode: PermissionMode, network: bool) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.source = SecuritySourceRef::Tool { identity_hash: c.op.fingerprint.0.clone() };
    c.op.content.action_kind = "user.org.example.tools.fixture".into();
    c.op.content.action = serde_json::json!({"input":"fixture"});
    c.op.content.capabilities = vec![ResolvedCapability::ProcessExecute];
    c.op.content.network = OperationNetwork::None;
    if network {
        c.op.content.capabilities.push(ResolvedCapability::OrdinaryNetwork);
        c.op.content.network = OperationNetwork::BroadProcess;
    }
    c.op.content.resources = vec![
        ResourceDescriptor::WorkspaceRoot { workspace_id: c.snapshot.workspace_id },
        package_resource("version one", PackageRuntimeClass::Program),
    ];
    c.backend.class = BackendClass::Native;
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

/// The production approval resolution records the package consent for a session answer.
fn session_answer(c: &mut Case) {
    let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
        panic!("a package program asks first")
    };
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession));
    let mut scope = proposal.scope;
    let session_id = c.snapshot.session_id;
    let grant_id = aura_domain_common::PermissionGrantId::new();
    scope.lifetime = AuthorizationLifetime::Session { session_id };
    let Some(ResourceDescriptor::ToolPackage { package_id, package_fingerprint, .. }) =
        c.op.content
            .resources
            .iter()
            .find(|resource| matches!(resource, ResourceDescriptor::ToolPackage { .. }))
            .cloned()
    else {
        panic!("package resource")
    };
    scope.network_consent = Some(NetworkSessionConsentRef {
        version: aura_domain_common::ToolWireVersionV1,
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        grant_id,
        grant_revision: Revision::INITIAL,
        workspace_id: c.snapshot.workspace_id,
        session_id,
        principal_id: c.op.content.identity.principal_id,
        caller: NetworkConsentCaller::PackageTools,
        destinations: NetworkConsentDestinations::Package { package_id, package_fingerprint },
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        revoked_grant_revision: None,
    });
    c.grants.push(AuthorizationGrant {
        grant_id,
        revision: Revision::INITIAL,
        scope,
        issued_at: c.snapshot.created_at,
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    });
}

/// A new call of the same package: different arguments, so a different fingerprint.
fn next_call(c: &mut Case) {
    c.op = SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), {
        let mut content = c.op.content.clone();
        content.action = serde_json::json!({"input":"another call"});
        content
    })
    .expect("new operation");
}

#[test]
fn package_programs_follow_the_owner_matrix() {
    for network in [false, true] {
        let full = program(PermissionMode::FullAccess, network);
        assert!(matches!(
            authorize(&full.input(), &full.backend, None),
            PolicyAuthorization::Allow { .. }
        ));

        let mut workspace = program(PermissionMode::WorkspaceReadWrite, network);
        let assessed = assess(&workspace.input()).expect("structurally eligible");
        assert!(assessed.requirement.allowed_classes.contains(&BackendClass::Native));
        assert!(
            !assessed.requirement.guarantees.contains(&EnforcementGuarantee::ReadOnlyUserFiles)
        );
        session_answer(&mut workspace);
        next_call(&mut workspace);
        assert!(
            matches!(
                authorize(&workspace.input(), &workspace.backend, None),
                PolicyAuthorization::Allow { .. }
            ),
            "the session answer covers later calls of the same package version"
        );

        let read_only = program(PermissionMode::ReadOnly, network);
        let PolicyAuthorization::Ask { proposal } =
            authorize(&read_only.input(), &read_only.backend, None)
        else {
            panic!("ReadOnly asks every time")
        };
        assert_eq!(
            proposal.allowed_decisions,
            vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
            "no session answer is offered in ReadOnly"
        );
    }
}

#[test]
fn a_workspace_session_answer_does_not_reach_read_only_another_version_or_another_package() {
    let mut c = program(PermissionMode::WorkspaceReadWrite, false);
    session_answer(&mut c);
    let grants = c.grants.clone();

    let mut read_only = program(PermissionMode::ReadOnly, false);
    read_only.grants = grants.clone();
    next_call(&mut read_only);
    assert!(matches!(
        authorize(&read_only.input(), &read_only.backend, None),
        PolicyAuthorization::Ask { .. }
    ));

    let mut upgraded = program(PermissionMode::WorkspaceReadWrite, false);
    upgraded.grants = grants.clone();
    upgraded.op.content.resources[1] =
        package_resource("version two", PackageRuntimeClass::Program);
    upgraded.refresh();
    assert!(
        matches!(
            authorize(&upgraded.input(), &upgraded.backend, None),
            PolicyAuthorization::Ask { .. }
        ),
        "a new fingerprint is a new question"
    );

    let mut network = program(PermissionMode::WorkspaceReadWrite, true);
    network.grants = grants;
    assert!(
        matches!(
            authorize(&network.input(), &network.backend, None),
            PolicyAuthorization::Ask { .. }
        ),
        "an answer that did not include the network does not cover it"
    );
}

/// One named change to an otherwise valid package program call.
type Mutation = (&'static str, fn(&mut Case));

#[test]
fn nothing_else_borrows_the_package_exception() {
    // Each mutation leaves the shape of a package program; in Workspace mode none of
    // them may reach the Native backend.
    let mutations: [Mutation; 5] = [
        ("built-in source", |c| {
            c.op.content.source =
                SecuritySourceRef::BuiltInTool { identity_hash: c.op.fingerprint.0.clone() }
        }),
        ("not a package tool name", |c| {
            c.op.content.action_kind = "shell.powershell.extension".into()
        }),
        ("extra capability", |c| {
            c.op.content.capabilities.push(ResolvedCapability::WorkspaceFileWrite);
            c.backend.capabilities.insert(ResolvedCapability::WorkspaceFileWrite);
        }),
        ("no package resource", |c| c.op.content.resources.truncate(1)),
        ("HTTP package resource", |c| {
            c.op.content.resources[1] = package_resource("version one", PackageRuntimeClass::Http)
        }),
    ];
    for (name, mutate) in mutations {
        let mut c = program(PermissionMode::WorkspaceReadWrite, false);
        mutate(&mut c);
        c.refresh();
        assert!(
            matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }),
            "{name}: must be refused, not asked"
        );
    }
}

#[test]
fn package_programs_still_require_the_job_and_output_guarantees() {
    for missing in [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput]
    {
        let mut c = program(PermissionMode::FullAccess, false);
        c.backend.guarantees.remove(&missing);
        assert!(matches!(
            authorize(&c.input(), &c.backend, None),
            PolicyAuthorization::Deny { .. }
        ));
    }
}

#[test]
fn mode_and_session_answers_survive_the_real_dispatch_guard() {
    let full = program(PermissionMode::FullAccess, true);
    let authority = full.allow();
    dispatch_guard::revalidate(
        full.request(&authority),
        &authority,
        &full.op,
        &full.input(),
        &full.backend,
        &cancel(),
    )
    .expect("FullAccess authority reaches the Native backend");
    let mut workspace = program(PermissionMode::WorkspaceReadWrite, false);
    session_answer(&mut workspace);
    next_call(&mut workspace);
    let authority = workspace.allow();
    dispatch_guard::revalidate(
        workspace.request(&authority),
        &authority,
        &workspace.op,
        &workspace.input(),
        &workspace.backend,
        &cancel(),
    )
    .expect("the package session answer reaches the Native backend");
    workspace.grants[0].revoked_at = Some(workspace.snapshot.created_at);
    assert!(
        dispatch_guard::revalidate(
            workspace.request(&authority),
            &authority,
            &workspace.op,
            &workspace.input(),
            &workspace.backend,
            &cancel(),
        )
        .is_err(),
        "a revoked answer stops the dispatch"
    );
}
