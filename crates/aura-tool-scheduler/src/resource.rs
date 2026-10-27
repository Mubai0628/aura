use std::collections::BTreeMap;
use std::path::PathBuf;

use aura_domain_common::{ContentHashAlgorithm, ErrorCode};
use aura_tool_contract::{
    CoreInvocationIdentityV1, NormalizedToolCallInputStateV1, ResolvedResourceAccessV1,
    ResolvedResourceClaimV1, ResolvedResourceKindV1, ResourceClaimV1, ToolNotExecutedReasonV1,
    ToolResourceErrorCode,
};
use serde_json::Value;
use thiserror::Error;

use crate::ValidatedToolCallV1;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPackageResourceFactV1 {
    pub claim_key: aura_tool_contract::PackageResourceClaimKeyV1,
    pub scheduler_claim: ResolvedResourceClaimV1,
    pub permission_scope: aura_permission_domain::ResolvedScope,
    pub risk_flags: Vec<aura_security_contract::OperationRiskFlag>,
    /// Present only when the exact claim is eligible as a declared Runtime
    /// working directory. It is Core-resolved and never Package-authored.
    pub resolved_directory: Option<PathBuf>,
}

pub trait PackageResourceClaimResolverV1: Send + Sync {
    /// Resolves one resource-specific Package claim through the owning
    /// P03/P09/P04 resolver. Implementations must not accept a Package-provided
    /// canonical path, lock key, or permission scope.
    fn resolve_package_claim(
        &self,
        claim: &aura_tool_contract::PackageDeclaredResourceClaimV1,
        arguments: &Value,
        invocation_identity: &CoreInvocationIdentityV1,
    ) -> Result<ResolvedPackageResourceFactV1, ResourceClaimResolutionIssueV1>;

