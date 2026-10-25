use aura_domain_common::{
    CredentialId, CredentialRevision, PrincipalId, Revision, SessionId, UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{ToolId, ToolInstanceId, ToolPackageId};
use aura_tool_contract::{CredentialSlotKey, EnvironmentVariableNameV1, HttpHeaderNameV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolCredentialKindV1 {
    ApiKey,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolCredentialStateV1 {
    Configured,
    Suspended,
    Revoked,
    RecoveryRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCredentialScopeV1 {
    User,
    Workspace { workspace_id: WorkspaceId },
    Session { workspace_id: WorkspaceId, session_id: SessionId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "placement", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCredentialPlacementV1 {
    CommandEnvironment { environment_variable: EnvironmentVariableNameV1 },
    HttpBearerAuthorization,
    HttpFixedHeader { header_name: HttpHeaderNameV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialPlacementPolicyV1 {
    pub allowed_placements: Vec<ToolCredentialPlacementV1>,
}

impl ToolCredentialPlacementPolicyV1 {
    pub fn validate(&self) -> Result<(), ToolCredentialContractErrorV1> {
        if self.allowed_placements.is_empty()
            || self.allowed_placements.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ToolCredentialContractErrorV1::InvalidPlacementPolicy);
        }
        Ok(())
    }

    #[must_use]
    pub fn allows(&self, placement: &ToolCredentialPlacementV1) -> bool {
        self.allowed_placements.binary_search(placement).is_ok()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "policy", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolCredentialUsagePolicyV1 {
    Explicit { tool_package_ids: Vec<ToolPackageId>, tool_ids: Vec<ToolId> },
}

impl ToolCredentialUsagePolicyV1 {
    pub fn validate(&self) -> Result<(), ToolCredentialContractErrorV1> {
        let Self::Explicit { tool_package_ids, tool_ids } = self;
        if (tool_package_ids.is_empty() && tool_ids.is_empty())
            || tool_package_ids.windows(2).any(|pair| pair[0] >= pair[1])
            || tool_ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ToolCredentialContractErrorV1::InvalidUsagePolicy);
        }
        Ok(())
    }

    #[must_use]
    pub fn allows(&self, tool_package_id: Option<ToolPackageId>, tool_id: ToolId) -> bool {
        let Self::Explicit { tool_package_ids, tool_ids } = self;
        tool_ids.binary_search(&tool_id).is_ok()
            || tool_package_id.is_some_and(|id| tool_package_ids.binary_search(&id).is_ok())
    }
}

/// Authority metadata for a Tool Credential. Secret bytes and WinCred
/// locators are deliberately held by the P02 storage binding, never by this
/// renderer-safe domain record.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCredentialAuthorityRecordV1 {
    /// User-configured HTTP origins. Empty historical records authorize no model-selected URL.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub authenticated_owner: PrincipalId,
    pub scope: ToolCredentialScopeV1,
    pub credential_kind: ToolCredentialKindV1,
    pub state: ToolCredentialStateV1,
    pub placement_policy: ToolCredentialPlacementPolicyV1,
    pub usage_policy: ToolCredentialUsagePolicyV1,
    pub display_name: String,
    pub row_revision: Revision,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

impl ToolCredentialAuthorityRecordV1 {
    pub fn validate(&self) -> Result<(), ToolCredentialContractErrorV1> {
        if self.row_revision.value() == 0
            || self.display_name.trim() != self.display_name
            || self.display_name.is_empty()
            || self.display_name.len() > 120
            || self.display_name.chars().any(char::is_control)
        {
            return Err(ToolCredentialContractErrorV1::InvalidRecord);
        }
        self.placement_policy.validate()?;
        super::credential_origins::validate_origins(&self.allowed_origins)?;
        self.usage_policy.validate()
    }

    #[must_use]
    pub fn scope_allows(
        &self,
        workspace_id: Option<WorkspaceId>,
        session_id: Option<SessionId>,
    ) -> bool {
        match self.scope {
            ToolCredentialScopeV1::User => true,
            ToolCredentialScopeV1::Workspace { workspace_id: expected } => {
                workspace_id == Some(expected)
            }
            ToolCredentialScopeV1::Session {
                workspace_id: expected_workspace,
                session_id: expected_session,
            } => workspace_id == Some(expected_workspace) && session_id == Some(expected_session),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInstanceCredentialBindingRecordV1 {
    pub tool_instance_id: ToolInstanceId,
    pub credential_slot_key: CredentialSlotKey,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub placement: ToolCredentialPlacementV1,
    pub binding_revision: Revision,
    pub binding_hash: aura_domain_common::jcs::CanonicalHashV1,
    pub updated_at: UtcTimestamp,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolInstanceCredentialBindingHashMaterialV1<'a> {
    tool_instance_id: ToolInstanceId,
    credential_slot_key: &'a CredentialSlotKey,
    credential_id: CredentialId,
    credential_revision: CredentialRevision,
    placement: &'a ToolCredentialPlacementV1,
    binding_revision: Revision,
}

impl ToolInstanceCredentialBindingRecordV1 {
    pub fn new(
        tool_instance_id: ToolInstanceId,
        credential_slot_key: CredentialSlotKey,
        credential_id: CredentialId,
        credential_revision: CredentialRevision,
        placement: ToolCredentialPlacementV1,
        binding_revision: Revision,
        updated_at: UtcTimestamp,
    ) -> Result<Self, ToolCredentialContractErrorV1> {
        if binding_revision.value() == 0 {
            return Err(ToolCredentialContractErrorV1::InvalidBinding);
        }
        let binding_hash = aura_domain_common::jcs::canonical_sha256(
            &ToolInstanceCredentialBindingHashMaterialV1 {
                tool_instance_id,
                credential_slot_key: &credential_slot_key,
                credential_id,
                credential_revision,
                placement: &placement,
                binding_revision,
            },
        )
        .map_err(|_| ToolCredentialContractErrorV1::InvalidBinding)?;
        Ok(Self {
            tool_instance_id,
            credential_slot_key,
            credential_id,
            credential_revision,
            placement,
            binding_revision,
            binding_hash,
            updated_at,
        })
    }

    pub fn validate(&self) -> Result<(), ToolCredentialContractErrorV1> {
        if self.binding_revision.value() == 0
            || aura_domain_common::jcs::canonical_sha256(
                &ToolInstanceCredentialBindingHashMaterialV1 {
                    tool_instance_id: self.tool_instance_id,
                    credential_slot_key: &self.credential_slot_key,
                    credential_id: self.credential_id,
                    credential_revision: self.credential_revision,
                    placement: &self.placement,
                    binding_revision: self.binding_revision,
                },
            )
            .map_err(|_| ToolCredentialContractErrorV1::InvalidBinding)?
                != self.binding_hash
        {
            return Err(ToolCredentialContractErrorV1::InvalidBinding);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolCredentialContractErrorV1 {
    #[error("the Tool Credential authority record is invalid")]
    InvalidRecord,
    #[error("the Tool Credential placement policy is empty, duplicated, or unsorted")]
    InvalidPlacementPolicy,
    #[error("the Tool Credential usage policy is empty, duplicated, or unsorted")]
    InvalidUsagePolicy,
    #[error("the Tool Credential instance binding is invalid")]
    InvalidBinding,
}
