use std::collections::BTreeSet;

use aura_domain_common::ToolExecutionAttemptId;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{NetworkGrantId, NetworkRequestId, SecurityOperationId, UtcTimestamp};
use aura_network_control::{
    GatewayRequest, ToolHttpGatewayRequestV1, ToolHttpGrantContractErrorV1,
    ToolHttpRequestAuthorityV1,
};
use aura_permission_domain::{
    NetworkAccessRequirement, NetworkByteLimits, NetworkDataReference, NetworkRequestPlan,
    PermissionDecision, PermissionGrant, RedirectPolicy, SecuritySource, ToolCredentialPlacementV1,
    ToolCredentialUseV1, TurnDenyRecord, W7ToolOperationFingerprintPreimageV1,
};
use aura_permission_engine::{DefaultPermissionEvaluator, PermissionEvaluation};
use aura_persistence::ExactToolExecutionAuthorityV1;
use aura_security_contract::TrustState;
use aura_security_contract::{
    InvocationIdentity, NetworkCallerKind, PermissionAllowAuthorizationBasisV1, PermissionSnapshot,
    ResolvedNetworkDestination,
};
use aura_tool_contract::{
    MAX_HTTP_REQUEST_BODY_BYTES, MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES,
    MAX_HTTP_RESPONSE_WIRE_BYTES, NormalizedToolCallInputStateV1,
};
use aura_tool_runtime::{
    PreparedDeclarativeHttpRequestV1, PreparedHttpCredentialSlotV1, PreparedHttpRequestErrorV1,
};
use aura_tool_scheduler::{
    AdmittedToolBatchV1, PlannedToolBatchV1, PlannedToolRuntimeCallV1,
    security_source_for_admitted_tool_call_v1,
};
use thiserror::Error;
use url::Url;

use crate::p07::{
    ToolSandboxAssemblyErrorV1, source_security_fingerprint, validate_common_authority_chain,
    validate_trust_binding,
};

/// Core-owned HTTP facts after P1a/R1 preparation and authoritative DNS
/// resolution, but before P04 evaluation. No `tea_` exists at this boundary.
pub(crate) struct ToolHttpAssemblyInputV1<'a> {
    pub admitted_batch: &'a AdmittedToolBatchV1,
    pub planned_batch: &'a PlannedToolBatchV1,
    pub planned_call: &'a PlannedToolRuntimeCallV1,
    pub authority: &'a ExactToolExecutionAuthorityV1,
    pub operation_id: SecurityOperationId,
    pub operation_identity: InvocationIdentity,
    pub prepared_request: PreparedDeclarativeHttpRequestV1,
    pub resolved_destination: ResolvedNetworkDestination,
    pub network_grant_ids: Vec<NetworkGrantId>,
    pub permission_snapshot: PermissionSnapshot,
    pub trust_state: &'a TrustState,
    pub permission_grants: &'a [PermissionGrant],
    pub turn_denies: &'a [TurnDenyRecord],
    pub network_control_available: bool,
    pub evaluated_at: UtcTimestamp,
}

pub(crate) struct ToolHttpAssemblyEvaluationV1 {
    pub decision: PermissionDecision,
    pub authorized_operation: Option<AuthorizedToolHttpOperationV1>,
}

/// Opaque output of one exact P04 Allow. The `tea_` is deliberately absent
/// until the final pre-dispatch transaction prepares the concrete attempt.
pub(crate) struct AuthorizedToolHttpOperationV1 {
    operation_id: SecurityOperationId,
    operation: W7ToolOperationFingerprintPreimageV1,
    operation_identity: InvocationIdentity,
    permission_snapshot: PermissionSnapshot,
    authorization_basis: PermissionAllowAuthorizationBasisV1,
    gateway_request: GatewayRequest,
}

