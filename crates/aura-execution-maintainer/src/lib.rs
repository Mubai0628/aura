#![forbid(unsafe_code)]
mod apply_migration;
mod engine;
mod journal_stage;
mod migrate;
#[cfg(test)]
mod migrate_tests;
pub mod policy;
mod storage;
#[cfg(test)]
mod tests;
use anyhow::{Result, ensure};
pub use apply_migration::apply_bound_migration;
use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_windows_platform::execution_maintenance::{self as native, MaintenanceEvent};
use aura_windows_user_contract::*;
pub use engine::Engine;
pub use engine::remove_installed;
pub use migrate::{MIGRATION_BINDING_VERSION, PreGenerationMigrationBinding};
use std::{
    io,
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

pub fn run(events: Receiver<MaintenanceEvent>) -> io::Result<()> {
    worker(events).map_err(io::Error::other)
}
fn worker(events: Receiver<MaintenanceEvent>) -> Result<()> {
    native::require_system()?;
    let _image = storage::verify_image()?;
    let store = storage::ProtectedStorage::open()?;
    let (config, bytes, _config) =
        store.read::<MaintenanceConfig>("config.json", 4 * 1024 * 1024)?;
    policy::validate(&config)?;
    let hash = ContentHash::sha256(&bytes);
    let journal = if store.has_journal() {
        let (journal, _) = store.load_journal(&config, &hash)?;
        ensure!(
            journal.schema == MAINTENANCE_VERSION && journal.config_sha256 == hash,
            "maintenance.journal_config_changed"
        );
        for receipt in &journal.resources {
            policy::validate_receipt(&config, receipt)?;
        }
        journal
    } else {
        MaintenanceJournal {
            schema: MAINTENANCE_VERSION.into(),
            config_sha256: hash,
            resources: config.initial_resources.clone(),
            observed_at: UtcTimestamp::now(),
            error: None,
        }
    };
    let mut engine = Engine { config, journal, store };
    // First save also covers the reviewed handoff from legacy owned resources.
    engine.persist()?;
    let mut refresh = true;
    let mut retry_at = None;
    let mut retry_count = 0;
    loop {
        if native::stopping() {
            return Ok(());
        }
        if refresh {
            let outcome = engine.refresh();
            retry_at = transient_delay(&outcome, retry_count).map(|delay| Instant::now() + delay);
            retry_count = if retry_at.is_some() { retry_count.saturating_add(1) } else { 0 };
            engine.journal.error = outcome.err().map(|e| e.to_string());
            engine.persist()?;
        }
        refresh = match events.recv_timeout(Duration::from_secs(1)) {
            Ok(MaintenanceEvent::Refresh) => {
                retry_count = 0;
                true
            }
            Ok(MaintenanceEvent::Stop) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => retry_at.is_some_and(|at| Instant::now() >= at),
        };
    }
}

// Boot/logon notifications can precede creation of the fixed kernel objects.
// Retry only a missing/not-ready OS resource, with a capped delay. Unrelated
// DACL drift stays fail-closed and never retries. A new OS baseline on NUL/Ksec
// after Restart/servicing is a receipt generation, not drift.
fn transient_delay(outcome: &Result<()>, count: u32) -> Option<Duration> {
    let error = outcome.as_ref().err()?;
    let transient = error
        .chain()
        .filter_map(|e| e.downcast_ref::<io::Error>())
        .any(|e| matches!(e.raw_os_error(), Some(2 | 3 | 21 | 1168)));
    transient.then(|| Duration::from_secs((1u64 << count.min(5)).min(30)))
}
