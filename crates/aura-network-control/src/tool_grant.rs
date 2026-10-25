use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    ContentHash, CredentialId, CredentialRevision, PermissionSnapshotId, SecurityOperationId,
    UtcTimestamp,
};
use aura_domain_common::{ToolBatchId, ToolCallId, ToolExecutionAttemptId};
use aura_permission_domain::{
    CoreRequestGrant, NetworkDestinationPolicy, NetworkProtocolClass, NonSecretToolRuntimeFactsV1,
    RedirectPolicy, ToolCredentialUseV1, W7ToolOperationFingerprintPreimageV1,
    W7ToolOperationFingerprintV1,
};
use aura_security_contract::{
    InvocationIdentity, NetworkCallerKind, PermissionAllowAuthorizationBasisV1, PermissionMode,
    PermissionSnapshot,
};
use serde::Serialize;
use thiserror::Error;

use crate::{GatewayRequest, NetworkControlError};

/// Exact, non-secret credential identity and placement returned by the Core
/// credential authority for one Tool HTTP attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCredentialBinding {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub origin: aura_permission_domain::NormalizedOrigin,
    pub header_name: String,
}

/// A resolved Tool credential exists only while one authorized Gateway request
/// is being materialized. The value is a complete Header value and is zeroized
/// when it leaves scope.
pub struct ResolvedToolCredential {
    pub binding: ToolCredentialBinding,
    pub secret_header_value: Vec<u8>,
}

impl Drop for ResolvedToolCredential {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret_header_value.zeroize();
    }
}

pub trait ToolCredentialResolver: Send + Sync {
    fn resolve(
        &self,
        binding: &ToolCredentialUseV1,
    ) -> Result<ResolvedToolCredential, NetworkControlError>;
}

/// Frozen P04 authorization plus the exact W7 attempt identity. This value is
/// Core-owned and never accepts Renderer, model, Package, or Runtime input.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolHttpRequestAuthorityV1 {
    operation_id: SecurityOperationId,
    operation: W7ToolOperationFingerprintPreimageV1,
    operation_fingerprint: W7ToolOperationFingerprintV1,
    operation_identity: InvocationIdentity,
    permission_snapshot: PermissionSnapshot,
    authorization_basis: PermissionAllowAuthorizationBasisV1,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    prepared_at: UtcTimestamp,
    authorization_binding_hash: CanonicalHashV1,
}

/// Exact request-purpose restriction consumed once by `CoreHttpClientGateway`.
/// General Tool networking derives from a Core-authorized FullAccess snapshot,
/// independently of the historical per-origin/source-egress Grant store.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolHttpAttemptGrantV1 {
    operation_id: SecurityOperationId,
    operation_fingerprint: W7ToolOperationFingerprintV1,
    permission_snapshot_id: PermissionSnapshotId,
    authorization_binding_hash: CanonicalHashV1,
    tool_batch_id: ToolBatchId,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    source_security_fingerprint: ContentHash,
    execution_plan_hash: CanonicalHashV1,
    runtime_binding_hash: CanonicalHashV1,
    prepared_request_hash: CanonicalHashV1,
    grant_manifest_hash: CanonicalHashV1,
    request_plan_hash: CanonicalHashV1,
    non_secret_headers_hash: CanonicalHashV1,
    body_hash: CanonicalHashV1,
    body_byte_length: u64,
    success_status_ranges: Vec<(u16, u16)>,
    credential: Option<ToolCredentialUseV1>,
}

pub struct ToolHttpGatewayRequestV1 {
    pub authority: ToolHttpRequestAuthorityV1,
    pub grant: CoreRequestGrant<ToolHttpAttemptGrantV1>,
    pub request: GatewayRequest,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolHttpAuthorizationBindingHashMaterialV1<'a> {
    kind: &'static str,
    binding_version: u32,
    operation_id: SecurityOperationId,
    operation_fingerprint: &'a W7ToolOperationFingerprintV1,
    operation_identity: &'a InvocationIdentity,
    permission_snapshot: &'a PermissionSnapshot,
    authorization_basis: &'a PermissionAllowAuthorizationBasisV1,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    prepared_at: UtcTimestamp,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ToolHttpGrantContractErrorV1 {
    #[error("the P04-authorized W7 HTTP operation is inconsistent")]
    AuthorityMismatch,
    #[error("the exact Tool HTTP request differs from the authorized operation")]
    RequestMismatch,
    #[error("the Tool HTTP authorization cannot be represented as canonical JSON")]
    Canonicalization,
}

