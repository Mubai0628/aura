use crate::policy;
use anyhow::{Result, bail, ensure};
use aura_domain_common::ContentHash;
use aura_windows_platform::{
    DaclAceIdentity, ExecutionDevice, VerifiedFile, ensure_only_added_ace,
};
use aura_windows_user_contract::*;
use serde::Deserialize;
use std::path::Path;

pub const MIGRATION_BINDING_VERSION: &str = "execution-maintenance-pre-generation-migration-v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreGenerationMigrationBinding {
    pub schema: String,
    pub installation_id: String,
    pub config_sha256: ContentHash,
    pub journal_sha256: ContentHash,
    pub boot_id: String,
    pub target: MaintenanceTarget,
    pub observed_dacl: String,
    pub live_generation: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RestoreAction {
    PersistIntentAndApply,
    PersistAppliedOnly,
    AlreadyComplete,
}

#[derive(Clone, Debug)]
pub struct RestorePlan {
    pub action: RestoreAction,
    pub journal: MaintenanceJournal,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lineage {
    OriginalActive,
    Successor(MaintenanceResourceState),
}

pub fn bind_original_journal(
    current: &[u8],
    expected: &ContentHash,
    backup: Option<&[u8]>,
) -> Result<Vec<u8>> {
    match backup {
        Some(bytes) => {
            ensure!(
                ContentHash::sha256(bytes) == *expected,
                "maintenance.migration_original_changed"
            );
            Ok(bytes.to_vec())
        }
        None => {
            ensure!(
                ContentHash::sha256(current) == *expected,
                "maintenance.migration_original_missing"
            );
            Ok(current.to_vec())
        }
    }
}

pub fn lock_migration_file(path: &Path) -> Result<VerifiedFile> {
    match aura_windows_platform::lock_execution_resources(path) {
        Ok(file) => Ok(file),
        Err(error) if error.raw_os_error() == Some(32) => {
            bail!("maintenance.migration_in_use")
        }
        Err(error) => Err(error.into()),
    }
}

pub fn plan_restore(
    config: &MaintenanceConfig,
    journal: MaintenanceJournal,
    original: &MaintenanceJournal,
    binding: &PreGenerationMigrationBinding,
    live_dacl: &str,
    live_generation: &str,
    live_boot: &str,
) -> Result<RestorePlan> {
    ensure!(binding.schema == MIGRATION_BINDING_VERSION, "maintenance.migration_schema");
    ensure!(
        binding.installation_id == config.installation_id,
        "maintenance.migration_installation"
    );
    ensure!(journal.schema == MAINTENANCE_VERSION, "maintenance.version");
    ensure!(original.schema == MAINTENANCE_VERSION, "maintenance.version");
    ensure!(journal.config_sha256 == binding.config_sha256, "maintenance.migration_config");
    ensure!(original.config_sha256 == binding.config_sha256, "maintenance.migration_config");
    ensure!(live_boot == binding.boot_id, "maintenance.migration_boot");
    ensure!(live_generation == binding.live_generation, "maintenance.migration_generation");
    ensure!(
        (8..=128).contains(&binding.live_generation.len())
            && binding.live_generation.bytes().all(|byte| byte.is_ascii_graphic()),
        "maintenance.object_generation"
    );
    ensure!(
        original.error.as_deref() == Some("maintenance.acl_drift"),
        "maintenance.migration_error"
    );
    ensure!(
        journal.error.as_deref() == Some("maintenance.acl_drift"),
        "maintenance.migration_error"
    );
    let device = execution_device(&binding.target)?;
    let bound = ExecutionDevice::canonicalize_dacl(&binding.observed_dacl)?;
    let live = ExecutionDevice::canonicalize_dacl(live_dacl)?;
    let planned = device.plan(&config.installation_id, &bound, true)?;
    let capability =
        DaclAceIdentity::allow_mask(device.access_mask(), &device.sid(&config.installation_id)?);
    ensure_only_added_ace(&bound, &planned, &capability)?;
    ensure!(planned != bound, "maintenance.migration_no_capability_gap");
    let lineage = verify_lineage(original, &journal, binding, &bound, &planned)?;
    ensure!(live == bound || live == planned, "maintenance.migration_live_dacl_changed");
    match lineage {
        Lineage::Successor(existing) => {
            let mut journal = journal;
            let action = match existing {
                MaintenanceResourceState::Applied if live == planned => {
                    RestoreAction::AlreadyComplete
                }
                MaintenanceResourceState::Pending if live == planned => {
                    mark_applied(&mut journal, &binding.target, &binding.boot_id)?;
                    RestoreAction::PersistAppliedOnly
                }
                MaintenanceResourceState::Pending if live == bound => {
                    RestoreAction::PersistIntentAndApply
                }
                _ => bail!("maintenance.migration_receipt_mismatch"),
            };
            ensure!(error_retained(&journal), "maintenance.migration_error");
            Ok(RestorePlan { action, journal, before: bound, after: planned })
        }
        Lineage::OriginalActive => {
            let (current_generation, current_state) = {
                let current = current_device_receipt(&journal, &binding.boot_id, &binding.target)?;
                (current.object_generation.clone(), current.state)
            };
            ensure!(current_generation.is_none(), "maintenance.migration_has_generation");
            ensure!(
                matches!(
                    current_state,
                    MaintenanceResourceState::Applied | MaintenanceResourceState::Pending
                ),
                "maintenance.migration_receipt_state"
            );
            let mut next = journal;
            retire_current(&mut next, &binding.boot_id, &binding.target)?;
            let state = if live == planned {
                MaintenanceResourceState::Applied
            } else {
                MaintenanceResourceState::Pending
            };
            next.resources.push(MaintenanceResourceReceipt {
                boot_id: binding.boot_id.clone(),
                target: binding.target.clone(),
                parent_dacl: None,
                before: bound.clone(),
                after: planned.clone(),
                state,
                object_generation: Some(binding.live_generation.clone()),
            });
            let successor = next
                .resources
                .last()
                .ok_or_else(|| anyhow::anyhow!("maintenance.migration_receipt_mismatch"))?;
            policy::validate_receipt(config, successor)?;
            ensure!(error_retained(&next), "maintenance.migration_error");
            let action = if live == planned {
                RestoreAction::PersistAppliedOnly
            } else {
                RestoreAction::PersistIntentAndApply
            };
            Ok(RestorePlan { action, journal: next, before: bound, after: planned })
        }
    }
}

pub fn error_retained(journal: &MaintenanceJournal) -> bool {
    journal.error.as_deref() == Some("maintenance.acl_drift")
}

fn execution_device(target: &MaintenanceTarget) -> Result<ExecutionDevice> {
    match target {
        MaintenanceTarget::Null => Ok(ExecutionDevice::Null),
        MaintenanceTarget::Ksec => Ok(ExecutionDevice::Ksec),
        MaintenanceTarget::OwnerNamespace { .. } => bail!("maintenance.migration_target"),
    }
}

fn verify_lineage(
    original: &MaintenanceJournal,
    current: &MaintenanceJournal,
    binding: &PreGenerationMigrationBinding,
    bound: &str,
    planned: &str,
) -> Result<Lineage> {
    ensure!(
        current.resources.len() >= original.resources.len(),
        "maintenance.migration_receipt_mismatch"
    );
    let extras = current.resources.len() - original.resources.len();
    ensure!(extras <= 1, "maintenance.migration_duplicate_successor");
    let prefix = &current.resources[..original.resources.len()];
    let mut retired = 0usize;
    for (old, new) in original.resources.iter().zip(prefix) {
        if old == new {
            continue;
        }
        if old.boot_id == binding.boot_id && old.target == binding.target && retired_from(old, new)
        {
            retired += 1;
            continue;
        }
        bail!("maintenance.migration_unrelated_receipt");
    }
    if extras == 0 {
        ensure!(retired == 0, "maintenance.migration_receipt_mismatch");
        return Ok(Lineage::OriginalActive);
    }
    ensure!(retired == 1, "maintenance.migration_receipt_mismatch");
    let extra = current
        .resources
        .last()
        .ok_or_else(|| anyhow::anyhow!("maintenance.migration_receipt_mismatch"))?;
    ensure!(
        bound_successor(extra, binding, bound, planned),
        "maintenance.migration_receipt_mismatch"
    );
    Ok(Lineage::Successor(extra.state))
}

fn bound_successor(
    extra: &MaintenanceResourceReceipt,
    binding: &PreGenerationMigrationBinding,
    bound: &str,
    planned: &str,
) -> bool {
    extra.boot_id == binding.boot_id
        && extra.target == binding.target
        && extra.parent_dacl.is_none()
        && extra.before == bound
        && extra.after == planned
        && extra.object_generation.as_deref() == Some(binding.live_generation.as_str())
        && matches!(
            extra.state,
            MaintenanceResourceState::Pending | MaintenanceResourceState::Applied
        )
}

fn retired_from(old: &MaintenanceResourceReceipt, new: &MaintenanceResourceReceipt) -> bool {
    old.boot_id == new.boot_id
        && old.target == new.target
        && old.parent_dacl == new.parent_dacl
        && old.before == new.before
        && old.after == new.after
        && old.object_generation == new.object_generation
        && old.object_generation.is_none()
        && matches!(
            old.state,
            MaintenanceResourceState::Applied | MaintenanceResourceState::Pending
        )
        && new.state == MaintenanceResourceState::Retired
}

fn current_device_receipt<'a>(
    journal: &'a MaintenanceJournal,
    boot: &str,
    target: &MaintenanceTarget,
) -> Result<&'a MaintenanceResourceReceipt> {
    let matching = journal
        .resources
        .iter()
        .filter(|item| {
            item.boot_id == boot
                && item.target == *target
                && matches!(
                    item.state,
                    MaintenanceResourceState::Pending | MaintenanceResourceState::Applied
                )
        })
        .collect::<Vec<_>>();
    ensure!(matching.len() == 1, "maintenance.migration_receipt_mismatch");
    Ok(matching[0])
}

