//! Which plugins' background services should run now, and what each is started as.
use aura_extension_service::plugins::{PluginRuntime, ReadyPlugin};
use aura_plugin_host::ServiceLaunch;
use aura_plugins::{PluginOptionKind, PluginService};

/// What a service is started as, short of its secrets' values: what the user trusted and the
/// values of its settings. A secret setting's new value is a new credential, so it changes this
/// too.
#[must_use]
pub fn service_identity(ready: &ReadyPlugin) -> String {
    let mut hash = blake3::Hasher::new_derive_key("aura plugin service identity v1");
    let trusted = ready.setting.trusted_fingerprint.as_deref().unwrap_or_default();
    hash.update(trusted.as_bytes());
    hash.update(b"\0");
    hash.update(&serde_json::to_vec(&ready.setting.values).unwrap_or_default());
    hash.finalize().to_hex().to_string()
}

/// The service a ready plugin should run now: it declares one, the plugin page turned it on and
/// every required setting has a value.
pub(crate) fn should_run<'a>(
    plugins: &PluginRuntime,
    ready: &'a ReadyPlugin,
) -> Option<&'a PluginService> {
    let service = ready.package.manifest.declared.service.as_ref()?;
    (ready.setting.remote.service && plugins.service_blocker(ready).is_none()).then_some(service)
}

/// What the host starts the service as; reads its secret settings from the Credential Manager.
pub(crate) fn launch_of(
    plugins: &PluginRuntime,
    ready: &ReadyPlugin,
    service: &PluginService,
    identity: String,
) -> Result<ServiceLaunch, String> {
    let name = ready.package.manifest.name.clone();
    let (data_folder, scratch) = plugins.service_folders(&name);
    let options = ready.package.manifest.declared.options.iter();
    let settings = options
        .filter(|option| option.kind == PluginOptionKind::Text)
        .filter_map(|option| {
            Some((option.key.clone(), ready.setting.values.text.get(&option.key)?.clone()))
        })
        .collect();
    Ok(ServiceLaunch {
        secrets: plugins.service_secrets(ready)?,
        plugin: name,
        identity,
        command: service.command.clone(),
        arguments: service.arguments.clone(),
        data_folder,
        scratch,
        limits: Some(ready.job_limits()),
        channels: ready.package.manifest.declared.channels.clone(),
        settings,
    })
}
