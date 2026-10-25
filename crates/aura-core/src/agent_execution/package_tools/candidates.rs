//! Installed package tools as V2 Registry candidates. Every value here is derived
//! from the installed, trusted version; the same inputs always derive the same
//! definition, so a frozen call can be checked against the live install.
use anyhow::{Result, ensure};
use aura_domain_common::{Revision, ToolPackageId, UtcTimestamp};
use aura_persistence_core::Connection;
use aura_persistence_v2::packages::{self, catalog::CatalogEntry};
use aura_tool_contract::{
    ToolAudienceV1, ToolSourceContractFingerprintV1, ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::*;
use aura_tool_registry::{RegistryCandidateV1, ToolSourceEligibilityEvidenceV1};
use std::sync::Arc;

/// One tool of an installed, trusted and enabled package whose content is present.
#[derive(Clone, Debug)]
pub(crate) struct InstalledPackageTool {
    pub package_id: ToolPackageId,
    pub version: PackageVersionBindingV1,
    pub descriptor: Arc<PackageContentDescriptorV1>,
    pub tool: CanonicalToolPackageToolV1,
    pub settings: Option<PackageToolSettingsV1>,
    pub definition: ToolDefinitionRecordV1,
    pub instance: ToolInstanceRecordV1,
    pub trust_revision: Revision,
    pub credential: Option<PackageCredentialBindingV1>,
    /// The HTTP credential slot this tool declares, if any.
    pub credential_slot: Option<aura_tool_contract::CredentialSlotKey>,
}

impl InstalledPackageTool {
    #[must_use]
    pub fn http(&self) -> bool {
        matches!(self.tool.runtime, CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. })
    }
    /// The registry may expose it: a declared HTTP credential slot must be bound.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.credential_slot.is_none() || self.credential.is_some()
    }
    pub fn candidate(&self) -> RegistryCandidateV1 {
        RegistryCandidateV1 {
            definition: self.definition.clone(),
            instance: self.instance.clone(),
            source_evidence: ToolSourceEligibilityEvidenceV1::Package {
                current_security_fingerprint: self.descriptor.security_fingerprint.clone(),
                trust_record: Some(ToolPackageTrustRecordV1 {
                    tool_package_id: self.package_id,
                    package_security_fingerprint: self.descriptor.security_fingerprint.clone(),
                    trusted: true,
                    entity_revision: self.trust_revision,
                    reviewed_at: self.definition.created_at,
                }),
                payload_verified: true,
                conformance_passed: true,
                instance_configuration_ready: true,
                credential_binding_ready: self.ready(),
                deletion_pending: false,
            },
            active_for_unpinned: true,
        }
    }
}

/// Every tool of every eligible package: installed, trusted, enabled, content present.
/// One package whose stored facts no longer derive never hides the others or the
/// built-in tools; it is reported and left out.
pub(crate) fn installed_tools(db: &Connection) -> Result<Vec<InstalledPackageTool>> {
    let mut tools = Vec::new();
    for entry in packages::catalog::catalog(db)? {
        if entry.eligible() {
            match entry_tools(db, &entry) {
                Ok(entry_tools) => tools.extend(entry_tools),
                Err(error) => {
                    eprintln!("package {} tools were left out: {error}", entry.package_id);
                }
            }
        }
    }
    Ok(tools)
}

