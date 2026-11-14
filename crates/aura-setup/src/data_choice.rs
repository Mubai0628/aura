//! Where the data goes, chosen in the setup program's window or with `--data`. At a new
//! installation a folder that holds this installation's data marker is used as it is (after
//! reinstalling Windows, say). Any other folder is checked as the storage page checks one; before
//! there is any data it is where the data starts, and otherwise the move is asked of the launcher,
//! which does it at the next start while Aura is closed.
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use aura_data_location::{
    DataMarker, InstallCopy, Locations, point_at, verify_location, write_marker,
};
use aura_data_move::{Choice, Refusal};
use aura_launcher::{read_install_state, write_install_state};
use aura_runtime_contract::{ActiveInstallState, BundleNamespace, PRODUCTION_BUNDLE_NAMESPACE};

use crate::{host::Host, private_folder};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataChoice {
    Keep,
    /// A folder that holds this installation's data already.
    Reuse(PathBuf),
    /// A folder the data starts in, when there is no data yet.
    Start(PathBuf),
    /// A folder the data moves to at the next start.
    Move(PathBuf),
}

/// The data's places now, with global memory; `state` is the installation's, if there is one.
pub fn locations(
    host: &dyn Host,
    namespace: &BundleNamespace,
    state: Option<&ActiveInstallState>,
) -> Result<Locations> {
    let roots = host.roots()?;
    let install = state.map_or(InstallCopy::NotInstalled, InstallCopy::of);
    Locations::resolve(&roots, namespace, &install)
        .and_then(|locations| locations.with_global_memory(&roots, namespace))
        .context("cannot find where this installation's data is")
}

/// The data's folder as the window shows it.
#[must_use]
pub fn shown(locations: &Locations) -> PathBuf {
    locations.location.clone().unwrap_or_else(|| locations.data_root.clone())
}

/// What picking `picked` means for an installation in `install_root`. `fresh`: nothing is
/// installed yet, so a folder with this installation's marker is taken as it is. `on_trial`: a
/// version waits as a trial, and nothing moves until it is confirmed.
pub fn choose(
    host: &dyn Host,
    namespace: &BundleNamespace,
    current: &Locations,
    picked: &Path,
    install_root: &Path,
    fresh: bool,
    on_trial: bool,
) -> Result<DataChoice, Vec<String>> {
    if fresh {
        for folder in [picked.to_path_buf(), picked.join("Aura")] {
            if verify_location(&folder, namespace).is_ok() {
                return Ok(DataChoice::Reuse(folder));
            }
        }
    }
    let roots = host.roots().map_err(|error| vec![error.to_string()])?;
    let context = aura_data_move::Context {
        roots: &roots,
        namespace,
        current,
        install_root: Some(install_root),
        // The Workspaces are recorded in the store, which the setup program does not open. A
        // model's access to the data folder asks every time wherever the folder is.
        workspaces: &[],
        on_trial,
        production: namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE,
    };
    match aura_data_move::check(&context, &Choice::Folder(picked.to_path_buf())) {
        Ok(accepted) => Ok(match accepted.destination {
            None => DataChoice::Keep,
            Some(destination) if !current.data_root.exists() => DataChoice::Start(destination),
            Some(destination) => DataChoice::Move(destination),
        }),
        Err(refusals) if refusals == [Refusal::AlreadyThere] => Ok(DataChoice::Keep),
        Err(refusals) => Err(refusals.iter().map(ToString::to_string).collect()),
    }
}

/// Carries out `choice` for the installation in `root`, once its program is in place.
pub fn apply(
    host: &dyn Host,
    root: &Path,
    namespace: &BundleNamespace,
    choice: &DataChoice,
) -> Result<()> {
    let roots = host.roots()?;
    let fixed_root = Locations::for_place(&roots, namespace, None).fixed_root;
    let take = |folder: &Path| -> Result<()> {
        fs::create_dir_all(&fixed_root)?;
        let mut state = read_install_state(root)?;
        state.data_location = Some(folder.display().to_string());
        write_install_state(root, &state)?;
        point_at(&roots, namespace, folder)
            .with_context(|| format!("cannot use the data in {}", folder.display()))
    };
    match choice {
        DataChoice::Keep => Ok(()),
        DataChoice::Reuse(folder) => take(folder),
        DataChoice::Start(folder) => {
            private_folder::make(folder)
                .with_context(|| format!("cannot set the permissions of {}", folder.display()))?;
            write_marker(folder, &DataMarker::new(namespace.clone()))?;
            take(folder)
        }
        DataChoice::Move(destination) => {
            // The launcher makes the destination itself, but not the folders above it.
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::create_dir_all(&fixed_root)?;
            let state = read_install_state(root)?;
            let current = locations(host, namespace, Some(&state))?;
            aura_data_move::request(namespace, &current, Some(destination.clone()))
                .context("cannot record where the data goes")
        }
    }
}
