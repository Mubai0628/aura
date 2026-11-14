//! Core's locations: worked out again from its release and the data folder Main gave it, and
//! refused when they name another folder. From then on Core's diagnostics go to its log there.
//! Once Core has opened its stores there, the copy a move left at the old place is removed.
use std::{sync::Mutex, time::Duration};

use anyhow::{Context, Result};
use aura_config::AuraPaths;
use aura_data_location::{InstallCopy, Locations, Roots, confirm, route_core_log};
use aura_runtime_contract::BundleNamespace;
use serde_json::json;

use crate::{core_log::append_core_log, options::CoreOptions, release::CoreReleaseContext};

/// After the start, so the removal does not compete with it for the disk.
const REMOVAL_DELAY: Duration = Duration::from_secs(20);

static CONFIRMED: Mutex<Option<(Roots, BundleNamespace, Locations)>> = Mutex::new(None);

pub fn confirm_core_locations(
    options: &CoreOptions,
    release: &CoreReleaseContext,
) -> Result<AuraPaths> {
    let namespace = &release.release_descriptor.bundle_namespace;
    let roots = Roots::from_system()?;
    let install = InstallCopy::for_manifest(&release.manifest_path)?;
    let locations = confirm(&roots, namespace, &install, &options.data_root)?;
    let _ = route_core_log(locations.core_log.clone());
    let locations = locations
        .with_global_memory(&roots, namespace)
        .context("cannot find where global memory is")?;
    if let Ok(mut confirmed) = CONFIRMED.lock() {
        *confirmed = Some((roots, namespace.clone(), locations.clone()));
    }
    Ok(AuraPaths::from_locations(locations))
}

/// The roots, namespace and locations Core confirmed at its start, with global memory.
pub fn confirmed() -> Option<(Roots, BundleNamespace, Locations)> {
    CONFIRMED.lock().ok().and_then(|confirmed| confirmed.clone())
}

/// Removes the copies a move left behind, on a thread of its own after the start. Called once the
/// stores opened at the place the data is now; until then the old copy is the one that works.
pub fn remove_old_copies_later() {
    let Some((roots, namespace, locations)) = confirmed() else {
        return;
    };
    if locations.pending_removal.is_empty() {
        return;
    }
    let removing = move || {
        std::thread::sleep(REMOVAL_DELAY);
        let background = aura_windows_platform::enter_background_mode();
        let started = std::time::Instant::now();
        match aura_data_move::remove_old_copies(&roots, &namespace, &locations) {
            Ok(removal) => append_core_log(
                "data_old_copies_removed",
                json!({
                    "background": background,
                    "removed": removal.removed.len(),
                    "failed": removal.failed.iter().map(|(_, reason)| reason).collect::<Vec<_>>(),
                    "refused": removal.refused.len(),
                    "elapsed_ms": started.elapsed().as_millis(),
                }),
            ),
            Err(error) => append_core_log(
                "data_old_copies_removal_failed",
                json!({ "reason": error.to_string() }),
            ),
        }
    };
    // A thread that cannot start leaves the copies listed; the next start removes them.
    let _ = std::thread::Builder::new().name("aura-old-copies".to_owned()).spawn(removing);
}