    fn resolve_workspace_mutation_ownership(
        &self,
        invocation_identity: &CoreInvocationIdentityV1,
    ) -> Result<ResolvedResourceClaimV1, ResourceClaimResolutionIssueV1>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPackageResourceSetV1 {
    pub scheduler_claims: Vec<ResolvedResourceClaimV1>,
    pub permission_scopes: Vec<aura_permission_domain::ResolvedScope>,
    pub risk_flags: Vec<aura_security_contract::OperationRiskFlag>,
    pub resolved_directories: BTreeMap<aura_tool_contract::PackageResourceClaimKeyV1, PathBuf>,
}

/// Forms the exact P04 scope set for one Tool operation without widening or
/// otherwise merging distinct resources. The three inputs retain their
/// frozen provenance order: declared Claim scopes, Runtime-required scopes,
/// then scopes added by an authoritative resolver (for example private
/// network classification). Exact duplicates are removed; related paths,
/// recursive ranges, origins, and Package versions remain distinct.
#[must_use]
pub fn compose_tool_operation_scopes_v1(
    explicit_claim_scopes: &[aura_permission_domain::ResolvedScope],
    runtime_required_scopes: &[aura_permission_domain::ResolvedScope],
    resolver_additional_scopes: &[aura_permission_domain::ResolvedScope],
) -> Vec<aura_permission_domain::ResolvedScope> {
    let mut scopes = Vec::with_capacity(
        explicit_claim_scopes.len()
            + runtime_required_scopes.len()
            + resolver_additional_scopes.len(),
    );
    for scope in explicit_claim_scopes
        .iter()
        .chain(runtime_required_scopes)
        .chain(resolver_additional_scopes)
    {
        if !scopes.contains(scope) {
            scopes.push(scope.clone());
        }
    }
    scopes
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageResourcePreflightRejectionV1 {
    pub reason: ToolNotExecutedReasonV1,
    pub error_code: ErrorCode,
    pub claim_key: Option<aura_tool_contract::PackageResourceClaimKeyV1>,
}

pub fn resolve_package_resource_claims_v1(
    claims: &[aura_tool_contract::PackageDeclaredResourceClaimV1],
    arguments: &Value,
    invocation_identity: &CoreInvocationIdentityV1,
    resolver: &dyn PackageResourceClaimResolverV1,
) -> Result<ResolvedPackageResourceSetV1, PackageResourcePreflightRejectionV1> {
    let mut facts = Vec::with_capacity(claims.len());
    let mut needs_workspace_ownership = false;
    for declared in claims {
        let fact =
            resolver.resolve_package_claim(declared, arguments, invocation_identity).map_err(
                |issue| package_resource_rejection(issue, Some(declared.claim_key().clone())),
            )?;
        validate_resolved_package_fact(declared, &fact).map_err(|issue| {
            package_resource_rejection(issue, Some(declared.claim_key().clone()))
        })?;
        needs_workspace_ownership |= is_workspace_or_git_mutation(&fact.scheduler_claim);
        facts.push(fact);
    }

    let mut scheduler_claims =
        facts.iter().map(|fact| fact.scheduler_claim.clone()).collect::<Vec<_>>();
    if needs_workspace_ownership {
        let ownership = resolver
            .resolve_workspace_mutation_ownership(invocation_identity)
            .map_err(|issue| package_resource_rejection(issue, None))?;
        if !ownership.is_workspace_mutation_ownership() {
            return Err(package_resource_rejection(
                ResourceClaimResolutionIssueV1::ScopeResolutionFailed,
                None,
            ));
        }
        validate_resolved_common(&ownership)
            .map_err(|issue| package_resource_rejection(issue, None))?;
        scheduler_claims.push(ownership);
    }
    scheduler_claims.sort_by(|left, right| {
        left.canonical_lock_key.as_bytes().cmp(right.canonical_lock_key.as_bytes())
    });
    if scheduler_claims
        .windows(2)
        .any(|pair| pair[0].canonical_lock_key == pair[1].canonical_lock_key)
    {
        return Err(package_resource_rejection(
            ResourceClaimResolutionIssueV1::LockKeyInvalid,
            None,
        ));
    }

    let mut permission_scopes = Vec::with_capacity(facts.len());
    let mut risk_flags = Vec::new();
    let mut resolved_directories = BTreeMap::new();
    for fact in facts {
        permission_scopes.push(fact.permission_scope);
        risk_flags.extend(fact.risk_flags);
        if let Some(directory) = fact.resolved_directory
            && resolved_directories.insert(fact.claim_key, directory).is_some()
        {
            return Err(package_resource_rejection(
                ResourceClaimResolutionIssueV1::ScopeResolutionFailed,
                None,
            ));
        }
    }
    risk_flags.sort_by_key(|flag| risk_flag_order(*flag));
    risk_flags.dedup();
    Ok(ResolvedPackageResourceSetV1 {
        scheduler_claims,
        permission_scopes,
        risk_flags,
        resolved_directories,
    })
}

const fn risk_flag_order(flag: aura_security_contract::OperationRiskFlag) -> u8 {
    use aura_security_contract::OperationRiskFlag as Risk;
    match flag {
        Risk::UnknownSideEffect => 0,
        Risk::OutsideWorkspaceWrite => 1,
        Risk::OutsideWorkspaceDelete => 2,
        Risk::SensitivePathAccess => 3,
        Risk::DataLeavesDevice => 4,
        Risk::PrivateOrLoopbackNetwork => 5,
        Risk::ExecutableArtifact => 6,
        Risk::SoftwareInstallation => 7,
        Risk::SystemConfigurationMutation => 8,
        Risk::Elevation => 9,
        Risk::DriverOrServiceMutation => 10,
        Risk::DestructiveRemoteMutation => 11,
        Risk::SensitiveBrowserAction => 12,
        Risk::GitBareForcePush => 13,
        Risk::GitSequencerSkipOrQuit => 14,
        Risk::InlineScript => 15,
        Risk::UntrustedSource => 16,
        Risk::PermanentFileDeletion => 17,
        Risk::ChromeCredentialField => 18,
        Risk::ChromeCaptcha => 19,
        Risk::ChromePasskey => 20,
        Risk::ChromeFileChooser => 21,
    }
}

fn validate_resolved_package_fact(
    declared: &aura_tool_contract::PackageDeclaredResourceClaimV1,
    fact: &ResolvedPackageResourceFactV1,
) -> Result<(), ResourceClaimResolutionIssueV1> {
    validate_resolved_common(&fact.scheduler_claim)?;
    if fact.claim_key != *declared.claim_key() || fact.scheduler_claim.core_derived {
        return Err(ResourceClaimResolutionIssueV1::ScopeResolutionFailed);
    }
    let (expected_kind, expected_access) = package_declared_kind_and_access(declared);
    if fact.scheduler_claim.kind != expected_kind || fact.scheduler_claim.access != expected_access
    {
        return Err(ResourceClaimResolutionIssueV1::ScopeResolutionFailed);
    }
    Ok(())
}

fn package_resource_rejection(
    issue: ResourceClaimResolutionIssueV1,
    claim_key: Option<aura_tool_contract::PackageResourceClaimKeyV1>,
) -> PackageResourcePreflightRejectionV1 {
    PackageResourcePreflightRejectionV1 {
        reason: ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
        error_code: issue.error_code().error_code(),
        claim_key,
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ResourceClaimResolutionIssueV1 {
    #[error("the claim selector is missing")]
    SelectorMissing,
    #[error("the selected argument has the wrong type")]
    SelectorTypeMismatch,
    #[error("the selected resource cannot be canonicalized")]
    CanonicalizationFailed,
    #[error("the canonical resource identity cannot be resolved")]
    IdentityResolutionFailed,
    #[error("the resolved permission scope is invalid")]
    ScopeResolutionFailed,
    #[error("the canonical lock key is invalid")]
    LockKeyInvalid,
    #[error("the resource kind is unavailable in this phase")]
    UnsupportedClaim,
    #[error("the authoritative resource resolver is unavailable")]
    ResolverUnavailable,
}

impl ResourceClaimResolutionIssueV1 {
    #[must_use]
    pub const fn error_code(self) -> ToolResourceErrorCode {
        match self {
            Self::SelectorMissing => ToolResourceErrorCode::SelectorMissing,
            Self::SelectorTypeMismatch => ToolResourceErrorCode::SelectorTypeMismatch,
            Self::CanonicalizationFailed => ToolResourceErrorCode::CanonicalizationFailed,
            Self::IdentityResolutionFailed => ToolResourceErrorCode::IdentityResolutionFailed,
            Self::ScopeResolutionFailed => ToolResourceErrorCode::ScopeResolutionFailed,
            Self::LockKeyInvalid => ToolResourceErrorCode::LockKeyInvalid,
            Self::UnsupportedClaim => ToolResourceErrorCode::UnsupportedClaim,
            Self::ResolverUnavailable => ToolResourceErrorCode::ResolverUnavailable,
        }
    }
}

pub trait ResourceClaimResolverV1: Send + Sync {
    /// Implements the frozen P03/P09/P04 resolution sequence for one declared
    /// claim. The returned fact is verified again by P07 before scheduling.
    fn resolve_declared_claim(
        &self,
        claim: &ResourceClaimV1,
        arguments: &Value,
        invocation_identity: &CoreInvocationIdentityV1,
    ) -> Result<ResolvedResourceClaimV1, ResourceClaimResolutionIssueV1>;

    /// Core derives this additional claim for any real Workspace/Git
    /// mutation. A Package cannot provide it through its Manifest.
    fn resolve_workspace_mutation_ownership(
        &self,
        invocation_identity: &CoreInvocationIdentityV1,
    ) -> Result<ResolvedResourceClaimV1, ResourceClaimResolutionIssueV1>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePreflightRejectionV1 {
    pub reason: ToolNotExecutedReasonV1,
    pub error_code: ErrorCode,
    pub claim_index: Option<u32>,
}

pub fn resolve_tool_resource_claims(
    call: &ValidatedToolCallV1,
    invocation_identity: &CoreInvocationIdentityV1,
    resolver: &dyn ResourceClaimResolverV1,
) -> Result<Vec<ResolvedResourceClaimV1>, ResourcePreflightRejectionV1> {
    let NormalizedToolCallInputStateV1::Ready { arguments, .. } = &call.call.input else {
        return Err(resource_rejection(ResourceClaimResolutionIssueV1::SelectorMissing, None));
    };
    let mut resolved = Vec::with_capacity(call.definition.resource_claims.len() + 1);
    let mut needs_workspace_ownership = false;
    for (index, declared) in call.definition.resource_claims.iter().enumerate() {
        let claim = resolver
            .resolve_declared_claim(declared, arguments, invocation_identity)
            .map_err(|issue| resource_rejection(issue, u32::try_from(index).ok()))?;
        validate_resolved_declared_claim(declared, &claim)
            .map_err(|issue| resource_rejection(issue, u32::try_from(index).ok()))?;
        needs_workspace_ownership |= is_workspace_or_git_mutation(&claim);
        resolved.push(claim);
    }
    if needs_workspace_ownership {
        let ownership = resolver
            .resolve_workspace_mutation_ownership(invocation_identity)
            .map_err(|issue| resource_rejection(issue, None))?;
        if !ownership.is_workspace_mutation_ownership() {
            return Err(resource_rejection(
                ResourceClaimResolutionIssueV1::ScopeResolutionFailed,
                None,
            ));
        }
        validate_resolved_common(&ownership).map_err(|issue| resource_rejection(issue, None))?;
        resolved.push(ownership);
    }
    canonicalize_resolved_claims(resolved).map_err(|issue| resource_rejection(issue, None))
}

fn validate_resolved_declared_claim(
    declared: &ResourceClaimV1,
    resolved: &ResolvedResourceClaimV1,
) -> Result<(), ResourceClaimResolutionIssueV1> {
    validate_resolved_common(resolved)?;
    if resolved.core_derived {
        return Err(ResourceClaimResolutionIssueV1::ScopeResolutionFailed);
    }
    let (expected_kind, expected_access) = declared_kind_and_access(declared);
    if resolved.kind != expected_kind || resolved.access != expected_access {
        return Err(ResourceClaimResolutionIssueV1::ScopeResolutionFailed);
    }
    Ok(())
}

fn validate_resolved_common(
    resolved: &ResolvedResourceClaimV1,
) -> Result<(), ResourceClaimResolutionIssueV1> {
    let valid_text = |value: &str| {
        !value.is_empty() && !value.contains('\0') && !value.chars().any(char::is_control)
    };
    if !valid_text(&resolved.canonical_resource_identity) {
        return Err(ResourceClaimResolutionIssueV1::IdentityResolutionFailed);
    }
    if !valid_text(&resolved.canonical_lock_key) {
        return Err(ResourceClaimResolutionIssueV1::LockKeyInvalid);
    }
    if resolved.resolution_evidence_hash.algorithm() != ContentHashAlgorithm::Sha256 {
        return Err(ResourceClaimResolutionIssueV1::ScopeResolutionFailed);
    }
    Ok(())
}

fn declared_kind_and_access(
    claim: &ResourceClaimV1,
) -> (ResolvedResourceKindV1, ResolvedResourceAccessV1) {
    match claim {
        ResourceClaimV1::WorkspacePath { access, .. } => {
            (ResolvedResourceKindV1::WorkspacePath, file_access(*access))
        }
        ResourceClaimV1::ExternalFile { access, .. } => {
            (ResolvedResourceKindV1::ExternalFile, file_access(*access))
        }
        ResourceClaimV1::NetworkTarget { access, .. } => (
            ResolvedResourceKindV1::NetworkTarget,
            match access {
                aura_tool_contract::NetworkResourceAccessV1::Read => ResolvedResourceAccessV1::Read,
                aura_tool_contract::NetworkResourceAccessV1::Write => {
                    ResolvedResourceAccessV1::Write
                }
                aura_tool_contract::NetworkResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
            },
        ),
        ResourceClaimV1::Process { access, .. } => (
            ResolvedResourceKindV1::Process,
            match access {
                aura_tool_contract::ProcessResourceAccessV1::Execute => {
                    ResolvedResourceAccessV1::Execute
                }
                aura_tool_contract::ProcessResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
                aura_tool_contract::ProcessResourceAccessV1::Control => {
                    ResolvedResourceAccessV1::Control
                }
            },
        ),
        ResourceClaimV1::TerminalSession { access, .. } => (
            ResolvedResourceKindV1::TerminalSession,
            match access {
                aura_tool_contract::TerminalResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
                aura_tool_contract::TerminalResourceAccessV1::Control => {
                    ResolvedResourceAccessV1::Control
                }
            },
        ),
        ResourceClaimV1::GitRepository { access, .. } => (
            ResolvedResourceKindV1::GitRepository,
            match access {
                aura_tool_contract::RepositoryResourceAccessV1::Read => {
                    ResolvedResourceAccessV1::Read
                }
                aura_tool_contract::RepositoryResourceAccessV1::Write => {
                    ResolvedResourceAccessV1::Write
                }
                aura_tool_contract::RepositoryResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
            },
        ),
        ResourceClaimV1::GitIndex { access, .. } => {
            (ResolvedResourceKindV1::GitIndex, read_write_access(*access))
        }
        ResourceClaimV1::GitRef { access, .. } => (
            ResolvedResourceKindV1::GitRef,
            match access {
                aura_tool_contract::GitRefResourceAccessV1::Read => ResolvedResourceAccessV1::Read,
                aura_tool_contract::GitRefResourceAccessV1::Write => {
                    ResolvedResourceAccessV1::Write
                }
                aura_tool_contract::GitRefResourceAccessV1::Create => {
                    ResolvedResourceAccessV1::Create
                }
                aura_tool_contract::GitRefResourceAccessV1::Delete => {
                    ResolvedResourceAccessV1::Delete
                }
            },
        ),
        ResourceClaimV1::GitSequencer { access, .. } => (
            ResolvedResourceKindV1::GitSequencer,
            match access {
                aura_tool_contract::GitSequencerResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
                aura_tool_contract::GitSequencerResourceAccessV1::Control => {
                    ResolvedResourceAccessV1::Control
                }
            },
        ),
        ResourceClaimV1::Artifact { access, .. } => (
            ResolvedResourceKindV1::Artifact,
            match access {
                aura_tool_contract::ArtifactResourceAccessV1::Read => {
                    ResolvedResourceAccessV1::Read
                }
                aura_tool_contract::ArtifactResourceAccessV1::Create => {
                    ResolvedResourceAccessV1::Create
                }
                aura_tool_contract::ArtifactResourceAccessV1::Delete => {
                    ResolvedResourceAccessV1::Delete
                }
            },
        ),
    }
}

fn package_declared_kind_and_access(
    claim: &aura_tool_contract::PackageDeclaredResourceClaimV1,
) -> (ResolvedResourceKindV1, ResolvedResourceAccessV1) {
    use aura_tool_contract::PackageDeclaredResourceClaimV1 as Claim;
    match claim {
        Claim::WorkspacePath { access, .. } => {
            (ResolvedResourceKindV1::WorkspacePath, file_access(*access))
        }
        Claim::ExternalFile { access, .. } => {
            (ResolvedResourceKindV1::ExternalFile, file_access(*access))
        }
        Claim::NetworkTarget { access, .. } => (
            ResolvedResourceKindV1::NetworkTarget,
            match access {
                aura_tool_contract::NetworkResourceAccessV1::Read => ResolvedResourceAccessV1::Read,
                aura_tool_contract::NetworkResourceAccessV1::Write => {
                    ResolvedResourceAccessV1::Write
                }
                aura_tool_contract::NetworkResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
            },
        ),
        Claim::Process { access, .. } => (
            ResolvedResourceKindV1::Process,
            match access {
                aura_tool_contract::ProcessResourceAccessV1::Execute => {
                    ResolvedResourceAccessV1::Execute
                }
                aura_tool_contract::ProcessResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
                aura_tool_contract::ProcessResourceAccessV1::Control => {
                    ResolvedResourceAccessV1::Control
                }
            },
        ),
        Claim::TerminalSession { access, .. } => (
            ResolvedResourceKindV1::TerminalSession,
            match access {
                aura_tool_contract::TerminalResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
                aura_tool_contract::TerminalResourceAccessV1::Control => {
                    ResolvedResourceAccessV1::Control
                }
            },
        ),
        Claim::GitRepository { access, .. } => (
            ResolvedResourceKindV1::GitRepository,
            match access {
                aura_tool_contract::RepositoryResourceAccessV1::Read => {
                    ResolvedResourceAccessV1::Read
                }
                aura_tool_contract::RepositoryResourceAccessV1::Write => {
                    ResolvedResourceAccessV1::Write
                }
                aura_tool_contract::RepositoryResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
            },
        ),
        Claim::GitIndex { access, .. } => {
            (ResolvedResourceKindV1::GitIndex, read_write_access(*access))
        }
        Claim::GitRef { access, .. } => (
            ResolvedResourceKindV1::GitRef,
            match access {
                aura_tool_contract::GitRefResourceAccessV1::Read => ResolvedResourceAccessV1::Read,
                aura_tool_contract::GitRefResourceAccessV1::Write => {
                    ResolvedResourceAccessV1::Write
                }
                aura_tool_contract::GitRefResourceAccessV1::Create => {
                    ResolvedResourceAccessV1::Create
                }
                aura_tool_contract::GitRefResourceAccessV1::Delete => {
                    ResolvedResourceAccessV1::Delete
                }
            },
        ),
        Claim::GitSequencer { access, .. } => (
            ResolvedResourceKindV1::GitSequencer,
            match access {
                aura_tool_contract::GitSequencerResourceAccessV1::Observe => {
                    ResolvedResourceAccessV1::Observe
                }
                aura_tool_contract::GitSequencerResourceAccessV1::Control => {
                    ResolvedResourceAccessV1::Control
                }
            },
        ),
        Claim::Artifact { selector, .. } => (
            ResolvedResourceKindV1::Artifact,
            match selector {
                aura_tool_contract::PackageArtifactSelectorV1::ArgumentJsonPointer {
                    access,
                    ..
                } => match access {
                    aura_tool_contract::ArtifactResourceAccessV1::Read => {
                        ResolvedResourceAccessV1::Read
                    }
                    aura_tool_contract::ArtifactResourceAccessV1::Create => {
                        ResolvedResourceAccessV1::Create
                    }
                    aura_tool_contract::ArtifactResourceAccessV1::Delete => {
                        ResolvedResourceAccessV1::Delete
                    }
                },
                aura_tool_contract::PackageArtifactSelectorV1::AttemptStaging { .. } => {
                    ResolvedResourceAccessV1::Create
                }
            },
        ),
    }
}

const fn file_access(access: aura_tool_contract::FileResourceAccessV1) -> ResolvedResourceAccessV1 {
    match access {
        aura_tool_contract::FileResourceAccessV1::Read => ResolvedResourceAccessV1::Read,
        aura_tool_contract::FileResourceAccessV1::Write => ResolvedResourceAccessV1::Write,
        aura_tool_contract::FileResourceAccessV1::Create => ResolvedResourceAccessV1::Create,
        aura_tool_contract::FileResourceAccessV1::Delete => ResolvedResourceAccessV1::Delete,
        aura_tool_contract::FileResourceAccessV1::Execute => ResolvedResourceAccessV1::Execute,
        aura_tool_contract::FileResourceAccessV1::Observe => ResolvedResourceAccessV1::Observe,
    }
}

const fn read_write_access(
    access: aura_tool_contract::ReadWriteResourceAccessV1,
) -> ResolvedResourceAccessV1 {
    match access {
        aura_tool_contract::ReadWriteResourceAccessV1::Read => ResolvedResourceAccessV1::Read,
        aura_tool_contract::ReadWriteResourceAccessV1::Write => ResolvedResourceAccessV1::Write,
    }
}

fn is_workspace_or_git_mutation(claim: &ResolvedResourceClaimV1) -> bool {
    let mutable_access = matches!(
        claim.access,
        ResolvedResourceAccessV1::Write
            | ResolvedResourceAccessV1::Create
            | ResolvedResourceAccessV1::Delete
            | ResolvedResourceAccessV1::Control
    );
    mutable_access
        && matches!(
            claim.kind,
            ResolvedResourceKindV1::WorkspacePath
                | ResolvedResourceKindV1::GitRepository
                | ResolvedResourceKindV1::GitIndex
                | ResolvedResourceKindV1::GitRef
                | ResolvedResourceKindV1::GitSequencer
        )
}

fn canonicalize_resolved_claims(
    claims: Vec<ResolvedResourceClaimV1>,
) -> Result<Vec<ResolvedResourceClaimV1>, ResourceClaimResolutionIssueV1> {
    let mut by_lock_key = BTreeMap::new();
    for claim in claims {
        match by_lock_key.entry(claim.canonical_lock_key.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(claim);
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &claim => {}
            std::collections::btree_map::Entry::Occupied(_) => {
                return Err(ResourceClaimResolutionIssueV1::LockKeyInvalid);
            }
        }
    }
    Ok(by_lock_key.into_values().collect())
}

fn resource_rejection(
    issue: ResourceClaimResolutionIssueV1,
    claim_index: Option<u32>,
) -> ResourcePreflightRejectionV1 {
    ResourcePreflightRejectionV1 {
        reason: ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
        error_code: issue.error_code().error_code(),
        claim_index,
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
    use aura_domain_common::{
        AgentRunId, CommandInvocationId, ModelStepId, Revision, SessionId, TurnTaskId, UtcTimestamp,
    };
    use aura_domain_common::{ToolCallId, ToolId, ToolInstanceId, ToolRegistrySnapshotId};
    use aura_permission_domain::ResolvedScope;
    use aura_tool_contract::{
        ArtifactResourceAccessV1, EffectiveToolBindingSnapshot, FileResourceAccessV1,
        NormalizedToolCallInputStateV1, NormalizedToolCallV1, ResourceClaimSelectorV1,
        ResourceInvocationContextV1, ToolAudienceV1, ToolCanonicalName, ToolDefinitionSnapshotRef,
        ToolExposureV1, ToolInputSchemaV1, ToolRegistrySnapshotRef, ToolResultContractV1,
        ToolRuntimeDescriptorV1, ToolSourceKindV1, ToolVersion,
    };
    use aura_tool_domain::{ToolDefinitionRecordV1, ToolExecutionSemanticsV1};
    use serde_json::json;

    use super::*;

    struct FixtureResolver {
        fail: Option<ResourceClaimResolutionIssueV1>,
    }

    impl ResourceClaimResolverV1 for FixtureResolver {
        fn resolve_declared_claim(
            &self,
            claim: &ResourceClaimV1,
            _arguments: &Value,
            _invocation_identity: &CoreInvocationIdentityV1,
        ) -> Result<ResolvedResourceClaimV1, ResourceClaimResolutionIssueV1> {
            if let Some(issue) = self.fail {
                return Err(issue);
            }
            let (kind, access) = declared_kind_and_access(claim);
            let lock = match kind {
                ResolvedResourceKindV1::WorkspacePath => "lock:workspace:z",
                ResolvedResourceKindV1::Artifact => "lock:artifact:a",
                _ => "lock:resource:m",
            };
            Ok(ResolvedResourceClaimV1 {
                kind,
                access,
                canonical_resource_identity: format!("resource:{kind:?}"),
                canonical_lock_key: lock.to_owned(),
                resolution_evidence_hash: CanonicalHashV1::sha256(lock.as_bytes()),
                core_derived: false,
            })
        }

        fn resolve_workspace_mutation_ownership(
            &self,
            _invocation_identity: &CoreInvocationIdentityV1,
        ) -> Result<ResolvedResourceClaimV1, ResourceClaimResolutionIssueV1> {
            Ok(ResolvedResourceClaimV1 {
                kind: ResolvedResourceKindV1::WorkspaceMutationOwnership,
                access: ResolvedResourceAccessV1::Control,
                canonical_resource_identity: "workspace-mutation-owner:fixture".to_owned(),
                canonical_lock_key: "lock:ownership:0".to_owned(),
                resolution_evidence_hash: CanonicalHashV1::sha256(b"ownership"),
                core_derived: true,
            })
        }
    }

    fn invocation() -> CoreInvocationIdentityV1 {
        CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::new(),
            session_id: SessionId::new(),
            turn_task_id: TurnTaskId::new(),
            agent_run_id: AgentRunId::new(),
            model_step_id: ModelStepId::new(),
        }
    }

    fn validated_call(resource_claims: Vec<ResourceClaimV1>) -> ValidatedToolCallV1 {
        let tool_id = ToolId::from_bytes([7; 16]);
        let tool_version = ToolVersion::new(None, Revision::INITIAL).expect("tool version");
        let input_schema = ToolInputSchemaV1::new(json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        }))
        .expect("schema");
        let definition = ToolDefinitionRecordV1::new(
            tool_id,
            tool_version.clone(),
            None,
            None,
            None,
            ToolCanonicalName::new("fixture.mutate").expect("name"),
            "Fixture mutate".to_owned(),
            "Mutate one fixture resource".to_owned(),
            "Fixture mutation".to_owned(),
            input_schema,
            true,
            ToolResultContractV1::Unit,
            ToolRuntimeDescriptorV1::ExternalSource {
                source_contract_key: "fixture.resource.v1".to_owned(),
            },
            Vec::new(),
            resource_claims,
            ToolAudienceV1::SubagentConfigurable,
            ToolSourceKindV1::BuiltIn,
            ToolExecutionSemanticsV1 {
                automatic_retry_allowed: false,
                repeat_is_idempotent: true,
                may_create_persistent_effects: true,
            },
            CanonicalHashV1::sha256(b"runtime"),
            Revision::INITIAL,
            UtcTimestamp::now(),
        )
        .expect("definition");
        let registry_snapshot_ref = ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"registry"),
        };
        let arguments = json!({"path": "src/lib.rs"});
        ValidatedToolCallV1 {
            call: NormalizedToolCallV1 {
                tool_call_id: ToolCallId::new(),
                ordinal: 1,
                definition_ref: Some(ToolDefinitionSnapshotRef {
                    tool_id,
                    tool_version: tool_version.clone(),
                    registry_snapshot_ref,
                }),
                input: NormalizedToolCallInputStateV1::Ready {
                    purpose: "Update the fixture".parse().expect("purpose"),
                    arguments: arguments.clone(),
                    arguments_hash: canonical_sha256(&arguments).expect("arguments hash"),
                },
                provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"envelope"),
            },
            definition: definition.clone(),
            binding: EffectiveToolBindingSnapshot {
                tool_id,
                tool_version,
                tool_instance_id: ToolInstanceId::new(),
                source_kind: ToolSourceKindV1::BuiltIn,
                exposure: ToolExposureV1::Core,
                definition_fingerprint: definition.definition_fingerprint,
                source_contract_fingerprint:
                    aura_tool_contract::ToolSourceContractFingerprintV1::BuiltIn {
                        signed_builtin_tool_contract_fingerprint:
                            aura_tool_contract::SignedBuiltinToolContractFingerprintV1::new(
                                CanonicalHashV1::sha256(b"source"),
                            ),
                    },
            },
        }
    }

