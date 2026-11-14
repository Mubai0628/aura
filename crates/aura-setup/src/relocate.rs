//! Moving an installation to another folder while Aura is closed: the folder is made private
//! before anything is written into it, the installation is copied and the copy read back, the
//! copy is marked, every registration is pointed at it, and then the old folder goes. Until the
//! registrations point at the copy, the installation where it was is untouched: a move that fails
//! or is stopped takes back what it made and leaves Aura where it was.
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use aura_data_location::{InstallCopy, Locations};
use aura_data_move::TreeCopy;
use aura_launcher::read_install_state;
use aura_runtime_contract::BundleNamespace;

use crate::{
    folder_check::{self, Refusal, Size},
    host::Host,
    install::ensure_not_running,
    marker::{self, INSTALL_MARKER},
    private_folder, register,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Copying,
    Verifying,
    Registering,
    Removing,
}

/// A failure a test puts in on purpose.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    CopyDiffers,
    /// Once every registration points at the copy.
    RegistrationFails,
}

#[derive(Clone, Debug)]
pub struct Relocation {
    pub from: PathBuf,
    pub to: PathBuf,
    pub namespace: BundleNamespace,
    pub version: String,
    pub size: Size,
}

#[derive(Debug, Default)]
pub struct Relocated {
    /// What the old folder still holds that is not Aura's: it stays with the folder.
    pub left_behind: Vec<PathBuf>,
    /// What the user may have to put right, for the last page.
    pub notes: Vec<String>,
}

/// Whether the installation in `from` can move to `root`; every reason it cannot.
pub fn plan(host: &dyn Host, from: &Path, root: &Path) -> Result<Relocation, Vec<Refusal>> {
    let unreadable = |error: &dyn std::fmt::Display| vec![Refusal::Unreadable(error.to_string())];
    let state = read_install_state(from).map_err(|error| unreadable(&error))?;
    let namespace = state.release_descriptor.bundle_namespace.clone();
    if !marker::is_installation_of(from, &namespace) {
        return Err(unreadable(&format!("{} 不是 Aura 的安装文件夹", from.display())));
    }
    let roots = host.roots().map_err(|error| unreadable(&error))?;
    let locations = Locations::resolve(&roots, &namespace, &InstallCopy::of(&state))
        .and_then(|locations| locations.with_global_memory(&roots, &namespace))
        .map_err(|error| unreadable(&error))?;
    let size = folder_check::size_of(from);
    let to = folder_check::check_root(root, size, &locations, Some(from))?;
    Ok(Relocation { from: from.to_path_buf(), to, namespace, version: state.active, size })
}

/// Leftovers of an earlier run directly in the folder, which do not move.
fn leftovers(from: &Path) -> Vec<String> {
    fs::read_dir(from)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.starts_with(".replaced-")
                || name.starts_with(".staging-")
                || name.eq_ignore_ascii_case("active.tmp")
                || name.eq_ignore_ascii_case(&format!("{INSTALL_MARKER}.writing"))
        })
        .collect()
}

/// Moves the installation as `relocation` says. `progress` gets the stage and the bytes done out
/// of the stage's total.
pub fn relocate(
    host: &dyn Host,
    relocation: &Relocation,
    progress: &mut dyn FnMut(Stage, u64, u64),
    cancelled: &dyn Fn() -> bool,
    fault: Option<Fault>,
) -> Result<Relocated> {
    let Relocation { from, to, namespace, version, size } = relocation;
    ensure_not_running(host, from)?;
    // Settings > Apps names the version the installation runs next: one installed after the move
    // was planned is on trial, not yet active.
    let version = &read_install_state(from)
        .map(|state| state.pending.unwrap_or(state.active))
        .unwrap_or_else(|_| version.clone());
    let made = private_folder::make(to)
        .with_context(|| format!("cannot set the permissions of {}", to.display()))?;
    let mut copy = TreeCopy::new();
    let mut registering = false;
    let result = (|| -> Result<Vec<String>> {
        let mut done = 0_u64;
        copy.copy(
            from,
            to,
            &leftovers(from),
            &mut |bytes| {
                done += bytes;
                progress(Stage::Copying, done, size.bytes);
            },
            cancelled,
        )
        .map_err(|reason| anyhow!(reason))?;
        if fault == Some(Fault::CopyDiffers) {
            fs::write(to.join("active.json"), b"not what was copied")?;
        }
        let mut checked = 0_u64;
        let total = copy.bytes();
        copy.verify(
            &mut |bytes| {
                checked += bytes;
                progress(Stage::Verifying, checked, total);
            },
            cancelled,
        )
        .map_err(|reason| anyhow!(reason))?;
        if let Some(problem) = aura_windows_platform::private_folder_problem(to)? {
            bail!("权限没有设好：{problem}");
        }
        marker::write(to, namespace)?;
        progress(Stage::Registering, 0, 0);
        registering = true;
        let notes = register::point_at(host, from, to, namespace, version)?;
        if fault == Some(Fault::RegistrationFails) {
            bail!("登记出错（测试）");
        }
        Ok(notes)
    })();
    let mut notes = match result {
        Ok(notes) => notes,
        Err(error) => {
            if registering {
                let _ = register::point_at(host, to, from, namespace, version);
            }
            let _ = fs::remove_file(to.join(INSTALL_MARKER));
            let mut left = copy.undo();
            if let Some(highest) = &made
                && !private_folder::take_back(to, highest)
            {
                left.push(to.display().to_string());
            }
            let kept = if left.is_empty() {
                String::new()
            } else {
                format!("；没删掉的：{}", left.join("、"))
            };
            return Err(error.context(format!("Aura 留在 {}{kept}", from.display())));
        }
    };
    progress(Stage::Removing, 0, 0);
    let left_behind = match marker::remove_own(from) {
        Ok(left) => left,
        Err(error) => {
            notes.push(format!("旧位置没有删干净：{error}"));
            vec![from.clone()]
        }
    };
    Ok(Relocated { left_behind, notes })
}
