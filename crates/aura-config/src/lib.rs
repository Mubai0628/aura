#![forbid(unsafe_code)]

mod browser;
pub use browser::{
    BROWSER_SITE_LIST_MAX, BrowserSettings, BrowserSettingsSnapshot, BrowserSettingsUpdate,
};
mod chrome;
pub use chrome::{ChromeControlSettings, ChromeControlSettingsSnapshot, ChromePairing};
mod commands;
pub use commands::DisabledCommands;
mod hooks;
pub use hooks::{HookRegistration, HookRegistry};
mod mcp;
pub use mcp::McpRegistry;
mod plugins;
pub use plugins::PluginRegistry;
mod skills;
pub use skills::SkillRegistry;
mod document;
mod health;
mod journal;
mod memory;
mod module_validation;
pub use memory::{MemorySettings, MemorySettingsSnapshot, MemorySettingsUpdate};
pub(crate) use module_validation::validate_builtin_module_document;
mod network_proxy;
pub use network_proxy::{
    NETWORK_PROXY_FIELD, NetworkProxyConfigState, NetworkProxyRecoveryPreview,
    network_proxy_draft_from_value, network_proxy_from_document, network_proxy_state_from_document,
};
mod ownership;
mod paths;
mod prompt;
pub mod recovery;
pub mod recovery_contract;
mod service;
mod tools;
mod ui;
pub use ui::{DefaultTerminal, UiSettings, UiSettingsSnapshot, UiSettingsUpdate};
mod watch;

pub use document::{
    BaseModuleConfigValidator, CONFIG_SCHEMA_VERSION, ConfigValidationError, DocumentCodecError,
    ModuleConfigDocument, ModuleConfigValidator, encode_document, parse_document,
    validate_base_document,
};
pub use health::{
    ConfigHealthRegistry, DomainFaultKind, DomainHealth, DomainHealthState, DomainRecoveryAttempt,
    SharedConfigHealth, degraded_summary,
};
pub use journal::{
    ConfigTransactionError, ConfigTransactionHandle, ConfigTransactionJournal,
    ConfigTransactionRecovery, ConfigTransactionRecoveryAction, ConfigTransactionState,
    ConfigTransactionTarget,
};
pub use ownership::{DomainClaim, DomainClaimBusy, DomainClaimGuard, DomainClaimRegistry};
pub use paths::{AuraPaths, AuraPathsError, ConfigFormat, ConfigModule};
pub use prompt::{CUSTOM_SYSTEM_ADDENDUM_FIELD, PromptSettingsSnapshot};
pub use service::{
    ConfigBundleFaultPoint, ConfigEvent, ConfigEventKind, ConfigFaultPoint,
    ConfigModuleBundleEntry, ConfigSnapshot, ConfigSource, ConfigWatchHandle,
    DOMAIN_UNAVAILABLE_CODE, PreparedConfigBundle, PreparedConfigBundleEntry,
    SETTINGS_FILE_MAX_BYTES, SETTINGS_FILE_TOO_LARGE_CODE, SettingsFacade, SettingsFacadeError,
};
pub use tools::{
    DEFAULT_TOOL_POLICY_FIELDS, FIXED_TOOL_IDS_FIELD, FIXED_TOOL_RECOMMENDATIONS_FIELD,
    MAX_CONCURRENT_TOOL_CALLS_FIELD, MAX_DYNAMIC_TOOL_SCHEMAS_FIELD, MAX_RETRIES_FIELD,
    TOOL_DEFAULT_TIMEOUT_MS_FIELD, WEB_SERVICE_FIELD, fixed_tool_ids_from_document,
    fixed_tool_recommendations_from_document, tool_settings_snapshot_from_document,
    web_service_from_document,
};
pub use watch::WATCH_WORKERS;