pub(crate) struct MaterializedToolHttpDispatchV1 {
    pub gateway_request: ToolHttpGatewayRequestV1,
    pub dispatch_evidence_hash: CanonicalHashV1,
    pub success_status_ranges: Vec<(u16, u16)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ToolHttpAssemblyServiceV1;

impl ToolHttpAssemblyServiceV1 {
    pub fn evaluate(
        self,
        input: ToolHttpAssemblyInputV1<'_>,
    ) -> Result<ToolHttpAssemblyEvaluationV1, ToolHttpAssemblyErrorV1> {
        input.prepared_request.validate_integrity()?;
        let plan_call = input
            .planned_batch
            .execution_plan
            .calls
            .iter()
            .find(|call| call.tool_call_id == input.planned_call.validated_call.call.tool_call_id)
            .ok_or(ToolHttpAssemblyErrorV1::AuthorityMismatch)?;
        validate_common_authority_chain(
            input.admitted_batch,
            input.planned_batch,
            input.planned_call,
            input.authority,
            &input.operation_identity,
            &input.permission_snapshot,
            plan_call.timeout_ms,
            false,
        )
        .map_err(|_| ToolHttpAssemblyErrorV1::AuthorityMismatch)?;
        if !input
            .planned_call
            .permission_scopes
            .contains(input.prepared_request.network_origin_scope())
        {
            return Err(ToolHttpAssemblyErrorV1::AuthorityMismatch);
        }

        let source = security_source_for_admitted_tool_call_v1(
            &input.planned_call.validated_call,
            &input.authority.registry,
        )
        .map_err(|_| ToolHttpAssemblyErrorV1::AuthorityMismatch)?;
        let SecuritySource::Tool { tool } = &source else {
            return Err(ToolHttpAssemblyErrorV1::AuthorityMismatch);
        };
        let source_security_fingerprint = source_security_fingerprint(input.authority, tool)
            .map_err(|_| ToolHttpAssemblyErrorV1::AuthorityMismatch)?;
        validate_trust_binding(tool.source_kind, &source_security_fingerprint, input.trust_state)
            .map_err(ToolHttpAssemblyErrorV1::SandboxAssembly)?;

        let credential = exact_http_credential(input.authority, &input.prepared_request)?;
        let mut operation_identity = input.operation_identity;
        operation_identity.permission_snapshot_id = Some(input.permission_snapshot.id);
        let gateway_request = build_gateway_request(
            &input.prepared_request,
            operation_identity.clone(),
            input.resolved_destination,
            input.network_grant_ids,
            plan_call.timeout_ms,
        )?;
        validate_gateway_request(&input.prepared_request, &gateway_request, credential.as_ref())?;

        let NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash } =
            &input.planned_call.validated_call.call.input
        else {
            return Err(ToolHttpAssemblyErrorV1::InputNotReady);
        };
        let runtime = aura_permission_domain::NonSecretHttpRuntimeFactsV1 {
            runtime_kind: aura_permission_domain::ToolPreparedRuntimeKindV1::DeclarativeHttp,
            method: input.prepared_request.method.clone(),
            exact_url: input.prepared_request.exact_url.clone(),
            canonical_path: input.prepared_request.canonical_path.clone(),
            headers_hash: canonical_sha256(&input.prepared_request.headers)
                .map_err(|_| ToolHttpAssemblyErrorV1::Canonicalization)?,
            body_hash: CanonicalHashV1::sha256(&input.prepared_request.body),
            body_byte_length: u64::try_from(input.prepared_request.body.len()).unwrap_or(u64::MAX),
            credential,
            success_status_ranges: input.prepared_request.success_status_ranges.clone(),
            prepared_request_hash: input.prepared_request.request_hash.clone(),
            grant_manifest_hash: input.prepared_request.grant_manifest_hash.clone(),
            timeout_ms: plan_call.timeout_ms,
            response_wire_limit: u64::try_from(MAX_HTTP_RESPONSE_WIRE_BYTES).unwrap_or(u64::MAX),
            response_decompressed_limit: u64::try_from(MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES)
                .unwrap_or(u64::MAX),
        };
        let operation = W7ToolOperationFingerprintPreimageV1::new(
            input.admitted_batch.batch.invocation_identity.clone(),
            input.admitted_batch.batch.tool_batch_id,
            input.planned_call.validated_call.call.tool_call_id,
            (**tool).clone(),
            source_security_fingerprint.clone(),
            input.planned_batch.execution_plan.execution_plan_hash.clone(),
            input.authority.definition.capabilities.clone(),
            input.planned_call.permission_scopes.clone(),
            input.planned_call.risk_flags.clone(),
            purpose.clone(),
            arguments.clone(),
            arguments_hash.clone(),
            aura_permission_domain::NonSecretToolRuntimeFactsV1::NetworkGateway {
                facts: Box::new(runtime),
            },
            None,
            NetworkAccessRequirement::Required,
        )?;
        let security_plan = aura_permission_domain::OperationSecurityPlan {
            operation_id: input.operation_id,
            identity: operation_identity.clone(),
            source,
            capabilities: operation.capabilities.clone(),
            scopes: operation.scopes.clone(),
            risk_flags: operation.risks.clone(),
            purpose: Some(operation.purpose.as_str().to_owned()),
            source_fingerprint: Some(source_security_fingerprint),
            requested_sandbox: None,
            network_access: NetworkAccessRequirement::Required,
            network_plan: Some(gateway_request.plan.clone()),
            operation_fingerprint: operation.fingerprint()?.as_content_hash(),
        };
        let evaluated =
            DefaultPermissionEvaluator.evaluate_with_authorization_basis(&PermissionEvaluation {
                plan: &security_plan,
                snapshot: input.permission_snapshot.clone(),
                source_enabled: input.authority.instance.enabled,
                trust_state: input.trust_state,
                grants: input.permission_grants,
                turn_denies: input.turn_denies,
                available_sandbox_modes: &[],
                network_control_available: input.network_control_available,
                allow_unrestricted_inline_scripts_in_full_access: false,
                now: input.evaluated_at,
            });
        let permission_snapshot = match &evaluated.decision {
            PermissionDecision::Allow { snapshot } => snapshot.clone(),
            PermissionDecision::Ask { .. } | PermissionDecision::Deny { .. } => {
                return Ok(ToolHttpAssemblyEvaluationV1 {
                    decision: evaluated.decision,
                    authorized_operation: None,
                });
            }
        };
        let authorization_basis = evaluated
            .allow_authorization_basis
            .ok_or(ToolHttpAssemblyErrorV1::AuthorizationBasisMissing)?;
        Ok(ToolHttpAssemblyEvaluationV1 {
            decision: evaluated.decision,
            authorized_operation: Some(AuthorizedToolHttpOperationV1 {
                operation_id: input.operation_id,
                operation,
                operation_identity,
                permission_snapshot,
                authorization_basis,
                gateway_request,
            }),
        })
    }
}

