//! What the plugin page changes for a plugin: its background service's switch, its settings'
//! values, its scope, allowlist and remote approval switch. A secret setting's value arrives only
//! on the secret input and lives in the Credential Manager; only its credential is in
//! `plugins.json`. A plugin that declares `settings` may set its own text settings the same way;
//! nothing else here is the plugin's to change.
use super::lifecycle::replaced;
use super::runtime::PluginRuntime;
use super::service::secret_owner;
use aura_core_base::append_core_log;
use aura_plugins::{
    PluginConfigureRequest, PluginDeclarations, PluginOptionKind, PluginSecretSetRequest,
    PluginSecretSetResult, PluginSetting, check_settings,
};
use aura_windows_platform::SecretBytes;
use std::collections::BTreeMap;

/// The longest secret value a setting takes.
const MAX_SECRET_BYTES: usize = 8 * 1024;

fn option_kind(
    declared: &PluginDeclarations,
    name: &str,
    key: &str,
) -> anyhow::Result<PluginOptionKind> {
    declared
        .options
        .iter()
        .find(|option| option.key == key)
        .map(|option| option.kind)
        .ok_or_else(|| anyhow::anyhow!("plugins.setting_unknown: 插件 {name} 没有设置 {key}"))
}

impl PluginRuntime {
    pub(crate) fn configure(&self, request: &PluginConfigureRequest) -> anyhow::Result<()> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (registry, located) = self.located(&request.name, request.expected_revision)?;
        let name = located.name.clone();
        let package = located.scanned.as_ref().map_err(|_| {
            anyhow::anyhow!("plugins.unreadable: 插件 {name} 现在读不出来，它的设置改不了")
        })?;
        let declared = &package.manifest.declared;
        let mut setting =
            located.setting.clone().unwrap_or_else(|| PluginSetting::imported(&name, None));
        let remote_part = request.service.is_some()
            || request.scope.is_some()
            || request.allowed.is_some()
            || request.remote_approval.is_some();
        anyhow::ensure!(
            !remote_part || declared.service.is_some(),
            "plugins.no_service: 插件 {name} 没有后台服务"
        );
        if let Some(text) = &request.text {
            for (key, value) in text {
                anyhow::ensure!(
                    option_kind(declared, &name, key)? == PluginOptionKind::Text,
                    "plugins.setting_secret: {key} 是保密设置，用保密输入框设置"
                );
                match value.trim() {
                    "" => setting.values.text.remove(key),
                    value => setting.values.text.insert(key.clone(), value.to_owned()),
                };
            }
        }
        let forgotten: Vec<_> = request
            .forget_secrets
            .iter()
            .filter_map(|key| setting.values.secrets.remove(key))
            .collect();
        if let Some(on) = request.service {
            setting.remote.service = on;
        }
        if let Some(scope) = &request.scope {
            setting.remote.scope = scope.clone();
        }
        if let Some(allowed) = &request.allowed {
            let mut people = allowed.clone();
            for person in &mut people {
                anyhow::ensure!(
                    declared.channels.iter().any(|channel| channel.id == person.channel),
                    "plugins.channel_unknown: 插件 {name} 没有渠道 {}",
                    person.channel
                );
                person.id = person.id.trim().to_owned();
                person.name = person.name.trim().to_owned();
            }
            setting.remote.allowed = people;
        }
        if let Some(on) = request.remote_approval {
            setting.remote.remote_approval = on;
        }
        let plugins = replaced(&registry, setting);
        check_settings(&plugins)
            .map_err(|reason| anyhow::anyhow!("plugins.setting_invalid: {reason}"))?;
        self.write_registry(registry.revision, &plugins)?;
        self.forget_credentials(&forgotten);
        Ok(())
    }

    /// One of the plugin's text settings, set by the plugin itself as the plugin page sets it and
    /// with the same checks; an empty value clears it. Its service then starts again with the new
    /// value, as after the plugin page. The values of its text settings afterwards.
    pub fn set_text_setting(
        &self,
        name: &str,
        key: &str,
        value: &str,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        let request = PluginConfigureRequest {
            name: name.to_owned(),
            expected_revision: self.settings().plugin_registry().revision,
            service: None,
            text: Some(BTreeMap::from([(key.to_owned(), value.to_owned())])),
            forget_secrets: Vec::new(),
            scope: None,
            allowed: None,
            remote_approval: None,
        };
        self.configure(&request)?;
        self.changed();
        let ready = self.ready();
        let found = ready.iter().find(|ready| ready.package.manifest.name == name);
        Ok(found.map(super::ReadyPlugin::text_values).unwrap_or_default())
    }

    /// Writes a secret setting's value and names its credential in `plugins.json`.
    pub fn set_secret(
        &self,
        request: &PluginSecretSetRequest,
        value: &SecretBytes,
    ) -> anyhow::Result<PluginSecretSetResult> {
        let text = std::str::from_utf8(value.expose()).ok();
        anyhow::ensure!(
            text.is_some_and(|text| {
                !text.trim().is_empty() && !text.contains('\0') && text.len() <= MAX_SECRET_BYTES
            }),
            "plugins.secret_invalid: 保密值要是不空的文字，最多 {MAX_SECRET_BYTES} 字节"
        );
        let mcp = self
            .mcp
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("plugins.secret_unavailable: 凭据管理器现在不能用"))?;
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let (_, located) = self.locate(&request.name)?;
        let name = located.name.clone();
        let package = located.scanned.as_ref().map_err(|_| {
            anyhow::anyhow!("plugins.unreadable: 插件 {name} 现在读不出来，它的设置改不了")
        })?;
        anyhow::ensure!(
            option_kind(&package.manifest.declared, &name, &request.key)?
                == PluginOptionKind::Secret,
            "plugins.setting_not_secret: {} 不是保密设置",
            request.key
        );
        let registry = self.current(self.settings().plugin_registry().revision)?;
        let mut setting =
            located.setting.clone().unwrap_or_else(|| PluginSetting::imported(&name, None));
        // A new value is a new credential, so the service starts again with it. A credential
        // left unnamed by a failure here is removed by the next cleanup.
        let previous = setting.values.secrets.get(&request.key).copied();
        if let Some(previous) = previous
            && let Ok((_, value)) =
                mcp.secrets().value(&secret_owner(&name, &request.key), previous)
            && let Ok(text) = std::str::from_utf8(value.expose())
        {
            self.remember_log_secret(&name, text);
        }
        if let Ok(text) = std::str::from_utf8(value.expose()) {
            self.remember_log_secret(&name, text);
        }
        let id = mcp
            .secrets()
            .set(&secret_owner(&name, &request.key), None, value)
            .map_err(|reason| anyhow::anyhow!("plugins.secret_failed: {reason}"))?;
        setting.values.secrets.insert(request.key.clone(), id);
        self.write_registry(registry.revision, &replaced(&registry, setting))?;
        self.changed();
        self.forget_credentials(previous.as_slice());
        Ok(PluginSecretSetResult { revision: self.settings().plugin_registry().revision })
    }

    /// Deletes values no setting names any more. A value that cannot be deleted now is removed
    /// by the next cleanup, since nothing names it.
    fn forget_credentials(&self, ids: &[aura_domain_common::CredentialId]) {
        let Some(mcp) = &self.mcp else { return };
        for id in ids {
            if let Err(reason) = mcp.secrets().forget(*id) {
                append_core_log(
                    "plugins.credential_cleanup_failed",
                    serde_json::json!({"reason": reason}),
                );
            }
        }
    }
}
