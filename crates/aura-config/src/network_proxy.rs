//! Proxy faults are local to the proxy field: behave as disabled,
//! retain the faulty input for repair, and keep other Tools values readable and writable.
use std::{net::SocketAddr, sync::Arc};

use aura_tool_contract::{ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
use serde_json::Value;

use crate::{
    ConfigModule, ConfigSnapshot, ConfigValidationError, ModuleConfigDocument, SettingsFacade,
    SettingsFacadeError,
};

pub const NETWORK_PROXY_FIELD: &str = "network_proxy";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NetworkProxyConfigState {
    Disabled,
    Configured(ToolNetworkProxyV1),
    /// The fault remains inspectable even though the effective snapshot uses no proxy.
    Unavailable(ConfigValidationError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkProxyRecoveryPreview {
    pub expected_revision: u64,
    pub source_revision: u64,
    pub proxy: Option<ToolNetworkProxyV1>,
}

impl NetworkProxyConfigState {
    #[must_use]
    pub fn effective_proxy(&self) -> Option<ToolNetworkProxyV1> {
        match self {
            Self::Configured(proxy) => Some(proxy.clone()),
            Self::Disabled | Self::Unavailable(_) => None,
        }
    }
}

pub fn network_proxy_from_document(
    document: &ModuleConfigDocument,
) -> Result<Option<ToolNetworkProxyV1>, ConfigValidationError> {
    let Some(value) = document.values.get(NETWORK_PROXY_FIELD) else { return Ok(None) };
    let error = |field: &str, message: &str| {
        ConfigValidationError::Semantic(format!("network_proxy.{field}: {message}"))
    };
    let object = value.as_object().ok_or_else(|| error("enabled", "代理设置必须为对象"))?;
    for field in object.keys() {
        if !matches!(field.as_str(), "enabled" | "protocol" | "host" | "port") {
            return Err(error(field, "不支持此字段；代理不支持认证"));
        }
    }
    let enabled = object
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| error("enabled", "开关必须为布尔值"))?;
    let protocol = match object.get("protocol").and_then(Value::as_str) {
        Some("http") => ToolNetworkProxyProtocolV1::Http,
        Some("socks5") => ToolNetworkProxyProtocolV1::Socks5,
        _ => return Err(error("protocol", "协议必须为 http 或 socks5")),
    };
    let host = object
        .get("host")
        .and_then(Value::as_str)
        .ok_or_else(|| error("host", "地址必须为字符串"))?;
    let port = object
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| error("port", "端口必须为 1–65535"))?;
    ToolNetworkProxyV1::new(enabled, protocol, host.to_owned(), port)
        .map(Some)
        .map_err(|error| ConfigValidationError::Semantic(error.to_string()))
}

#[must_use]
pub fn network_proxy_draft_from_value(
    value: &Value,
) -> Option<aura_tool_contract::AgentNetworkProxyDraftV1> {
    let object = value.as_object()?;
    Some(aura_tool_contract::AgentNetworkProxyDraftV1 {
        enabled: object.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        protocol: object.get("protocol").and_then(Value::as_str).unwrap_or("").to_owned(),
        host: object.get("host").and_then(Value::as_str).unwrap_or("").to_owned(),
        port: object
            .get("port")
            .and_then(Value::as_u64)
            .and_then(|port| u32::try_from(port).ok())
            .unwrap_or(0),
    })
}

pub fn network_proxy_state_from_document(
    document: &ModuleConfigDocument,
) -> NetworkProxyConfigState {
    match network_proxy_from_document(document) {
        Ok(None) => NetworkProxyConfigState::Disabled,
        Ok(Some(proxy)) => NetworkProxyConfigState::Configured(proxy),
        Err(error) => NetworkProxyConfigState::Unavailable(error),
    }
}

pub(crate) fn validate_proxy_change(
    module: ConfigModule,
    before: &ModuleConfigDocument,
    candidate: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    if module == ConfigModule::Tools
        && before.values.get(NETWORK_PROXY_FIELD) != candidate.values.get(NETWORK_PROXY_FIELD)
    {
        network_proxy_from_document(candidate)?;
    }
    Ok(())
}

impl SettingsFacade {
    pub fn network_proxy_state(&self) -> Result<NetworkProxyConfigState, SettingsFacadeError> {
        Ok(network_proxy_state_from_document(&self.snapshot(ConfigModule::Tools)?.document))
    }

    /// Field-scoped repair. CAS, domain admission, persistence and current validation remain
    /// those of Tools; no other Tools key is reset, and credentials are never accessed here.
    pub fn update_network_proxy(
        &self,
        expected_revision: u64,
        proxy: Option<ToolNetworkProxyV1>,
        control_endpoints: &[SocketAddr],
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        if let Some(proxy) = &proxy {
            proxy.validate_control_endpoints(control_endpoints).map_err(|error| {
                SettingsFacadeError::Validation {
                    module: ConfigModule::Tools,
                    source: ConfigValidationError::Semantic(error.to_string()),
                }
            })?;
        }
        let wire = proxy.map(|proxy| serde_json::to_value(proxy).expect("proxy serialization"));
        self.update(ConfigModule::Tools, expected_revision, |values| {
            if let Some(wire) = wire {
                values.insert(NETWORK_PROXY_FIELD.into(), wire);
            } else {
                values.remove(NETWORK_PROXY_FIELD);
            }
        })
    }

    pub fn retry_network_proxy(&self) -> Result<NetworkProxyConfigState, SettingsFacadeError> {
        self.reload(ConfigModule::Tools)?;
        self.network_proxy_state()
    }

    /// The preview returns only the most recent valid proxy field, never an entire old Tools
    /// document. The caller can inspect it, then apply it through update_network_proxy's CAS.
    pub fn previous_valid_network_proxy(
        &self,
    ) -> Result<Option<NetworkProxyRecoveryPreview>, SettingsFacadeError> {
        let revision = self.snapshot(ConfigModule::Tools)?.document.revision;
        for version in crate::recovery::recorded_versions(self.paths(), ConfigModule::Tools)? {
            if version.revision >= revision {
                continue;
            }
            if let Ok(document) = crate::recovery::validate_recorded_version(
                ConfigModule::Tools,
                &version,
                &crate::BaseModuleConfigValidator,
            ) && let Ok(proxy) = network_proxy_from_document(&document)
            {
                return Ok(Some(NetworkProxyRecoveryPreview {
                    expected_revision: revision,
                    source_revision: version.revision,
                    proxy,
                }));
            }
        }
        Ok(None)
    }
}
