//! What Core was built and released as: its release descriptor, component and commit, and the
//! identities of the elevated helper and the two security sidecars it may start.
use aura_domain_common::{ContentHash, ContractVersion};
use aura_runtime_contract::{ReleaseDescriptor, RuntimeProcessKind};
use std::path::PathBuf;

#[derive(Clone)]
pub struct CoreReleaseContext {
    pub release_descriptor: ReleaseDescriptor,
    pub component_version: String,
    pub build_commit: String,
    pub manifest_path: PathBuf,
    pub elevated_helper_executable: PathBuf,
    pub elevated_helper_hash: ContentHash,
    pub sandbox_host: SidecarReleaseIdentity,
    pub egress_proxy: SidecarReleaseIdentity,
}

#[derive(Clone)]
pub struct SidecarReleaseIdentity {
    pub process_kind: RuntimeProcessKind,
    pub component_version: String,
    pub executable: PathBuf,
    pub content_hash: ContentHash,
    pub protocol_version: Option<ContractVersion>,
}
