use crate::ResolvedScope;
use aura_domain_common::jcs::canonical_sha256;
use aura_security_contract::ResourceDescriptor;

impl ResolvedScope {
    /// Adapts immutable domain resources for policy comparison. The complete
    /// original scope remains available for dispatch-time resolution.
    #[must_use]
    pub fn resource_descriptor(&self) -> ResourceDescriptor {
        match self {
            Self::WorkspaceRoot { workspace_id } => {
                ResourceDescriptor::WorkspaceRoot { workspace_id: *workspace_id }
            }
            Self::WorkspacePath { workspace_id, canonical_relative_path } => {
                ResourceDescriptor::WorkspacePath {
                    workspace_id: *workspace_id,
                    canonical_relative_path: canonical_relative_path.clone(),
                    file_target: None,
                }
            }
            Self::SessionAuthorizedFile { file_reference_id } => {
                ResourceDescriptor::SessionAuthorizedFile { file_reference_id: *file_reference_id }
            }
            Self::ExternalPath { canonical_path, identity, recursive } => {
                ResourceDescriptor::ExternalPath {
                    canonical_path: canonical_path.clone(),
                    identity: identity.clone(),
                    // A resolved scope describes an object that was resolved, so there is no
                    // create case here and no parent to carry.
                    parent_identity: None,
                    directory: *recursive,
                    recursive: *recursive,
                    file_target: None,
                }
            }
            Self::ExternalGrant { grant_id } => {
                ResourceDescriptor::ExternalGrant { grant_id: *grant_id }
            }
            Self::NetworkOrigin { scheme, host, port } => ResourceDescriptor::NetworkOrigin {
                scheme: scheme.clone(),
                host: host.clone(),
                port: *port,
            },
            Self::PrivateNetwork { class } => ResourceDescriptor::PrivateNetwork { class: *class },
            Self::ExternalService { service_id, object_ref } => {
                ResourceDescriptor::ExternalService {
                    service_id: service_id.clone(),
                    object_ref: object_ref.clone(),
                }
            }
            // A resolved scope for a protected location keeps only its path, so it adapts to a
            // descriptor without an identity. The identity a descriptor carries comes from
            // classification, which inspects the path; this adapter must not invent one.
            Self::SystemPath { canonical_path } => ResourceDescriptor::SystemPath {
                canonical_path: canonical_path.clone(),
                identity: None,
                parent_identity: None,
                file_target: None,
            },
            Self::SensitiveUserPath { canonical_path } => ResourceDescriptor::SensitiveUserPath {
                canonical_path: canonical_path.clone(),
                identity: None,
                parent_identity: None,
                file_target: None,
            },
            Self::RegistryKey { hive, path } => {
                ResourceDescriptor::RegistryKey { hive: hive.clone(), path: path.clone() }
            }
            Self::EnvironmentScope { scope } => {
                ResourceDescriptor::EnvironmentScope { scope: *scope }
            }
            Self::GitRepository { workspace_id } => {
                ResourceDescriptor::GitRepository { workspace_id: *workspace_id }
            }
            Self::GitRemote { remote } => ResourceDescriptor::GitRemote { remote: remote.clone() },
            Self::BrowserPage { page_ref } => {
                ResourceDescriptor::BrowserPage { page_ref: page_ref.clone() }
            }
            Self::PackageRuntime { .. } => ResourceDescriptor::PackageRuntime {
                resource_fingerprint: canonical_sha256(self)
                    .expect("resolved scope is canonical JSON"),
            },
            Self::ToolConformanceRuntime { .. } => ResourceDescriptor::ToolConformanceRuntime {
                resource_fingerprint: canonical_sha256(self)
                    .expect("resolved scope is canonical JSON"),
            },
            Self::ToolRegistrySnapshot { .. } => ResourceDescriptor::ToolRegistrySnapshot {
                resource_fingerprint: canonical_sha256(self)
                    .expect("resolved scope is canonical JSON"),
            },
            Self::TurnTaskToolExposure { session_id, turn_task_id, agent_run_id } => {
                ResourceDescriptor::TurnTaskToolExposure {
                    session_id: *session_id,
                    turn_task_id: *turn_task_id,
                    agent_run_id: *agent_run_id,
                }
            }
        }
    }
}