impl ToolHttpRequestAuthorityV1 {
    pub fn new(
        operation_id: SecurityOperationId,
        operation: W7ToolOperationFingerprintPreimageV1,
        operation_identity: InvocationIdentity,
        permission_snapshot: PermissionSnapshot,
        authorization_basis: PermissionAllowAuthorizationBasisV1,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        prepared_at: UtcTimestamp,
    ) -> Result<Self, ToolHttpGrantContractErrorV1> {
        operation.validate().map_err(|_| ToolHttpGrantContractErrorV1::AuthorityMismatch)?;
        if !matches!(operation.runtime, NonSecretToolRuntimeFactsV1::NetworkGateway { .. })
            || permission_snapshot.mode != PermissionMode::FullAccess
            || operation.sandbox_mode.is_some()
            || !identity_matches_operation(&operation_identity, &operation)
            || operation_identity.workspace_id != Some(permission_snapshot.workspace_id)
            || operation_identity.permission_snapshot_id != Some(permission_snapshot.id)
            || operation.invocation_identity.session_id != permission_snapshot.session_id
            || !authorization_basis_matches_snapshot(&authorization_basis, &permission_snapshot)
        {
            return Err(ToolHttpGrantContractErrorV1::AuthorityMismatch);
        }
        let operation_fingerprint =
            operation.fingerprint().map_err(|_| ToolHttpGrantContractErrorV1::Canonicalization)?;
        let authorization_binding_hash =
            canonical_sha256(&ToolHttpAuthorizationBindingHashMaterialV1 {
                kind: "aura.p07.tool_http_authorization_binding",
                binding_version: 1,
                operation_id,
                operation_fingerprint: &operation_fingerprint,
                operation_identity: &operation_identity,
                permission_snapshot: &permission_snapshot,
                authorization_basis: &authorization_basis,
                tool_execution_attempt_id,
                prepared_at,
            })
            .map_err(|_| ToolHttpGrantContractErrorV1::Canonicalization)?;
        Ok(Self {
            operation_id,
            operation,
            operation_fingerprint,
            operation_identity,
            permission_snapshot,
            authorization_basis,
            tool_execution_attempt_id,
            prepared_at,
            authorization_binding_hash,
        })
    }

    pub fn issue(
        &self,
        request: &GatewayRequest,
    ) -> Result<CoreRequestGrant<ToolHttpAttemptGrantV1>, ToolHttpGrantContractErrorV1> {
        validate_request_against_authority(self, request)?;
        let destination = request
            .plan
            .destination
            .as_ref()
            .ok_or(ToolHttpGrantContractErrorV1::RequestMismatch)?;
        let request_plan_hash = canonical_sha256(&request.plan)
            .map_err(|_| ToolHttpGrantContractErrorV1::Canonicalization)?;
        let non_secret_headers_hash = canonical_sha256(&request.headers)
            .map_err(|_| ToolHttpGrantContractErrorV1::Canonicalization)?;
        let body_hash = CanonicalHashV1::sha256(&request.body);
        let NonSecretToolRuntimeFactsV1::NetworkGateway { facts } = &self.operation.runtime else {
            return Err(ToolHttpGrantContractErrorV1::AuthorityMismatch);
        };
        let timeout_ms = i64::try_from(facts.timeout_ms)
            .map_err(|_| ToolHttpGrantContractErrorV1::AuthorityMismatch)?;
        let purpose = ToolHttpAttemptGrantV1 {
            operation_id: self.operation_id,
            operation_fingerprint: self.operation_fingerprint.clone(),
            permission_snapshot_id: self.permission_snapshot.id,
            authorization_binding_hash: self.authorization_binding_hash.clone(),
            tool_batch_id: self.operation.tool_batch_id,
            tool_call_id: self.operation.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            source_security_fingerprint: self.operation.source_security_fingerprint.clone(),
            execution_plan_hash: self.operation.execution_plan_hash.clone(),
            runtime_binding_hash: self.operation.runtime_binding_hash.clone(),
            prepared_request_hash: facts.prepared_request_hash.clone(),
            grant_manifest_hash: facts.grant_manifest_hash.clone(),
            request_plan_hash,
            non_secret_headers_hash,
            body_hash,
            body_byte_length: u64::try_from(request.body.len()).unwrap_or(u64::MAX),
            success_status_ranges: facts.success_status_ranges.clone(),
            credential: facts.credential.clone(),
        };
        Ok(CoreRequestGrant {
            logical_request_id: request.plan.request_id,
            purpose,
            caller: NetworkCallerKind::AuraTool,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: destination.host.clone(),
                port: destination.port,
                allowed_destination_classes: destination.classes.clone(),
            },
            methods: vec![request.plan.method.clone()],
            redirect_policy: request.plan.redirect_policy,
            data_classes: request.plan.data_classes.clone(),
            byte_limits: request.plan.byte_limits.clone(),
            expires_at: UtcTimestamp::from_value(
                self.prepared_at.value() + time::Duration::milliseconds(timeout_ms),
            ),
        })
    }

    pub(crate) fn validate_grant(
        &self,
        grant: &CoreRequestGrant<ToolHttpAttemptGrantV1>,
    ) -> Result<(), NetworkControlError> {
        let NonSecretToolRuntimeFactsV1::NetworkGateway { facts } = &self.operation.runtime else {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        };
        if grant.purpose.operation_id != self.operation_id
            || grant.purpose.operation_fingerprint != self.operation_fingerprint
            || grant.purpose.permission_snapshot_id != self.permission_snapshot.id
            || grant.purpose.authorization_binding_hash != self.authorization_binding_hash
            || grant.purpose.tool_batch_id != self.operation.tool_batch_id
            || grant.purpose.tool_call_id != self.operation.tool_call_id
            || grant.purpose.tool_execution_attempt_id != self.tool_execution_attempt_id
            || grant.purpose.source_security_fingerprint
                != self.operation.source_security_fingerprint
            || grant.purpose.execution_plan_hash != self.operation.execution_plan_hash
            || grant.purpose.runtime_binding_hash != self.operation.runtime_binding_hash
            || grant.purpose.success_status_ranges != facts.success_status_ranges
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub const fn tool_execution_attempt_id(&self) -> ToolExecutionAttemptId {
        self.tool_execution_attempt_id
    }

    #[must_use]
    pub const fn authorization_binding_hash(&self) -> &CanonicalHashV1 {
        &self.authorization_binding_hash
    }
}

