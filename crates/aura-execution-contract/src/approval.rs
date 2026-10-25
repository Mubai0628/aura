//! Authenticated user approval of the exact immutable execution proposal.
use crate::{BackendDescriptor, SecuredOperation};
use aura_domain_common::*;
use aura_security_contract::{
    ApprovalDecisionKind, AuthorizationApproval, NativeSessionConsentFact, PermissionMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionApprovalRequest {
    pub session_id: SessionId,
    pub approval_id: ApprovalRequestId,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionApprovalResolutionRequest {
    pub session_id: SessionId,
    pub approval_id: ApprovalRequestId,
    pub expected_revision: Revision,
    pub decision: ApprovalDecisionKind,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionApprovalView {
    pub approval: AuthorizationApproval,
    pub operation: SecuredOperation,
    pub backend: BackendDescriptor,
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_capabilities: Option<Vec<aura_security_contract::ResolvedCapability>>,
    /// The permission mode in force, which is the *allowed range*. It is separate
    /// from `backend`, which is *how this call will run*: FullAccess with a
    /// restricted backend and FullAccess with Native are different executions of
    /// the same allowance, and the confirmation has to say which one it is.
    pub mode: PermissionMode,
    /// Present only when a session approval here would create a reusable Native
    /// session consent, so the dialog describes the range the button really grants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session_consent: Option<NativeSessionConsentFact>,
    /// Present when this call will use the frozen step proxy. The sentence is the whole fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_proxy_notice: Option<String>,
}
