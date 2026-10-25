//! 设置 → 网络与浏览器. Whether the built-in browser is on, the sites a ReadOnly session opens
//! without asking, the sites it never loads, and the advanced debugging switch.
//!
//! Unlike the interface settings, a damaged file does not fall back to the defaults: the defaults
//! have an empty block list, and showing them would quietly unblock what the user blocked. The
//! browser is reported off with an issue until the user retries, restores or resets the domain.
use crate::memory::current_input_matches;
use crate::{
    ConfigModule, ConfigSnapshot, ConfigValidationError, ModuleConfigDocument, SettingsFacade,
    SettingsFacadeError,
};
use aura_browser_domain::BrowserSiteRule;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// At most this many sites in each list.
pub const BROWSER_SITE_LIST_MAX: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserSettings {
    /// Whether the built-in browser can be used at all (default on).
    pub enabled: bool,
    /// Sites a ReadOnly session opens without asking.
    pub allowed_sites: Vec<String>,
    /// Sites the browser never loads, in every mode, including requests a page makes itself.
    pub blocked_sites: Vec<String>,
    /// Advanced: lets the model run scripts in pages and read full network records (default off).
    pub debugging: bool,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self { enabled: true, allowed_sites: vec![], blocked_sites: vec![], debugging: false }
    }
}

impl BrowserSettings {
    /// The rules, already checked when the settings were read or saved.
    #[must_use]
    pub fn allowed_rules(&self) -> Vec<BrowserSiteRule> {
        self.allowed_sites.iter().filter_map(|site| BrowserSiteRule::parse(site).ok()).collect()
    }
    #[must_use]
    pub fn blocked_rules(&self) -> Vec<BrowserSiteRule> {
        self.blocked_sites.iter().filter_map(|site| BrowserSiteRule::parse(site).ok()).collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserSettingsSnapshot {
    pub revision: Option<u64>,
    pub settings: BrowserSettings,
    pub issue: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrowserSettingsUpdate {
    pub expected_revision: u64,
    pub settings: BrowserSettings,
}

fn invalid(field: &str, message: String) -> ConfigValidationError {
    ConfigValidationError::Semantic(format!("{field}: {message}"))
}

/// The two lists in canonical form: every rule valid, none twice, none in both.
fn checked_sites(
    allowed: &[String],
    blocked: &[String],
) -> Result<(Vec<String>, Vec<String>), ConfigValidationError> {
    let canonical = |field: &str, sites: &[String]| -> Result<Vec<String>, ConfigValidationError> {
        if sites.len() > BROWSER_SITE_LIST_MAX {
            return Err(invalid(field, format!("最多 {BROWSER_SITE_LIST_MAX} 个网站")));
        }
        let mut seen = Vec::with_capacity(sites.len());
        for site in sites {
            let rule = BrowserSiteRule::parse(site)
                .map_err(|error| invalid(field, format!("“{site}”：{error}")))?
                .to_string();
            if seen.contains(&rule) {
                return Err(invalid(field, format!("“{rule}”重复")));
            }
            seen.push(rule);
        }
        Ok(seen)
    };
    let allowed = canonical("allowed_sites", allowed)?;
    let blocked = canonical("blocked_sites", blocked)?;
    if let Some(both) = allowed.iter().find(|site| blocked.contains(site)) {
        return Err(invalid("blocked_sites", format!("“{both}”不能同时允许和阻止")));
    }
    Ok((allowed, blocked))
}

pub(crate) fn settings_from_document(
    document: &ModuleConfigDocument,
) -> Result<BrowserSettings, ConfigValidationError> {
    let mut settings = BrowserSettings::default();
    let flag = |field: &str| -> Result<Option<bool>, ConfigValidationError> {
        document
            .values
            .get(field)
            .map(|value| {
                value.as_bool().ok_or_else(|| ConfigValidationError::InvalidModuleFieldType {
                    module: ConfigModule::Browser,
                    field: field.into(),
                    expected: "boolean",
                })
            })
            .transpose()
    };
    let list = |field: &str| -> Result<Vec<String>, ConfigValidationError> {
        let Some(value) = document.values.get(field) else { return Ok(vec![]) };
        value
            .as_array()
            .and_then(|items| items.iter().map(|item| item.as_str().map(str::to_owned)).collect())
            .ok_or_else(|| ConfigValidationError::InvalidModuleFieldType {
                module: ConfigModule::Browser,
                field: field.into(),
                expected: "array of strings",
            })
    };
    if let Some(enabled) = flag("enabled")? {
        settings.enabled = enabled;
    }
    if let Some(debugging) = flag("debugging")? {
        settings.debugging = debugging;
    }
    (settings.allowed_sites, settings.blocked_sites) =
        checked_sites(&list("allowed_sites")?, &list("blocked_sites")?)?;
    Ok(settings)
}

impl SettingsFacade {
    pub fn browser_settings(&self) -> BrowserSettingsSnapshot {
        if self.health(ConfigModule::Browser).state == crate::DomainHealthState::Ready
            && let Ok(snapshot) = self.snapshot(ConfigModule::Browser)
            && let Ok(settings) = settings_from_document(&snapshot.document)
            && current_input_matches(self, ConfigModule::Browser, &snapshot)
        {
            return BrowserSettingsSnapshot {
                revision: Some(snapshot.document.revision),
                settings,
                issue: None,
            };
        }
        BrowserSettingsSnapshot {
            revision: None,
            settings: BrowserSettings { enabled: false, ..BrowserSettings::default() },
            issue: Some(
                "浏览器设置不可用，内置浏览器已暂停。请重试、恢复上一版有效配置或重置此设置域。"
                    .into(),
            ),
        }
    }

    pub fn update_browser_settings(
        &self,
        expected_revision: u64,
        settings: &BrowserSettings,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        let (allowed, blocked) =
            checked_sites(&settings.allowed_sites, &settings.blocked_sites).map_err(|source| {
                SettingsFacadeError::Validation { module: ConfigModule::Browser, source }
            })?;
        self.update(ConfigModule::Browser, expected_revision, |values| {
            values.insert("enabled".into(), Value::Bool(settings.enabled));
            values.insert("debugging".into(), Value::Bool(settings.debugging));
            values.insert(
                "allowed_sites".into(),
                Value::Array(allowed.iter().cloned().map(Value::String).collect()),
            );
            values.insert(
                "blocked_sites".into(),
                Value::Array(blocked.iter().cloned().map(Value::String).collect()),
            );
        })
    }
}