    fn selector() -> ResourceClaimSelectorV1 {
        ResourceClaimSelectorV1::ArgumentJsonPointer { pointer: "/path".parse().expect("pointer") }
    }

    #[test]
    fn selector_failure_rejects_only_the_current_call_before_locks_or_runtime() {
        let call = validated_call(vec![ResourceClaimV1::WorkspacePath {
            selector: selector(),
            access: FileResourceAccessV1::Read,
        }]);
        let rejection = resolve_tool_resource_claims(
            &call,
            &invocation(),
            &FixtureResolver { fail: Some(ResourceClaimResolutionIssueV1::SelectorTypeMismatch) },
        )
        .expect_err("selector must fail closed");

        assert_eq!(rejection.reason, ToolNotExecutedReasonV1::ResourceClaimResolutionFailed);
        assert_eq!(rejection.claim_index, Some(0));
        assert_eq!(rejection.error_code, ToolResourceErrorCode::SelectorTypeMismatch.error_code());
    }

    #[test]
    fn workspace_mutation_adds_core_ownership_and_sorts_canonical_locks() {
        let call = validated_call(vec![
            ResourceClaimV1::WorkspacePath {
                selector: selector(),
                access: FileResourceAccessV1::Write,
            },
            ResourceClaimV1::Artifact {
                selector: ResourceClaimSelectorV1::InvocationContext {
                    context: ResourceInvocationContextV1::ToolCall,
                },
                access: ArtifactResourceAccessV1::Read,
            },
        ]);
        let claims =
            resolve_tool_resource_claims(&call, &invocation(), &FixtureResolver { fail: None })
                .expect("claims");

        assert_eq!(claims.len(), 3);
        assert!(claims.windows(2).all(|pair| {
            pair[0].canonical_lock_key.as_bytes() < pair[1].canonical_lock_key.as_bytes()
        }));
        assert!(claims.iter().any(ResolvedResourceClaimV1::is_workspace_mutation_ownership));
        assert_eq!(
            claims
                .iter()
                .filter(|claim| claim.kind == ResolvedResourceKindV1::WorkspaceMutationOwnership)
                .count(),
            1
        );
    }

