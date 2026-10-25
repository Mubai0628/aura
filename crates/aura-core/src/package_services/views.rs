//! Read models for the tool-package page. Built from V2 facts only.
use super::PackageServices;
use anyhow::{Result, ensure};
use aura_domain_common::Revision;
use aura_persistence_core::Connection;
use aura_persistence_v2::packages::{self, catalog::CatalogEntry, conformance::PackageRunRecord};
use aura_tool_contract::{DeclarativeHttpTargetV1, HttpCredentialSlotV1};
use aura_tool_domain::*;

impl PackageServices {
    pub fn version_view(&self, reference: &PackageVersionRefV1) -> Result<PackageVersionViewV1> {
        let store = self.store()?;
        version_view(store.database.connection(), reference)
    }

    pub fn list(&self) -> Result<PackageListV1> {
        let store = self.store()?;
        let db = store.database.connection();
        let packages = packages::catalog::catalog(db)?
            .iter()
            .map(|entry| summary(db, entry))
            .collect::<Result<Vec<_>>>()?;
        Ok(PackageListV1 { packages })
    }

    pub fn summary(&self, package: aura_domain_common::ToolPackageId) -> Result<PackageSummaryV1> {
        let store = self.store()?;
        let db = store.database.connection();
        let entry = packages::catalog::package(db, package)?
            .ok_or_else(|| anyhow::anyhow!("package.not_found"))?;
        summary(db, &entry)
    }
}

pub(super) fn find_version(
    db: &Connection,
    reference: &PackageVersionRefV1,
) -> Result<(CatalogEntry, PackageVersionBindingV1)> {
    let entry = packages::catalog::package(db, reference.package_id)?
        .ok_or_else(|| anyhow::anyhow!("package.not_found"))?;
    let binding = entry
        .versions
        .iter()
        .find(|version| {
            version.binding.package_version == reference.package_version
                && version.binding.package_fingerprint == reference.package_fingerprint
        })
        .map(|version| version.binding.clone())
        .ok_or_else(|| anyhow::anyhow!("package.version_not_found"))?;
    Ok((entry, binding))
}

pub(super) fn version_view(
    db: &Connection,
    reference: &PackageVersionRefV1,
) -> Result<PackageVersionViewV1> {
    let (entry, binding) = find_version(db, reference)?;
    let content = packages::load_content(db, &binding)?;
    let run = packages::conformance::conformance(db, &binding)?;
    let conformance = conformance_view(&binding, run.as_ref())?;
    let settings = match &run {
        Some(run) => run.settings.clone(),
        None if entry.installed.as_ref() == Some(&binding) => {
            packages::installed_settings(db, &binding)?
        }
        None => vec![],
    };
    let preview = aura_tool_package::package_content_preview_v1(
        content.descriptor,
        binding,
        conformance.state,
    )?;
    Ok(PackageVersionViewV1 { preview, conformance, settings, package_revision: entry.revision })
}

pub(super) fn conformance_view(
    binding: &PackageVersionBindingV1,
    run: Option<&PackageRunRecord>,
) -> Result<PackageConformanceViewV1> {
    let Some(run) = run else {
        return Ok(PackageConformanceViewV1 {
            version: binding.clone(),
            state: PackageConformanceStateV1::NotRun,
            run_id: None,
            started_at: None,
            report: None,
            report_hash: None,
        });
    };
    ensure!(run.version == *binding, "package.conformance_binding");
    let report_hash = run.report.as_ref().map(packages::conformance::report_hash).transpose()?;
    Ok(PackageConformanceViewV1 {
        version: binding.clone(),
        state: run.state,
        run_id: Some(run.run_id),
        started_at: Some(run.started_at),
        report: run.report.clone(),
        report_hash,
    })
}

fn summary(db: &Connection, entry: &CatalogEntry) -> Result<PackageSummaryV1> {
    let display = entry
        .installed_version()
        .or_else(|| entry.versions.first())
        .and_then(|version| version.descriptor.as_ref())
        .map_or_else(
            || entry.package_key.as_str().to_owned(),
            |descriptor| descriptor.manifest.ui.display_name.as_str().to_owned(),
        );
    let installed = match (entry.installed_version(), entry.installed.as_ref()) {
        (Some(version), Some(binding)) => {
            let descriptor = version
                .descriptor
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("package.content_unavailable"))?;
            let settings = packages::installed_settings(db, binding)?;
            let revision = entry.trust_revision.unwrap_or(Revision::INITIAL);
            let tools = descriptor
                .manifest
                .tools
                .iter()
                .map(|tool| tool_summary(db, entry, tool, &settings, revision))
                .collect::<Result<Vec<_>>>()?;
            Some(PackageInstalledV1 {
                version: PackageVersionRefV1 {
                    package_id: binding.package_id,
                    package_version: binding.package_version.clone(),
                    package_fingerprint: binding.package_fingerprint.clone(),
                },
                enabled: entry.enabled,
                trusted: entry.trusted,
                tools,
            })
        }
        _ => None,
    };
    Ok(PackageSummaryV1 {
        package_id: entry.package_id,
        package_key: entry.package_key.clone(),
        display_name: display,
        revision: entry.revision,
        installed,
        versions: entry
            .versions
            .iter()
            .map(|version| PackageVersionSummaryV1 {
                version: version.binding.clone(),
                conformance: version.conformance,
                imported_at: version.created_at,
            })
            .collect(),
    })
}

