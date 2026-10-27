//! Removing an installation. By default the program, its shortcuts, its Settings > Apps entry and
//! Chrome's registration of its relay go and the user's data stays; with `delete_data`, the
//! installation's data and settings folders and its own Credential Manager entries go too.
//! Workspaces are never touched.
use std::{
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use aura_launcher::read_install_state;
use aura_runtime_contract::BundleNamespace;

use crate::{
    host::{Host, shortcut_name},
    install::ensure_not_running,
    payload::LAUNCHER,
};

#[derive(Clone, Debug)]
pub struct UninstallPlan {
    pub root: PathBuf,
    pub namespace: BundleNamespace,
    pub version: String,
    pub data_root: PathBuf,
    pub data_bytes: u64,
    pub config_root: PathBuf,
    pub config_bytes: u64,
    pub credentials: usize,
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

/// What uninstalling `root` would remove. Refuses a folder that is not an Aura installation.
pub fn plan_uninstall(host: &dyn Host, root: &Path) -> Result<UninstallPlan> {
    ensure!(
        root.join("active.json").is_file()
            && root.join("versions").is_dir()
            && root.join(LAUNCHER).is_file(),
        "{} is not an Aura installation",
        root.display()
    );
    let state = read_install_state(root)?;
    let namespace = state.release_descriptor.bundle_namespace;
    let data_root = host.data_root(&namespace)?;
    let config_root = host.config_root(&namespace)?;
    Ok(UninstallPlan {
        root: root.to_path_buf(),
        version: state.active,
        data_bytes: folder_size(&data_root),
        config_bytes: folder_size(&config_root),
        credentials: host.credentials(&namespace)?.len(),
        data_root,
        config_root,
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
}

pub fn uninstall(host: &dyn Host, plan: &UninstallPlan, delete_data: bool) -> Result<Removed> {
    ensure_not_running(host, &plan.root)?;
    let mut removed = Removed::default();
    for folder in host.shortcut_folders()? {
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
    remove_program_folder(&plan.root)?;
    // Chrome's manifest for the relay and the copy of the extension Chrome loads belong to the
    // program, not to the user's data.
    remove_folder(&plan.data_root.join("chrome"))?;
    if delete_data {
        for folder in [&plan.data_root, &plan.config_root] {
            removed.data |= remove_folder(folder)?;
        }
        removed.credentials = host.delete_credentials(&plan.namespace)?;
    }
    Ok(removed)
}

/// While Aura is closed, Chrome starts the relay from the installation for about two seconds at a
/// time to look for it; one started just before its registration went keeps its file in use for
/// that long, so a refused removal is tried again for a few seconds.
fn remove_program_folder(root: &Path) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::remove_dir_all(root) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(_) if Instant::now() < until => thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                return Err(error).with_context(|| format!("cannot remove {}", root.display()));
            }
        }
    }
}

/// `false` when there was no folder.
fn remove_folder(folder: &Path) -> Result<bool> {
    if !folder.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(folder).with_context(|| format!("cannot remove {}", folder.display()))?;
    Ok(true)
}
