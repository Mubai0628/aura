//! Core's one tool-package service: import, version views, Conformance preparation
//! and runs, install, list. Main supplies dialog paths and approvals; every
//! authority check and transaction is here or below. Blocking work runs on the
//! caller's bounded synchronous boundary.
mod credentials;
mod import;
mod install;
mod runs;
#[cfg(test)]
pub(crate) mod test_support;
mod views;
use crate::package_lifecycle::{
    PackageConformanceHandle, PackageConformanceOwner, PreparedPackageConformance,
};
use anyhow::{Result, ensure};
use aura_domain_common::ToolConformanceRunId;
use aura_persistence_v2::V2Store;
pub(crate) use import::ImportSource;
pub(crate) use runs::RunEnvironment;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
pub(crate) use views::{credential_facts, http_credential};

/// How many concrete previews may wait for the user's answer at once. Each holds a
/// restored payload directory, so the oldest is discarded rather than growing.
const MAX_PREPARATIONS: usize = 4;
/// A preparation the user did not answer within this time is discarded.
const PREPARATION_LIFETIME: Duration = Duration::from_secs(15 * 60);

struct Preparation {
    id: String,
    created: Instant,
    prepared: PreparedPackageConformance,
}

pub(crate) struct PackageServices {
    owner: PackageConformanceOwner,
    store: Arc<Mutex<V2Store>>,
    /// The credential store, for binding checks. Never locked while `store` is held.
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    import_root: PathBuf,
    preparations: Mutex<VecDeque<Preparation>>,
    runs: Mutex<BTreeMap<ToolConformanceRunId, PackageConformanceHandle>>,
}

impl PackageServices {
    /// Process startup only, after the previous Core and its Jobs are gone: the owner
    /// interrupts unfinished runs and removes leftover scratch and import directories.
    pub fn open(
        store: Arc<Mutex<V2Store>>,
        persistence: Arc<aura_persistence::PersistenceCoordinator>,
        root: &Path,
    ) -> Result<Self> {
        let owner = PackageConformanceOwner::open(store.clone(), root)?;
        let import_root = root.join("import");
        if import_root.exists() {
            let parent = aura_windows_platform::open_verified_tool_output_root(root)?;
            let held = aura_windows_platform::open_verified_import_source(
                &import_root,
                aura_windows_platform::NativeFileAccess::Metadata,
            )?;
            ensure!(
                held.info().is_directory
                    && aura_windows_platform::final_path_within(parent.info(), held.info()),
                "package.import_root_changed"
            );
            let final_path = held.info().final_path.clone();
            drop(held);
            // Staging copies the user's source; a crash can leave a copy behind. It is
            // our own namespace and nothing in it is a source of truth.
            std::fs::remove_dir_all(final_path)?;
        }
        std::fs::create_dir(&import_root)?;
        Ok(Self {
            owner,
            store,
            persistence,
            import_root,
            preparations: Mutex::new(VecDeque::new()),
            runs: Mutex::new(BTreeMap::new()),
        })
    }

    fn store(&self) -> Result<MutexGuard<'_, V2Store>> {
        self.store.lock().map_err(|_| anyhow::anyhow!("package.store_unavailable"))
    }

    pub fn shutdown(&self) {
        self.owner.shutdown();
    }
}

/// A fixed-length random identity for one preparation. It names a held object and
/// carries no authority by itself: start also requires the preview hash.
fn preparation_id() -> String {
    let fresh =
        format!("{}{}", aura_domain_common::CorrelationId::new(), ToolConformanceRunId::new());
    aura_domain_common::jcs::CanonicalSha256HashV1::sha256(fresh.as_bytes()).hex()[..32].to_owned()
}

#[cfg(test)]
mod tests;
