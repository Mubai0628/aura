use aura_domain_common::{
    Revision,
    jcs::{CanonicalHashV1, canonical_sha256},
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use super::{
    DEFAULT_MAX_CONCURRENT_TOOL_CALLS, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS, DEFAULT_MAX_TOOL_RETRIES,
    DEFAULT_TOOL_TIMEOUT_MS, MAX_MAX_CONCURRENT_TOOL_CALLS, MAX_TOOL_RETRIES, MAX_TOOL_TIMEOUT_MS,
    MIN_MAX_CONCURRENT_TOOL_CALLS, MIN_TOOL_TIMEOUT_MS, ToolRegistryContractError,
};
use crate::ToolNetworkProxyV1;

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSettingsSnapshotV1 {
    pub revision: Revision,
    pub content_hash: CanonicalHashV1,
    pub max_dynamic_tool_schemas: u32,
    pub max_concurrent_tool_calls: u32,
    pub default_timeout_ms: u64,
    pub max_retries: u8,
    /// Absent in pre-L01 snapshots. Absence preserves their original serialization and hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_proxy: Option<ToolNetworkProxyV1>,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ToolSettingsHashMaterial<'a> {
    revision: Revision,
    max_dynamic_tool_schemas: u32,
    max_concurrent_tool_calls: u32,
    default_timeout_ms: u64,
    max_retries: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    network_proxy: Option<&'a ToolNetworkProxyV1>,
}

impl ToolSettingsSnapshotV1 {
    pub fn new(
        revision: Revision,
        max_dynamic_tool_schemas: u32,
        max_concurrent_tool_calls: u32,
        default_timeout_ms: u64,
        max_retries: u8,
    ) -> Result<Self, ToolRegistryContractError> {
        let content_hash = canonical_sha256(&ToolSettingsHashMaterial {
            revision,
            max_dynamic_tool_schemas,
            max_concurrent_tool_calls,
            default_timeout_ms,
            max_retries,
            network_proxy: None,
        })
        .map_err(|_| ToolRegistryContractError::SettingsHashMismatch)?;
        let value = Self {
            revision,
            content_hash,
            max_dynamic_tool_schemas,
            max_concurrent_tool_calls,
            default_timeout_ms,
            max_retries,
            network_proxy: None,
        };
        value.validate_values()?;
        Ok(value)
    }

    pub fn with_network_proxy(
        mut self,
        proxy: Option<ToolNetworkProxyV1>,
    ) -> Result<Self, ToolRegistryContractError> {
        self.network_proxy = proxy;
        self.validate_values()?;
        self.content_hash = self.hash()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), ToolRegistryContractError> {
        self.validate_values()?;
        if self.hash()? != self.content_hash {
            return Err(ToolRegistryContractError::SettingsHashMismatch);
        }
        Ok(())
    }

    fn validate_values(&self) -> Result<(), ToolRegistryContractError> {
        if self.max_dynamic_tool_schemas == 0
            || !(MIN_MAX_CONCURRENT_TOOL_CALLS..=MAX_MAX_CONCURRENT_TOOL_CALLS)
                .contains(&self.max_concurrent_tool_calls)
            || !(MIN_TOOL_TIMEOUT_MS..=MAX_TOOL_TIMEOUT_MS).contains(&self.default_timeout_ms)
            || self.max_retries > MAX_TOOL_RETRIES
        {
            return Err(ToolRegistryContractError::InvalidSettings);
        }
        if let Some(proxy) = &self.network_proxy {
            proxy.validate()?;
        }
        Ok(())
    }

    fn hash(&self) -> Result<CanonicalHashV1, ToolRegistryContractError> {
        canonical_sha256(&ToolSettingsHashMaterial {
            revision: self.revision,
            max_dynamic_tool_schemas: self.max_dynamic_tool_schemas,
            max_concurrent_tool_calls: self.max_concurrent_tool_calls,
            default_timeout_ms: self.default_timeout_ms,
            max_retries: self.max_retries,
            network_proxy: self.network_proxy.as_ref(),
        })
        .map_err(|_| ToolRegistryContractError::SettingsHashMismatch)
    }
}

impl Default for ToolSettingsSnapshotV1 {
    fn default() -> Self {
        Self::new(
            Revision::INITIAL,
            DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS,
            DEFAULT_MAX_CONCURRENT_TOOL_CALLS,
            DEFAULT_TOOL_TIMEOUT_MS,
            DEFAULT_MAX_TOOL_RETRIES,
        )
        .expect("frozen default tool settings are valid")
    }
}

impl<'de> Deserialize<'de> for ToolSettingsSnapshotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSettings {
            revision: Revision,
            content_hash: CanonicalHashV1,
            max_dynamic_tool_schemas: u32,
            max_concurrent_tool_calls: u32,
            default_timeout_ms: u64,
            max_retries: u8,
            #[serde(default)]
            network_proxy: Option<ToolNetworkProxyV1>,
        }
        let wire = WireSettings::deserialize(deserializer)?;
        let snapshot = Self {
            revision: wire.revision,
            content_hash: wire.content_hash,
            max_dynamic_tool_schemas: wire.max_dynamic_tool_schemas,
            max_concurrent_tool_calls: wire.max_concurrent_tool_calls,
            default_timeout_ms: wire.default_timeout_ms,
            max_retries: wire.max_retries,
            network_proxy: wire.network_proxy,
        };
        snapshot.validate().map_err(D::Error::custom)?;
        Ok(snapshot)
    }
}
