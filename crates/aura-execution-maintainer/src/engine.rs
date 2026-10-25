use crate::{policy, storage::ProtectedStorage};
use anyhow::{Result, bail, ensure};
use aura_domain_common::UtcTimestamp;
use aura_windows_platform::{ExecutionDevice, execution_maintenance as native};
use aura_windows_user_contract::*;

pub struct Engine {
    pub config: MaintenanceConfig,
    pub journal: MaintenanceJournal,
    pub(crate) store: ProtectedStorage,
}
impl Engine {
    pub(crate) fn persist(&mut self) -> Result<()> {
        self.journal.observed_at = UtcTimestamp::now();
        self.store.save(&self.journal)
    }
    fn entry(
        &mut self,
        boot: &str,
        target: MaintenanceTarget,
        parent: Option<String>,
        observed: &str,
        live_generation: Option<String>,
    ) -> Result<usize> {
        let index = apply_device_observation(
            &mut self.journal,
            &self.config,
            boot,
            target,
            parent,
            observed,
            live_generation,
        )?;
        self.persist()?;
        Ok(index)
    }
    pub fn refresh(&mut self) -> Result<()> {
        let boot = native::boot_identity()?;
        for item in &mut self.journal.resources {
            if item.boot_id != boot {
                item.state = MaintenanceResourceState::Retired;
            }
        }
        self.persist()?;
        for device in ExecutionDevice::EXECUTION {
            let observed = device.capture_dacl()?;
            let generation = match device.capture_generation() {
                Ok(generation) => Some(generation),
                Err(error) if error.to_string() == "maintenance.object_generation" => None,
                Err(error) => return Err(error.into()),
            };
            let target = if device == ExecutionDevice::Null {
                MaintenanceTarget::Null
            } else {
                MaintenanceTarget::Ksec
            };
            let index = self.entry(&boot, target, None, &observed, generation)?;
            let entry = &self.journal.resources[index];
            device.apply(&self.config.installation_id, &entry.before, &entry.after, true)?;
            self.journal.resources[index].state = MaintenanceResourceState::Applied;
            self.persist()?;
        }
        for session in native::owner_sessions(&self.config.owner_sid)? {
            let directory = session.namespace()?;
            let observed = directory.capture()?;
            let target = MaintenanceTarget::OwnerNamespace {
                session_id: session.id(),
                logon_id: session.logon().into(),
            };
            let index =
                self.entry(&boot, target, Some(directory.parent_dacl().into()), &observed, None)?;
            let entry = &self.journal.resources[index];
            directory.apply(
                &self.config.installation_id,
                &policy::accounts(&self.config),
                &entry.before,
                &entry.after,
                true,
            )?;
            for (other, record) in self.journal.resources.iter_mut().enumerate() {
                if other != index
                    && record.boot_id == boot
                    && matches!(&record.target,MaintenanceTarget::OwnerNamespace{session_id,..}if *session_id==session.id())
                {
                    record.state = MaintenanceResourceState::Retired;
                }
            }
            self.journal.resources[index].state = MaintenanceResourceState::Applied;
            self.persist()?;
        }
        Ok(())
    }
}

/// One device observation against the in-memory journal. Persist is the
/// caller's job so Current generation advances retire and seed together.
pub(crate) fn apply_device_observation(
    journal: &mut MaintenanceJournal,
    config: &MaintenanceConfig,
    boot: &str,
    target: MaintenanceTarget,
    parent: Option<String>,
    observed: &str,
    live_generation: Option<String>,
) -> Result<usize> {
    let matching = journal
        .resources
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.boot_id == boot
                && r.target == target
                && matches!(
                    r.state,
                    MaintenanceResourceState::Pending | MaintenanceResourceState::Applied
                )
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    ensure!(matching.len() <= 1, "maintenance.duplicate_resource");
    if let Some(&i) = matching.first() {
        let previous = &journal.resources[i];
        ensure!(previous.parent_dacl == parent, "maintenance.parent_drift");
        ensure!(
            matches!(
                previous.state,
                MaintenanceResourceState::Pending | MaintenanceResourceState::Applied
            ),
            "maintenance.removal_pending"
        );
        if let Some(device) = execution_device(&target) {
            match policy::device_generation(
                previous,
                observed,
                live_generation.as_deref(),
                &config.installation_id,
                device,
            )? {
                policy::DeviceGeneration::Current => {
                    policy::desired(previous, observed, false)?;
                    return adopt_current_object_generation(
                        config,
                        &mut journal.resources,
                        i,
                        live_generation,
                    );
                }
                policy::DeviceGeneration::NewOsBaseline => {
                    journal.resources[i].state = MaintenanceResourceState::Retired;
                }
                policy::DeviceGeneration::UnrelatedDrift => {
                    bail!("maintenance.acl_drift");
                }
            }
        } else {
            policy::desired(previous, observed, false)?;
            return Ok(i);
        }
    }
    ensure!(journal.resources.len() < 4096, "maintenance.receipt_capacity");
    let prior = policy::prior_namespace(&journal.resources, boot, &target, observed)?;
    let before = prior.map_or_else(|| observed.to_owned(), |i| journal.resources[i].before.clone());
    let after = policy::planned(config, &target, &before)?;
    // A scoped ACE that is present without a current boot journal is not
    // adopted. Setup must supply its exact reviewed seed receipt.
    ensure!(prior.is_some() || after != observed, "maintenance.unowned_existing_resource");
    let entry = MaintenanceResourceReceipt {
        boot_id: boot.into(),
        target,
        parent_dacl: parent,
        before,
        after,
        state: MaintenanceResourceState::Pending,
        object_generation: live_generation,
    };
    policy::validate_receipt(config, &entry)?;
    if let Some(index) = prior {
        journal.resources[index].state = MaintenanceResourceState::Retired;
    }
    journal.resources.push(entry);
    Ok(journal.resources.len() - 1)
}