    #[test]
    fn final_scope_composition_removes_only_exact_duplicates_in_provenance_order() {
        let workspace_id = aura_domain_common::WorkspaceId::new();
        let explicit = vec![ResolvedScope::WorkspaceRoot { workspace_id }];
        let runtime = vec![
            ResolvedScope::NetworkOrigin {
                scheme: "https".to_owned(),
                host: "example.com".to_owned(),
                port: 443,
            },
            ResolvedScope::WorkspaceRoot { workspace_id },
        ];
        let additional = vec![ResolvedScope::PrivateNetwork {
            class: aura_security_contract::PrivateNetworkClass::Private,
        }];

        let scopes = compose_tool_operation_scopes_v1(&explicit, &runtime, &additional);

        assert_eq!(scopes.len(), 3);
        assert_eq!(scopes[0], explicit[0]);
        assert_eq!(scopes[1], runtime[0]);
        assert_eq!(scopes[2], additional[0]);
    }

    #[test]
    fn final_scope_composition_never_widens_distinct_paths_or_network_targets() {
        let workspace_id = aura_domain_common::WorkspaceId::new();
        let scopes = compose_tool_operation_scopes_v1(
            &[
                ResolvedScope::WorkspacePath {
                    workspace_id,
                    canonical_relative_path: aura_domain_common::WorkspacePath::parse("src")
                        .expect("path"),
                },
                ResolvedScope::WorkspaceRoot { workspace_id },
            ],
            &[
                ResolvedScope::NetworkOrigin {
                    scheme: "https".to_owned(),
                    host: "example.com".to_owned(),
                    port: 443,
                },
                ResolvedScope::NetworkOrigin {
                    scheme: "https".to_owned(),
                    host: "example.com".to_owned(),
                    port: 8443,
                },
            ],
            &[],
        );

        assert_eq!(scopes.len(), 4);
    }
}