fn tool_summary(
    db: &Connection,
    entry: &CatalogEntry,
    tool: &CanonicalToolPackageToolV1,
    settings: &[PackageToolSettingsV1],
    revision: Revision,
) -> Result<PackageToolSummaryV1> {
    let (tool_id, tool_instance_id) =
        package_tool_identity_v1(entry.package_id, &tool.tool_key, revision);
    let http = matches!(tool.runtime, CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. });
    let credential = match http_credential(tool, settings)? {
        Some((slot_key, placement, origin)) => Some(PackageCredentialSlotViewV1 {
            binding: packages::credentials::binding(
                db,
                entry.package_id,
                &tool.tool_key,
                &slot_key,
            )?,
            target: ToolCredentialManagementTargetV1 {
                tool_instance_id,
                expected_instance_revision: revision,
                credential_slot_key: slot_key.clone(),
                placement: placement.clone(),
                workspace_id: None,
                session_id: None,
            },
            slot_key,
            placement,
            origin,
        }),
        None => None,
    };
    Ok(PackageToolSummaryV1 {
        tool_key: tool.tool_key.clone(),
        display_name: tool.ui.display_name.as_str().to_owned(),
        canonical_name: tool.model.canonical_name.clone(),
        execution_location: if http {
            PackageExecutionLocationV1::ControlledHttp
        } else {
            PackageExecutionLocationV1::UserJob
        },
        tool_id,
        tool_instance_id,
        credential,
    })
}

/// An HTTP tool's credential slot, its placement and the exact origin a bound
/// credential must allow. `None` for tools without a slot.
pub(crate) fn http_credential(
    tool: &CanonicalToolPackageToolV1,
    settings: &[PackageToolSettingsV1],
) -> Result<Option<(aura_tool_contract::CredentialSlotKey, ToolCredentialPlacementV1, String)>> {
    let Ok(request) = crate::package_http::declarative(tool) else { return Ok(None) };
    let Some(slot) = &request.credential_slot else { return Ok(None) };
    let (slot_key, placement) = match slot {
        HttpCredentialSlotV1::BearerAuthorization { slot_key } => {
            (slot_key.clone(), ToolCredentialPlacementV1::HttpBearerAuthorization)
        }
        HttpCredentialSlotV1::FixedHeader { slot_key, header_name } => (
            slot_key.clone(),
            ToolCredentialPlacementV1::HttpFixedHeader { header_name: header_name.clone() },
        ),
    };
    let origin = match &request.target {
        DeclarativeHttpTargetV1::FixedHttps { origin, .. } => origin.as_str().to_owned(),
        DeclarativeHttpTargetV1::UserBoundEndpoint { endpoint_slot_key } => settings
            .iter()
            .find(|entry| entry.tool_key == tool.tool_key)
            .and_then(|entry| {
                entry
                    .endpoints
                    .iter()
                    .find(|endpoint| &endpoint.endpoint_slot_key == endpoint_slot_key)
            })
            .map(|endpoint| endpoint.origin.as_str().to_owned())
            .ok_or_else(|| anyhow::anyhow!("package.endpoint_required"))?,
    };
    Ok(Some((slot_key, placement, origin)))
}

/// The credential facts of an installed, trusted package tool whose derived instance
/// is `instance` (D3). Read from V2 by the store owner before a V1 credential step.
pub(crate) fn credential_facts(
    db: &Connection,
    instance: aura_domain_common::ToolInstanceId,
) -> Result<Option<aura_persistence::PackageToolCredentialFacts>> {
    for entry in packages::catalog::catalog(db)? {
        let (Some(version), Some(binding), Some(revision)) =
            (entry.installed_version(), entry.installed.as_ref(), entry.trust_revision)
        else {
            continue;
        };
        let Some(descriptor) = &version.descriptor else { continue };
        for tool in &descriptor.manifest.tools {
            let (tool_id, tool_instance_id) =
                package_tool_identity_v1(entry.package_id, &tool.tool_key, revision);
            if tool_instance_id != instance {
                continue;
            }
            let settings = packages::installed_settings(db, binding)?;
            return Ok(http_credential(tool, &settings)?.map(|(slot_key, placement, _)| {
                aura_persistence::PackageToolCredentialFacts {
                    tool_id,
                    package_id: entry.package_id,
                    instance_revision: revision,
                    slot_key,
                    placement,
                }
            }));
        }
    }
    Ok(None)
}
