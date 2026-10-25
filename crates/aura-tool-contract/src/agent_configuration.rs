//! User-owned global defaults and model-owned session exposure are separate authorities.
use aura_domain_common::{EndpointId, Revision, SessionId, ToolId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolDescribeInput {
    pub tool_id: ToolId,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolLoadInput {
    pub tool_ids: Vec<ToolId>,
    /// Explicit dynamic members to remove atomically before loading tool_ids.
    pub replace: Vec<ToolId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolUnloadInput {
    pub tool_ids: Vec<ToolId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionDynamicTools {
    pub session_id: SessionId,
    pub revision: Revision,
    pub tool_ids: Vec<ToolId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolSettingsRequest {}

/// One revision-checked edit of every user-owned execution-strategy value.
/// Rust owns the ranges; the renderer never widens them by editing a generated
/// artifact. `fixed_tool_ids` is carried along so a strategy save cannot silently
/// drop a concurrently updated tool list.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolSettingsUpdate {
    pub expected_revision: Revision,
    pub fixed_tool_ids: Vec<ToolId>,
    #[schemars(range(min = 1))]
    pub max_dynamic_tool_schemas: u32,
    #[schemars(range(min = 1, max = 32))]
    pub max_concurrent_tool_calls: u32,
    #[schemars(range(min = 1000, max = 1800000))]
    pub default_timeout_ms: u64,
    #[schemars(range(min = 0, max = 3))]
    pub max_retries: u8,
    /// Restore the recommended default tool list instead of `fixed_tool_ids`.
    ///
    /// A user who once saved their own list keeps it across upgrades, so the
    /// recommended list only reaches an untouched configuration. This is the
    /// explicit user action that opts back in; it is never applied silently.
    #[serde(default)]
    pub use_recommended_fixed_tool_ids: bool,
    /// Absent leaves the stored proxy untouched. JSON `null` clears it. An object replaces it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_proxy_edit"
    )]
    pub network_proxy: Option<Option<crate::ToolNetworkProxyV1>>,
    /// Absent leaves the stored web service untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_service: Option<WebServiceSettingV1>,
}

/// The service `web.search` and `web.fetch` use.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebServiceSettingV1 {
    /// DeepSeek's search and fetch through the first DeepSeek connection with a key; this
    /// computer's own search and fetch when there is none.
    #[default]
    Automatic,
    /// This computer's own search (Bing's result page) and fetch.
    Local,
    /// DeepSeek's search and fetch through this connection and its key.
    #[serde(rename = "deepseek")]
    DeepSeek { endpoint_id: EndpointId },
}

/// A model connection the web service can use: DeepSeek's own API, with a key.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WebServiceConnectionV1 {
    pub endpoint_id: EndpointId,
    pub display_name: String,
}

fn deserialize_proxy_edit<'de, D>(
    deserializer: D,
) -> Result<Option<Option<crate::ToolNetworkProxyV1>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // A present null is a clear. Absence uses the field default and never calls this.
    Ok(Some(Option::<crate::ToolNetworkProxyV1>::deserialize(deserializer)?))
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolCatalogEntry {
    pub tool_id: ToolId,
    pub name: String,
    pub description: String,
    pub source: String,
    pub default_loaded: bool,
    pub required: bool,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentToolSettingsView {
    pub revision: Revision,
    pub max_dynamic_tool_schemas: u32,
    pub max_concurrent_tool_calls: u32,
    pub default_timeout_ms: u64,
    pub max_retries: u8,
    pub fixed_tool_ids: Vec<ToolId>,
    pub tools: Vec<AgentToolCatalogEntry>,
    /// The proxy that tools will actually use. Empty when disabled or the stored field is unusable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_proxy: Option<crate::ToolNetworkProxyV1>,
    /// Why the stored field is not in effect. Other Tools settings stay readable and writable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_proxy_diagnostic: Option<String>,
    /// The stored field, including a value that failed validation, so the form can keep it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_proxy_draft: Option<AgentNetworkProxyDraftV1>,
    /// A field-only recovery preview. Applying it must use this preview's Tools revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_proxy_recovery: Option<AgentNetworkProxyRecoveryPreviewV1>,
    /// The stored web service choice.
    #[serde(default)]
    pub web_service: WebServiceSettingV1,
    /// The DeepSeek connections with a key it can use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub web_service_connections: Vec<WebServiceConnectionV1>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentNetworkProxyRecoveryPreviewV1 {
    pub expected_revision: Revision,
    pub source_revision: Revision,
    /// Null restores the absence of a proxy; no other setting is restored.
    pub proxy: Option<crate::ToolNetworkProxyV1>,
}

/// What the settings form shows for the proxy field. It is not a validated proxy.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentNetworkProxyDraftV1 {
    pub enabled: bool,
    pub protocol: String,
    pub host: String,
    pub port: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentNetworkProxyTestRequest {
    pub protocol: crate::ToolNetworkProxyProtocolV1,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentNetworkProxyTestResult {
    pub ok: bool,
    pub message: String,
}

/// Which execution-strategy field a rejected update named. The renderer can put
/// the message on that field instead of showing one opaque failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentToolSettingsField {
    MaxDynamicToolSchemas,
    MaxConcurrentToolCalls,
    DefaultTimeoutMs,
    MaxRetries,
    FixedToolIds,
}

impl AgentToolSettingsField {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MaxDynamicToolSchemas => "max_dynamic_tool_schemas",
            Self::MaxConcurrentToolCalls => "max_concurrent_tool_calls",
            Self::DefaultTimeoutMs => "default_timeout_ms",
            Self::MaxRetries => "max_retries",
            Self::FixedToolIds => "fixed_tool_ids",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentToolSettingsInvalid {
    pub field: AgentToolSettingsField,
}

impl std::fmt::Display for AgentToolSettingsInvalid {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "tools.invalid_setting:{}", self.field.label())
    }
}

