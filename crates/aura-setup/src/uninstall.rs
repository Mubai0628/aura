//! Removing an installation. By default the program, its shortcuts and its Settings > Apps entry go
//! and the user's data stays; with `delete_data`, the installation's data folder and its own
//! Credential Manager entries go too. Workspaces are never touched.
use std::{
    fs,
    path::{Path, PathBuf},
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
    Ok(UninstallPlan {
        root: root.to_path_buf(),
        version: state.active,
        data_bytes: folder_size(&data_root),
        credentials: host.credentials(&namespace)?.len(),
        data_root,
        namespace,
    })
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Removed {
    pub shortcuts: usize,
    pub entry: bool,
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
    fs::remove_dir_all(&plan.root)
        .with_context(|| format!("cannot remove {}", plan.root.display()))?;
    if delete_data {
        if plan.data_root.exists() {
            fs::remove_dir_all(&plan.data_root)
                .with_context(|| format!("cannot remove {}", plan.data_root.display()))?;
            removed.data = true;
        }
        removed.credentials = host.delete_credentials(&plan.namespace)?;
    }
    Ok(removed)
}
