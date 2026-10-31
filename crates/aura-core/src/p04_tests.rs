//! p04's tests that need a started Core: publication recovery after a real termination, the
//! waits of a publication, the host preparation plan and the permission defaults. They read
//! p04's names through `use super::*`, as they did inside its module; these imports give them
//! the same names, p04's items and the ones p04 itself imported.
use aura_artifact_contract::ArtifactAccess;
use aura_domain_common::ContentHash;
use aura_domain_common::CorrelationId;
use aura_domain_common::SecurityOperationId;
use aura_domain_common::SessionId;
use aura_domain_common::UtcTimestamp;
use aura_domain_common::WorkspaceId;
use aura_domain_common::WorkspacePath;
use aura_ipc::method;
use aura_permission_domain::ExternalGrantCapability;
use aura_permission_domain::PrepareQuarantineReleaseInput;
use aura_permission_domain::QuarantineReleaseTarget;
use aura_permission_domain::ReleaseQuarantineInput;
use aura_persistence::ArtifactOverwriteJournalState;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::ApprovalDecisionKind;
use aura_security_service::p04::*;
use aura_windows_platform::capture_null_device_security_descriptor;
use aura_windows_platform::current_process_is_elevated;
use aura_windows_platform::installation_identity::probe_null_device_readiness;
use aura_windows_platform::probe_host_compatibility;
use aura_workspace::domain_identity;
use aura_workspace_domain::ArtifactExportRequest;
use aura_workspace_domain::ArtifactExportTarget;
use aura_workspace_domain::FsExpectedState;
use serde_json::Value;
use sha2::Digest as _;
use sha2::Sha256;
use std::path::PathBuf;

mod permission_defaults;
#[cfg(test)]
mod publication_recovery_tests;
mod tests;
