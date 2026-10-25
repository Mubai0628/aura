use anyhow::{Result, ensure};
use aura_windows_platform::{ExecutionDevice, execution_maintenance as native};
use aura_windows_user_contract::*;
use std::path::Path;

pub fn config(root: &Path) -> Result<MaintenanceConfig> {
    let (record, _root) = crate::resources::record(root)?;
    ensure!(
        record["state"] == "installed"
            && record.get("namespace_capability").is_none()
            && record.get("namespace_transition").is_none(),
        "maintenance.finish_namespace_handoff"
    );
    let text = |key: &str| {
        record[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("maintenance.record_field:{key}"))
    };
    let mut config = MaintenanceConfig {
        schema: MAINTENANCE_VERSION.into(),
        installation_id: text("install_id")?,
        owner_sid: text("owner_sid")?,
        offline_sid: text("offline_sid")?,
        online_sid: None,
        initial_resources: vec![],
    };
    // Online identity is a separate owned journal; its presence never changes
    // the Offline WFP record or confers network permission on an Agent mode.
    let online = root.join("private/online-account.json");
    if online.exists() {
        aura_windows_platform::verify_execution_installation_acl(&online, &config.owner_sid, true)?;
        let bytes = std::fs::read(&online)?;
        ensure!(bytes.len() <= 65536, "maintenance.online_record_size");
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        ensure!(
            value["schema"] == "execution-online-account-v1"
                && value["install_id"] == config.installation_id
                && value["owner_sid"] == config.owner_sid
                && value["state"] == "installed",
            "maintenance.online_record_binding"
        );
        config.online_sid = Some(
            value["sid"].as_str().ok_or_else(|| anyhow::anyhow!("maintenance.online_sid"))?.into(),
        );
    }
    let boot = native::boot_identity()?;
    for device in ExecutionDevice::EXECUTION {
        let owned = &record[format!("{}_capability", device.name())];
        let before =
            owned["before"].as_str().ok_or_else(|| anyhow::anyhow!("maintenance.device_before"))?;
        let after =
            owned["after"].as_str().ok_or_else(|| anyhow::anyhow!("maintenance.device_after"))?;
        ensure!(
            owned["sid"] == device.sid(&config.installation_id)? && device.capture_dacl()? == after,
            "maintenance.device_handoff_changed"
        );
        config.initial_resources.push(MaintenanceResourceReceipt {
            boot_id: boot.clone(),
            target: if device == ExecutionDevice::Null {
                MaintenanceTarget::Null
            } else {
                MaintenanceTarget::Ksec
            },
            parent_dacl: None,
            before: before.into(),
            after: after.into(),
            state: MaintenanceResourceState::Applied,
            object_generation: match device.capture_generation() {
                Ok(generation) => Some(generation),
                Err(error)
                    if error.kind() == std::io::ErrorKind::Other
                        && error.to_string() == "maintenance.object_generation" =>
                {
                    None
                }
                Err(error) => return Err(error.into()),
            },
        });
    }
    aura_execution_maintainer::policy::validate(&config)?;
    Ok(config)
}
pub fn state(root: &Path) -> Result<serde_json::Value> {
    let (record, _root) = crate::resources::record(root)?;
    let path = native::data_directory()?.join("config.json");
    native::verify_protected_path(&path)?;
    let bytes = std::fs::read(path)?;
    ensure!(bytes.len() <= 4 * 1024 * 1024, "maintenance.config_limit");
    let config: MaintenanceConfig = serde_json::from_slice(&bytes)?;
    let online = crate::online::account(root, &record)?;
    let outcome = aura_backend_windows_user::maintenance::verify_current(
        &aura_domain_common::ContentHash::sha256(&bytes),
        record["install_id"].as_str().unwrap_or_default(),
        record["owner_sid"].as_str().unwrap_or_default(),
        record["offline_sid"].as_str().unwrap_or_default(),
        Some(&online.sid),
    );
    Ok(
        serde_json::json!({"ready":outcome.is_ok(),"error":outcome.err().map(|e|e.to_string()),"configuration_version":config.schema,"host_acceptance":false}),
    )
}

pub fn migrate_pre_generation(root: &Path, binding_path: &Path) -> Result<serde_json::Value> {
    let (record, _root) = crate::resources::record(root)?;
    let install_id =
        record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installer.id_missing"))?;
    let bytes = std::fs::read(binding_path)?;
    ensure!(bytes.len() <= 1024 * 1024, "maintenance.migration_binding_size");
    let binding: aura_execution_maintainer::PreGenerationMigrationBinding =
        serde_json::from_slice(&bytes)?;
    aura_execution_maintainer::apply_bound_migration(install_id, &binding)
}