impl AuthorizedToolHttpOperationV1 {
    #[must_use]
    pub(crate) fn operation_scopes(&self) -> &[aura_permission_domain::ResolvedScope] {
        &self.operation.scopes
    }

    pub fn materialize_dispatch(
        self,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        prepared_at: UtcTimestamp,
    ) -> Result<MaterializedToolHttpDispatchV1, ToolHttpAssemblyErrorV1> {
        let aura_permission_domain::NonSecretToolRuntimeFactsV1::NetworkGateway { facts } =
            &self.operation.runtime
        else {
            return Err(ToolHttpAssemblyErrorV1::GatewayRequestMismatch);
        };
        let success_status_ranges = facts.success_status_ranges.clone();
        let authority = ToolHttpRequestAuthorityV1::new(
            self.operation_id,
            self.operation,
            self.operation_identity,
            self.permission_snapshot,
            self.authorization_basis,
            tool_execution_attempt_id,
            prepared_at,
        )?;
        let grant = authority.issue(&self.gateway_request)?;
        let dispatch_evidence_hash = grant.purpose.dispatch_evidence_hash()?;
        Ok(MaterializedToolHttpDispatchV1 {
            gateway_request: ToolHttpGatewayRequestV1 {
                authority,
                grant,
                request: self.gateway_request,
            },
            dispatch_evidence_hash,
            success_status_ranges,
        })
    }
}

fn build_gateway_request(
    prepared: &PreparedDeclarativeHttpRequestV1,
    identity: InvocationIdentity,
    destination: ResolvedNetworkDestination,
    mut network_grant_ids: Vec<NetworkGrantId>,
    timeout_ms: u64,
) -> Result<GatewayRequest, ToolHttpAssemblyErrorV1> {
    network_grant_ids.sort();
    if network_grant_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ToolHttpAssemblyErrorV1::GatewayRequestMismatch);
    }
    let mut data_classes = prepared
        .argument_evidence
        .iter()
        .map(|item| item.data_class)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    data_classes.sort();
    let mut request_data = prepared
        .headers
        .iter()
        .map(|(name, value)| NetworkDataReference::Header {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    if !prepared.body.is_empty() {
        request_data.push(NetworkDataReference::JsonBody {
            value: serde_json::from_slice(&prepared.body)
                .map_err(|_| ToolHttpAssemblyErrorV1::GatewayRequestMismatch)?,
        });
    }
    Ok(GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity,
            caller: NetworkCallerKind::AuraTool,
            method: prepared.method.clone(),
            requested_url: prepared.exact_url.clone(),
            destination: Some(destination),
            request_data,
            data_classes,
            redirect_policy: RedirectPolicy::SameOrigin { max_hops: 3 },
            byte_limits: NetworkByteLimits {
                max_upload_bytes: u64::try_from(MAX_HTTP_REQUEST_BODY_BYTES).unwrap_or(u64::MAX),
                max_download_bytes: u64::try_from(MAX_HTTP_RESPONSE_WIRE_BYTES).unwrap_or(u64::MAX),
                max_decompressed_bytes: u64::try_from(MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES)
                    .unwrap_or(u64::MAX),
            },
            timeout_ms,
            credential_refs: Vec::new(),
            network_grant_ids,
        },
        headers: prepared.headers.clone(),
        body: prepared.body.clone(),
    })
}

