use super::*;

#[test]
fn only_core_controlled_web_is_allowed_in_workspace_mode() {
    let mut c = Case::new(false);
    c.op.content.action_kind = "web.search".into();
    c.op.content.network = OperationNetwork::ExactOrigins;
    c.op.content.capabilities = vec![ResolvedCapability::PublicNetworkRead];
    c.op.content.risks = vec![OperationRiskFlag::DataLeavesDevice];
    c.op.content.resources.push(ResourceDescriptor::NetworkOrigin {
        scheme: "https".into(),
        host: "example.test".into(),
        port: 443,
    });
    c.refresh();
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees.insert(EnforcementGuarantee::ExactNetworkOrigins);
    assert_eq!(assess(&c.input()).err(), Some(PolicyDenyReason::ModeDenied));
    let mut input = c.input();
    input.network_caller = NetworkCallerKind::WebRuntime;
    assert!(matches!(authorize(&input, &c.backend, None), PolicyAuthorization::Allow { .. }));
    c.snapshot.mode = PermissionMode::ReadOnly;
    let mut input = c.input();
    input.network_caller = NetworkCallerKind::WebRuntime;
    assert_eq!(assess(&input).err(), Some(PolicyDenyReason::ModeDenied));
    c.snapshot.mode = PermissionMode::WorkspaceReadWrite;
    c.op.content.source = SecuritySourceRef::Tool { identity_hash: c.op.fingerprint.0.clone() };
    c.refresh();
    let mut input = c.input();
    input.network_caller = NetworkCallerKind::WebRuntime;
    assert_eq!(
        assess(&input).err(),
        Some(PolicyDenyReason::ModeDenied),
        "custom tool cannot claim Web"
    );
}
