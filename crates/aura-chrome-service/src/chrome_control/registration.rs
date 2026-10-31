//! Chrome finds the relay through a manifest file and the per-user registry entry that names it.
//! Both are this installation's own: the host name and the manifest belong to its namespace.
use super::ChromeControlOptions;
use aura_windows_platform::installer::{
    read_chrome_native_host, remove_chrome_native_host, write_chrome_native_host,
};
use serde_json::{Value, json};
use std::fs;

pub(super) fn manifest(options: &ChromeControlOptions) -> Value {
    json!({
        "name": options.host_name,
        "description": "Aura Chrome control",
        "path": options.relay_executable,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{}/", options.extension_id)],
    })
}

pub(super) fn register(options: &ChromeControlOptions) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(&manifest(options))?;
    if let Some(folder) = options.manifest_path.parent() {
        fs::create_dir_all(folder)?;
    }
    let temporary = options.manifest_path.with_extension("json.tmp");
    fs::write(&temporary, &bytes)?;
    fs::rename(&temporary, &options.manifest_path)?;
    write_chrome_native_host(&options.host_name, &options.manifest_path)?;
    Ok(())
}

pub(super) fn deregister(options: &ChromeControlOptions) -> anyhow::Result<()> {
    remove_chrome_native_host(&options.host_name)?;
    match fs::remove_file(&options.manifest_path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Whether Chrome would start this installation's relay for this installation's extension.
pub(super) fn is_registered(options: &ChromeControlOptions) -> bool {
    read_chrome_native_host(&options.host_name)
        .ok()
        .flatten()
        .is_some_and(|manifest| manifest == options.manifest_path)
        && fs::read(&options.manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|written| written == manifest(options))
}
