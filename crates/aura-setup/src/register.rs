//! What an installation registers outside its folder: its Start menu shortcut, a desktop shortcut
//! and a taskbar pin when there are any, its Settings > Apps entry, Chrome's registration of its
//! relay and its notification icon; and pointing all of them at the folder it moved to.
use std::{fs, path::Path};

use anyhow::{Context, Result};
use aura_runtime_contract::BundleNamespace;
use aura_windows_platform::installer::{
    Shortcut, UninstallEntry, moved_path, notification_identity,
};

use crate::{
    host::{Host, shortcut_name},
    install::{PUBLISHER, UNINSTALLER},
    payload::{ICON, LAUNCHER},
};

fn shortcut(
    host: &dyn Host,
    folder: &Path,
    root: &Path,
    namespace: &BundleNamespace,
) -> Result<()> {
    let launcher = root.join(LAUNCHER);
    let icon = root.join(ICON);
    let path = folder.join(shortcut_name(namespace));
    // The identity Aura's windows run under: a pin and the running window share one button, and
    // Windows takes this shortcut for the window instead of making one from it.
    let identity = notification_identity(namespace.as_str());
    host.create_shortcut(&Shortcut {
        path: &path,
        target: &launcher,
        arguments: "",
        working_directory: root,
        description: "Aura",
        icon: icon.is_file().then_some(icon.as_path()),
        app_id: Some(&identity),
    })
    .with_context(|| format!("cannot write {}", path.display()))
}

/// The Start menu shortcut, and a desktop one when `desktop`.
pub fn shortcuts(
    host: &dyn Host,
    root: &Path,
    namespace: &BundleNamespace,
    desktop: bool,
) -> Result<()> {
    shortcut(host, &host.start_menu()?, root, namespace)?;
    if desktop {
        shortcut(host, &host.desktop()?, root, namespace)?;
    }
    Ok(())
}

/// The Settings > Apps entry for the installation in `root`.
pub fn entry(
    host: &dyn Host,
    root: &Path,
    namespace: &BundleNamespace,
    version: &str,
) -> Result<()> {
    let launcher = root.join(LAUNCHER);
    let icon = root.join(ICON);
    let uninstall_command = format!("\"{}\" --uninstall", root.join(UNINSTALLER).display());
    host.write_entry(
        namespace,
        &UninstallEntry {
            key: "",
            display_name: "Aura",
            display_version: version,
            publisher: PUBLISHER,
            install_location: root,
            display_icon: if icon.is_file() { &icon } else { &launcher },
            uninstall_command: &uninstall_command,
            estimated_size_kb: u32::try_from(aura_data_move::folder_size(root) / 1024)
                .unwrap_or(u32::MAX),
        },
    )
    .context("cannot write the Settings > Apps entry")
}

/// Points every registration from `from` at `to`, where a complete copy of the installation is.
/// What has to work for Aura to start fails the move; a taskbar pin, Chrome's relay and the
/// notification icon, which Aura or the user can put right afterwards, only add a note.
pub fn point_at(
    host: &dyn Host,
    from: &Path,
    to: &Path,
    namespace: &BundleNamespace,
    version: &str,
) -> Result<Vec<String>> {
    entry(host, to, namespace, version)?;
    shortcut(host, &host.start_menu()?, to, namespace)?;
    let name = shortcut_name(namespace);
    if host.desktop()?.join(&name).is_file() {
        shortcut(host, &host.desktop()?, to, namespace)?;
    }
    let mut notes = Vec::new();
    let pins = host.taskbar_pins()?;
    if pins.join(&name).is_file() && shortcut(host, &pins, to, namespace).is_err() {
        notes.push("任务栏上固定的 Aura 没能改到新位置，请重新固定".to_owned());
    }
    if let Err(error) = chrome_relay(host, from, to, namespace) {
        notes.push(format!("Chrome 控制的登记没有改好（{error:#}），下次开启 Chrome 控制时会重写"));
    }
    // Aura writes the icon again when it starts.
    let _ = host.relocate_notification_icon(namespace, from, to);
    Ok(notes)
}

/// Chrome's manifest for the relay names the relay's path, inside the installation.
fn chrome_relay(
    host: &dyn Host,
    from: &Path,
    to: &Path,
    namespace: &BundleNamespace,
) -> Result<()> {
    let Some(manifest) = host.chrome_host_manifest(namespace)? else { return Ok(()) };
    if !manifest.is_file() {
        return Ok(());
    }
    let bytes = fs::read(&manifest)?;
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let Some(path) = value.get("path").and_then(serde_json::Value::as_str) else { return Ok(()) };
    let Some(moved) = moved_path(path, from, to) else { return Ok(()) };
    value["path"] = serde_json::Value::String(moved.to_string_lossy().into_owned());
    fs::write(&manifest, serde_json::to_vec_pretty(&value)?)?;
    Ok(())
}