fn adopt_current_object_generation(
    config: &MaintenanceConfig,
    resources: &mut Vec<MaintenanceResourceReceipt>,
    index: usize,
    live_generation: Option<String>,
) -> Result<usize> {
    let Some(generation) = live_generation else {
        return Ok(index);
    };
    if resources[index].object_generation.as_deref() == Some(generation.as_str()) {
        return Ok(index);
    }
    if resources[index].object_generation.is_none() {
        let mut filled = resources[index].clone();
        filled.object_generation = Some(generation);
        policy::validate_receipt(config, &filled)?;
        resources[index] = filled;
        return Ok(index);
    }
    let mut successor = resources[index].clone();
    successor.state = MaintenanceResourceState::Pending;
    successor.object_generation = Some(generation);
    policy::validate_receipt(config, &successor)?;
    ensure!(resources.len() < 4096, "maintenance.receipt_capacity");
    resources[index].state = MaintenanceResourceState::Retired;
    resources.push(successor);
    Ok(resources.len() - 1)
}

fn execution_device(target: &MaintenanceTarget) -> Option<ExecutionDevice> {
    match target {
        MaintenanceTarget::Null => Some(ExecutionDevice::Null),
        MaintenanceTarget::Ksec => Some(ExecutionDevice::Ksec),
        MaintenanceTarget::OwnerNamespace { .. } => None,
    }
}

/// Invoked by the elevated, fixed installer only after SCM stop/deletion. All
/// paths come from the protected fixed data directory, not an invocation.
pub fn remove_installed() -> Result<MaintenanceJournal> {
    ensure!(aura_windows_platform::current_process_is_elevated()?, "maintenance.removal_elevation");
    let store = ProtectedStorage::open()?;
    let (config, bytes, _config) =
        store.read::<MaintenanceConfig>("config.json", 4 * 1024 * 1024)?;
    policy::validate(&config)?;
    let (caller, restricted) = aura_windows_platform::current_user_token_identity()?;
    ensure!(
        !restricted && (caller == config.owner_sid || caller == "S-1-5-18"),
        "maintenance.removal_owner"
    );
    let (mut journal, _, _journal) =
        store.read::<MaintenanceJournal>("journal.json", 8 * 1024 * 1024)?;
    ensure!(
        journal.schema == MAINTENANCE_VERSION
            && journal.config_sha256 == aura_domain_common::ContentHash::sha256(&bytes),
        "maintenance.removal_binding"
    );
    drop(_journal);
    for receipt in &journal.resources {
        policy::validate_receipt(&config, receipt)?;
    }
    let boot = native::boot_identity()?;
    for index in (0..journal.resources.len()).rev() {
        let receipt = &journal.resources[index];
        if receipt.boot_id != boot {
            journal.resources[index].state = MaintenanceResourceState::Retired;
            continue;
        }
        if matches!(
            receipt.state,
            MaintenanceResourceState::Removed | MaintenanceResourceState::Retired
        ) {
            continue;
        }
        journal.resources[index].state = MaintenanceResourceState::Removing;
        store.save(&journal)?;
        let receipt = &journal.resources[index];
        match &receipt.target {
            MaintenanceTarget::Null | MaintenanceTarget::Ksec => {
                let device = if receipt.target == MaintenanceTarget::Null {
                    ExecutionDevice::Null
                } else {
                    ExecutionDevice::Ksec
                };
                policy::desired(receipt, &device.capture_dacl()?, true)?;
                device.apply(&config.installation_id, &receipt.after, &receipt.before, false)?;
            }
            MaintenanceTarget::OwnerNamespace { session_id, .. } => {
                native::remove_recorded_namespace(
                    &config.installation_id,
                    &policy::accounts(&config),
                    *session_id,
                    receipt
                        .parent_dacl
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("maintenance.parent_missing"))?,
                    &receipt.before,
                    &receipt.after,
                )?;
            }
        }
        journal.resources[index].state = MaintenanceResourceState::Removed;
        store.save(&journal)?;
    }
    journal.observed_at = UtcTimestamp::now();
    journal.error = None;
    store.save(&journal)?;
    Ok(journal)
}
