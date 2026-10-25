use super::*;
use aura_domain_common::*;
use aura_permission_domain::NetworkByteLimits;
use aura_security_contract::InvocationIdentity;
fn plan() -> NetworkRequestPlan {
    NetworkRequestPlan {
        request_id: NetworkRequestId::new(),
        identity: InvocationIdentity {
            principal_id: PrincipalId::new(),
            actor: InvocationActor::ParentAgent,
            workspace_id: Some(WorkspaceId::new()),
            session_id: Some(SessionId::new()),
            turn_task_id: Some(TurnTaskId::new()),
            agent_run_id: Some(AgentRunId::new()),
            parent_agent_run_id: None,
            correlation_id: CorrelationId::new(),
            permission_snapshot_id: Some(PermissionSnapshotId::new()),
        },
        caller: NetworkCallerKind::WebRuntime,
        method: "GET".into(),
        requested_url: "https://example.org/?q=code%20error".into(),
        destination: None,
        request_data: vec![],
        data_classes: vec![NetworkDataClass::UserFileContent, NetworkDataClass::WorkspaceSource],
        redirect_policy: RedirectPolicy::Deny,
        byte_limits: NetworkByteLimits {
            max_upload_bytes: 0,
            max_download_bytes: 1024,
            max_decompressed_bytes: 1024,
        },
        timeout_ms: 1000,
        credential_refs: vec![],
        network_grant_ids: vec![],
    }
}
fn issue(
    plan: &NetworkRequestPlan,
    mode: PermissionMode,
) -> Result<CoreRequestGrant<ControlledWebRequestGrant>, NetworkControlError> {
    ControlledWebRequestGrant::issue(
        plan,
        mode,
        2,
        UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::minutes(1)),
    )
}
#[test]
fn ordinary_source_text_needs_no_stored_network_grant_but_readonly_still_denied() {
    let plan = plan();
    assert!(issue(&plan, PermissionMode::ReadOnly).is_err());
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
        let grant = issue(&plan, mode).expect("core grant");
        assert_eq!(grant.data_classes, plan.data_classes);
        grant
            .purpose
            .validate(&GatewayRequest { plan: plan.clone(), headers: vec![], body: vec![] })
            .expect("unchanged");
    }
}
#[test]
fn channel_substitution_endpoint_change_and_upload_inputs_are_rejected() {
    let original = plan();
    let grant = issue(&original, PermissionMode::WorkspaceReadWrite).expect("grant");
    for mutate in [
        |p: &mut NetworkRequestPlan| p.caller = NetworkCallerKind::ModelTransport,
        |p: &mut NetworkRequestPlan| p.identity.actor = InvocationActor::User,
        |p: &mut NetworkRequestPlan| p.requested_url = "https://other.example.org/".into(),
        |p: &mut NetworkRequestPlan| p.method = "CONNECT".into(),
        |p: &mut NetworkRequestPlan| p.data_classes.push(NetworkDataClass::CredentialMaterial),
        |p: &mut NetworkRequestPlan| p.identity.session_id = Some(SessionId::new()),
    ] {
        let mut plan = original.clone();
        mutate(&mut plan);
        assert!(
            grant
                .purpose
                .validate(&GatewayRequest { plan, headers: vec![], body: vec![] })
                .is_err()
        );
    }
    assert!(
        grant
            .purpose
            .validate(&GatewayRequest {
                plan: original.clone(),
                headers: vec![("x-file".into(), "body".into())],
                body: vec![]
            })
            .is_err()
    );
    assert!(
        grant
            .purpose
            .validate(&GatewayRequest { plan: original, headers: vec![], body: b"file".to_vec() })
            .is_err()
    );
}
#[tokio::test]
async fn local_forwarders_and_metadata_are_denied_without_sending_http() {
    for url in
        ["http://127.0.0.1/", "http://[::1]/", "http://192.168.0.1/", "http://169.254.169.254/"]
    {
        let mut plan = plan();
        plan.requested_url = url.into();
        assert!(matches!(
            authorize(plan, std::time::Duration::from_secs(1)).await,
            Err(NetworkControlError::DestinationBlocked)
        ));
    }
}