/// The installed tool a frozen definition names, only while its package is still
/// eligible and derives exactly this definition. Anything else is a refusal.
pub(crate) fn installed_tool_for(
    db: &Connection,
    definition: &ToolDefinitionRecordV1,
) -> Result<InstalledPackageTool> {
    let package =
        definition.tool_package_id.ok_or_else(|| anyhow::anyhow!("package.tool_not_package"))?;
    let entry = packages::catalog::package(db, package)?
        .ok_or_else(|| anyhow::anyhow!("package.uninstalled: 这个工具包已卸载"))?;
    ensure!(entry.trusted, "package.not_trusted: 这个工具包已撤销信任");
    ensure!(entry.enabled, "package.disabled: 这个工具包已停用");
    ensure!(entry.eligible(), "package.content_unavailable: 这个工具包的内容不可用");
    let tool = entry_tools(db, &entry)?
        .into_iter()
        .find(|tool| tool.definition.tool_id == definition.tool_id)
        .ok_or_else(|| anyhow::anyhow!("package.tool_missing: 当前版本没有这个工具"))?;
    ensure!(
        tool.definition == *definition,
        "package.version_changed: 这个工具包已更新，请重新发起调用"
    );
    Ok(tool)
}

/// Whether the package version this operation ran is no longer the installed,
/// trusted and enabled one: it started before a disable, a revoke, an uninstall or
/// an upgrade, and finished on the content it started with.
pub(crate) fn changed_during_call(
    db: &Connection,
    content: &aura_execution_contract::OperationContent,
) -> bool {
    let Some((package_id, fingerprint)) =
        content.resources.iter().find_map(|resource| match resource {
            aura_security_contract::ResourceDescriptor::ToolPackage {
                package_id,
                package_fingerprint,
                ..
            } => Some((*package_id, package_fingerprint)),
            _ => None,
        })
    else {
        return false;
    };
    !matches!(
        packages::catalog::package(db, package_id),
        Ok(Some(entry)) if entry.eligible()
            && entry.installed.as_ref().is_some_and(|binding| binding.package_fingerprint.0 == *fingerprint)
    )
}

/// The issue code a result carries when [`changed_during_call`] holds.
pub(crate) const CHANGED_DURING_CALL: &str = "tool.package.changed_during_call";

fn entry_tools(db: &Connection, entry: &CatalogEntry) -> Result<Vec<InstalledPackageTool>> {
    let (Some(version), Some(binding), Some(trust_revision)) =
        (entry.installed_version(), entry.installed.as_ref(), entry.trust_revision)
    else {
        return Ok(vec![]);
    };
    let descriptor = Arc::new(
        version.descriptor.clone().ok_or_else(|| anyhow::anyhow!("package.content_unavailable"))?,
    );
    let settings = packages::installed_settings(db, binding)?;
    let mut tools = Vec::with_capacity(descriptor.manifest.tools.len());
    for tool in &descriptor.manifest.tools {
        let definition =
            definition(entry, binding, &descriptor, tool, trust_revision, version.created_at)?;
        let fingerprint = aura_tool_package::tool_contract_fingerprint_v1(
            &descriptor.manifest,
            tool,
            &descriptor.runtime_closure_hash,
        )?;
        let instance = ToolInstanceRecordV1 {
            tool_instance_id: package_tool_identity_v1(
                entry.package_id,
                &tool.tool_key,
                trust_revision,
            )
            .1,
            tool_id: definition.tool_id,
            tool_version: definition.tool_version.clone(),
            source_kind: ToolSourceKindV1::Imported,
            source_contract_fingerprint: ToolSourceContractFingerprintV1::Imported {
                tool_contract_fingerprint: fingerprint,
            },
            // Only enabled packages become candidates; a disable is a package change,
            // never a mutation of this install's immutable instance record.
            enabled: true,
            runtime_health: aura_tool_domain::ToolRuntimeHealthV1::Healthy,
            overrides: ToolInstanceOverridesV1 {
                timeout_ms: None,
                max_retries: None,
                max_concurrent_attempts: None,
                subagent_enabled: None,
            },
            entity_revision: trust_revision,
            created_at: version.created_at,
            updated_at: version.created_at,
        };
        let credential_slot =
            crate::package_services::http_credential(tool, &settings)?.map(|(slot, _, _)| slot);
        let credential = match &credential_slot {
            Some(slot) => {
                packages::credentials::binding(db, entry.package_id, &tool.tool_key, slot)?
            }
            None => None,
        };
        tools.push(InstalledPackageTool {
            package_id: entry.package_id,
            version: binding.clone(),
            descriptor: Arc::clone(&descriptor),
            tool: tool.clone(),
            settings: settings.iter().find(|entry| entry.tool_key == tool.tool_key).cloned(),
            definition,
            instance,
            trust_revision,
            credential,
            credential_slot,
        });
    }
    Ok(tools)
}

