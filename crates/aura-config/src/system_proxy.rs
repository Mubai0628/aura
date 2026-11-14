//! The system's proxy, which model requests and tools follow while no proxy is switched on in
//! Aura's own settings (Tools' `network_proxy`). `settings/network.toml` holds an older Core's
//! choice whether tools followed it; this Core always does, and only keeps that file readable.
use std::sync::{Arc, PoisonError};

use aura_system_proxy::{SystemProxyLookup, SystemProxySource};
use aura_tool_contract::{ToolNetworkProxyV1, ToolSettingsSnapshotV1};
use serde_json::Value;

use crate::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, NetworkProxyConfigState,
    SettingsFacade, SettingsFacadeError,
};

/// An older Core's choice in `settings/network.toml`: `"system"` when its tools followed the
/// system's proxy.
pub const TOOLS_PROXY_FIELD: &str = "tools_proxy";
const FOLLOW_SYSTEM: &str = "system";

pub(crate) fn tools_follow_system_from_document(
    document: &ModuleConfigDocument,
) -> Result<bool, ConfigValidationError> {
    for field in document.values.keys() {
        if field != TOOLS_PROXY_FIELD {
            return Err(ConfigValidationError::UnknownModuleField {
                module: ConfigModule::Network,
                field: field.clone(),
            });
        }
    }
    match document.values.get(TOOLS_PROXY_FIELD) {
        None => Ok(false),
        Some(Value::String(value)) if value == FOLLOW_SYSTEM => Ok(true),
        Some(_) => Err(ConfigValidationError::InvalidModuleFieldType {
            module: ConfigModule::Network,
            field: TOOLS_PROXY_FIELD.into(),
            expected: "\"system\"",
        }),
    }
}

/// Where a facade reads the system's proxy until a test gives another source.
pub(crate) fn default_source() -> Arc<dyn SystemProxySource> {
    if cfg!(feature = "test-system-proxy") {
        Arc::new(aura_system_proxy::FixedSystemProxy(Ok(Default::default())))
    } else {
        Arc::new(aura_system_proxy::CurrentUserProxy)
    }
}

impl SettingsFacade {
    /// Where the system's proxy is read from; tests give fixed values.
    pub fn use_system_proxy_source(&self, source: Arc<dyn SystemProxySource>) {
        *self.system_proxy.lock().unwrap_or_else(PoisonError::into_inner) = source;
    }

    /// What the system's proxy is, read now.
    #[must_use]
    pub fn system_proxy(&self) -> SystemProxyLookup {
        let source = Arc::clone(&self.system_proxy.lock().unwrap_or_else(PoisonError::into_inner));
        aura_system_proxy::lookup(source.as_ref())
    }

    /// The proxy a model request takes: the one switched on in Aura's settings, else the
    /// system's, read now. A stored proxy Aura cannot read counts as switched off.
    #[must_use]
    pub fn model_proxy(&self) -> Option<ToolNetworkProxyV1> {
        match self.network_proxy_state() {
            Ok(NetworkProxyConfigState::Configured(proxy)) if proxy.enabled => Some(proxy),
            _ => self.system_proxy().proxy().cloned(),
        }
    }

    /// The tool settings a model step freezes.
    pub fn tool_settings_snapshot(&self) -> Result<ToolSettingsSnapshotV1, SettingsFacadeError> {
        let snapshot = self.snapshot(ConfigModule::Tools)?;
        self.tool_settings_from(&snapshot.document)
    }

    /// The tool settings as Tools stores them, without the system's proxy: what Aura's own update
    /// checks read, which use the system's proxy by themselves while Tools' own is switched off.
    pub fn stored_tool_settings_snapshot(
        &self,
    ) -> Result<ToolSettingsSnapshotV1, SettingsFacadeError> {
        let snapshot = self.snapshot(ConfigModule::Tools)?;
        crate::tool_settings_snapshot_from_document(&snapshot.document).map_err(|source| {
            SettingsFacadeError::Validation { module: ConfigModule::Tools, source }
        })
    }

    /// The tool settings a model step freezes from this Tools document: its own proxy while
    /// that is switched on, else the system's, read for each step. None Aura can use leaves
    /// tools on the system's own way out (direct, or a TUN proxy).
    pub fn tool_settings_from(
        &self,
        document: &ModuleConfigDocument,
    ) -> Result<ToolSettingsSnapshotV1, SettingsFacadeError> {
        let invalid =
            |source| SettingsFacadeError::Validation { module: ConfigModule::Tools, source };
        let settings = crate::tool_settings_snapshot_from_document(document).map_err(invalid)?;
        if settings.network_proxy.as_ref().is_some_and(|proxy| proxy.enabled) {
            return Ok(settings);
        }
        let system = self.system_proxy().proxy().cloned();
        // Unchanged settings keep their stored hash; nothing is serialized again.
        if settings.network_proxy == system {
            return Ok(settings);
        }
        settings
            .with_network_proxy(system)
            .map_err(|error| invalid(ConfigValidationError::Semantic(error.to_string())))
    }
}
