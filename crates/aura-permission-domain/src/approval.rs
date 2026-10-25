use aura_domain_common::{
    ApprovalDecisionId, ApprovalRequestId, ContentHash, PermissionSnapshotId, SecurityOperationId,
    UtcTimestamp,
};
use aura_runtime_contract::ReleaseDescriptor;
use aura_security_contract::{ApprovalDecisionKind, ApprovalState};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ResolvedScope, SecuritySource};
use aura_security_contract::{InvocationIdentity, OperationRiskFlag, ResolvedCapability};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequestDraft {
    pub identity: InvocationIdentity,
    pub operation_ids: Vec<SecurityOperationId>,
    pub operation_fingerprints: Vec<ContentHash>,
    pub title: String,
    pub model_purpose: Option<String>,
    pub actual_operation_summary: String,
    pub source_summary: String,
    pub source: SecuritySource,
    pub source_fingerprint: Option<ContentHash>,
    pub capabilities: Vec<ResolvedCapability>,
    pub scopes: Vec<ResolvedScope>,
    pub risk_flags: Vec<OperationRiskFlag>,
    pub allowed_decisions: Vec<ApprovalDecisionKind>,
    pub permission_snapshot_id: PermissionSnapshotId,
    pub release_descriptor: Option<ReleaseDescriptor>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequest {
    pub id: ApprovalRequestId,
    pub identity: InvocationIdentity,
    pub operation_ids: Vec<SecurityOperationId>,
    pub operation_fingerprints: Vec<ContentHash>,
    pub title: String,
    pub model_purpose: Option<String>,
    pub actual_operation_summary: String,
    pub source_summary: String,
    pub source: SecuritySource,
    pub source_fingerprint: Option<ContentHash>,
    pub capabilities: Vec<ResolvedCapability>,
    pub scopes: Vec<ResolvedScope>,
    pub risk_flags: Vec<OperationRiskFlag>,
    pub allowed_decisions: Vec<ApprovalDecisionKind>,
    pub permission_snapshot_id: PermissionSnapshotId,
    pub release_descriptor: Option<ReleaseDescriptor>,
    pub request_revision: u64,
    pub state: ApprovalState,
    pub created_at: UtcTimestamp,
    pub resolved_at: Option<UtcTimestamp>,
    pub invalidation_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveApprovalInput {
    pub approval_id: ApprovalRequestId,
    pub expected_request_revision: u64,
    pub decision: ApprovalDecisionKind,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GetApprovalDetailsInput {
    pub approval_id: ApprovalRequestId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalResolution {
    pub approval_id: ApprovalRequestId,
    pub decision_id: ApprovalDecisionId,
    pub state: ApprovalState,
    pub decision: ApprovalDecisionKind,
    pub grant_ids: Vec<aura_domain_common::PermissionGrantId>,
    pub resolved_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalView {
    pub id: ApprovalRequestId,
    /// Originating session; None denotes a non-conversation approval.
    pub session_id: Option<aura_domain_common::SessionId>,
    pub title: String,
    pub model_purpose: Option<String>,
    pub actual_operation_summary: String,
    pub source_summary: String,
    pub risk_flags: Vec<OperationRiskFlag>,
    pub allowed_decisions: Vec<ApprovalDecisionKind>,
    pub request_revision: u64,
    pub created_at: UtcTimestamp,
    pub release_descriptor: Option<ReleaseDescriptor>,
}

impl From<&ApprovalRequest> for ApprovalView {
    fn from(value: &ApprovalRequest) -> Self {
        Self {
            id: value.id,
            session_id: value.identity.session_id,
            title: value.title.clone(),
            model_purpose: value.model_purpose.clone(),
            actual_operation_summary: value.actual_operation_summary.clone(),
            source_summary: value.source_summary.clone(),
            risk_flags: value.risk_flags.clone(),
            allowed_decisions: value.allowed_decisions.clone(),
            request_revision: value.request_revision,
            created_at: value.created_at,
            release_descriptor: value.release_descriptor.clone(),
        }
    }
}
