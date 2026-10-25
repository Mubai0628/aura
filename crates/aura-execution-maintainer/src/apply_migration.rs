use super::{
    migrate::{
        PreGenerationMigrationBinding, RestoreAction, bind_original_journal, error_retained,
        lock_migration_file, plan_restore,
    },
    storage::ProtectedStorage,
};
use anyhow::{Result, ensure};
use aura_domain_common::{ContentHash, UtcTimestamp};
use aura_windows_platform::{ExecutionDevice, VerifiedFile, execution_maintenance as native};
use aura_windows_user_contract::*;
use serde_json::{Value, json};
use std::io::{self, Write};

pub fn apply_bound_migration(
    expected_installation_id: &str,
    binding: &PreGenerationMigrationBinding,
) -> Result<Value> {
    ensure!(
        aura_windows_platform::current_process_is_elevated()?,
        "maintenance.migration_elevation"
    );
    native::require_service_stopped()?;
    let store = ProtectedStorage::open()?;
    let _lock = acquire_migration_lock(&store)?;
    native::require_service_stopped()?;
    let (config, config_bytes, _config) =
        store.read::<MaintenanceConfig>("config.json", 4 * 1024 * 1024)?;
    crate::policy::validate(&config)?;
    ensure!(
        config.installation_id == expected_installation_id
            && expected_installation_id == binding.installation_id,
        "maintenance.migration_installation"
    );
    ensure!(
        ContentHash::sha256(&config_bytes) == binding.config_sha256,
        "maintenance.migration_config"
    );
    let (caller, restricted) = aura_windows_platform::current_user_token_identity()?;
    ensure!(!restricted && caller == config.owner_sid, "maintenance.migration_owner");
    let config_hash = ContentHash::sha256(&config_bytes);
    let (journal, journal_bytes) = store.load_journal(&config, &config_hash)?;
    ensure!(
        journal.schema == MAINTENANCE_VERSION && journal.config_sha256 == binding.config_sha256,
        "maintenance.journal_config_changed"
    );
    for receipt in &journal.resources {
        crate::policy::validate_receipt(&config, receipt)?;
    }
    let original = load_original_journal(&store, &journal_bytes, &binding.journal_sha256)?;
    let device = match binding.target {
        MaintenanceTarget::Null => ExecutionDevice::Null,
        MaintenanceTarget::Ksec => ExecutionDevice::Ksec,
        MaintenanceTarget::OwnerNamespace { .. } => {
            anyhow::bail!("maintenance.migration_target")
        }
    };
    let live_dacl = device.capture_dacl()?;
    let live_generation = device.capture_generation()?;
    let live_boot = native::boot_identity()?;
    let mut plan = plan_restore(
        &config,
        journal,
        &original,
        binding,
        &live_dacl,
        &live_generation,
        &live_boot,
    )?;
    ensure!(error_retained(&plan.journal), "maintenance.migration_error");
    if plan.action != RestoreAction::AlreadyComplete {
        persist(&store, &mut plan.journal)?;
    }
    if plan.action == RestoreAction::PersistIntentAndApply {
        device.apply(&config.installation_id, &plan.before, &plan.after, true)?;
        let recapture = device.capture_dacl()?;
        ensure!(recapture == plan.after, "maintenance.migration_postcondition");
        if let Some(item) = plan.journal.resources.iter_mut().rev().find(|item| {
            item.target == binding.target
                && item.boot_id == binding.boot_id
                && item.state == MaintenanceResourceState::Pending
        }) {
            item.state = MaintenanceResourceState::Applied;
        }
        persist(&store, &mut plan.journal)?;
    }
    ensure!(error_retained(&plan.journal), "maintenance.migration_error");
    Ok(json!({
        "schema": "execution-maintenance-pre-generation-migration-receipt-v1",
        "installation_id": binding.installation_id,
        "target": binding.target,
        "action": match plan.action {
            RestoreAction::PersistIntentAndApply => "applied",
            RestoreAction::PersistAppliedOnly => "receipt_backfilled",
            RestoreAction::AlreadyComplete => "already_complete",
        },
        "error": plan.journal.error,
        "error_cleared": false
    }))
}

fn persist(store: &ProtectedStorage, journal: &mut MaintenanceJournal) -> Result<()> {
    journal.observed_at = UtcTimestamp::now();
    store.save(journal)
}

fn acquire_migration_lock(store: &ProtectedStorage) -> Result<VerifiedFile> {
    let path = store.root.join("migration.lock");
    if !path.exists() {
        match std::fs::OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                file.write_all(b"execution-maintenance-migration-lock-v1")?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    native::verify_protected_path(&path)?;
    lock_migration_file(&path)
}

fn load_original_journal(
    store: &ProtectedStorage,
    current: &[u8],
    expected: &ContentHash,
) -> Result<MaintenanceJournal> {
    let path = store.root.join("journal.pre-migration.json");
    let backup = if path.exists() {
        native::verify_protected_path(&path)?;
        Some(std::fs::read(&path)?)
    } else {
        None
    };
    let bytes = bind_original_journal(current, expected, backup.as_deref())?;
    if backup.is_none() {
        std::fs::write(&path, &bytes)?;
        native::verify_protected_path(&path)?;
    }
    Ok(serde_json::from_slice(&bytes)?)
}
