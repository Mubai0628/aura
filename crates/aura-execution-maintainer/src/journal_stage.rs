use crate::policy;
use anyhow::{Result, bail, ensure};
use aura_domain_common::ContentHash;
use aura_windows_user_contract::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalStageDecision {
    UseCommitted,
    PromoteStaged,
    CorruptCommitted,
    Conflict,
}

pub fn decide_journal_stage(
    committed: Option<Result<MaintenanceJournal, ()>>,
    staged: Option<Result<MaintenanceJournal, ()>>,
    config: &MaintenanceConfig,
) -> JournalStageDecision {
    match (committed, staged) {
        (None, None) => JournalStageDecision::CorruptCommitted,
        (None, Some(Ok(_))) => JournalStageDecision::PromoteStaged,
        (None, Some(Err(()))) | (Some(Err(())), _) => JournalStageDecision::CorruptCommitted,
        (Some(Ok(_)), None) => JournalStageDecision::UseCommitted,
        (Some(Ok(_)), Some(Err(()))) => JournalStageDecision::UseCommitted,
        (Some(Ok(committed)), Some(Ok(staged))) => {
            if journal_receipts_equal(&committed, &staged) {
                JournalStageDecision::UseCommitted
            } else if service_persist_continues(&committed, &staged, config) {
                JournalStageDecision::PromoteStaged
            } else {
                JournalStageDecision::Conflict
            }
        }
    }
}

#[cfg(test)]
pub fn staged_continues_committed(
    committed: &MaintenanceJournal,
    staged: &MaintenanceJournal,
    config: &MaintenanceConfig,
) -> bool {
    service_persist_continues(committed, staged, config)
}

fn service_persist_continues(
    committed: &MaintenanceJournal,
    staged: &MaintenanceJournal,
    config: &MaintenanceConfig,
) -> bool {
    if committed.schema != staged.schema || committed.config_sha256 != staged.config_sha256 {
        return false;
    }
    if staged.resources.len() < committed.resources.len() {
        return false;
    }
    if !committed.resources.iter().zip(&staged.resources).all(|(old, new)| {
        receipt_identity_stable(old, new) && state_continues(old.state, new.state)
    }) {
        return false;
    }
    staged.resources[committed.resources.len()..]
        .iter()
        .all(|extra| extra_receipt_continues(config, extra))
}

fn extra_receipt_continues(config: &MaintenanceConfig, extra: &MaintenanceResourceReceipt) -> bool {
    matches!(extra.state, MaintenanceResourceState::Pending | MaintenanceResourceState::Applied)
        && policy::validate_receipt(config, extra).is_ok()
}

fn state_continues(old: MaintenanceResourceState, new: MaintenanceResourceState) -> bool {
    old == new
        || new == MaintenanceResourceState::Retired
        || matches!(
            (old, new),
            (
                MaintenanceResourceState::Pending,
                MaintenanceResourceState::Applied | MaintenanceResourceState::Removing
            ) | (MaintenanceResourceState::Applied, MaintenanceResourceState::Removing)
                | (MaintenanceResourceState::Removing, MaintenanceResourceState::Removed)
        )
}

fn journal_receipts_equal(left: &MaintenanceJournal, right: &MaintenanceJournal) -> bool {
    left.schema == right.schema
        && left.config_sha256 == right.config_sha256
        && left.error == right.error
        && left.resources == right.resources
}

fn receipt_identity_stable(
    old: &MaintenanceResourceReceipt,
    new: &MaintenanceResourceReceipt,
) -> bool {
    old.boot_id == new.boot_id
        && old.target == new.target
        && old.parent_dacl == new.parent_dacl
        && old.before == new.before
        && old.after == new.after
        && generation_continues(&old.object_generation, &new.object_generation)
}

fn generation_continues(old: &Option<String>, new: &Option<String>) -> bool {
    match (old, new) {
        (None, None) | (None, Some(_)) => true,
        (Some(left), Some(right)) => left == right,
        (Some(_), None) => false,
    }
}

pub fn journal_matches_config(
    journal: &MaintenanceJournal,
    config: &MaintenanceConfig,
    config_hash: &ContentHash,
) -> Result<()> {
    ensure!(journal.schema == MAINTENANCE_VERSION, "maintenance.version");
    ensure!(journal.config_sha256 == *config_hash, "maintenance.journal_config_changed");
    for receipt in &journal.resources {
        policy::validate_receipt(config, receipt)?;
    }
    Ok(())
}

pub fn quarantine_name(kind: &str, bytes: &[u8]) -> String {
    let digest = ContentHash::sha256(bytes);
    format!("{kind}.corrupt-{}", &digest.hex[..16])
}

pub fn load_journal_bytes(
    committed: Option<&[u8]>,
    staged: Option<&[u8]>,
    config: &MaintenanceConfig,
    config_hash: &ContentHash,
) -> Result<(MaintenanceJournal, bool)> {
    let parse = |bytes: Option<&[u8]>| {
        bytes.map(|bytes| serde_json::from_slice::<MaintenanceJournal>(bytes).map_err(|_| ()))
    };
    match decide_journal_stage(parse(committed), parse(staged), config) {
        JournalStageDecision::UseCommitted => {
            let bytes = committed.ok_or_else(|| anyhow::anyhow!("maintenance.journal_corrupt"))?;
            Ok((serde_json::from_slice(bytes)?, false))
        }
        JournalStageDecision::PromoteStaged => {
            let bytes = staged.ok_or_else(|| anyhow::anyhow!("maintenance.journal_corrupt"))?;
            let journal = serde_json::from_slice(bytes)?;
            journal_matches_config(&journal, config, config_hash)?;
            Ok((journal, true))
        }
        JournalStageDecision::CorruptCommitted => bail!("maintenance.journal_corrupt"),
        JournalStageDecision::Conflict => bail!("maintenance.journal_stage_conflict"),
    }
}
