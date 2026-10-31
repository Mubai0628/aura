#[cfg(test)]
use aura_projection_contract::{BackendLifecycle, CapabilityReadiness};
use aura_projection_contract::{CoreHealth, RuntimeReadiness};
#[cfg(test)]
use std::sync::{Arc, Mutex};

pub(crate) use aura_core_base::readiness::ReadinessStore;
#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;

#[cfg(test)]
pub(super) fn current(state: &crate::RuntimeState) -> RuntimeReadiness {
    match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            // The sidecar readiness set exists only when the tool-policy
            // chain was really built. When its configuration domain is damaged the
            // honest answer is the default store's own `Unavailable` backends —
            // never `Ready`, and never a panic on a control path.
            let core_health = if state.ready.load(std::sync::atomic::Ordering::Acquire) {
                CoreHealth::Ready
            } else {
                CoreHealth::Starting
            };
            match services.security_sidecars() {
                Ok(sidecars) => sidecars.readiness.snapshot(core_health),
                Err(_) => ReadinessStore::default().snapshot(CoreHealth::Starting),
            }
        }
        crate::StartupMode::RecoveryRequired { .. } => {
            ReadinessStore::default().snapshot(CoreHealth::RecoveryRequired)
        }
    }
}

pub(super) async fn current_nonblocking(state: &crate::RuntimeState) -> RuntimeReadiness {
    match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            let core_health = if state.ready.load(std::sync::atomic::Ordering::Acquire) {
                CoreHealth::Ready
            } else {
                CoreHealth::Starting
            };
            // The sandbox sidecar set exists only when the tool-policy
            // chain was really built. When its configuration domain is damaged
            // there is no sidecar readiness to report, and the honest answer is
            // the default store's own `Unavailable` backends — never `Ready`, and
            // never a panic on the control path the user's confirmed rule requires
            // to survive.
            //
            // `CoreHealth::Starting` is the right value here rather than
            // `RecoveryRequired`: Aura's data does not require recovery, and the
            // capability-level reason is reported by the CoreHealth projection's
            // `core.ready_degraded` status, which names the capability and its
            // stable code.
            match services.security_sidecars() {
                Ok(sidecars) => sidecars.readiness.snapshot_nonblocking(core_health).await,
                Err(_) => ReadinessStore::default().snapshot(CoreHealth::Starting),
            }
        }
        crate::StartupMode::RecoveryRequired { .. } => {
            ReadinessStore::default().snapshot(CoreHealth::RecoveryRequired)
        }
    }
}
