//! Removing an installation. By default the program, its shortcuts, its Settings > Apps entry and
//! Chrome's registration of its relay go and the user's data stays; with `delete_data`, the
//! installation's data and settings folders and its own Credential Manager entries go too, and
//! for data that was moved, what the moved folder holds of it and the record that pointed there.
//! Workspaces and global memory are never touched.
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use aura_data_location::{InstallCopy, Locations, MARKER_FILE};
use aura_launcher::read_install_state;
use aura_runtime_contract::BundleNamespace;

use crate::{
    host::{Host, shortcut_name},
    install::ensure_not_running,
    marker,
};

#[derive(Clone, Debug)]
pub struct UninstallPlan {
    pub root: PathBuf,
    pub namespace: BundleNamespace,
    pub version: String,
    /// What the installation folder holds.
    pub program_bytes: u64,
    pub data_root: PathBuf,
    pub data_bytes: u64,
    pub config_root: PathBuf,
    pub config_bytes: u64,
    pub credentials: usize,
    /// Chrome's manifest for the relay and the copy of the extension Chrome loads.
    pub chrome_root: PathBuf,
    /// For moved data: the rest of what the moved folder holds of this installation, and the
    /// namespace's folder in the default place with the record.
    pub moved_data: Vec<PathBuf>,
}

fn folder_size(folder: &Path) -> u64 {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_size(&entry.path()),
            Ok(_) => entry.metadata().map(|metadata| metadata.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// What uninstalling `root` would remove. Refuses a folder that is not an Aura installation: one
/// whose marker names another installation, or without a marker and without the layout every
/// installer wrote.
pub fn plan_uninstall(host: &dyn Host, root: &Path) -> Result<UninstallPlan> {
    let not_installation = || anyhow::anyhow!("{} is not an Aura installation", root.display());
    let state = read_install_state(root).map_err(|_| not_installation())?;
    let namespace = state.release_descriptor.bundle_namespace.clone();
    ensure!(marker::is_installation_of(root, &namespace), not_installation());
    let locations = Locations::resolve(&host.roots()?, &namespace, &InstallCopy::of(&state))
        .context("cannot find where this installation's data is")?;
    let mut moved_data = Vec::new();
    if let Some(location) = &locations.location {
        moved_data.extend(locations.core_log.parent().map(Path::to_path_buf));
        moved_data.push(locations.ui_profile.clone());
        moved_data.push(location.join(MARKER_FILE));
        moved_data.extend(locations.pending_removal.iter().cloned());
        moved_data.push(locations.fixed_root.clone());
    }
    Ok(UninstallPlan {
        root: root.to_path_buf(),
        version: state.active,
        program_bytes: folder_size(root),
        data_bytes: folder_size(&locations.data_root),
        config_bytes: folder_size(&locations.settings_root),
        credentials: host.credentials(&namespace)?.len(),
        data_root: locations.data_root,
        config_root: locations.settings_root,
        chrome_root: locations.chrome_root,
        moved_data,
        namespace,
    })
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Removed {
    pub shortcuts: usize,
    pub entry: bool,
    /// Chrome's registration of the installation's relay.
    pub chrome: bool,
    pub data: bool,
    pub credentials: usize,
    /// What the installation folder held that is not Aura's: it stays, with the folder.
    pub left_behind: Vec<PathBuf>,
}

pub fn uninstall(host: &dyn Host, plan: &UninstallPlan, delete_data: bool) -> Result<Removed> {
    ensure_not_running(host, &plan.root)?;
    let mut removed = Removed::default();
    let mut folders = host.shortcut_folders()?;
    folders.push(host.taskbar_pins()?);
    for folder in folders {
        let shortcut = folder.join(shortcut_name(&plan.namespace));
        if shortcut.is_file() && fs::remove_file(&shortcut).is_ok() {
            removed.shortcuts += 1;
        }
    }
    removed.entry = host.remove_entry(&plan.namespace)?;
    // Chrome would otherwise keep starting a relay that is about to be gone.
    removed.chrome = host.remove_chrome_host(&plan.namespace)?;
    // Only a name for notifications; one that cannot be removed does not stop the uninstall.
    let _ = host.remove_notification_identity(&plan.namespace);
    removed.left_behind = marker::remove_own(&plan.root)
        .with_context(|| format!("cannot remove {}", plan.root.display()))?;
    // Chrome's manifest for the relay and the copy of the extension Chrome loads belong to the
    // program, not to the user's data.
    remove_folder(&plan.chrome_root)?;
    if delete_data {
        for folder in [&plan.data_root, &plan.config_root].into_iter().chain(&plan.moved_data) {
            removed.data |= remove_folder(folder)?;
        }
        removed.credentials = host.delete_credentials(&plan.namespace)?;
    }
    Ok(removed)
}

/// `false` when there was nothing there; a file in the folder's place is removed too.
fn remove_folder(folder: &Path) -> Result<bool> {
    if !folder.exists() {
        return Ok(false);
    }
    if folder.is_file() { fs::remove_file(folder) } else { fs::remove_dir_all(folder) }
        .with_context(|| format!("cannot remove {}", folder.display()))?;
    Ok(true)
}
