//! The Hook registry in `hooks.toml`. It keeps package paths in execution order, whether
//! each is enabled and the fingerprint the user trusted; the packages stay where they are.
use crate::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, SettingsFacade, SettingsFacadeError,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const HOOKS: &str = "hooks";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookRegistration {
    pub path: String,
    pub enabled: bool,
    /// Absent until the user trusts the package's current content.
    pub trusted_fingerprint: Option<String>,
}

/// The registry in execution order. When `hooks.toml` cannot be read, no Hook runs and
/// `issue` says why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookRegistry {
    pub entries: Vec<HookRegistration>,
    pub revision: u64,
    pub issue: Option<String>,
}

pub(crate) fn validate_hooks_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    registrations(&document.values).map(|_| ())
}

fn invalid(message: impl Into<String>) -> ConfigValidationError {
    ConfigValidationError::Semantic(format!("hooks.toml: {}", message.into()))
}

fn registrations(
    values: &BTreeMap<String, Value>,
) -> Result<Vec<HookRegistration>, ConfigValidationError> {
    if let Some(field) = values.keys().find(|key| key.as_str() != HOOKS) {
        return Err(ConfigValidationError::UnknownModuleField {
            module: ConfigModule::Hooks,
            field: field.clone(),
        });
    }
    let Some(items) = values.get(HOOKS) else { return Ok(Vec::new()) };
    let items = items.as_array().ok_or_else(|| invalid("hooks 必须是列表"))?;
    let mut entries = Vec::<HookRegistration>::new();
    for item in items {
        let object = item.as_object().ok_or_else(|| invalid("每个 Hook 必须是表"))?;
        if let Some(field) = object
            .keys()
            .find(|k| !["path", "enabled", "trusted_fingerprint"].contains(&k.as_str()))
        {
            return Err(invalid(format!("不认识的字段 {field}")));
        }
        let path = object
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| invalid("path 必须是文字"))?;
        let enabled = object
            .get("enabled")
            .map_or(Some(false), Value::as_bool)
            .ok_or_else(|| invalid("enabled 必须是 true 或 false"))?;
        let trusted_fingerprint = match object.get("trusted_fingerprint") {
            None => None,
            Some(value) => Some(
                value
                    .as_str()
                    .filter(|f| f.len() == 64 && f.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| invalid("trusted_fingerprint 必须是 64 位十六进制"))?
                    .to_owned(),
            ),
        };
        if entries.iter().any(|entry| entry.path.eq_ignore_ascii_case(path)) {
            return Err(invalid(format!("{path} 重复")));
        }
        entries.push(HookRegistration { path: path.to_owned(), enabled, trusted_fingerprint });
    }
    Ok(entries)
}

impl SettingsFacade {
    /// The registry in force. When the domain is not healthy, or the served document is a
    /// last-known-good fallback rather than the file on disk, no Hook runs: an edit that
    /// failed may have been the user removing one.
    pub fn hook_registry(&self) -> HookRegistry {
        let module = ConfigModule::Hooks;
        let snapshot = match self.snapshot(module) {
            Ok(snapshot)
                if self.health(module).state == crate::DomainHealthState::Ready
                    && crate::memory::current_input_matches(self, module, &snapshot) =>
            {
                snapshot
            }
            _ => {
                return HookRegistry {
                    issue: Some(
                        "hooks.toml 当前无法使用，所有 Hook 都不运行。请在设置里重试、恢复上一版有效配置或重置。".into(),
                    ),
                    ..HookRegistry::default()
                };
            }
        };
        let revision = snapshot.document.revision;
        match registrations(&snapshot.document.values) {
            Ok(entries) => HookRegistry { entries, revision, issue: None },
            Err(error) => HookRegistry {
                entries: Vec::new(),
                revision,
                issue: Some(format!("{error}；所有 Hook 都不运行")),
            },
        }
    }

    /// Replaces the registry at `expected_revision`. A concurrent change is a conflict for
    /// the caller to reread, never a silent overwrite.
    pub fn replace_hook_registry(
        &self,
        expected_revision: u64,
        entries: &[HookRegistration],
    ) -> Result<u64, SettingsFacadeError> {
        let items = entries
            .iter()
            .map(|entry| {
                let mut item = json!({"path": entry.path, "enabled": entry.enabled});
                if let Some(fingerprint) = &entry.trusted_fingerprint {
                    item["trusted_fingerprint"] = json!(fingerprint);
                }
                item
            })
            .collect::<Vec<_>>();
        let snapshot = self.update(ConfigModule::Hooks, expected_revision, |values| {
            if items.is_empty() {
                values.remove(HOOKS);
            } else {
                values.insert(HOOKS.to_owned(), Value::Array(items));
            }
        })?;
        Ok(snapshot.document.revision)
    }
}
