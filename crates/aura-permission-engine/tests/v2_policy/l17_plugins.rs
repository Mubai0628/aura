//! Installing a workspace folder as a plugin in development: inert until the user trusts it,
//! so the modes that may change things allow it without asking, and ReadOnly refuses it.
use super::*;

fn dev_install(mode: PermissionMode) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.profile = SecurityProfile::for_permission_mode(mode);
    c.op.content.action_kind = "plugins.dev_install".into();
    c.op.content.action = serde_json::json!({"path": "plugins/usage-panel"});
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    c.op.content.capabilities = vec![ResolvedCapability::PluginDevelopmentInstall];
    let identity = &c.op.content.identity;
    c.op.content.resources = vec![ResourceDescriptor::AgentTask {
        session_id: identity.session_id.expect("session"),
        turn_task_id: identity.turn_task_id.expect("turn"),
        agent_run_id: identity.agent_run_id.expect("run"),
    }];
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities = [ResolvedCapability::PluginDevelopmentInstall].into();
    c.backend.guarantees =
        [EnforcementGuarantee::AuthorizedWritesOnly, EnforcementGuarantee::CredentialIsolation]
            .into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

#[test]
fn a_plugin_in_development_is_installed_without_asking_except_in_read_only() {
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
        let c = dev_install(mode);
        assert!(
            matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }),
            "{mode:?}: {:?}",
            authorize(&c.input(), &c.backend, None)
        );
    }
    let c = dev_install(PermissionMode::ReadOnly);
    match authorize(&c.input(), &c.backend, None) {
        PolicyAuthorization::Deny { reason } => assert_eq!(reason, PolicyDenyReason::ModeDenied),
        other => panic!("ReadOnly refuses, never asks: {other:?}"),
    }
}