impl ToolHttpAttemptGrantV1 {
    pub(crate) fn validate_request(
        &self,
        request: &GatewayRequest,
    ) -> Result<(), NetworkControlError> {
        let request_plan_hash = canonical_sha256(&request.plan)
            .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
        let non_secret_headers_hash = canonical_sha256(&request.headers)
            .map_err(|_| NetworkControlError::CoreGrantBindingMismatch)?;
        if request.plan.caller != NetworkCallerKind::AuraTool
            || !request.plan.credential_refs.is_empty()
            || request_plan_hash != self.request_plan_hash
            || non_secret_headers_hash != self.non_secret_headers_hash
            || CanonicalHashV1::sha256(&request.body) != self.body_hash
            || u64::try_from(request.body.len()).unwrap_or(u64::MAX) != self.body_byte_length
        {
            return Err(NetworkControlError::CoreGrantBindingMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub const fn credential(&self) -> Option<&ToolCredentialUseV1> {
        self.credential.as_ref()
    }

    #[must_use]
    pub(crate) fn success_status_ranges(&self) -> &[(u16, u16)] {
        &self.success_status_ranges
    }

    #[must_use]
    pub const fn authorization_binding_hash(&self) -> &CanonicalHashV1 {
        &self.authorization_binding_hash
    }

    pub fn dispatch_evidence_hash(&self) -> Result<CanonicalHashV1, ToolHttpGrantContractErrorV1> {
        canonical_sha256(self).map_err(|_| ToolHttpGrantContractErrorV1::Canonicalization)
    }
}

fn identity_matches_operation(
    identity: &InvocationIdentity,
    operation: &W7ToolOperationFingerprintPreimageV1,
) -> bool {
    identity.session_id == Some(operation.invocation_identity.session_id)
        && identity.turn_task_id == Some(operation.invocation_identity.turn_task_id)
        && identity.agent_run_id == Some(operation.invocation_identity.agent_run_id)
}

fn validate_request_against_authority(
    authority: &ToolHttpRequestAuthorityV1,
    request: &GatewayRequest,
) -> Result<(), ToolHttpGrantContractErrorV1> {
    let NonSecretToolRuntimeFactsV1::NetworkGateway { facts } = &authority.operation.runtime else {
        return Err(ToolHttpGrantContractErrorV1::AuthorityMismatch);
    };
    let body_length = u64::try_from(request.body.len()).unwrap_or(u64::MAX);
    let header_hash = canonical_sha256(&request.headers)
        .map_err(|_| ToolHttpGrantContractErrorV1::Canonicalization)?;
    let body_hash = CanonicalHashV1::sha256(&request.body);
    if request.plan.caller != NetworkCallerKind::AuraTool
        || request.plan.destination.is_none()
        || !request.plan.credential_refs.is_empty()
        || request.plan.identity.session_id
            != Some(authority.operation.invocation_identity.session_id)
        || request.plan.identity.turn_task_id
            != Some(authority.operation.invocation_identity.turn_task_id)
        || request.plan.identity.agent_run_id
            != Some(authority.operation.invocation_identity.agent_run_id)
        || request.plan.identity.permission_snapshot_id != Some(authority.permission_snapshot.id)
        || request.plan.method != facts.method
        || request.plan.requested_url != facts.exact_url
        || header_hash != facts.headers_hash
        || body_hash != facts.body_hash
        || body_length != facts.body_byte_length
        || request.plan.redirect_policy != (RedirectPolicy::SameOrigin { max_hops: 3 })
        || request.plan.timeout_ms != facts.timeout_ms
        || request.plan.byte_limits.max_download_bytes != facts.response_wire_limit
        || request.plan.byte_limits.max_decompressed_bytes != facts.response_decompressed_limit
        || request.plan.byte_limits.max_upload_bytes < body_length
    {
        return Err(ToolHttpGrantContractErrorV1::RequestMismatch);
    }
    Ok(())
}

fn authorization_basis_matches_snapshot(
    basis: &PermissionAllowAuthorizationBasisV1,
    snapshot: &PermissionSnapshot,
) -> bool {
    match basis {
        PermissionAllowAuthorizationBasisV1::PermissionMode { mode } => *mode == snapshot.mode,
        PermissionAllowAuthorizationBasisV1::PermissionGrant { permission_grant_id, .. } => {
            snapshot.active_grant_ids.contains(permission_grant_id)
        }
    }
}

#[cfg(test)]
mod tests;