fn retire_current(
    journal: &mut MaintenanceJournal,
    boot: &str,
    target: &MaintenanceTarget,
) -> Result<()> {
    let mut found = false;
    for item in &mut journal.resources {
        if item.boot_id == boot
            && item.target == *target
            && matches!(
                item.state,
                MaintenanceResourceState::Pending | MaintenanceResourceState::Applied
            )
        {
            ensure!(!found, "maintenance.duplicate_resource");
            ensure!(item.object_generation.is_none(), "maintenance.migration_has_generation");
            item.state = MaintenanceResourceState::Retired;
            found = true;
        }
    }
    ensure!(found, "maintenance.migration_receipt_mismatch");
    Ok(())
}

fn mark_applied(
    journal: &mut MaintenanceJournal,
    target: &MaintenanceTarget,
    boot: &str,
) -> Result<()> {
    let mut found = false;
    for item in &mut journal.resources {
        if item.boot_id == boot
            && item.target == *target
            && item.state == MaintenanceResourceState::Pending
            && item.object_generation.is_some()
        {
            ensure!(!found, "maintenance.duplicate_resource");
            item.state = MaintenanceResourceState::Applied;
            found = true;
        }
    }
    ensure!(found, "maintenance.migration_receipt_mismatch");
    Ok(())
}
