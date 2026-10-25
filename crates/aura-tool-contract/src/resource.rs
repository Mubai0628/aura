use std::{borrow::Cow, fmt, str::FromStr};

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

use aura_domain_common::jcs::CanonicalHashV1;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("resource claim argument selector must be a canonical JSON Pointer")]
pub struct ResourceClaimSelectorError;

pub const MAX_PACKAGE_RESOURCE_CLAIM_KEY_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ArgumentJsonPointer(String);

impl ArgumentJsonPointer {
    pub fn new(value: impl Into<String>) -> Result<Self, ResourceClaimSelectorError> {
        let value = value.into();
        let valid = value.len() <= 4_096
            && (value.is_empty() || value.starts_with('/'))
            && !value.chars().any(|character| character.is_control() || character == '\u{007f}')
            && value.split('/').skip(1).all(valid_json_pointer_segment);
        valid.then_some(Self(value)).ok_or(ResourceClaimSelectorError)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for ArgumentJsonPointer {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ArgumentJsonPointer")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "maxLength": 4096,
            "pattern": "^(?:|(?:/(?:[^~]|~[01])*)*)$"
        })
    }
}

impl fmt::Display for ArgumentJsonPointer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ArgumentJsonPointer {
    type Err = ResourceClaimSelectorError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for ArgumentJsonPointer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

fn valid_json_pointer_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    while let Some(character) = chars.next() {
        if character == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct PackageResourceClaimKeyV1(String);

impl PackageResourceClaimKeyV1 {
    pub fn new(value: impl Into<String>) -> Result<Self, ResourceClaimSelectorError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_PACKAGE_RESOURCE_CLAIM_KEY_BYTES
            && value.is_ascii()
            && valid_external_key(&value);
        valid.then_some(Self(value)).ok_or(ResourceClaimSelectorError)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackageResourceClaimKeyV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

impl JsonSchema for PackageResourceClaimKeyV1 {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("PackageResourceClaimKeyV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
            "pattern": "^[a-z0-9]+(?:[-_][a-z0-9]+)*$"
        })
    }
}

fn valid_external_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
        && !bytes
            .windows(2)
            .any(|pair| matches!(pair[0], b'_' | b'-') && matches!(pair[1], b'_' | b'-'))
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourceInvocationContextV1 {
    WorkspaceRoot,
    PackageRoot,
    Session,
    TurnTask,
    AgentRun,
    ToolCall,
    AttemptStaging,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceClaimSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer },
    InvocationContext { context: ResourceInvocationContextV1 },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FileResourceAccessV1 {
    Read,
    Write,
    Create,
    Delete,
    Execute,
    Observe,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NetworkResourceAccessV1 {
    Read,
    Write,
    Observe,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ProcessResourceAccessV1 {
    Execute,
    Observe,
    Control,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TerminalResourceAccessV1 {
    Observe,
    Control,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryResourceAccessV1 {
    Read,
    Write,
    Observe,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ReadWriteResourceAccessV1 {
    Read,
    Write,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum GitRefResourceAccessV1 {
    Read,
    Write,
    Create,
    Delete,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum GitSequencerResourceAccessV1 {
    Observe,
    Control,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactResourceAccessV1 {
    Read,
    Create,
    Delete,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackagePathScopeV1 {
    Exact,
    Subtree,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageWorkspaceRootPathScopeV1 {
    Subtree,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageWorkspacePathSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer, path_scope: PackagePathScopeV1 },
    WorkspaceRoot { path_scope: PackageWorkspaceRootPathScopeV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageExternalFileSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer, path_scope: PackagePathScopeV1 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageNetworkTargetSelectorV1 {
    RuntimeHttpTarget,
    ArgumentJsonPointer { pointer: ArgumentJsonPointer },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageProcessSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageTerminalSessionSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageGitRepositorySelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer },
    WorkspaceRoot,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageGitRefSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PackageAttemptStagingArtifactAccessV1 {
    Create,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageArtifactSelectorV1 {
    ArgumentJsonPointer { pointer: ArgumentJsonPointer, access: ArtifactResourceAccessV1 },
    AttemptStaging { access: PackageAttemptStagingArtifactAccessV1 },
}

/// Package-authored resource requirements. Every variant exposes only the
/// selectors and invocation contexts frozen for that resource class.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageDeclaredResourceClaimV1 {
    WorkspacePath {
        claim_key: PackageResourceClaimKeyV1,
        selector: PackageWorkspacePathSelectorV1,
        access: FileResourceAccessV1,
    },
    ExternalFile {
        claim_key: PackageResourceClaimKeyV1,
        selector: PackageExternalFileSelectorV1,
        access: FileResourceAccessV1,
    },
    NetworkTarget {
        claim_key: PackageResourceClaimKeyV1,
        selector: PackageNetworkTargetSelectorV1,
        access: NetworkResourceAccessV1,
    },
    Process {
        claim_key: PackageResourceClaimKeyV1,
        selector: PackageProcessSelectorV1,
        access: ProcessResourceAccessV1,
    },
    TerminalSession {
        claim_key: PackageResourceClaimKeyV1,
        selector: PackageTerminalSessionSelectorV1,
        access: TerminalResourceAccessV1,
    },
    GitRepository {
        claim_key: PackageResourceClaimKeyV1,
        repository_selector: PackageGitRepositorySelectorV1,
        access: RepositoryResourceAccessV1,
    },
    GitIndex {
        claim_key: PackageResourceClaimKeyV1,
        repository_selector: PackageGitRepositorySelectorV1,
        access: ReadWriteResourceAccessV1,
    },
    GitRef {
        claim_key: PackageResourceClaimKeyV1,
        repository_selector: PackageGitRepositorySelectorV1,
        ref_selector: PackageGitRefSelectorV1,
        access: GitRefResourceAccessV1,
    },
    GitSequencer {
        claim_key: PackageResourceClaimKeyV1,
        repository_selector: PackageGitRepositorySelectorV1,
        access: GitSequencerResourceAccessV1,
    },
    Artifact {
        claim_key: PackageResourceClaimKeyV1,
        selector: PackageArtifactSelectorV1,
    },
}

impl PackageDeclaredResourceClaimV1 {
    #[must_use]
    pub const fn claim_key(&self) -> &PackageResourceClaimKeyV1 {
        match self {
            Self::WorkspacePath { claim_key, .. }
            | Self::ExternalFile { claim_key, .. }
            | Self::NetworkTarget { claim_key, .. }
            | Self::Process { claim_key, .. }
            | Self::TerminalSession { claim_key, .. }
            | Self::GitRepository { claim_key, .. }
            | Self::GitIndex { claim_key, .. }
            | Self::GitRef { claim_key, .. }
            | Self::GitSequencer { claim_key, .. }
            | Self::Artifact { claim_key, .. } => claim_key,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceClaimV1 {
    WorkspacePath { selector: ResourceClaimSelectorV1, access: FileResourceAccessV1 },
    ExternalFile { selector: ResourceClaimSelectorV1, access: FileResourceAccessV1 },
    NetworkTarget { selector: ResourceClaimSelectorV1, access: NetworkResourceAccessV1 },
    Process { selector: ResourceClaimSelectorV1, access: ProcessResourceAccessV1 },
    TerminalSession { selector: ResourceClaimSelectorV1, access: TerminalResourceAccessV1 },
    GitRepository { selector: ResourceClaimSelectorV1, access: RepositoryResourceAccessV1 },
    GitIndex { selector: ResourceClaimSelectorV1, access: ReadWriteResourceAccessV1 },
    GitRef { selector: ResourceClaimSelectorV1, access: GitRefResourceAccessV1 },
    GitSequencer { selector: ResourceClaimSelectorV1, access: GitSequencerResourceAccessV1 },
    Artifact { selector: ResourceClaimSelectorV1, access: ArtifactResourceAccessV1 },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedResourceKindV1 {
    MemoryVault,
    WorkspacePath,
    ExternalFile,
    NetworkTarget,
    Process,
    TerminalSession,
    GitRepository,
    GitIndex,
    GitRef,
    GitSequencer,
    Artifact,
    WorkspaceMutationOwnership,
    ExternalSourceInstance,
    AgentTaskState,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedResourceAccessV1 {
    Read,
    Write,
    Create,
    Delete,
    Execute,
    Observe,
    Control,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedResourceClaimV1 {
    pub kind: ResolvedResourceKindV1,
    pub access: ResolvedResourceAccessV1,
    pub canonical_resource_identity: String,
    pub canonical_lock_key: String,
    pub resolution_evidence_hash: CanonicalHashV1,
    pub core_derived: bool,
}

impl ResolvedResourceClaimV1 {
    #[must_use]
    pub fn is_workspace_mutation_ownership(&self) -> bool {
        self.kind == ResolvedResourceKindV1::WorkspaceMutationOwnership
            && self.access == ResolvedResourceAccessV1::Control
            && self.core_derived
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ArgumentJsonPointer, FileResourceAccessV1, NetworkResourceAccessV1,
        PackageDeclaredResourceClaimV1, ResourceClaimSelectorV1, ResourceClaimV1,
    };

    #[test]
    fn claim_wire_schema_cannot_express_illegal_access_for_variant() {
        let selector = ResourceClaimSelectorV1::ArgumentJsonPointer {
            pointer: "/path".parse().expect("pointer"),
        };
        let claim =
            ResourceClaimV1::WorkspacePath { selector, access: FileResourceAccessV1::Execute };
        assert_eq!(serde_json::to_value(claim).expect("wire")["access"], json!("execute"));
        assert!(
            serde_json::from_value::<ResourceClaimV1>(json!({
                "type": "network_target",
                "selector": {"type": "invocation_context", "context": "tool_call"},
                "access": "execute"
            }))
            .is_err()
        );
        let _ = NetworkResourceAccessV1::Read;
    }

    #[test]
    fn package_claims_expose_only_resource_specific_selectors() {
        let process_with_workspace_root = json!({
            "type": "process",
            "claim_key": "target_process",
            "selector": { "type": "workspace_root" },
            "access": "execute"
        });
        assert!(
            serde_json::from_value::<PackageDeclaredResourceClaimV1>(process_with_workspace_root)
                .is_err()
        );

        let exact_workspace_root = json!({
            "type": "workspace_path",
            "claim_key": "workspace",
            "selector": { "type": "workspace_root", "path_scope": "exact" },
            "access": "read"
        });
        assert!(
            serde_json::from_value::<PackageDeclaredResourceClaimV1>(exact_workspace_root).is_err()
        );

        ArgumentJsonPointer::new("").expect("root pointer is explicitly valid");
        assert!(ArgumentJsonPointer::new("not/a/pointer").is_err());
        assert!(ArgumentJsonPointer::new("/~2").is_err());
    }
}
