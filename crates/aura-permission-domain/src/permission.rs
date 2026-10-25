use aura_domain_common::CanonicalSemVer;
use aura_domain_common::ToolPackageId;
use aura_domain_common::{
    ContentHash, ExternalGrantId, FileIdentity, FileReferenceId, PermissionGrantId, SessionId,
    TurnTaskId, UtcTimestamp, WorkspaceId, WorkspacePath,
};
use aura_security_contract::{
    CanonicalPath, EnvironmentVariableScope, OperationRiskFlag,
    PermissionAllowAuthorizationBasisV1, PermissionGrantLifetime, PermissionMode,
    PermissionSnapshot, PrivateNetworkClass, ResolvedCapability,
};
use aura_tool_contract::{PackageRuntimeClosureHashV1, ToolRegistrySnapshotRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ApprovalRequestDraft, SecuritySource};

/// Exact immutable Package Runtime resource authorized for one P07 Tool
/// operation. This scope grants access only to the verified full-package
/// Runtime Root identified by these three fields; it does not imply access to
/// a Workspace, external path, system path, network, or Credential.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRuntimeScopeV1 {
    pub tool_package_id: ToolPackageId,
    pub package_version: CanonicalSemVer,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolConformanceRuntimeScopeV1 {
    pub conformance: crate::ToolConformanceSecuritySourceRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "scope_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedScope {
    WorkspaceRoot {
        workspace_id: WorkspaceId,
    },
    WorkspacePath {
        workspace_id: WorkspaceId,
        canonical_relative_path: WorkspacePath,
    },
    SessionAuthorizedFile {
        file_reference_id: FileReferenceId,
    },
    ExternalPath {
        canonical_path: CanonicalPath,
        identity: Option<FileIdentity>,
        recursive: bool,
    },
    ExternalGrant {
        grant_id: ExternalGrantId,
    },
    NetworkOrigin {
        scheme: String,
        host: String,
        port: u16,
    },
    PrivateNetwork {
        class: PrivateNetworkClass,
    },
    ExternalService {
        service_id: String,
        object_ref: Option<String>,
    },
    SystemPath {
        canonical_path: CanonicalPath,
    },
    SensitiveUserPath {
        canonical_path: CanonicalPath,
    },
    RegistryKey {
        hive: String,
        path: String,
    },
    EnvironmentScope {
        scope: EnvironmentVariableScope,
    },
    GitRepository {
        workspace_id: WorkspaceId,
    },
    GitRemote {
        remote: String,
    },
    BrowserPage {
        page_ref: String,
    },
    PackageRuntime {
        package: PackageRuntimeScopeV1,
    },
    ToolConformanceRuntime {
        conformance: ToolConformanceRuntimeScopeV1,
    },
    ToolRegistrySnapshot {
        registry_snapshot_ref: ToolRegistrySnapshotRef,
    },
    TurnTaskToolExposure {
        session_id: SessionId,
        turn_task_id: TurnTaskId,
        agent_run_id: aura_domain_common::AgentRunId,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionEvaluationOutcomeV1 {
    pub decision: PermissionDecision,
    pub allow_authorization_basis: Option<PermissionAllowAuthorizationBasisV1>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionGrant {
    pub id: PermissionGrantId,
    pub lifetime: PermissionGrantLifetime,
    pub workspace_id: WorkspaceId,
    pub session_id: Option<SessionId>,
    pub turn_task_id: Option<TurnTaskId>,
    pub source: SecuritySource,
    pub source_fingerprint: Option<ContentHash>,
    pub capabilities: Vec<ResolvedCapability>,
    pub scopes: Vec<ResolvedScope>,
    pub approved_risk_flags: Vec<OperationRiskFlag>,
    pub operation_fingerprint: Option<ContentHash>,
    pub created_at: UtcTimestamp,
    pub expires_at: Option<UtcTimestamp>,
    pub revoked_at: Option<UtcTimestamp>,
    pub consumed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TurnDenyRecord {
    pub turn_task_id: TurnTaskId,
    pub source: SecuritySource,
    pub operation_fingerprint: ContentHash,
    pub denied_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDenyReason {
    ModeDenied,
    IdentityInvalid,
    PurposeRequired,
    SourceDisabled,
    SourceUntrusted,
    CapabilityUnresolved,
    ScopeUnresolved,
    HardPolicy,
    DeniedForCurrentTurn,
    SandboxUnavailable,
    NetworkPlanUnavailable,
    NetworkAccessDeclarationRequired,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum PermissionDecision {
    Allow { snapshot: PermissionSnapshot },
    Ask { request: Box<ApprovalRequestDraft> },
    Deny { reason: PermissionDenyReason },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GetSessionPermissionStateInput {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetSessionPermissionModeInput {
    pub session_id: SessionId,
    pub expected_revision: u64,
    pub mode: PermissionMode,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListPermissionGrantsInput {
    pub workspace_id: WorkspaceId,
    pub session_id: Option<SessionId>,
}

/// Defaults persisted in aura.toml. Existing session permissions are independent.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDefaults {
    pub revision: u64,
    pub mode: PermissionMode,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetPermissionDefaultsInput {
    pub expected_revision: u64,
    pub mode: PermissionMode,
}

#[cfg(test)]
mod tests {
    use aura_domain_common::CanonicalSemVer;
    use aura_domain_common::ToolPackageId;
    use aura_domain_common::jcs::CanonicalSha256HashV1;
    use aura_tool_contract::PackageRuntimeClosureHashV1;

    use super::ResolvedScope;

    #[test]
    fn package_runtime_scope_round_trips_and_rejects_unknown_fields() {
        let scope = ResolvedScope::PackageRuntime {
            package: super::PackageRuntimeScopeV1 {
                tool_package_id: ToolPackageId::new(),
                package_version: CanonicalSemVer::new("1.2.3").expect("version"),
                runtime_closure_hash: PackageRuntimeClosureHashV1(CanonicalSha256HashV1::sha256(
                    b"runtime closure",
                )),
            },
        };
        let value = serde_json::to_value(&scope).expect("serialize scope");
        let decoded: ResolvedScope = serde_json::from_value(value.clone()).expect("round trip");
        assert_eq!(decoded, scope);

        let mut invalid = value;
        invalid
            .get_mut("package")
            .and_then(serde_json::Value::as_object_mut)
            .expect("package object")
            .insert("tool_call_id".to_owned(), serde_json::json!("tcl_forbidden"));
        assert!(serde_json::from_value::<ResolvedScope>(invalid).is_err());
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokePermissionGrantInput {
    pub grant_id: PermissionGrantId,
}
