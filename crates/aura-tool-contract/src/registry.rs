use std::{fmt, str::FromStr};

use aura_domain_common::{AgentRunId, Revision, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

mod settings;
pub use settings::ToolSettingsSnapshotV1;

use crate::{
    EffectiveToolBindingSnapshot, ToolCanonicalName, ToolDefinitionSnapshotRef, ToolExposureV1,
    ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1, ToolSourceContractFingerprintV1,
    ToolSourceKindV1, ToolVersion,
};
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{ToolId, ToolInstanceId, ToolRegistrySnapshotId};

pub const DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS: u32 = 50;
pub const DEFAULT_MAX_CONCURRENT_TOOL_CALLS: u32 = 4;
/// Ten minutes, so ordinary builds and tests are not cut off.
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 600_000;
pub const DEFAULT_MAX_TOOL_RETRIES: u8 = 3;
pub const MIN_MAX_CONCURRENT_TOOL_CALLS: u32 = 1;
pub const MAX_MAX_CONCURRENT_TOOL_CALLS: u32 = 32;
pub const MIN_TOOL_TIMEOUT_MS: u64 = 1_000;
pub const MAX_TOOL_TIMEOUT_MS: u64 = 1_800_000;
pub const MAX_TOOL_RETRIES: u8 = 3;
pub const MAX_TOOL_SEARCH_QUERY_BYTES: usize = 512;
pub const MAX_TOOL_SEARCH_PAGE_SIZE: u8 = 50;
pub const TOOL_SEARCH_ALGORITHM_VERSION_V1: &str = "unicode_default_casefold_17_0_v1";

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolRegistryContractError {
    #[error("tool settings are outside their frozen ranges")]
    InvalidSettings,
    #[error(transparent)]
    InvalidNetworkProxy(#[from] crate::ToolNetworkProxyErrorV1),
    #[error("tool settings hash is inconsistent")]
    SettingsHashMismatch,
    #[error("tool registry entries are not strictly ordered by ToolId")]
    InvalidEntryOrder,
    #[error("effective tool bindings are not strictly ordered by ToolId")]
    InvalidBindingOrder,
    #[error("loaded registry entries, effective bindings, and schemas are inconsistent")]
    ExposureMismatch,
    #[error("tool registry snapshot hash is inconsistent")]
    SnapshotHashMismatch,
    #[error("tool search query must be non-blank and at most 512 UTF-8 bytes")]
    InvalidSearchQuery,
    #[error("tool search limit must be in 1..=50")]
    InvalidSearchLimit,
    #[error("tool search cursor is invalid")]
    InvalidCursor,
    #[error("tool definition refs must be unique and strictly ordered")]
    InvalidDefinitionRefs,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolPinResolutionStateV1 {
    Resolved,
    Unresolved,
    Degraded,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPinV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub state: ToolPinResolutionStateV1,
    pub safe_reason: Option<String>,
    pub revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolRegistryScopeV1 {
    Workspace {
        workspace_id: WorkspaceId,
    },
    Session {
        workspace_id: WorkspaceId,
        session_id: SessionId,
        turn_task_id: TurnTaskId,
        agent_run_id: AgentRunId,
    },
    ChildAgent {
        workspace_id: WorkspaceId,
        session_id: SessionId,
        turn_task_id: TurnTaskId,
        child_agent_run_id: AgentRunId,
    },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolLoadStateV1 {
    Core,
    Pinned,
    Ephemeral,
    Discoverable,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRegistryCatalogEntryV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub tool_instance_id: ToolInstanceId,
    pub canonical_name: ToolCanonicalName,
    pub display_name: String,
    pub summary: String,
    pub source: ToolSourceKindV1,
    pub definition_fingerprint: CanonicalHashV1,
    pub source_contract_fingerprint: ToolSourceContractFingerprintV1,
    pub load_state: ToolLoadStateV1,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRegistrySnapshotV1 {
    pub snapshot_ref: ToolRegistrySnapshotRef,
    pub scope: ToolRegistryScopeV1,
    pub parent_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    pub derivation_hash: Option<CanonicalHashV1>,
    // Hash of the frozen mode, permission, settings, Pin, Ephemeral Load,
    // source eligibility, and entity-revision inputs used for this scope.
    pub eligibility_context_hash: CanonicalHashV1,
    pub schema_set: ToolSchemaSetSnapshotV1,
    /// Every discoverable exact Tool version frozen in this scoped snapshot.
    pub catalog_entries: Vec<ToolRegistryCatalogEntryV1>,
    /// Only the Core/Pinned/Ephemeral tools exposed to the current ModelStep.
    pub effective_bindings: Vec<EffectiveToolBindingSnapshot>,
    /// The discoverable tools this step can call through `tools.call` without their
    /// definitions being sent; each is `ToolExposureV1::UnifiedEntry`. Absent in snapshots made
    /// before the unified entry, which therefore offer it nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entry_bindings: Vec<EffectiveToolBindingSnapshot>,
    pub created_at: UtcTimestamp,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryHashMaterial<'a> {
    tool_registry_snapshot_id: ToolRegistrySnapshotId,
    registry_revision: Revision,
    scope: &'a ToolRegistryScopeV1,
    parent_registry_snapshot_ref: &'a Option<ToolRegistrySnapshotRef>,
    derivation_hash: &'a Option<CanonicalHashV1>,
    eligibility_context_hash: &'a CanonicalHashV1,
    tool_set_hash: &'a CanonicalHashV1,
    catalog_entries: &'a [ToolRegistryCatalogEntryV1],
    effective_bindings: &'a [EffectiveToolBindingSnapshot],
    // Left out when empty, so every snapshot made by an older version keeps its hash.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    entry_bindings: &'a [EffectiveToolBindingSnapshot],
    created_at: UtcTimestamp,
}

impl ToolRegistrySnapshotV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_registry_snapshot_id: ToolRegistrySnapshotId,
        registry_revision: Revision,
        scope: ToolRegistryScopeV1,
        parent_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
        derivation_hash: Option<CanonicalHashV1>,
        eligibility_context_hash: CanonicalHashV1,
        schemas: Vec<crate::ToolSchemaSnapshotV1>,
        catalog_entries: Vec<ToolRegistryCatalogEntryV1>,
        effective_bindings: Vec<EffectiveToolBindingSnapshot>,
        created_at: UtcTimestamp,
    ) -> Result<Self, ToolRegistryContractError> {
        Self::with_entry_bindings(
            tool_registry_snapshot_id,
            registry_revision,
            scope,
            parent_registry_snapshot_ref,
            derivation_hash,
            eligibility_context_hash,
            schemas,
            catalog_entries,
            effective_bindings,
            Vec::new(),
            created_at,
        )
    }

    /// [`Self::new`] for a snapshot that also offers discoverable tools through `tools.call`.
    #[allow(clippy::too_many_arguments)]
    pub fn with_entry_bindings(
        tool_registry_snapshot_id: ToolRegistrySnapshotId,
        registry_revision: Revision,
        scope: ToolRegistryScopeV1,
        parent_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
        derivation_hash: Option<CanonicalHashV1>,
        eligibility_context_hash: CanonicalHashV1,
        schemas: Vec<crate::ToolSchemaSnapshotV1>,
        catalog_entries: Vec<ToolRegistryCatalogEntryV1>,
        effective_bindings: Vec<EffectiveToolBindingSnapshot>,
        entry_bindings: Vec<EffectiveToolBindingSnapshot>,
        created_at: UtcTimestamp,
    ) -> Result<Self, ToolRegistryContractError> {
        validate_registry_collections(&catalog_entries, &effective_bindings, &schemas)?;
        validate_entry_bindings(&catalog_entries, &entry_bindings)?;
        let placeholder_ref = ToolRegistrySnapshotRef {
            tool_registry_snapshot_id,
            registry_revision,
            content_hash: CanonicalHashV1::sha256(&[]),
        };
        let schema_set = ToolSchemaSetSnapshotV1::new(placeholder_ref.clone(), schemas)
            .map_err(|_| ToolRegistryContractError::SnapshotHashMismatch)?;
        let content_hash = canonical_sha256(&RegistryHashMaterial {
            tool_registry_snapshot_id,
            registry_revision,
            scope: &scope,
            parent_registry_snapshot_ref: &parent_registry_snapshot_ref,
            derivation_hash: &derivation_hash,
            eligibility_context_hash: &eligibility_context_hash,
            tool_set_hash: &schema_set.tool_set_hash,
            catalog_entries: &catalog_entries,
            effective_bindings: &effective_bindings,
            entry_bindings: &entry_bindings,
            created_at,
        })
        .map_err(|_| ToolRegistryContractError::SnapshotHashMismatch)?;
        let snapshot_ref =
            ToolRegistrySnapshotRef { tool_registry_snapshot_id, registry_revision, content_hash };
        let schema_set =
            ToolSchemaSetSnapshotV1 { registry_snapshot_ref: snapshot_ref.clone(), ..schema_set };
        let snapshot = Self {
            snapshot_ref,
            scope,
            parent_registry_snapshot_ref,
            derivation_hash,
            eligibility_context_hash,
            schema_set,
            catalog_entries,
            effective_bindings,
            entry_bindings,
            created_at,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), ToolRegistryContractError> {
        validate_registry_collections(
            &self.catalog_entries,
            &self.effective_bindings,
            &self.schema_set.tools,
        )?;
        validate_entry_bindings(&self.catalog_entries, &self.entry_bindings)?;
        self.schema_set.validate().map_err(|_| ToolRegistryContractError::SnapshotHashMismatch)?;
        if self.schema_set.registry_snapshot_ref != self.snapshot_ref {
            return Err(ToolRegistryContractError::SnapshotHashMismatch);
        }
        let expected = canonical_sha256(&RegistryHashMaterial {
            tool_registry_snapshot_id: self.snapshot_ref.tool_registry_snapshot_id,
            registry_revision: self.snapshot_ref.registry_revision,
            scope: &self.scope,
            parent_registry_snapshot_ref: &self.parent_registry_snapshot_ref,
            derivation_hash: &self.derivation_hash,
            eligibility_context_hash: &self.eligibility_context_hash,
            tool_set_hash: &self.schema_set.tool_set_hash,
            catalog_entries: &self.catalog_entries,
            effective_bindings: &self.effective_bindings,
            entry_bindings: &self.entry_bindings,
            created_at: self.created_at,
        })
        .map_err(|_| ToolRegistryContractError::SnapshotHashMismatch)?;
        if expected != self.snapshot_ref.content_hash {
            return Err(ToolRegistryContractError::SnapshotHashMismatch);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolRegistrySnapshotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSnapshot {
            snapshot_ref: ToolRegistrySnapshotRef,
            scope: ToolRegistryScopeV1,
            parent_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
            derivation_hash: Option<CanonicalHashV1>,
            eligibility_context_hash: CanonicalHashV1,
            schema_set: ToolSchemaSetSnapshotV1,
            catalog_entries: Vec<ToolRegistryCatalogEntryV1>,
            effective_bindings: Vec<EffectiveToolBindingSnapshot>,
            #[serde(default)]
            entry_bindings: Vec<EffectiveToolBindingSnapshot>,
            created_at: UtcTimestamp,
        }

        let wire = WireSnapshot::deserialize(deserializer)?;
        let snapshot = Self {
            snapshot_ref: wire.snapshot_ref,
            scope: wire.scope,
            parent_registry_snapshot_ref: wire.parent_registry_snapshot_ref,
            derivation_hash: wire.derivation_hash,
            eligibility_context_hash: wire.eligibility_context_hash,
            schema_set: wire.schema_set,
            catalog_entries: wire.catalog_entries,
            effective_bindings: wire.effective_bindings,
            entry_bindings: wire.entry_bindings,
            created_at: wire.created_at,
        };
        snapshot.validate().map_err(D::Error::custom)?;
        Ok(snapshot)
    }
}

fn validate_registry_collections(
    catalog_entries: &[ToolRegistryCatalogEntryV1],
    effective_bindings: &[EffectiveToolBindingSnapshot],
    schemas: &[crate::ToolSchemaSnapshotV1],
) -> Result<(), ToolRegistryContractError> {
    if !catalog_entries.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id) {
        return Err(ToolRegistryContractError::InvalidEntryOrder);
    }
    if !effective_bindings.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id) {
        return Err(ToolRegistryContractError::InvalidBindingOrder);
    }
    if effective_bindings.len() != schemas.len() {
        return Err(ToolRegistryContractError::ExposureMismatch);
    }
    for (binding, schema) in effective_bindings.iter().zip(schemas) {
        if binding.tool_id != schema.tool_id || binding.tool_version != schema.tool_version {
            return Err(ToolRegistryContractError::ExposureMismatch);
        }
        let Some(entry) = catalog_entries.iter().find(|entry| entry.tool_id == binding.tool_id)
        else {
            return Err(ToolRegistryContractError::ExposureMismatch);
        };
        if entry.tool_version != binding.tool_version
            || entry.tool_instance_id != binding.tool_instance_id
            || entry.definition_fingerprint != binding.definition_fingerprint
            || entry.source_contract_fingerprint != binding.source_contract_fingerprint
            || entry.load_state == ToolLoadStateV1::Discoverable
            || binding.exposure == ToolExposureV1::UnifiedEntry
        {
            return Err(ToolRegistryContractError::ExposureMismatch);
        }
    }
    Ok(())
}

/// A tool callable through `tools.call` is a discoverable catalog entry of this snapshot,
/// bound exactly as the catalog freezes it, never one of the tools sent with the request.
fn validate_entry_bindings(
    catalog_entries: &[ToolRegistryCatalogEntryV1],
    entry_bindings: &[EffectiveToolBindingSnapshot],
) -> Result<(), ToolRegistryContractError> {
    if !entry_bindings.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id) {
        return Err(ToolRegistryContractError::InvalidBindingOrder);
    }
    for binding in entry_bindings {
        let Some(entry) = catalog_entries.iter().find(|entry| entry.tool_id == binding.tool_id)
        else {
            return Err(ToolRegistryContractError::ExposureMismatch);
        };
        if entry.tool_version != binding.tool_version
            || entry.tool_instance_id != binding.tool_instance_id
            || entry.source != binding.source_kind
            || entry.definition_fingerprint != binding.definition_fingerprint
            || entry.source_contract_fingerprint != binding.source_contract_fingerprint
            || entry.load_state != ToolLoadStateV1::Discoverable
            || binding.exposure != ToolExposureV1::UnifiedEntry
            || !crate::entry_callable(entry.canonical_name.as_str())
        {
            return Err(ToolRegistryContractError::ExposureMismatch);
        }
    }
    Ok(())
}

#[derive(Clone, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolSearchQueryV1(#[schemars(with = "String")] String);

impl ToolSearchQueryV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRegistryContractError> {
        let value = value.into().replace("\r\n", "\n").replace('\r', "\n");
        let valid = !value.is_empty()
            && value.len() <= MAX_TOOL_SEARCH_QUERY_BYTES
            && !value.chars().all(char::is_whitespace)
            && !value.chars().any(|value| value == '\0' || value == '\u{007f}');
        valid.then_some(Self(value)).ok_or(ToolRegistryContractError::InvalidSearchQuery)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ToolSearchQueryV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolSearchQueryV1").field(&self.0).finish()
    }
}

impl FromStr for ToolSearchQueryV1 {
    type Err = ToolRegistryContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ToolSearchQueryV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ToolSearchCursorV1(#[schemars(with = "String")] String);

impl ToolSearchCursorV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ToolRegistryContractError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 4_096
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        valid.then_some(Self(value)).ok_or(ToolRegistryContractError::InvalidCursor)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ToolSearchCursorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ToolSearchCursorV1").field(&"<opaque>").finish()
    }
}

impl<'de> Deserialize<'de> for ToolSearchCursorV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSearchRequestV1 {
    pub query: ToolSearchQueryV1,
    #[schemars(range(min = 1, max = 50))]
    pub limit: u8,
    pub cursor: Option<ToolSearchCursorV1>,
}

impl ToolSearchRequestV1 {
    pub fn validate(&self) -> Result<(), ToolRegistryContractError> {
        if self.limit == 0 || self.limit > MAX_TOOL_SEARCH_PAGE_SIZE {
            return Err(ToolRegistryContractError::InvalidSearchLimit);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolSearchRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            query: ToolSearchQueryV1,
            limit: u8,
            cursor: Option<ToolSearchCursorV1>,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self { query: wire.query, limit: wire.limit, cursor: wire.cursor };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSearchHitV1 {
    pub tool_id: ToolId,
    pub tool_version: ToolVersion,
    pub canonical_name: String,
    pub display_name: String,
    pub summary: String,
    pub source: ToolSourceKindV1,
    pub load_state: ToolLoadStateV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSearchPageV1 {
    pub registry_snapshot: ToolRegistrySnapshotRef,
    pub results: Vec<ToolSearchHitV1>,
    pub next_cursor: Option<ToolSearchCursorV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescribeRequestV1 {
    pub definition_ref: ToolDefinitionSnapshotRef,
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolLoadRequestV1 {
    #[schemars(length(min = 1, max = 50))]
    pub definitions: Vec<ToolDefinitionSnapshotRef>,
}

impl ToolLoadRequestV1 {
    pub fn validate(&self) -> Result<(), ToolRegistryContractError> {
        if self.definitions.is_empty()
            || self.definitions.len() > usize::from(MAX_TOOL_SEARCH_PAGE_SIZE)
            || !self.definitions.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id)
        {
            return Err(ToolRegistryContractError::InvalidDefinitionRefs);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolLoadRequestV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            definitions: Vec<ToolDefinitionSnapshotRef>,
        }

        let value = Self { definitions: Wire::deserialize(deserializer)?.definitions };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolLoadResultV1 {
    pub changed: bool,
    pub resulting_registry_snapshot_ref: ToolRegistrySnapshotRef,
}

pub type ToolUnloadRequestV1 = ToolLoadRequestV1;
pub type ToolUnloadResultV1 = ToolLoadResultV1;

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MAX_CONCURRENT_TOOL_CALLS, DEFAULT_MAX_TOOL_RETRIES, DEFAULT_TOOL_TIMEOUT_MS,
        ToolRegistryContractError, ToolSearchQueryV1, ToolSearchRequestV1, ToolSettingsSnapshotV1,
    };

    #[test]
    fn frozen_tool_setting_defaults_and_ranges_validate() {
        let settings = ToolSettingsSnapshotV1::default();
        assert_eq!(settings.max_concurrent_tool_calls, DEFAULT_MAX_CONCURRENT_TOOL_CALLS);
        assert_eq!(settings.default_timeout_ms, DEFAULT_TOOL_TIMEOUT_MS);
        assert_eq!(settings.max_retries, DEFAULT_MAX_TOOL_RETRIES);
        settings.validate().expect("defaults");
    }

    #[test]
    fn search_requires_query_and_explicit_bounded_limit() {
        assert!(ToolSearchQueryV1::new(" \t ").is_err());
        let request = ToolSearchRequestV1 {
            query: ToolSearchQueryV1::new("read file").expect("query"),
            limit: 0,
            cursor: None,
        };
        assert_eq!(request.validate(), Err(ToolRegistryContractError::InvalidSearchLimit));
        assert!(
            serde_json::from_value::<ToolSearchRequestV1>(serde_json::json!({
                "query": "read file",
                "limit": 51,
                "cursor": null
            }))
            .is_err()
        );
    }

    #[test]
    fn load_rejects_an_empty_batch_before_registry_work() {
        assert!(
            serde_json::from_value::<super::ToolLoadRequestV1>(serde_json::json!({
                "definitions": []
            }))
            .is_err()
        );
    }
}