fn definition(
    entry: &CatalogEntry,
    binding: &PackageVersionBindingV1,
    descriptor: &PackageContentDescriptorV1,
    tool: &CanonicalToolPackageToolV1,
    trust_revision: Revision,
    created_at: UtcTimestamp,
) -> Result<ToolDefinitionRecordV1> {
    let (tool_id, _) = package_tool_identity_v1(entry.package_id, &tool.tool_key, trust_revision);
    let mut capabilities =
        tool.capability_requirements.iter().copied().map(resolved_capability).collect::<Vec<_>>();
    capabilities.sort();
    capabilities.dedup();
    Ok(ToolDefinitionRecordV1::new(
        tool_id,
        ToolVersion::new(Some(binding.package_version.clone()), trust_revision)?,
        Some(entry.package_id),
        Some(descriptor.manifest.package_key.clone()),
        Some(tool.tool_key.clone()),
        tool.model.canonical_name.clone(),
        tool.ui.display_name.as_str().to_owned(),
        tool.model.description.as_str().to_owned(),
        tool.model.summary.as_str().to_owned(),
        tool.model.input_schema.clone(),
        tool.model.strict,
        tool.model.result_contract.clone(),
        tool.runtime.to_runtime_descriptor_v1(),
        capabilities,
        // The declared claims stay authoritative in the stored canonical manifest;
        // the generic claim type cannot carry package path scopes without loss.
        vec![],
        match tool.audience {
            PackageToolAudienceV1::SubagentConfigurable => ToolAudienceV1::SubagentConfigurable,
            PackageToolAudienceV1::ParentAgentOnly => ToolAudienceV1::ParentAgentOnly,
        },
        ToolSourceKindV1::Imported,
        tool.execution_semantics,
        descriptor.runtime_closure_hash.0.clone(),
        Revision::INITIAL,
        created_at,
    )?)
}

const fn resolved_capability(
    value: PackageCapabilityRequirementV1,
) -> aura_security_contract::ResolvedCapability {
    use PackageCapabilityRequirementV1 as P;
    use aura_security_contract::ResolvedCapability as C;
    match value {
        P::WorkspaceFileRead => C::WorkspaceFileRead,
        P::WorkspaceFileWrite => C::WorkspaceFileWrite,
        P::WorkspaceFileCreate => C::WorkspaceFileCreate,
        P::WorkspaceFileDelete => C::WorkspaceFileDelete,
        P::ExternalFileRead => C::ExternalFileRead,
        P::ExternalFileWrite => C::ExternalFileWrite,
        P::ExternalFileCreate => C::ExternalFileCreate,
        P::ExternalFileDelete => C::ExternalFileDelete,
        P::ProcessExecute => C::ProcessExecute,
        P::GitRead => C::GitRead,
        P::GitWrite => C::GitWrite,
        P::GitRemoteRead => C::GitRemoteRead,
        P::GitRemoteWrite => C::GitRemoteWrite,
        P::GitDestructive => C::GitDestructive,
        P::PublicNetworkRead => C::PublicNetworkRead,
        P::PrivateNetworkAccess => C::PrivateNetworkAccess,
        P::DataEgress => C::DataEgress,
        P::DownloadArtifact => C::DownloadArtifact,
        P::DownloadWindowsExecutable => C::DownloadWindowsExecutable,
        P::ExternalServiceRead => C::ExternalServiceRead,
        P::ExternalServiceMutation => C::ExternalServiceMutation,
        P::ExternalServiceDestructiveMutation => C::ExternalServiceDestructiveMutation,
    }
}
