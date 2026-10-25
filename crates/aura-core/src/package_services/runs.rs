//! Conformance preparation and runs. A preparation is a concrete preview the user
//! approves; only its exact hash may start it, and only once.
use super::{MAX_PREPARATIONS, PREPARATION_LIFETIME, PackageServices, Preparation, preparation_id};
use crate::package_lifecycle::PackageRunOptions;
use anyhow::{Result, ensure};
use aura_persistence_v2::packages;
use aura_tool_domain::*;
use std::{sync::Arc, time::Instant};

/// Core-resolved inputs a preparation needs besides the user's settings.
pub(crate) struct RunEnvironment {
    pub powershell: Option<Arc<aura_backend_windows_user::compat::CompatBackend>>,
    pub user_environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
}

/// Each case's wall-clock limit and output limit during Conformance.
const CASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const CASE_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

impl PackageServices {
    pub fn prepare(
        &self,
        request: &PackageConformancePrepareRequestV1,
        environment: RunEnvironment,
    ) -> Result<PackageConformancePreparationV1> {
        let content = {
            let store = self.store()?;
            packages::load_content(store.database.connection(), &request.version)?
        };
        let settings =
            canonical_package_settings_v1(&content.descriptor.manifest, &request.settings)?;
        let prepared = self.owner.prepare(
            &request.version,
            PackageRunOptions {
                // Conformance runs in its own empty scratch workspace.
                workspace_root: None,
                directory_claims: Default::default(),
                settings,
                powershell: environment.powershell,
                user_environment: environment.user_environment,
                timeout: CASE_TIMEOUT,
                max_output_bytes: CASE_OUTPUT_BYTES,
            },
        )?;
        let preview = prepared.preview()?;
        let preview_hash = prepared.preview_hash()?;
        let id = preparation_id();
        let mut held = self
            .preparations
            .lock()
            .map_err(|_| anyhow::anyhow!("package.preparations_unavailable"))?;
        expire(&mut held);
        while held.len() >= MAX_PREPARATIONS {
            if let Some(oldest) = held.pop_front() {
                let _ = oldest.prepared.cancel();
            }
        }
        held.push_back(Preparation { id: id.clone(), created: Instant::now(), prepared });
        Ok(PackageConformancePreparationV1 { preparation_id: id, preview, preview_hash })
    }

    /// The user's explicit approval of one concrete preview. The preparation is
    /// consumed whether or not the hash matches, so an approval cannot be retried
    /// against a different preview.
    pub fn start(
        &self,
        request: &PackageConformanceStartRequestV1,
    ) -> Result<PackageConformanceViewV1> {
        let preparation = {
            let mut held = self
                .preparations
                .lock()
                .map_err(|_| anyhow::anyhow!("package.preparations_unavailable"))?;
            expire(&mut held);
            let index =
                held.iter().position(|entry| entry.id == request.preparation_id).ok_or_else(
                    || anyhow::anyhow!("package.preparation_expired: 预览已失效，请重新准备验证"),
                )?;
            held.remove(index).ok_or_else(|| anyhow::anyhow!("package.preparation_expired"))?
        };
        if preparation.prepared.preview_hash()? != request.preview_hash {
            let _ = preparation.prepared.cancel();
            anyhow::bail!("package.preview_changed: 预览已变化，请重新查看后再批准");
        }
        let handle = self.owner.start(preparation.prepared)?;
        let version = handle.version.clone();
        let run_id = handle.run_id;
        self.runs
            .lock()
            .map_err(|_| anyhow::anyhow!("package.runs_unavailable"))?
            .insert(run_id, handle);
        let store = self.store()?;
        let run = packages::conformance::conformance(store.database.connection(), &version)?;
        super::views::conformance_view(&version, run.as_ref())
    }

    /// A request only: the durable terminal written by the run proves the stop.
    pub fn cancel(
        &self,
        request: &PackageConformanceCancelRequestV1,
    ) -> Result<PackageConformanceViewV1> {
        {
            let mut runs =
                self.runs.lock().map_err(|_| anyhow::anyhow!("package.runs_unavailable"))?;
            runs.retain(|_, handle| handle.status().is_ok_and(|status| status.running));
            let handle = runs
                .get(&request.run_id)
                .ok_or_else(|| anyhow::anyhow!("package.run_not_active"))?;
            ensure!(
                handle.version.package_id == request.version.package_id
                    && handle.version.package_version == request.version.package_version
                    && handle.version.package_fingerprint == request.version.package_fingerprint,
                "package.run_binding"
            );
            handle.cancel();
        }
        self.conformance(&request.version)
    }

    pub fn conformance(&self, reference: &PackageVersionRefV1) -> Result<PackageConformanceViewV1> {
        let store = self.store()?;
        let db = store.database.connection();
        let (_, binding) = super::views::find_version(db, reference)?;
        let run = packages::conformance::conformance(db, &binding)?;
        super::views::conformance_view(&binding, run.as_ref())
    }
}

fn expire(held: &mut std::collections::VecDeque<Preparation>) {
    while held.front().is_some_and(|entry| entry.created.elapsed() > PREPARATION_LIFETIME) {
        if let Some(stale) = held.pop_front() {
            let _ = stale.prepared.cancel();
        }
    }
}
