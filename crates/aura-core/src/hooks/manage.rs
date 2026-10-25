//! Settings → Hook: list, preview, trust, untrust, enable, remove and reorder. Every change
//! names the registry revision it was made against; a package is trusted only at the exact
//! fingerprint the user previewed. Removing a Hook never touches its folder.
use super::registry;
use aura_config::{HookRegistration, SettingsFacade, SettingsFacadeError};
use aura_hooks::{
    HookInvocationListRequest, HookListRequest, HookMoveDirection, HookMoveRequest,
    HookPathRequest, HookPreview, HookPreviewRequest, HookRegistryView, HookSetEnabledRequest,
    HookTrustRequest, scan_package,
};
use std::path::Path;
use std::sync::Arc;

pub(crate) struct HookContext {
    settings: Arc<SettingsFacade>,
    journal: Arc<crate::broker::BrokerJournal>,
    plugins: Option<Arc<crate::plugins::PluginRuntime>>,
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_end_matches(['\\', '/']).eq_ignore_ascii_case(b.trim_end_matches(['\\', '/']))
}

/// The registry key: an absolute path as the user chose it, without a trailing separator
/// or the `\\?\` prefix, so the settings page shows what the user picked.
fn normalized(path: &str) -> anyhow::Result<String> {
    let trimmed = path.trim().trim_end_matches(['\\', '/']);
    anyhow::ensure!(
        !trimmed.starts_with(r"\\?\") && Path::new(trimmed).is_absolute(),
        "hooks.invalid_path: 请选择一个完整的文件夹路径"
    );
    Ok(trimmed.to_owned())
}

impl HookContext {
    pub(crate) fn of(services: &crate::RuntimeServices) -> Self {
        Self {
            settings: services._settings.clone(),
            journal: services.broker.clone(),
            plugins: Some(services.plugins.clone()),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(
        settings: Arc<SettingsFacade>,
        journal: Arc<crate::broker::BrokerJournal>,
    ) -> Self {
        Self { settings, journal, plugins: None }
    }

    /// Runs one `agent.hooks.*` Core route.
    pub(crate) fn dispatch(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(match method {
            "agent.hooks.list" => {
                let _: HookListRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.view())?
            }
            "agent.hooks.preview" => {
                let request: HookPreviewRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.preview(&request.path)?)?
            }
            "agent.hooks.trust" => {
                serde_json::to_value(self.trust(serde_json::from_value(params)?)?)?
            }
            "agent.hooks.untrust" => {
                let request: HookPathRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.change(
                    &request.path,
                    request.expected_revision,
                    |entries, index| {
                        entries[index].trusted_fingerprint = None;
                    },
                )?)?
            }
            "agent.hooks.set_enabled" => {
                let request: HookSetEnabledRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.change(
                    &request.path,
                    request.expected_revision,
                    |entries, index| {
                        entries[index].enabled = request.enabled;
                    },
                )?)?
            }
            "agent.hooks.remove" => {
                let request: HookPathRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.change(
                    &request.path,
                    request.expected_revision,
                    |entries, index| {
                        entries.remove(index);
                    },
                )?)?
            }
            "agent.hooks.move" => {
                let request: HookMoveRequest = serde_json::from_value(params)?;
                serde_json::to_value(self.reorder(request)?)?
            }
            "agent.hooks.invocations" => {
                let request: HookInvocationListRequest = serde_json::from_value(params)?;
                let store = self.journal.lock()?;
                let db = store.database.connection();
                serde_json::to_value(aura_hooks::HookInvocationList {
                    invocations: aura_persistence_v2::hooks::list(db, &request)?,
                    statistics: request
                        .turn_task_id
                        .map(|turn| aura_persistence_v2::hooks::turn_statistics(db, turn))
                        .transpose()?,
                })?
            }
            other => anyhow::bail!("hooks.unknown_route: {other}"),
        })
    }

    fn view(&self) -> HookRegistryView {
        let registry = self.settings.hook_registry();
        let mut entries = registry::entry_views(&registry);
        if let Some(plugins) = &self.plugins {
            entries.extend(registry::plugin_entry_views(plugins));
        }
        HookRegistryView { revision: registry.revision, entries }
    }

    fn preview(&self, path: &str) -> anyhow::Result<HookPreview> {
        let path = normalized(path)?;
        let registered =
            self.settings.hook_registry().entries.iter().any(|entry| same_path(&entry.path, &path));
        Ok(match scan_package(Path::new(&path)) {
            Ok(package) => HookPreview {
                path,
                definition: Some(package.definition),
                files: package.files,
                fingerprint: Some(package.fingerprint),
                problems: Vec::new(),
                registered,
            },
            Err(error) => HookPreview {
                path,
                definition: None,
                files: Vec::new(),
                fingerprint: None,
                problems: vec![error.to_string()],
                registered,
            },
        })
    }

    fn trust(&self, request: HookTrustRequest) -> anyhow::Result<HookRegistryView> {
        let path = normalized(&request.path)?;
        let package = scan_package(Path::new(&path))
            .map_err(|error| anyhow::anyhow!("hooks.invalid_package: {error}"))?;
        anyhow::ensure!(
            package.fingerprint == request.expected_fingerprint,
            "hooks.content_changed: Hook 包在预览之后变了，请重新预览再信任"
        );
        let registry = self.settings.hook_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "hooks.registry_unavailable: {}",
            registry.issue.unwrap_or_default()
        );
        let mut entries = registry.entries;
        match entries.iter_mut().find(|entry| same_path(&entry.path, &path)) {
            Some(entry) => {
                entry.trusted_fingerprint = Some(package.fingerprint);
                entry.enabled = request.enabled;
            }
            None => entries.push(HookRegistration {
                path,
                enabled: request.enabled,
                trusted_fingerprint: Some(package.fingerprint),
            }),
        }
        self.write(request.expected_revision, &entries)
    }

    fn change(
        &self,
        path: &str,
        expected_revision: u64,
        edit: impl FnOnce(&mut Vec<HookRegistration>, usize),
    ) -> anyhow::Result<HookRegistryView> {
        let registry = self.settings.hook_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "hooks.registry_unavailable: {}",
            registry.issue.unwrap_or_default()
        );
        let mut entries = registry.entries;
        let index = entries
            .iter()
            .position(|entry| same_path(&entry.path, path))
            .ok_or_else(|| anyhow::anyhow!("hooks.not_found: 这个 Hook 不在列表里，请刷新"))?;
        edit(&mut entries, index);
        self.write(expected_revision, &entries)
    }

    fn reorder(&self, request: HookMoveRequest) -> anyhow::Result<HookRegistryView> {
        self.change(&request.path, request.expected_revision, |entries, index| {
            let other = match request.direction {
                HookMoveDirection::Up => index.checked_sub(1),
                HookMoveDirection::Down => (index + 1 < entries.len()).then_some(index + 1),
            };
            if let Some(other) = other {
                entries.swap(index, other);
            }
        })
    }

    fn write(
        &self,
        expected_revision: u64,
        entries: &[HookRegistration],
    ) -> anyhow::Result<HookRegistryView> {
        match self.settings.replace_hook_registry(expected_revision, entries) {
            Ok(_) => Ok(self.view()),
            Err(SettingsFacadeError::RevisionConflict { .. }) => {
                anyhow::bail!("hooks.revision_changed: Hook 列表已被修改，请刷新后再操作")
            }
            Err(error) => Err(error.into()),
        }
    }
}
