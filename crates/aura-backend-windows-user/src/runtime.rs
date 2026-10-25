//! Aura-owned process resources outlive an individual Tool result. ACL leases
//! form a stack: overlapping grants are restored in reverse order only after
//! all corresponding Jobs stop. No scheduler lock is held by a background job.
use crate::file_lease::FileLease;
use anyhow::{Result, ensure};
use aura_windows_platform::RootJob;
use aura_windows_user_contract::ExecutionFileBinding;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Unit {
    nonce: String,
    job: Arc<RootJob>,
    lease: FileLease,
    scratch: Option<ExecutionFileBinding>,
    stopped: bool,
    account: String,
    read_roots: Vec<ExecutionFileBinding>,
}

struct State {
    // Fixed installation locks exclude a second Core/Setup while this Core
    // owns live resources, not subsequent calls from this same Core.
    _setup: aura_windows_platform::VerifiedFile,
    _execution: aura_windows_platform::VerifiedFile,
    units: Vec<Unit>,
    root: PathBuf,
    /// What the recovery block that opened this state cost; taken once by the
    /// launch whose allocation paid it, then gone — a reused runtime reports no
    /// recovery work because it did none.
    recovery: Option<crate::work::RecoveryWork>,
}

#[derive(Default)]
pub(crate) struct RuntimeResources {
    state: Mutex<Option<State>>,
    closing: std::sync::atomic::AtomicBool,
}

/// How much the runtime currently holds. Counts only, never identities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResourceCounts {
    pub active: u64,
    pub background: u64,
}

impl RuntimeResources {
    pub fn allocate(
        &self,
        install: &crate::supervisor::LaunchFacts,
        nonce: &str,
        account: &str,
        read_roots: &[ExecutionFileBinding],
        prepare: impl FnOnce(&mut FileLease, &mut Option<ExecutionFileBinding>) -> Result<()>,
    ) -> Result<Arc<RootJob>> {
        let mut state = self.state.lock().map_err(|_| anyhow::anyhow!("runner.runtime_lock"))?;
        ensure!(!self.closing.load(std::sync::atomic::Ordering::Acquire), "runner.runtime_closed");
        if state.is_none() {
            let setup = aura_windows_platform::lock_execution_resources(
                &install.root.join("private/execution-setup.lock"),
            )?;
            let execution = aura_windows_platform::lock_execution_resources(
                &install.root.join("private/execution.lock"),
            )?;
            let (_receipt, recovery) = crate::lease_recovery::recover_locked(
                &install.root,
                &install.installation_id,
                &install.owner_sid,
            )?;
            *state = Some(State {
                _setup: setup,
                _execution: execution,
                units: vec![],
                root: install.root.clone(),
                recovery: Some(recovery),
            });
        }
        let state = state.as_mut().expect("initialized runtime");
        ensure!(state.root == install.root, "runner.runtime_installation_changed");
        // WRITE_RESTRICTED tokens share their account's read permissions. Never
        // grant a second readable workspace while its old background tree lives.
        ensure!(
            state.units.iter().all(|u| u.account != account || u.read_roots == read_roots),
            "runner.background_read_scope_conflict"
        );
        let mut lease = FileLease::create(
            &install.root.join("private/execution-leases").join(format!("{nonce}.jsonl")),
        )?;
        for previous in &state.units {
            lease.share_roots_from(&previous.lease);
        }
        lease.bind_runtime_process(
            &install.installation_id,
            nonce,
            &aura_windows_platform::execution_maintenance::boot_identity()?,
            state.units.len() as u64 + 1,
        )?;
        let job = Arc::new(RootJob::create_recoverable(&install.installation_id, nonce)?);
        // Register before the first grant. Partial preparation remains owned;
        // no failed attempt can silently drop a live ACL lease.
        state.units.push(Unit {
            nonce: nonce.into(),
            job: job.clone(),
            lease,
            scratch: None,
            stopped: false,
            account: account.into(),
            read_roots: read_roots.into(),
        });
        let unit = state.units.last_mut().expect("registered unit");
        prepare(&mut unit.lease, &mut unit.scratch)?;
        Ok(job)
    }

