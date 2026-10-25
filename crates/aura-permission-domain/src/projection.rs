use aura_domain_common::{Revision, RuntimeInstanceId, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ApprovalView, HostPreparationSummary, RuntimeComponentState};
use aura_security_contract::PermissionMode;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionPermissionProjection {
    pub session_id: SessionId,
    pub mode: PermissionMode,
    pub revision: u64,
    pub active_grant_count: u32,
    /// Older projection records predate coordinated active execution. Current
    /// responses always serialize the actual persisted transition phase.
    #[serde(default)]
    pub transition: PermissionModeTransition,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionModeTransition {
    #[default]
    Settled,
    Stopping,
    StopUnconfirmed,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PendingApprovalProjection {
    pub approvals: Vec<ApprovalView>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRuntimeProjection {
    pub sandbox_available: bool,
    pub host_preparation: HostPreparationSummary,
    pub sandbox_host_state: RuntimeComponentState,
    pub egress_proxy_state: RuntimeComponentState,
    pub elevated_helper_state: RuntimeComponentState,
    pub quarantine_count: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(
    tag = "projection_kind",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SecurityProjectionPayload {
    SessionPermission(SessionPermissionProjection),
    PendingApproval(PendingApprovalProjection),
    SecurityRuntime(SecurityRuntimeProjection),
}

impl SecurityProjectionPayload {
    #[must_use]
    pub const fn kind(&self) -> SecurityProjectionKind {
        match self {
            Self::SessionPermission(_) => SecurityProjectionKind::SessionPermission,
            Self::PendingApproval(_) => SecurityProjectionKind::PendingApproval,
            Self::SecurityRuntime(_) => SecurityProjectionKind::SecurityRuntime,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityProjectionKind {
    SessionPermission,
    PendingApproval,
    SecurityRuntime,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "projection_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecurityProjectionChange {
    SessionPermission {
        runtime_instance_id: RuntimeInstanceId,
        revision: Revision,
        payload: SessionPermissionProjection,
    },
    PendingApproval {
        runtime_instance_id: RuntimeInstanceId,
        revision: Revision,
        payload: PendingApprovalProjection,
    },
    SecurityRuntime {
        runtime_instance_id: RuntimeInstanceId,
        revision: Revision,
        payload: SecurityRuntimeProjection,
    },
}
