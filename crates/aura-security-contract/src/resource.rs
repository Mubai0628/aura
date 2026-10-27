use crate::{CanonicalPath, EnvironmentVariableScope, PrivateNetworkClass};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    ExternalGrantId, FileIdentity, FileReferenceId, SessionId, TurnTaskId, WorkspaceId,
    WorkspacePath,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Resolved policy resource. Opaque domain resources identify exact immutable
/// content; they grant no filesystem access without the owning domain resolver.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "scope_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceDescriptor {
    WorkspaceRoot {
        workspace_id: WorkspaceId,
    },
    WorkspacePath {
        workspace_id: WorkspaceId,
        canonical_relative_path: WorkspacePath,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_target: Option<crate::FileConsentTarget>,
    },
    SessionAuthorizedFile {
        file_reference_id: FileReferenceId,
    },
    /// An ordinary external path.
    ///
    /// `identity` is the object classification verified. When the path did not exist yet,
    /// `identity` is absent and `parent_identity` is the verified directory the new file will
    /// appear in — which is what the user's approval is about in that case, and what a create
    /// re-checks before it publishes.
    ExternalPath {
        canonical_path: CanonicalPath,
        identity: Option<FileIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_identity: Option<FileIdentity>,
        /// True when this object is a directory whose contents are part of the frozen scope.
        /// Absent on older operations and on single-file operations, so their fingerprints
        /// stay unchanged. A directory with `recursive: false` covers that directory and its
        /// direct children; `recursive: true` covers the approved subtree. The model's
        /// recursive argument is only a request until this fact is what the user approves.
        #[serde(default, skip_serializing_if = "is_false")]
        directory: bool,
        recursive: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_target: Option<crate::FileConsentTarget>,
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
    /// A system location. The identity is the object the classification verified, so the
    /// approved object is the one execution may act on: a same-named replacement is a
    /// different object and is refused. `parent_identity` carries the verified parent for the
    /// create case, exactly as it does for an ordinary path.
    SystemPath {
        canonical_path: CanonicalPath,
        /// Unbound when classification did not verify an object, including every operation
        /// stored before identity binding existed. Absent and explicit JSON null both mean
        /// unbound and are omitted from canonical JSON, so those fingerprints still verify.
        /// A verified identity is serialized and is part of the fingerprint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<FileIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_identity: Option<FileIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_target: Option<crate::FileConsentTarget>,
    },
    /// One of the user's own credential or browser-data locations, carried exactly as a
    /// system location is: the display path is for the user, the identity is for execution.
    SensitiveUserPath {
        canonical_path: CanonicalPath,
        /// Same canonical rule as [`ResourceDescriptor::SystemPath`]: an unbound identity is
        /// omitted, and a verified identity is bound into the operation fingerprint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<FileIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_identity: Option<FileIdentity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_target: Option<crate::FileConsentTarget>,
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
    MemoryVault {
        scope: crate::MemoryVaultScope,
        vault_identity: aura_domain_common::ContentHash,
    },
    GitRemote {
        remote: String,
    },
    GitRemoteDestination {
        facts: crate::GitRemoteFactsV1,
    },
    BrowserPage {
        page_ref: String,
    },
    /// The site a browser page is on, and the user's rule that lets a ReadOnly session open
    /// it without asking (none when no rule covers it).
    BrowserSite {
        host: String,
        allowed_by_rule: Option<String>,
    },
    PackageRuntime {
        resource_fingerprint: CanonicalSha256HashV1,
    },
    /// An installed V2 tool package this call runs. Core derives it from the
    /// installed, trusted version; a package session consent binds exactly this.
    ToolPackage {
        package_id: aura_domain_common::ToolPackageId,
        package_fingerprint: CanonicalSha256HashV1,
        runtime: crate::PackageRuntimeClass,
    },
    ToolConformanceRuntime {
        resource_fingerprint: CanonicalSha256HashV1,
    },
    /// A trusted MCP server, at the configuration fingerprint the user trusted.
    McpServer {
        server_id: String,
        config_fingerprint: aura_domain_common::ContentHash,
    },
    ToolRegistrySnapshot {
        resource_fingerprint: CanonicalSha256HashV1,
    },
    TurnTaskToolExposure {
        session_id: SessionId,
        turn_task_id: TurnTaskId,
        agent_run_id: aura_domain_common::AgentRunId,
    },
    AgentTask {
        session_id: SessionId,
        turn_task_id: TurnTaskId,
        agent_run_id: aura_domain_common::AgentRunId,
    },
    /// A tab the user shared from their own Chrome.
    ChromeShare {
        share_id: String,
    },
    /// The shared tab this call acts on.
    ChromeTab {
        tab_ref: String,
    },
    /// Where that tab is. `confirmed` is false until the user keeps sharing this origin.
    ChromeOrigin {
        origin: String,
        confirmed: bool,
    },
}

fn is_false(value: &bool) -> bool {
    !*value
}