fn validate_gateway_request(
    prepared: &PreparedDeclarativeHttpRequestV1,
    request: &GatewayRequest,
    credential: Option<&ToolCredentialUseV1>,
) -> Result<(), ToolHttpAssemblyErrorV1> {
    let parsed = Url::parse(&request.plan.requested_url)
        .map_err(|_| ToolHttpAssemblyErrorV1::GatewayRequestMismatch)?;
    let destination =
        request.plan.destination.as_ref().ok_or(ToolHttpAssemblyErrorV1::GatewayRequestMismatch)?;
    if request.plan.caller != NetworkCallerKind::AuraTool
        || request.plan.identity.permission_snapshot_id.is_none()
        || request.plan.method != prepared.method
        || request.plan.requested_url != prepared.exact_url
        || request.headers != prepared.headers
        || request.body != prepared.body
        || !request.plan.credential_refs.is_empty()
        || request.plan.redirect_policy != (RedirectPolicy::SameOrigin { max_hops: 3 })
        || request.plan.byte_limits.max_upload_bytes
            != u64::try_from(MAX_HTTP_REQUEST_BODY_BYTES).unwrap_or(u64::MAX)
        || request.plan.byte_limits.max_download_bytes
            != u64::try_from(MAX_HTTP_RESPONSE_WIRE_BYTES).unwrap_or(u64::MAX)
        || request.plan.byte_limits.max_decompressed_bytes
            != u64::try_from(MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES).unwrap_or(u64::MAX)
        || parsed.scheme() != destination.scheme
        || parsed.host_str().is_none_or(|host| !host.eq_ignore_ascii_case(&destination.host))
        || parsed.port_or_known_default() != Some(destination.port)
        || destination.addresses.is_empty()
        || destination.classes.is_empty()
        || credential_slot_matches(prepared, credential).is_err()
    {
        return Err(ToolHttpAssemblyErrorV1::GatewayRequestMismatch);
    }
    Ok(())
}

fn exact_http_credential(
    authority: &ExactToolExecutionAuthorityV1,
    prepared: &PreparedDeclarativeHttpRequestV1,
) -> Result<Option<ToolCredentialUseV1>, ToolHttpAssemblyErrorV1> {
    let credential = match (&prepared.credential_slot, &authority.frozen.credential_binding) {
        (None, None) => None,
        (Some(slot), Some(binding)) => {
            let (slot_key, placement) = match slot {
                PreparedHttpCredentialSlotV1::BearerAuthorization { slot_key } => {
                    (slot_key.clone(), ToolCredentialPlacementV1::BearerAuthorization)
                }
                PreparedHttpCredentialSlotV1::FixedHeader { slot_key, header_name } => (
                    slot_key.clone(),
                    ToolCredentialPlacementV1::FixedHeader {
                        header_name: aura_tool_contract::HttpHeaderNameV1::new(header_name.clone())
                            .map_err(|_| ToolHttpAssemblyErrorV1::CredentialMismatch)?,
                    },
                ),
            };
            Some(ToolCredentialUseV1 {
                credential_id: binding.credential_id,
                credential_revision: binding.credential_revision,
                slot_key,
                placement,
            })
        }
        _ => return Err(ToolHttpAssemblyErrorV1::CredentialMismatch),
    };
    credential_slot_matches(prepared, credential.as_ref())?;
    Ok(credential)
}

fn credential_slot_matches(
    prepared: &PreparedDeclarativeHttpRequestV1,
    credential: Option<&ToolCredentialUseV1>,
) -> Result<(), ToolHttpAssemblyErrorV1> {
    match (&prepared.credential_slot, credential) {
        (None, None) => Ok(()),
        (
            Some(PreparedHttpCredentialSlotV1::BearerAuthorization { slot_key }),
            Some(ToolCredentialUseV1 {
                slot_key: actual,
                placement: ToolCredentialPlacementV1::BearerAuthorization,
                ..
            }),
        ) if slot_key == actual => Ok(()),
        (
            Some(PreparedHttpCredentialSlotV1::FixedHeader { slot_key, header_name }),
            Some(ToolCredentialUseV1 {
                slot_key: actual,
                placement: ToolCredentialPlacementV1::FixedHeader { header_name: actual_header },
                ..
            }),
        ) if slot_key == actual && header_name.eq_ignore_ascii_case(actual_header.as_str()) => {
            Ok(())
        }
        _ => Err(ToolHttpAssemblyErrorV1::CredentialMismatch),
    }
}

