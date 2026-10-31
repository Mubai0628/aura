use anyhow::Result;
use aura_domain_common::ToolConformanceRunId;
use aura_tool_contract::PackageResourceClaimKeyV1;
use aura_tool_domain::PackageVersionBindingV1;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Core-resolved inputs, never deserialized from model arguments. Directory
/// claims must come from the user's selected resources. No secret slots here.
pub struct PackageRunOptions {
    pub workspace_root: Option<PathBuf>,
    pub directory_claims: BTreeMap<PackageResourceClaimKeyV1, PathBuf>,
    /// Canonical settings (`canonical_package_settings_v1`); the run record and report bind them.
    pub settings: Vec<aura_tool_domain::PackageToolSettingsV1>,
    pub powershell: Option<Arc<aura_backend_windows_user::compat::CompatBackend>>,
    pub user_environment: Vec<(OsString, OsString)>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PackageRunStatus {
    pub running: bool,
    pub process_id: Option<u32>,
    /// Fixed Core code only. Runtime stderr and arbitrary errors are not exposed.
    pub failure_code: Option<&'static str>,
}

pub(super) struct RunState {
    pub cancelled: AtomicBool,
    pub status: Mutex<PackageRunStatus>,
}

pub struct PackageConformanceHandle {
    pub run_id: ToolConformanceRunId,
    pub version: PackageVersionBindingV1,
    pub(super) state: Arc<RunState>,
}
impl PackageConformanceHandle {
    /// A request only. The durable terminal, not this signal, proves a stop.
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Release);
    }
    pub fn status(&self) -> Result<PackageRunStatus> {
        self.state
            .status
            .lock()
            .map(|value| value.clone())
            .map_err(|_| anyhow::anyhow!("package.run_status_unavailable"))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PackageRecovery {
    pub interrupted_runs: usize,
    pub removed_scratch_roots: usize,
}
