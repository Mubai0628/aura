//! Workspace prompt commands: every save or removal is the user's answer, in every mode.
use super::*;

fn command_write(mode: PermissionMode, action: &str) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.action_kind = action.into();
    c.op.content.action = serde_json::json!({"name":"review"});
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    c.op.content.capabilities = if action == "commands.delete" {
        vec![ResolvedCapability::WorkspaceFileDelete]
    } else {
        vec![ResolvedCapability::WorkspaceFileWrite, ResolvedCapability::WorkspaceFileCreate]
    };
    c.op.content.resources = vec![ResourceDescriptor::WorkspacePath {
        workspace_id: c.snapshot.workspace_id,
        canonical_relative_path: aura_domain_common::WorkspacePath::parse(".aura/commands/review")
            .expect("path"),
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

#[test]
fn every_mode_asks_once_for_a_command_save_or_removal() {
    for action in ["commands.save", "commands.delete"] {
        for mode in [
            PermissionMode::ReadOnly,
            PermissionMode::WorkspaceReadWrite,
            PermissionMode::FullAccess,
        ] {
            let c = command_write(mode, action);
            match authorize(&c.input(), &c.backend, None) {
                PolicyAuthorization::Ask { proposal } => assert_eq!(
                    proposal.allowed_decisions,
                    [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
                    "{action} in {mode:?}"
                ),
                other => panic!("{action} in {mode:?} must ask: {other:?}"),
            }
        }
    }
}

#[test]
fn a_package_cannot_borrow_the_command_rule_by_name() {
    let mut c = command_write(PermissionMode::FullAccess, "commands.save");
    c.op.content.source = SecuritySourceRef::UserInterface;
    c.refresh();
    assert!(
        !matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. }),
        "only the built-in tool carries the always-ask rule"
    );
}
