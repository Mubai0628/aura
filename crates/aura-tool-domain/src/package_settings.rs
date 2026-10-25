//! User-supplied settings for one package version: instance configuration and the
//! service addresses a tool asks the user for. Validated against the exact manifest.
use crate::CanonicalToolPackageManifestV1;
use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{ToolId, ToolInstanceId, ToolPackageId};
use aura_tool_contract::{
    CompiledToolSchemaV1, DeclarativeHttpTargetV1, FixedHttpsOriginV1, HttpBasePathV1,
    HttpEndpointSlotKeyV1, ToolKey, ToolRuntimeDescriptorV1,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// The service address the user supplied for one `user_bound_endpoint` slot.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageEndpointBindingV1 {
    pub endpoint_slot_key: HttpEndpointSlotKeyV1,
    pub origin: FixedHttpsOriginV1,
    pub base_path: HttpBasePathV1,
}

/// One tool's settings. A tool without an instance-configuration schema has no
/// configuration, and every user-bound endpoint slot it declares has exactly one address.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageToolSettingsV1 {
    pub tool_key: ToolKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<PackageEndpointBindingV1>,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PackageSettingsErrorV1 {
    #[error("package.settings_unknown_tool: 设置中的工具不在这个版本里")]
    UnknownTool,
    #[error("package.settings_duplicate: 同一个工具或服务地址填写了两次")]
    Duplicate,
    #[error("package.configuration_required: 这个工具需要先填写配置")]
    ConfigurationRequired,
    #[error("package.configuration_unexpected: 这个工具不接受配置")]
    ConfigurationUnexpected,
    #[error("package.configuration_invalid: 配置不符合工具声明的格式")]
    ConfigurationInvalid,
    #[error("package.endpoint_required: 这个工具需要填写服务地址")]
    EndpointRequired,
    #[error("package.endpoint_unexpected: 这个工具没有声明该服务地址")]
    EndpointUnexpected,
}

/// Settings in canonical order: one entry per tool that needs any, sorted by tool key,
/// endpoints sorted by slot key. Tools that need nothing have no entry.
pub fn canonical_package_settings_v1(
    manifest: &CanonicalToolPackageManifestV1,
    settings: &[PackageToolSettingsV1],
) -> Result<Vec<PackageToolSettingsV1>, PackageSettingsErrorV1> {
    let mut seen = BTreeSet::new();
    for entry in settings {
        if !manifest.tools.iter().any(|tool| tool.tool_key == entry.tool_key) {
            return Err(PackageSettingsErrorV1::UnknownTool);
        }
        if !seen.insert(entry.tool_key.clone()) {
            return Err(PackageSettingsErrorV1::Duplicate);
        }
    }
    let mut canonical = Vec::new();
    for tool in &manifest.tools {
        let entry = settings.iter().find(|entry| entry.tool_key == tool.tool_key);
        let configuration =
            match (&tool.instance_configuration.0, entry.and_then(|e| e.configuration.as_ref())) {
                (Some(schema), Some(value)) => {
                    let valid = CompiledToolSchemaV1::compile(schema.canonical_document())
                        .map_err(|_| PackageSettingsErrorV1::ConfigurationInvalid)?
                        .validate(value)
                        .valid;
                    if !valid {
                        return Err(PackageSettingsErrorV1::ConfigurationInvalid);
                    }
                    Some(value.clone())
                }
                (Some(_), None) => return Err(PackageSettingsErrorV1::ConfigurationRequired),
                (None, Some(_)) => return Err(PackageSettingsErrorV1::ConfigurationUnexpected),
                (None, None) => None,
            };
        let declared = endpoint_slots(&tool.runtime.to_runtime_descriptor_v1());
        let mut endpoints = entry.map(|e| e.endpoints.clone()).unwrap_or_default();
        endpoints.sort_by(|left, right| left.endpoint_slot_key.cmp(&right.endpoint_slot_key));
        if endpoints.windows(2).any(|pair| pair[0].endpoint_slot_key == pair[1].endpoint_slot_key) {
            return Err(PackageSettingsErrorV1::Duplicate);
        }
        if endpoints.iter().any(|endpoint| !declared.contains(&endpoint.endpoint_slot_key)) {
            return Err(PackageSettingsErrorV1::EndpointUnexpected);
        }
        if declared.len() != endpoints.len() {
            return Err(PackageSettingsErrorV1::EndpointRequired);
        }
        if configuration.is_some() || !endpoints.is_empty() {
            canonical.push(PackageToolSettingsV1 {
                tool_key: tool.tool_key.clone(),
                configuration,
                endpoints,
            });
        }
    }
    canonical.sort_by(|left, right| left.tool_key.cmp(&right.tool_key));
    Ok(canonical)
}

