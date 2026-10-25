use anyhow::{Result, ensure};
use aura_windows_platform::{ExecutionDevice, execution_maintenance::namespace_plan};
use aura_windows_user_contract::*;
pub fn accounts(config: &MaintenanceConfig) -> Vec<String> {
    let mut accounts = vec![config.offline_sid.clone()];
    accounts.extend(config.online_sid.clone());
    accounts
}
fn local_account(sid: &str) -> bool {
    let Some(parts) = sid.strip_prefix("S-1-5-21-") else { return false };
    let parts = parts.split('-').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>();
    parts.is_ok_and(|v| v.len() == 4 && v[3] >= 1000)
}
pub fn validate(config: &MaintenanceConfig) -> Result<()> {
    ensure!(config.schema == MAINTENANCE_VERSION, "maintenance.version");
    ExecutionDevice::Null.sid(&config.installation_id)?;
    let mut sids = accounts(config);
    sids.push(config.owner_sid.clone());
    ensure!(
        sids.iter().all(|s| local_account(s))
            && sids.iter().collect::<std::collections::BTreeSet<_>>().len() == sids.len(),
        "maintenance.account_identity"
    );
    ensure!(config.initial_resources.len() <= 3, "maintenance.seed_count");
    let mut seen = std::collections::BTreeSet::new();
    for receipt in &config.initial_resources {
        validate_receipt(config, receipt)?;
        ensure!(receipt.state == MaintenanceResourceState::Applied, "maintenance.seed_not_applied");
        ensure!(seen.insert(serde_json::to_string(&receipt.target)?), "maintenance.duplicate_seed");
    }
    Ok(())
}
pub fn planned(
    config: &MaintenanceConfig,
    target: &MaintenanceTarget,
    before: &str,
) -> Result<String> {
    Ok(match target {
        MaintenanceTarget::Null => {
            ExecutionDevice::Null.plan(&config.installation_id, before, true)?
        }
        MaintenanceTarget::Ksec => {
            ExecutionDevice::Ksec.plan(&config.installation_id, before, true)?
        }
        MaintenanceTarget::OwnerNamespace { session_id, logon_id } => {
            ensure!(
                *session_id > 0
                    && logon_id.len() == 16
                    && logon_id.bytes().all(|v| v.is_ascii_hexdigit()),
                "maintenance.session_identity"
            );
            namespace_plan(&config.installation_id, &accounts(config), before)?
        }
    })
}
pub fn validate_receipt(
    config: &MaintenanceConfig,
    receipt: &MaintenanceResourceReceipt,
) -> Result<()> {
    ExecutionDevice::Null.sid(&receipt.boot_id)?;
    ensure!(
        receipt.before.len() <= 1024 * 1024 && receipt.after.len() <= 1024 * 1024,
        "maintenance.acl_limit"
    );
    ensure!(
        receipt.before != receipt.after
            && planned(config, &receipt.target, &receipt.before)? == receipt.after,
        "maintenance.receipt_transform"
    );
    ensure!(
        matches!(receipt.target, MaintenanceTarget::OwnerNamespace { .. })
            == receipt.parent_dacl.is_some(),
        "maintenance.parent_binding"
    );
    if let Some(generation) = &receipt.object_generation {
        ensure!(
            (8..=128).contains(&generation.len())
                && generation.bytes().all(|byte| byte.is_ascii_graphic()),
            "maintenance.object_generation"
        );
    }
    Ok(())
}
/// Exact before/after comparison: never overwrite unrelated ACL drift. An
/// intent is durable before any write; interruption after writing is idempotent.
pub fn desired(
    receipt: &MaintenanceResourceReceipt,
    observed: &str,
    remove: bool,
) -> Result<String> {
    ensure!(observed == receipt.before || observed == receipt.after, "maintenance.acl_drift");
    Ok(if remove { &receipt.before } else { &receipt.after }.clone())
}

/// Restart/servicing may recreate NUL/KsecDD while NT BootIdentifier stays
/// the same. Adopt that baseline only when the kernel object generation changed
/// and `plan()` can add the missing Aura ACE. Same-object DACL edits, missing
/// generation, or a DACL that cannot be planned stay fail-closed drift.
///
/// Current still requires Engine to retire a recorded generation when live
/// capture advances. Image-base reuse (live==recorded after Restart) and a
/// failed capture keep fail-closed UnrelatedDrift. Journals without a
/// generation never become a compatible restore.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceGeneration {
    Current,
    NewOsBaseline,
    UnrelatedDrift,
}

pub fn device_generation(
    receipt: &MaintenanceResourceReceipt,
    observed: &str,
    live_generation: Option<&str>,
    installation_id: &str,
    device: ExecutionDevice,
) -> Result<DeviceGeneration> {
    if observed == receipt.before || observed == receipt.after {
        return Ok(DeviceGeneration::Current);
    }
    let Some(recorded) = receipt.object_generation.as_deref() else {
        return Ok(DeviceGeneration::UnrelatedDrift);
    };
    let Some(live) = live_generation else {
        return Ok(DeviceGeneration::UnrelatedDrift);
    };
    if live == recorded {
        return Ok(DeviceGeneration::UnrelatedDrift);
    }
    Ok(match device.plan(installation_id, observed, true) {
        Ok(planned) if planned != observed => DeviceGeneration::NewOsBaseline,
        _ => DeviceGeneration::UnrelatedDrift,
    })
}

/// An authenticated owner re-logon may reuse the same kernel directory. Only
/// an exact previous owned after-state can be carried into that new logon.
pub fn prior_namespace(
    resources: &[MaintenanceResourceReceipt],
    boot: &str,
    target: &MaintenanceTarget,
    observed: &str,
) -> Result<Option<usize>> {
    let MaintenanceTarget::OwnerNamespace { session_id, logon_id } = target else {
        return Ok(None);
    };
    let found=resources.iter().enumerate().filter(|(_,r)|r.boot_id==boot&&r.after==observed&&matches!(r.state,MaintenanceResourceState::Applied|MaintenanceResourceState::Pending)&&matches!(&r.target,MaintenanceTarget::OwnerNamespace{session_id:s,logon_id:l}if s==session_id&&l!=logon_id)).map(|(i,_)|i).collect::<Vec<_>>();
    ensure!(found.len() <= 1, "maintenance.ambiguous_previous_session");
    Ok(found.first().copied())
}