    /// Whether the lock-and-lease-recovery block is already open.
    ///
    /// Sampled before `allocate`, so a launch can report honestly whether it skipped
    /// that block. This is the only cache on the launch path and therefore the only
    /// place a "cache hit" count is a fact rather than a guess.
    pub fn is_open(&self) -> bool {
        self.state.lock().map(|guard| guard.is_some()).unwrap_or(false)
    }

    /// The recovery work that opened the current state, exactly once.
    pub fn take_recovery_work(&self) -> Option<crate::work::RecoveryWork> {
        self.state.lock().ok()?.as_mut()?.recovery.take()
    }

    /// Live resource units, and how many of them still own a background tree.
    ///
    /// Counts only: a caller records how much is currently held, never what is held.
    pub fn resource_counts(&self) -> ResourceCounts {
        let Ok(guard) = self.state.lock() else {
            return ResourceCounts::default();
        };
        let Some(state) = guard.as_ref() else {
            return ResourceCounts::default();
        };
        let active = state.units.iter().filter(|unit| !unit.stopped).count() as u64;
        let background =
            state.units.iter().filter(|unit| !unit.stopped && unit.scratch.is_some()).count()
                as u64;
        ResourceCounts { active, background }
    }

    /// The Runner must be authenticated before its real token can determine
    /// which workspace ancestors need metadata access. No Execute is sent yet.
    pub fn prepare_workspace(
        &self,
        nonce: &str,
        prepare: impl FnOnce(&mut FileLease) -> Result<()>,
    ) -> Result<()> {
        let mut guard = self.state.lock().map_err(|_| anyhow::anyhow!("runner.runtime_lock"))?;
        ensure!(!self.closing.load(std::sync::atomic::Ordering::Acquire), "runner.runtime_closed");
        let unit = guard
            .as_mut()
            .and_then(|s| s.units.iter_mut().find(|u| u.nonce == nonce))
            .ok_or_else(|| anyhow::anyhow!("runner.runtime_execution_missing"))?;
        ensure!(!unit.stopped, "runner.runtime_execution_stopped");
        prepare(&mut unit.lease)
    }

    pub fn stopped(&self, nonce: &str) -> Result<bool> {
        let mut guard = self.state.lock().map_err(|_| anyhow::anyhow!("runner.runtime_lock"))?;
        let Some(state) = guard.as_mut() else {
            return Ok(true);
        };
        let unit = state
            .units
            .iter_mut()
            .find(|u| u.nonce == nonce)
            .ok_or_else(|| anyhow::anyhow!("runner.runtime_execution_missing"))?;
        ensure!(unit.job.active_processes()? == 0, "runner.runtime_processes_not_stopped");
        unit.stopped = true;
        if state.units.iter().any(|u| !u.stopped) {
            return Ok(false);
        }
        restore(state)?;
        *guard = None;
        Ok(true)
    }

    /// Synchronous shutdown boundary used by Core lifecycle teardown. Only
    /// exact retained Jobs are terminated; no process names or file rollback.
    pub fn shutdown(&self) -> Result<()> {
        self.closing.store(true, std::sync::atomic::Ordering::Release);
        let mut guard = self.state.lock().map_err(|_| anyhow::anyhow!("runner.runtime_lock"))?;
        if let Some(state) = guard.as_mut() {
            for unit in &mut state.units {
                unit.job.stop_for_recovery()?;
                unit.stopped = true;
            }
            restore(state)?;
            *guard = None;
        }
        Ok(())
    }
}

fn restore(state: &mut State) -> Result<()> {
    // Restoration never rolls back file contents. Keep the complete stack on
    // failure so a later repair has all original identities and stop proofs.
    for unit in state.units.iter_mut().rev() {
        unit.lease.restore_existing()?;
    }
    for unit in state.units.iter_mut().rev() {
        unit.lease.finish_restore()?;
    }
    for unit in &state.units {
        if let Some(binding) = &unit.scratch {
            crate::supervisor::remove_scratch_root(&state.root, binding)?;
        }
    }
    Ok(())
}

impl Drop for RuntimeResources {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
