use crate::{BackendDescriptor, ExecutionContractError as Error, SecuredOperation};
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{Revision, SecurityOperationId, UtcTimestamp};
use aura_security_contract::{AuthorizationDecision, AuthorizationLifetime, OperationFingerprint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptRef {
    pub operation_id: SecurityOperationId,
    pub ordinal: std::num::NonZeroU32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedExecution {
    pub operation_fingerprint: OperationFingerprint,
    pub backend: BackendDescriptor,
    pub preparation: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlan {
    pub prepared: PreparedExecution,
    pub fingerprint: CanonicalSha256HashV1,
}

impl ExecutionPlan {
    pub fn new(prepared: PreparedExecution) -> Result<Self, Error> {
        let fingerprint = canonical_sha256(&prepared).map_err(|_| Error::InvalidContent)?;
        Ok(Self { prepared, fingerprint })
    }
    pub fn validate(&self) -> Result<(), Error> {
        if canonical_sha256(&self.prepared).map_err(|_| Error::InvalidContent)? != self.fingerprint
        {
            return Err(Error::FingerprintMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchBinding {
    pub attempt: AttemptRef,
    pub operation_fingerprint: OperationFingerprint,
    pub authorization_fingerprint: CanonicalSha256HashV1,
    pub plan_fingerprint: CanonicalSha256HashV1,
    pub cancellation_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CancellationState {
    pub revision: Revision,
    pub requested: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRequest {
    pub operation: SecuredOperation,
    pub plan: ExecutionPlan,
    pub binding: DispatchBinding,
}

/// A checked in-process dispatch. It cannot be deserialized from a runner or
/// client. R04 supplies live authority facts from Core-owned stores. Matching
/// snapshots is necessary but does not itself consume a Once grant or persist
/// an Attempt; that atomic transition belongs to the Orchestrator.
#[derive(Clone, Debug)]
pub struct AuthorizedDispatch(DispatchRequest);

impl AuthorizedDispatch {
    pub fn revalidate(
        request: DispatchRequest,
        authority: &AuthorizationDecision,
        backend: &BackendDescriptor,
        cancellation: &CancellationState,
        now: UtcTimestamp,
    ) -> Result<Self, Error> {
        let op = &request.operation;
        op.validate()?;
        request.plan.validate()?;
        let b = &request.binding;
        if cancellation.requested
            || cancellation.revision != b.cancellation_revision
            || b.attempt.operation_id != op.operation_id
        {
            return Err(Error::AttemptMismatch);
        }
        if b.operation_fingerprint != op.fingerprint
            || request.plan.prepared.operation_fingerprint != op.fingerprint
            || b.plan_fingerprint != request.plan.fingerprint
        {
            return Err(Error::FingerprintMismatch);
        }
        if authority.decided_at > now
            || authority.expires_at <= now
            || authority.scope.operation_fingerprint != op.fingerprint
            || authority.fingerprint().map_err(|_| Error::InvalidContent)?
                != b.authorization_fingerprint
        {
            return Err(Error::AuthorizationMismatch);
        }
        let identity = &op.content.identity;
        let lifetime_matches = match authority.scope.lifetime {
            AuthorizationLifetime::Once { operation_id } => operation_id == op.operation_id,
            AuthorizationLifetime::TurnTask { session_id, turn_task_id } => {
                identity.session_id == Some(session_id)
                    && identity.turn_task_id == Some(turn_task_id)
            }
            AuthorizationLifetime::Session { session_id } => {
                identity.session_id == Some(session_id)
            }
            AuthorizationLifetime::Workspace { workspace_id } => {
                identity.workspace_id == Some(workspace_id)
            }
        };
        if !lifetime_matches {
            return Err(Error::AuthorizationMismatch);
        }
        if let aura_security_contract::AuthorizationBasis::AuthenticatedMain {
            principal_id,
            core_runtime_instance_id,
            ..
        } = authority.basis
            && (principal_id != identity.principal_id
                || core_runtime_instance_id != backend.runtime_instance_id
                || identity.actor != aura_security_contract::InvocationActor::User
                || identity.agent_run_id.is_some()
                || identity.parent_agent_run_id.is_some()
                || identity.turn_task_id.is_some()
                || op.content.source != aura_security_contract::SecuritySourceRef::UserInterface
                || backend.class != aura_security_contract::BackendClass::CoreBroker
                || !matches!(authority.scope.lifetime, AuthorizationLifetime::Once { .. }))
        {
            return Err(Error::AuthorizationMismatch);
        }
        if matches!(authority.basis, aura_security_contract::AuthorizationBasis::Grant { revision, .. } if revision.value() == 0)
            || backend.class == aura_security_contract::BackendClass::Native
                && !crate::native_authority::valid(authority, op)
        {
            return Err(Error::AuthorizationMismatch);
        }
        if backend != &request.plan.prepared.backend
            || !backend.supports_operation(&authority.scope.backend_requirement, op)
        {
            return Err(Error::BackendMismatch);
        }
        Ok(Self(request))
    }
    #[must_use]
    pub fn request(&self) -> &DispatchRequest {
        &self.0
    }
}