#[derive(Debug, Error)]
pub(crate) enum ToolHttpAssemblyErrorV1 {
    #[error("the admitted call, execution plan, Registry authority, or P04 identity differs")]
    AuthorityMismatch,
    #[error("the normalized Tool input is not ready for execution")]
    InputNotReady,
    #[error("the prepared HTTP request differs from the exact Gateway request")]
    GatewayRequestMismatch,
    #[error("the exact Tool credential binding does not match the HTTP slot")]
    CredentialMismatch,
    #[error("P04 returned Allow without an exact authorization basis")]
    AuthorizationBasisMissing,
    #[error("the HTTP operation evidence cannot be canonicalized")]
    Canonicalization,
    #[error(transparent)]
    Prepared(#[from] PreparedHttpRequestErrorV1),
    #[error(transparent)]
    Authorization(#[from] aura_permission_domain::ToolRuntimeAuthorizationErrorV1),
    #[error(transparent)]
    HttpGrant(#[from] ToolHttpGrantContractErrorV1),
    #[error(transparent)]
    SandboxAssembly(#[from] ToolSandboxAssemblyErrorV1),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aura_domain_common::{
        AgentRunId, CorrelationId, PermissionSnapshotId, PrincipalId, SessionId, TurnTaskId,
        WorkspaceId,
    };
    use aura_security_contract::{
        InvocationActor, NetworkDestinationClass, ResolvedNetworkDestination,
    };
    use aura_tool_contract::{
        DeclarativeHttpBodyV1, DeclarativeHttpMethodV1, DeclarativeHttpTargetV1, DeclarativeHttpV1,
        FixedHttpsOriginV1, HttpBasePathV1, HttpStatusRangeV1,
    };
    use aura_tool_runtime::{OrdinaryToolIoMeterV1, prepare_declarative_http_request_v1};
    use serde_json::json;

    use super::*;

    fn prepared() -> PreparedDeclarativeHttpRequestV1 {
        let request = DeclarativeHttpV1 {
            target: DeclarativeHttpTargetV1::FixedHttps {
                origin: FixedHttpsOriginV1::new("https://example.com").expect("origin"),
                base_path: HttpBasePathV1::new("/v1/items").expect("path"),
            },
            method: DeclarativeHttpMethodV1::Get,
            path: Vec::new(),
            query: Vec::new(),
            headers: Vec::new(),
            body: DeclarativeHttpBodyV1::None,
            credential_slot: None,
            success_status_ranges: vec![HttpStatusRangeV1 { start: 200, end: 299 }],
        };
        prepare_declarative_http_request_v1(
            &request,
            &json!({}),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &mut OrdinaryToolIoMeterV1::default(),
        )
        .expect("prepared request")
    }

    fn identity() -> InvocationIdentity {
        InvocationIdentity {
            principal_id: PrincipalId::new(),
            actor: InvocationActor::ParentAgent,
            workspace_id: Some(WorkspaceId::new()),
            session_id: Some(SessionId::new()),
            turn_task_id: Some(TurnTaskId::new()),
            agent_run_id: Some(AgentRunId::new()),
            parent_agent_run_id: None,
            permission_snapshot_id: Some(PermissionSnapshotId::new()),
            correlation_id: CorrelationId::new(),
        }
    }

    #[test]
    fn gateway_request_is_derived_from_exact_prepared_facts() {
        let prepared = prepared();
        let request = build_gateway_request(
            &prepared,
            identity(),
            ResolvedNetworkDestination {
                scheme: "https".to_owned(),
                host: "example.com".to_owned(),
                port: 443,
                addresses: vec!["93.184.216.34".to_owned()],
                classes: vec![NetworkDestinationClass::Public],
            },
            Vec::new(),
            120_000,
        )
        .expect("gateway request");
        validate_gateway_request(&prepared, &request, None).expect("exact request");

        let mut changed = request;
        changed.plan.method = "DELETE".to_owned();
        assert!(matches!(
            validate_gateway_request(&prepared, &changed, None),
            Err(ToolHttpAssemblyErrorV1::GatewayRequestMismatch)
        ));
    }
}