/// The hash a Conformance report and the install approval bind.
pub fn package_settings_hash_v1(
    settings: &[PackageToolSettingsV1],
) -> Result<CanonicalSha256HashV1, PackageSettingsErrorV1> {
    canonical_sha256(&settings).map_err(|_| PackageSettingsErrorV1::ConfigurationInvalid)
}

/// The user-bound endpoint slots a runtime declares. Only HTTP tools have any.
#[must_use]
pub fn endpoint_slots(runtime: &ToolRuntimeDescriptorV1) -> BTreeSet<HttpEndpointSlotKeyV1> {
    match runtime {
        ToolRuntimeDescriptorV1::DeclarativeHttp { request } => match &request.target {
            DeclarativeHttpTargetV1::UserBoundEndpoint { endpoint_slot_key } => {
                [endpoint_slot_key.clone()].into()
            }
            DeclarativeHttpTargetV1::FixedHttps { .. } => BTreeSet::new(),
        },
        _ => BTreeSet::new(),
    }
}

/// Identities of one package tool. The tool ID is derived from the package and the
/// tool key, so it is stable across versions and is what a credential's use is
/// limited to. The instance ID also names the install (`install_revision`): each
/// install is a new, immutable instance record, and nothing stores either ID.
#[must_use]
pub fn package_tool_identity_v1(
    package_id: ToolPackageId,
    tool_key: &ToolKey,
    install_revision: aura_domain_common::Revision,
) -> (ToolId, ToolInstanceId) {
    let derive = |material: String| {
        let mut bytes = [0_u8; 16];
        let digest = CanonicalSha256HashV1::sha256(material.as_bytes());
        let hex = digest.hex();
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap_or(0);
        }
        bytes
    };
    (
        ToolId::from_bytes(derive(format!(
            "aura.package-tool.tool-id.v1\0{package_id}\0{}",
            tool_key.as_str()
        ))),
        ToolInstanceId::from_bytes(derive(format!(
            "aura.package-tool.instance-id.v2\0{package_id}\0{}\0{}",
            tool_key.as_str(),
            install_revision.value()
        ))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::Revision;

    #[test]
    fn identities_are_stable_and_distinct_per_package_tool_and_install() {
        let package = ToolPackageId::new();
        let other = ToolPackageId::new();
        let key = ToolKey::new("fixture").expect("key");
        let second = ToolKey::new("second").expect("key");
        let one = Revision::new(2);
        let id = |p, k: &ToolKey, r| package_tool_identity_v1(p, k, r);
        assert_eq!(id(package, &key, one), id(package, &key, one));
        assert_ne!(id(package, &key, one), id(other, &key, one));
        assert_ne!(id(package, &key, one), id(package, &second, one));
        let reinstalled = id(package, &key, Revision::new(3));
        assert_eq!(reinstalled.0, id(package, &key, one).0, "the tool keeps its identity");
        assert_ne!(reinstalled.1, id(package, &key, one).1, "each install is a new instance");
        assert_ne!(reinstalled.0.to_bytes(), reinstalled.1.to_bytes());
    }
}
