//! Current-boot resource verification consumed by Core and the dedicated Runner.
use anyhow::{Result, ensure};
use aura_domain_common::ContentHash;
use aura_windows_platform::execution_maintenance as native;
use aura_windows_user_contract::*;
use std::io::{Read, Seek};
fn read<T: serde::de::DeserializeOwned>(name: &str, max: u64) -> Result<(T, Vec<u8>)> {
    ensure!(matches!(name, "config.json" | "journal.json"), "maintenance.fixed_read");
    let path = native::data_directory()?.join(name);
    native::verify_protected_path(&path)?;
    let (_, mut file) = crate::file_binding::capture(&path, true)?;
    file.file_mut().rewind()?;
    let mut bytes = Vec::new();
    file.file_mut().take(max + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= max, "maintenance.read_limit");
    Ok((serde_json::from_slice(&bytes)?, bytes))
}
fn validate_receipt(
    config: &MaintenanceConfig,
    receipt: &MaintenanceResourceReceipt,
) -> Result<()> {
    let after = match &receipt.target {
        MaintenanceTarget::Null => aura_windows_platform::ExecutionDevice::Null.plan(
            &config.installation_id,
            &receipt.before,
            true,
        )?,
        MaintenanceTarget::Ksec => aura_windows_platform::ExecutionDevice::Ksec.plan(
            &config.installation_id,
            &receipt.before,
            true,
        )?,
        MaintenanceTarget::OwnerNamespace { .. } => {
            let mut accounts = vec![config.offline_sid.clone()];
            accounts.extend(config.online_sid.clone());
            native::namespace_plan(&config.installation_id, &accounts, &receipt.before)?
        }
    };
    ensure!(
        receipt.before != receipt.after && after == receipt.after,
        "maintenance.invalid_receipt"
    );
    Ok(())
}
/// The protected SYSTEM receipt binds this boot and the owner's logon. Normal
/// users deliberately have no READ_CONTROL on the restricted namespace. The
/// Runner checks actual fixed-object access with its final restricted token;
/// Setup/Repair and the maintainer alone compare the full live DACL.
pub fn verify_current(
    config_hash: &ContentHash,
    id: &str,
    owner: &str,
    offline: &str,
    online: Option<&str>,
) -> Result<()> {
    let (config, bytes) = read::<MaintenanceConfig>("config.json", 4 * 1024 * 1024)?;
    ensure!(
        ContentHash::sha256(&bytes) == *config_hash
            && config.installation_id == id
            && config.owner_sid == owner
            && config.offline_sid == offline
            && config.online_sid.as_deref() == online,
        "maintenance.installation_binding"
    );
    ensure!(config.schema == MAINTENANCE_VERSION, "maintenance.version");
    let (journal, _) = read::<MaintenanceJournal>("journal.json", 8 * 1024 * 1024)?;
    ensure!(
        journal.config_sha256 == *config_hash
            && journal.schema == MAINTENANCE_VERSION
            && journal.error.is_none(),
        "maintenance.receipt_binding"
    );
    ensure!(native::service_running()?, "maintenance.service_unavailable");
    let boot = native::boot_identity()?;
    let session = aura_windows_platform::current_windows_session_id()?;
    // Runner is logged on as the dedicated identity in the owner's session;
    // its AuthenticationId differs. Exact object state and owner-controlled
    // installation binding identify the resource, not the Runner's logon LUID.
    let records=journal.resources.iter().filter(|r|r.boot_id==boot&&matches!(r.target,MaintenanceTarget::OwnerNamespace{session_id,..}if session_id==session)&&r.state==MaintenanceResourceState::Applied).collect::<Vec<_>>();
    ensure!(records.len() == 1, "maintenance.current_session_unavailable");
    validate_receipt(&config, records[0])?;
    let (caller, restricted) = aura_windows_platform::current_user_token_identity()?;
    ensure!(
        !restricted && (caller == owner || caller == offline || online == Some(caller.as_str())),
        "maintenance.caller_identity"
    );
    // The maintainer journals the interactive WTS logon. Setup/Repair run from
    // the owner's linked UAC token in the same session, so their AuthenticationId
    // differs. Daily non-elevated owner still has to match that journaled logon.
    if caller == owner && !aura_windows_platform::current_process_is_elevated()? {
        let logon = native::current_logon_identity()?;
        ensure!(
            matches!(&records[0].target, MaintenanceTarget::OwnerNamespace { logon_id, .. } if *logon_id == logon),
            "maintenance.owner_logon_changed"
        );
    }
    if aura_windows_platform::current_process_is_elevated()? {
        ensure!(
            records[0].after == aura_windows_platform::execution_namespace_dacl()?,
            "maintenance.current_namespace_changed"
        );
    }
    for device in aura_windows_platform::ExecutionDevice::EXECUTION {
        let target = if device == aura_windows_platform::ExecutionDevice::Null {
            MaintenanceTarget::Null
        } else {
            MaintenanceTarget::Ksec
        };
        let entry = journal
            .resources
            .iter()
            .filter(|r| {
                r.boot_id == boot
                    && r.target == target
                    && r.state == MaintenanceResourceState::Applied
            })
            .collect::<Vec<_>>();
        ensure!(entry.len() == 1, "maintenance.current_device_unavailable");
        validate_receipt(&config, entry[0])?;
        if aura_windows_platform::current_process_is_elevated()? {
            ensure!(entry[0].after == device.capture_dacl()?, "maintenance.current_device_changed");
        }
    }
    Ok(())
}
