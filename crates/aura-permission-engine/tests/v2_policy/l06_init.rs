//! `/init`: one reviewed set of files, approved once in ReadOnly and never reusable.
use super::*;

fn init(mode: PermissionMode) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.action_kind = "workspace_init.complete".into();
    c.op.content.action = serde_json::json!({"language":"english"});
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    c.op.content.capabilities =
        vec![ResolvedCapability::WorkspaceFileWrite, ResolvedCapability::WorkspaceFileCreate];
    c.op.content.resources = ["AGENTS.md", "docs/agent-guides/style.md"]
        .into_iter()
        .map(|path| ResourceDescriptor::WorkspacePath {
            workspace_id: c.snapshot.workspace_id,
            canonical_relative_path: aura_domain_common::WorkspacePath::parse(path).expect("path"),
            file_target: None,
        })
        .collect();
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

#[test]
fn readonly_asks_once_for_the_whole_set_and_the_writable_modes_do_not_ask() {
    let c = init(PermissionMode::ReadOnly);
    match authorize(&c.input(), &c.backend, None) {
        PolicyAuthorization::Ask { proposal } => assert_eq!(
            proposal.allowed_decisions,
            [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
            "an init answer is never a session or workspace answer"
        ),
        other => panic!("ReadOnly must ask: {other:?}"),
    }
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
        let c = init(mode);
        assert!(
            matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }),
            "{mode:?} writes without a second question"
        );
    }
}

#[test]
fn the_once_only_answer_belongs_to_the_built_in_init_write_alone() {
    let mut write = init(PermissionMode::ReadOnly);
    write.op.content.action_kind = "fs.write".into();
    write.refresh();
    let PolicyAuthorization::Ask { proposal } = authorize(&write.input(), &write.backend, None)
    else {
        panic!("ReadOnly asks for a file write")
    };
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession));
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowWorkspace));
}
