//! Secret management requests contain metadata only. Secret bytes use the bound IPC channel.
use crate::{ToolCredentialAuthorityRecordV1, ToolCredentialPlacementV1, ToolCredentialScopeV1};
use aura_domain_common::{CredentialId, Revision, SessionId, ToolInstanceId, WorkspaceId};
use aura_tool_contract::CredentialSlotKey;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialManagementTargetV1 {
    pub tool_instance_id: ToolInstanceId,
    pub expected_instance_revision: Revision,
    pub credential_slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialCreateRequestV1 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    pub target: ToolCredentialManagementTargetV1,
    pub display_name: String,
    pub scope: ToolCredentialScopeV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialOriginsRequestV1 {
    pub target: ToolCredentialManagementTargetV1,
    pub credential_id: CredentialId,
    pub expected_row_revision: Revision,
    pub allowed_origins: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialChangeRequestV1 {
    pub target: ToolCredentialManagementTargetV1,
    pub credential_id: CredentialId,
    pub expected_row_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialListResultV1 {
    pub credentials: Vec<ToolCredentialAuthorityRecordV1>,
    pub pending_credential_ids: Vec<CredentialId>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialMutationResultV1 {
    pub credential: Option<ToolCredentialAuthorityRecordV1>,
    pub disposition: ToolCredentialMutationDispositionV1,
    /// The authoritative change committed; deletion of an old WinCred value may still need retry.
    pub cleanup_pending: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCredentialMutationDispositionV1 {
    Committed,
    Aborted,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialOutcomeRequestV1 {
    pub target: ToolCredentialManagementTargetV1,
    pub command_invocation_id: aura_domain_common::CommandInvocationId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialOutcomeV1 {
    pub command_invocation_id: aura_domain_common::CommandInvocationId,
    pub state: ToolCredentialOutcomeStateV1,
    pub result: Option<ToolCredentialMutationResultV1>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCredentialOutcomeStateV1 {
    Pending,
    Committed,
    Aborted,
    NotObtained,
}