impl std::error::Error for AgentToolSettingsInvalid {}

impl AgentToolSettingsUpdate {
    /// The single Rust-side range check for all four strategy values. Field-level
    /// so a rejected save points at the offending input, never a blanket error.
    pub fn validate(&self) -> Result<(), AgentToolSettingsInvalid> {
        use crate::registry::{
            MAX_MAX_CONCURRENT_TOOL_CALLS, MAX_TOOL_RETRIES, MAX_TOOL_TIMEOUT_MS,
            MIN_MAX_CONCURRENT_TOOL_CALLS, MIN_TOOL_TIMEOUT_MS,
        };
        let invalid = |field| Err(AgentToolSettingsInvalid { field });
        if self.max_dynamic_tool_schemas == 0 {
            return invalid(AgentToolSettingsField::MaxDynamicToolSchemas);
        }
        if !(MIN_MAX_CONCURRENT_TOOL_CALLS..=MAX_MAX_CONCURRENT_TOOL_CALLS)
            .contains(&self.max_concurrent_tool_calls)
        {
            return invalid(AgentToolSettingsField::MaxConcurrentToolCalls);
        }
        if !(MIN_TOOL_TIMEOUT_MS..=MAX_TOOL_TIMEOUT_MS).contains(&self.default_timeout_ms) {
            return invalid(AgentToolSettingsField::DefaultTimeoutMs);
        }
        if self.max_retries > MAX_TOOL_RETRIES {
            return invalid(AgentToolSettingsField::MaxRetries);
        }
        if !self.fixed_tool_ids.windows(2).all(|pair| pair[0] < pair[1]) {
            return invalid(AgentToolSettingsField::FixedToolIds);
        }
        Ok(())
    }

    /// The matching global snapshot. Callers persist both from one input so the
    /// view and the real scheduler capacity cannot disagree.
    pub fn snapshot(
        &self,
    ) -> Result<crate::registry::ToolSettingsSnapshotV1, crate::registry::ToolRegistryContractError>
    {
        crate::registry::ToolSettingsSnapshotV1::new(
            self.expected_revision,
            self.max_dynamic_tool_schemas,
            self.max_concurrent_tool_calls,
            self.default_timeout_ms,
            self.max_retries,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::AgentToolLoadInput;

    #[test]
    fn load_input_accepts_bare_tool_ids_with_an_explicit_replace_list() {
        let input: AgentToolLoadInput = serde_json::from_value(serde_json::json!({
            "tool_ids": ["tol_01ARZ3NDEKTSV4RRFFQ69G5FAV"],
            "replace": []
        }))
        .expect("current load schema");
        assert_eq!(input.tool_ids.len(), 1);
        assert!(input.replace.is_empty());
        assert!(
            serde_json::from_value::<AgentToolLoadInput>(serde_json::json!({
                "tool_ids": ["tol_01ARZ3NDEKTSV4RRFFQ69G5FAV"]
            }))
            .is_err(),
            "replace is required so a load cannot silently keep stale dynamic members"
        );
    }

    #[test]
    fn load_input_rejects_the_retired_definitions_shape() {
        assert!(
            serde_json::from_value::<AgentToolLoadInput>(serde_json::json!({
                "definitions": [{
                    "tool_id": "tol_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                    "tool_version": "1.0.0",
                    "registry_snapshot_ref": "trs_01ARZ3NDEKTSV4RRFFQ69G5FAV"
                }]
            }))
            .is_err(),
            "deny_unknown_fields must keep rejecting the pre-split load shape"
        );
    }

    #[test]
    fn a_strategy_save_can_omit_the_proxy_and_null_clears_it() {
        let mut value = serde_json::json!({
            "expected_revision": "1",
            "fixed_tool_ids": [],
            "max_dynamic_tool_schemas": 1,
            "max_concurrent_tool_calls": 1,
            "default_timeout_ms": 1000,
            "max_retries": 0
        });
        let omitted: super::AgentToolSettingsUpdate =
            serde_json::from_value(value.clone()).expect("old strategy payload");
        assert!(omitted.network_proxy.is_none());
        value["network_proxy"] = serde_json::Value::Null;
        let cleared: super::AgentToolSettingsUpdate =
            serde_json::from_value(value.clone()).expect("clear");
        assert_eq!(cleared.network_proxy, Some(None));
        value["network_proxy"] = serde_json::json!({
            "enabled": true,
            "protocol": "socks5",
            "host": "127.0.0.1",
            "port": 10808
        });
        let saved: super::AgentToolSettingsUpdate =
            serde_json::from_value(value.clone()).expect("proxy");
        let proxy = saved.network_proxy.flatten().expect("proxy object");
        assert!(proxy.enabled);
        assert_eq!(proxy.port, 10808);
        value["network_proxy"]["host"] = serde_json::json!("example.com");
        assert!(serde_json::from_value::<super::AgentToolSettingsUpdate>(value).is_err());
    }
}
