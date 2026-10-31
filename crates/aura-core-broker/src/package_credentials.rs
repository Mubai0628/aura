//! The credential facts of installed package tools, read from V2: which credential slot an HTTP
//! tool declares, where its value goes and which origin it must allow, and the facts the store
//! owner hands the V1 credential journal before that journal's own transaction.
use anyhow::Result;
use aura_persistence_core::Connection;
use aura_persistence_v2::packages;
use aura_tool_contract::{
    DeclarativeHttpTargetV1, DeclarativeHttpV1, HttpCredentialSlotV1, ToolRuntimeDescriptorV1,
};
use aura_tool_domain::*;

pub fn declarative(tool: &CanonicalToolPackageToolV1) -> Result<DeclarativeHttpV1> {
    match tool.runtime.to_runtime_descriptor_v1() {
        ToolRuntimeDescriptorV1::DeclarativeHttp { request } => Ok(request),
        _ => anyhow::bail!("package.http_runtime_required"),
    }
}

/// An HTTP tool's credential slot, its placement and the exact origin a bound
/// credential must allow. `None` for tools without a slot.
pub fn http_credential(
    tool: &CanonicalToolPackageToolV1,
    settings: &[PackageToolSettingsV1],
) -> Result<Option<(aura_tool_contract::CredentialSlotKey, ToolCredentialPlacementV1, String)>> {
    let Ok(request) = declarative(tool) else { return Ok(None) };
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
