//! Session file approval binds verified objects and native directory entries.
//! Mode changes and Core restarts do not withdraw explicit file approval.
use crate::{OperationFingerprint, ResolvedCapability, ResourceDescriptor};
use aura_domain_common::{ApprovalRequestId, FileIdentity, PrincipalId, Revision, SessionId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum FileConsentProtection {
    #[default]
    Ordinary,
    System,
    SensitiveUser,
    ControlAuthority,
}

#[derive(
    Clone, Debug, Default, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(deny_unknown_fields)]
pub struct FileConsentTarget {
    /// Display only. Equality and containment never fold this string.
    pub canonical_path: String,
    pub recursive: bool,
    /// Verified object identity or verified parent + native entry key for a create.
    #[serde(default)]
    pub identity_key: String,
    /// The directory entry, used only when consuming its own publication receipt.
    #[serde(default)]
    pub entry_key: String,
    /// Verified directory identities, outermost first, including the immediate parent.
    #[serde(default)]
    pub ancestors: Vec<String>,
    /// Recursive ordinary approval cannot inherit a protected descendant.
    #[serde(default)]
    pub protection: FileConsentProtection,
}

#[must_use]
pub fn file_object_key(identity: &FileIdentity) -> String {
    format!("object:{}:{}", identity.volume_serial_number, identity.file_id)
}

#[must_use]
pub fn file_entry_key(parent: &FileIdentity, native_name_key: &str) -> String {
    format!("entry:{}:{}:{}", parent.volume_serial_number, parent.file_id, native_name_key)
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileSessionConsentRef {
    pub approval_id: ApprovalRequestId,
    pub session_id: SessionId,
    pub principal_id: PrincipalId,
    pub capabilities: Vec<ResolvedCapability>,
    pub targets: Vec<FileConsentTarget>,
    /// Original approval audit identity, never rewritten on publication.
    pub approved_operation_fingerprint: OperationFingerprint,
    pub grant_revision: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_grant_revision: Option<Revision>,
}

impl FileSessionConsentRef {
    #[must_use]
    pub fn accepts_grant_revision(&self, revision: Revision) -> bool {
        revision == self.grant_revision || Some(revision) == self.revoked_grant_revision
    }
}

/// Unbound historical operations can only receive exact-operation approvals.
#[must_use]
pub fn file_consent_target_of(resource: &ResourceDescriptor) -> Option<FileConsentTarget> {
    match resource {
        ResourceDescriptor::WorkspacePath { file_target, .. }
        | ResourceDescriptor::ExternalPath { file_target, .. } => {
            file_target.clone().filter(|target| !target.identity_key.is_empty())
        }
        ResourceDescriptor::SystemPath { file_target, .. }
        | ResourceDescriptor::SensitiveUserPath { file_target, .. } => {
            let mut target =
                file_target.clone().filter(|target| !target.identity_key.is_empty())?;
            if target.protection != FileConsentProtection::ControlAuthority {
                target.protection = if matches!(resource, ResourceDescriptor::SystemPath { .. }) {
                    FileConsentProtection::System
                } else {
                    FileConsentProtection::SensitiveUser
                };
            }
            Some(target)
        }
        _ => None,
    }
}

#[must_use]
pub fn file_consent_target_covers(
    approved: &FileConsentTarget,
    candidate: &FileConsentTarget,
) -> bool {
    if approved.identity_key.is_empty()
        || candidate.identity_key.is_empty()
        || approved.protection != candidate.protection
    {
        return false;
    }
    approved.identity_key == candidate.identity_key
        || approved.recursive && candidate.ancestors.contains(&approved.identity_key)
}

#[must_use]
pub fn file_capabilities_are_consentable(capabilities: &[ResolvedCapability]) -> bool {
    !capabilities.is_empty()
        && capabilities.iter().all(|capability| {
            matches!(
                capability,
                ResolvedCapability::WorkspaceFileRead
                    | ResolvedCapability::WorkspaceFileWrite
                    | ResolvedCapability::WorkspaceFileCreate
                    | ResolvedCapability::WorkspaceFileDelete
                    | ResolvedCapability::WorkspaceFileMove
                    | ResolvedCapability::ExternalFileRead
                    | ResolvedCapability::ExternalFileWrite
                    | ResolvedCapability::ExternalFileCreate
                    | ResolvedCapability::ExternalFileDelete
            )
        })
}

/// File "write" scope includes ordinary create, overwrite, delete and rename.
/// Read consent stays read-only and no file consent carries network authority.
#[must_use]
pub fn file_consent_capabilities(capabilities: &[ResolvedCapability]) -> Vec<ResolvedCapability> {
    use ResolvedCapability::*;
    let mut result = capabilities.to_vec();
    if capabilities.iter().any(|c| {
        matches!(
            c,
            WorkspaceFileWrite | WorkspaceFileCreate | WorkspaceFileDelete | WorkspaceFileMove
        )
    }) {
        result.extend([
            WorkspaceFileWrite,
            WorkspaceFileCreate,
            WorkspaceFileDelete,
            WorkspaceFileMove,
        ]);
    }
    if capabilities
        .iter()
        .any(|c| matches!(c, ExternalFileWrite | ExternalFileCreate | ExternalFileDelete))
    {
        result.extend([ExternalFileWrite, ExternalFileCreate, ExternalFileDelete]);
    }
    result.sort();
    result.dedup();
    result
}
