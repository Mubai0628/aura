//! Policy for authenticated Main workspace commands. Filesystem services still
//! own final-path/handle checks and mutation journals; R05 wraps them as Backend.
use aura_domain_common::{Revision, SecurityOperationId, UtcTimestamp, WorkspacePath};
use aura_execution_contract::{OperationContent, OperationNetwork, SecuredOperation};
use aura_permission_engine::policy::{PolicyInput, assess};
use aura_security_contract::*;

pub(crate) fn authorize(
    method: &str,
    params: &serde_json::Value,
    identity: InvocationIdentity,
    snapshot: PermissionSnapshot,
    capabilities: Vec<ResolvedCapability>,
    paths: Vec<WorkspacePath>,
) -> Result<(), PolicyDenyReason> {
    let operation = SecuredOperation::new(
        SecurityOperationId::new(),
        OperationContent {
            compatibility: Default::default(),
            network: OperationNetwork::None,
            identity,
            source: SecuritySourceRef::UserInterface,
            source_revision_fingerprint: None,
            capabilities,
            resources: paths
                .into_iter()
                .map(|path| ResourceDescriptor::WorkspacePath {
                    workspace_id: snapshot.workspace_id,
                    canonical_relative_path: path,
                    file_target: None,
                })
                .collect(),
            risks: vec![],
            credentials: vec![],
            purpose: None,
            action_kind: method.into(),
            action_revision: Revision::INITIAL,
            action: params.clone(),
        },
    )
    .map_err(|_| PolicyDenyReason::InvalidOperation)?;
    let now = UtcTimestamp::now();
    let input = PolicyInput {
        network_caller: aura_security_contract::NetworkCallerKind::AuraTool,
        operation: &operation,
        snapshot: &snapshot,
        profile: SecurityProfile::for_permission_mode(snapshot.mode),
        source_enabled: true,
        trust: &TrustState::SystemTrusted,
        source_fingerprint: None,
        // This closed route family accepts only workspace paths and carries no
        // credential, external-network or release action. Typed decoding precedes us.
        hard_boundaries_satisfied: true,
        denied_for_turn: false,
        grants: &[],
        verified_external_capabilities: &[],
        verified_external_grants: &[],
        now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::seconds(30)),
        // This closed route family is the signed-in user's own workspace file
        // command. It cannot carry a Native session consent and must never be
        // read as one, so it deliberately names no Core runtime instance.
        core_runtime_instance_id: None,
    };
    let assessment = assess(&input)?;
    if assessment.asks {
        return Err(PolicyDenyReason::ApprovalRequired);
    }
    Ok(())
}
