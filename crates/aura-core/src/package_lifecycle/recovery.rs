use super::PackageRecovery;
use anyhow::{Result, ensure};
use aura_domain_common::UtcTimestamp;
use aura_persistence_v2::{V2Store, packages};
use aura_windows_platform::{NativeFileAccess, VerifiedFile};
use std::{
    fs::{self, OpenOptions},
    io::ErrorKind,
    path::Path,
    sync::Arc,
};

const MARKER: &[u8] = b"Aura V2 package Conformance scratch owner 1\n";

pub(super) struct OwnerLease {
    _marker: VerifiedFile,
    _root: VerifiedFile,
}

pub(super) fn open(root: &Path) -> Result<Arc<OwnerLease>> {
    ensure!(root.is_absolute(), "package.absolute_owner_root_required");
    fs::create_dir_all(root)?;
    let parent = aura_windows_platform::open_verified_tool_output_root(root)?;
    ensure!(parent.info().is_directory, "package.owner_directory_required");
    let marker = root.join("owner.lock");
    match OpenOptions::new().write(true).create_new(true).open(&marker) {
        Ok(mut created) => {
            use std::io::Write;
            created.write_all(MARKER)?;
            created.sync_all()?;
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    // The existing platform lease rejects reparse points and hardlink aliases.
    // Every worker retains the same lease until its Job and cleanup settle.
    let owner = aura_windows_platform::lock_execution_resources(&marker)?;
    use std::io::Read;
    let mut bytes = Vec::new();
    owner.file().try_clone()?.take(MARKER.len() as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes == MARKER, "package.owner_marker_invalid");
    Ok(Arc::new(OwnerLease { _marker: owner, _root: parent }))
}

pub(super) fn recover(store: &mut V2Store, root: &Path) -> Result<PackageRecovery> {
    // Caller holds the exclusive owner lease: no previous worker can still be
    // executing. The previous Core's kill-on-close Jobs have already gone away.
    let interrupted_runs = store.database.unit_of_work(|tx| {
        // Agent calls of the previous Core ended with its Jobs; their references go.
        packages::references::release_all_calls(tx)?;
        packages::conformance::interrupt_conformance(tx, UtcTimestamp::now())
    })?;
    let parent = aura_windows_platform::open_verified_tool_output_root(root)?;
    let mut removed_scratch_roots = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| valid_scratch_name(name)) else { continue };
        let path = root.join(name);
        let held =
            aura_windows_platform::open_verified_import_source(&path, NativeFileAccess::Metadata)?;
        ensure!(
            held.info().is_directory
                && aura_windows_platform::final_path_within(parent.info(), held.info()),
            "package.scratch_root_changed"
        );
        // Rust's remove_dir_all removes links inside the tree without following
        // them. Reject a root that is itself a link; only this checked absolute
        // child of our owned namespace can be removed.
        let final_path = held.info().final_path.clone();
        drop(held);
        fs::remove_dir_all(final_path)?;
        removed_scratch_roots += 1;
    }
    Ok(PackageRecovery { interrupted_runs, removed_scratch_roots })
}

pub(super) fn valid_scratch_name(name: &str) -> bool {
    name.strip_prefix("package-run-").is_some_and(|suffix| {
        suffix.len() == 32 && suffix.bytes().all(|value| value.is_ascii_alphanumeric())
    })
}
