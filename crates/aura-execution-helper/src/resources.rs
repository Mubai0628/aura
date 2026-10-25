use anyhow::{Result, bail, ensure};
use aura_windows_platform::{
    AccountNetworkPolicy, AccountNetworkSpec, ExecutionDevice, manage_account_network,
};
use serde_json::{Value, json};
use std::{
    io::{Read, Seek},
    path::Path,
};

pub fn record(root: &Path) -> Result<(Value, aura_windows_platform::VerifiedFile)> {
    let (owner, restricted) = aura_windows_platform::current_user_token_identity()?;
    ensure!(!restricted, "installer.restricted_caller");
    let root_handle = aura_windows_platform::open_verified_publish_parent(root)?;
    aura_windows_platform::verify_execution_installation_acl(root, &owner, false)?;
    let (_, mut file) =
        aura_backend_windows_user::file_binding::capture(&root.join("record.json"), true)?;
    aura_windows_platform::verify_execution_installation_acl(
        &root.join("record.json"),
        &owner,
        false,
    )?;
    file.file_mut().rewind()?;
    let mut bytes = Vec::new();
    file.file_mut().take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "installer.record_size");
    let value: Value = serde_json::from_slice(&bytes)?;
    let id = value["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installer.id_missing"))?;
    ensure!(
        value["owner_sid"] == owner
            && root.canonicalize()?
                == aura_windows_platform::common_application_data_directory()?
                    .join("Aura/R06-Spike")
                    .join(id)
                    .canonicalize()?,
        "installer.root_owner_changed"
    );
    Ok((value, root_handle))
}
fn kind(name: &str) -> Result<ExecutionDevice> {
    match name {
        "null" => Ok(ExecutionDevice::Null),
        "ksec" => Ok(ExecutionDevice::Ksec),
        "cng" => Ok(ExecutionDevice::Cng),
        _ => bail!("installer.unknown_device"),
    }
}
pub fn device_plan(name: &str, id: &str) -> Result<Value> {
    let device = kind(name)?;
    let before = device.capture_dacl()?;
    let after = device.plan(id, &before, true)?;
    let removed = device.plan(id, &before, false)?;
    Ok(
        json!({"schema":"r06-device-v1","device":device.name(),"access_mask":device.access_mask(),"sid":device.sid(id)?,"ready":before==after,"before":before,"after":after,"removed":removed}),
    )
}
pub fn device(name: &str, operation: &str, root: &Path) -> Result<Value> {
    let (record, _root) = record(root)?;
    let id =
        record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installer.id_missing"))?;
    let device = kind(name)?;
    if operation == "inspect" {
        return device_plan(name, id);
    }
    ensure!(aura_windows_platform::current_process_is_elevated()?, "installer.elevation_required");
    let install = match operation {
        "install" if name != "cng" => true,
        "remove" => false,
        _ => bail!("installer.unknown_or_retired_action"),
    };
    let transition = &record[format!("{name}_transition")];
    ensure!(
        (record["state"] == if install { "upgrading" } else { "removing" }
            || !install && record["state"] == format!("removing-{name}"))
            && record["pending"] == format!("{operation}-{name}")
            && transition["operation"] == operation
            && transition["sid"] == device.sid(id)?,
        "installer.unjournaled_device_change"
    );
    device.apply(
        id,
        transition["before"].as_str().ok_or_else(|| anyhow::anyhow!("installer.before_missing"))?,
        transition["after"].as_str().ok_or_else(|| anyhow::anyhow!("installer.after_missing"))?,
        install,
    )?;
    device_plan(name, id)
}
fn policy(value: &str) -> Result<AccountNetworkPolicy> {
    match value {
        "block_all" => Ok(AccountNetworkPolicy::BlockAll),
        "allow_loopback" => Ok(AccountNetworkPolicy::AllowLoopback),
        _ => bail!("installer.network_policy_unknown"),
    }
}
pub fn network_plan(id: &str, sid: &str, mode: &str) -> Result<Value> {
    Ok(manage_account_network(
        &AccountNetworkSpec {
            install_id: id.into(),
            offline_sid: sid.into(),
            policy: policy(mode)?,
            expected_sublayer_weight: None,
        },
        "plan",
    )?)
}
pub fn network(operation: &str, root: &Path) -> Result<Value> {
    let (record, _root) = record(root)?;
    if operation != "inspect" {
        ensure!(
            aura_windows_platform::current_process_is_elevated()?,
            "installer.elevation_required"
        );
        ensure!(matches!(operation, "install" | "remove"), "installer.unknown_network_action");
        ensure!(
            (record["pending"] == format!("{operation}-wfp")
                || operation == "remove" && record["pending"] == "remove-old-wfp")
                && (record["state"] == "upgrading" || record["state"] == "removing"),
            "installer.unjournaled_network_change"
        );
    }
    Ok(manage_account_network(
        &AccountNetworkSpec {
            install_id: record["install_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("installer.id_missing"))?
                .into(),
            offline_sid: record["offline_sid"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("installer.account_missing"))?
                .into(),
            policy: policy(
                record["wfp_policy"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("installer.policy_missing"))?,
            )?,
            expected_sublayer_weight: record["wfp"]["sublayer_weight"]
                .as_u64()
                .map(u16::try_from)
                .transpose()?,
        },
        operation,
    )?)
}
