use super::{
    PackageConformanceHandle, PackageRecovery, PackageRunOptions, PreparedPackageConformance,
    materialize, recovery, run,
    types::{PackageRunStatus, RunState},
};
use anyhow::{Result, ensure};
use aura_domain_common::{ToolConformanceRunId, ToolPackageId, UtcTimestamp};
use aura_persistence_v2::{
    V2Store,
    packages::{self, PackageFileBytes, conformance::PackageRunRecord},
};
use aura_tool_domain::PackageVersionBindingV1;
use aura_tool_package::StagedToolPackageV1;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

/// Owns Conformance independently of an IPC caller's wait. This is a Core
/// composition API, not an execution permission or an authenticated Main route.
/// Production routes must authorize the explicit run before calling start.
pub struct PackageConformanceOwner {
    store: Arc<Mutex<V2Store>>,
    root: PathBuf,
    lease: Arc<recovery::OwnerLease>,
    runs: Mutex<BTreeMap<ToolConformanceRunId, Arc<RunState>>>,
    stopping: AtomicBool,
    recovered: PackageRecovery,
}

impl PackageConformanceOwner {
    /// Core supplies its one existing V2 write authority; never open a second
    /// product writer. This is process startup work after the previous Core and
    /// Jobs have ended; never invoke recovery during a live configuration rebuild.
    pub fn open(store: Arc<Mutex<V2Store>>, root: &Path) -> Result<Self> {
        let lease = recovery::open(root)?;
        let recovered = recovery::recover(&mut *locked(&store)?, root)?;
        Ok(Self {
            store,
            root: root.to_owned(),
            lease,
            runs: Mutex::new(BTreeMap::new()),
            stopping: AtomicBool::new(false),
            recovered,
        })
    }
    pub fn recovery(&self) -> PackageRecovery {
        self.recovered
    }
    pub fn import_staged(&self, staged: &StagedToolPackageV1) -> Result<PackageVersionBindingV1> {
        ensure!(!self.stopping.load(Ordering::Acquire), "package.owner_stopping");
        let descriptor = aura_tool_package::package_content_descriptor_v1(staged)?;
        let bytes = descriptor
            .manifest
            .files
            .iter()
            .map(|file| fs::read(staged.root().join(file.path.as_str())))
            .collect::<std::io::Result<Vec<_>>>()?;
        let files = descriptor
            .manifest
            .files
            .iter()
            .zip(&bytes)
            .map(|(file, bytes)| PackageFileBytes { path: &file.path, bytes })
            .collect::<Vec<_>>();
        Ok(locked(&self.store)?.database.unit_of_work(|tx| {
            packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &files,
                UtcTimestamp::now(),
            )
        })?)
    }
    pub fn report(&self, version: &PackageVersionBindingV1) -> Result<Option<PackageRunRecord>> {
        Ok(packages::conformance::conformance(locked(&self.store)?.database.connection(), version)?)
    }
    pub fn start(&self, prepared: PreparedPackageConformance) -> Result<PackageConformanceHandle> {
        ensure!(Arc::ptr_eq(&self.lease, &prepared.lease), "package.preparation_owner_mismatch");
        ensure!(!self.stopping.load(Ordering::Acquire), "package.owner_stopping");
        prepared.staged.verify_integrity()?;
        let PreparedPackageConformance { version, options, staged, scratch, .. } = prepared;
        let mut runs =
            self.runs.lock().map_err(|_| anyhow::anyhow!("package.owner_unavailable"))?;
        ensure!(!self.stopping.load(Ordering::Acquire), "package.owner_stopping");
        runs.retain(|_, state| state.status.lock().map_or(true, |status| status.running));
        ensure!(runs.len() < 32, "package.conformance_capacity");
        let run_id = ToolConformanceRunId::new();
        let state = Arc::new(RunState {
            cancelled: AtomicBool::new(false),
            status: Mutex::new(PackageRunStatus { running: true, ..Default::default() }),
        });
        let started_at = UtcTimestamp::now();
        let version = locked(&self.store)?.database.unit_of_work(|tx| {
            packages::conformance::begin_conformance(
                tx,
                &version,
                run_id,
                &options.settings,
                started_at,
            )
        })?;
        runs.insert(run_id, state.clone());
        drop(runs);
        let work = run::Work {
            store: self.store.clone(),
            lease: self.lease.clone(),
            scratch,
            staged,
            state: state.clone(),
            version: version.clone(),
            run_id,
            started_at,
            options,
        };
        run::spawn(work)?;
        Ok(PackageConformanceHandle { run_id, version, state })
    }
    pub fn prepare(
        &self,
        version: &PackageVersionBindingV1,
        options: PackageRunOptions,
    ) -> Result<PreparedPackageConformance> {
        ensure!(!self.stopping.load(Ordering::Acquire), "package.owner_stopping");
        ensure!(
            options.timeout > std::time::Duration::ZERO
                && (1..=64 * 1024 * 1024).contains(&options.max_output_bytes),
            "package.invalid_process_limits"
        );
        let content = packages::load_content(locked(&self.store)?.database.connection(), version)?;
        ensure!(
            aura_tool_domain::canonical_package_settings_v1(
                &content.descriptor.manifest,
                &options.settings
            )? == options.settings,
            "package.settings_not_canonical"
        );
        let previous_state = self
            .report(version)?
            .map_or(aura_tool_domain::PackageConformanceStateV1::NotRun, |run| run.state);
        let scratch = tempfile::Builder::new()
            .prefix("package-run-")
            .rand_bytes(32)
            .tempdir_in(&self.root)?;
        let mut options = options;
        if options.workspace_root.is_none() {
            // Conformance never runs in the user's workspace: a case that needs one
            // gets an empty directory inside this run's own scratch tree.
            let workspace = scratch.path().join("workspace");
            fs::create_dir(&workspace)?;
            options.workspace_root = Some(workspace);
        }
        let staged = materialize::staged(&content, scratch.path())?;
        Ok(PreparedPackageConformance {
            staged,
            scratch,
            version: version.clone(),
            options,
            lease: self.lease.clone(),
            previous_state,
        })
    }
    pub fn active_count(&self) -> Result<usize> {
        Ok(self
            .runs
            .lock()
            .map_err(|_| anyhow::anyhow!("package.owner_unavailable"))?
            .values()
            .filter(|state| state.status.lock().map_or(true, |status| status.running))
            .count())
    }
    pub fn scratch_count(&self) -> Result<usize> {
        Ok(fs::read_dir(&self.root)?
            .collect::<std::io::Result<Vec<_>>>()?
            .iter()
            .filter(|entry| entry.file_name().to_str().is_some_and(recovery::valid_scratch_name))
            .count())
    }
    pub fn shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(runs) = self.runs.lock() {
            for state in runs.values() {
                state.cancelled.store(true, Ordering::Release);
            }
        }
    }
}
impl Drop for PackageConformanceOwner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(super) fn locked(store: &Mutex<V2Store>) -> Result<MutexGuard<'_, V2Store>> {
    store.lock().map_err(|_| anyhow::anyhow!("package.store_unavailable"))
}
