use anyhow::{Result, ensure};
use aura_backend_windows_user::file_binding;
use aura_domain_common::ContentHash;
use aura_windows_user_contract::ExecutionAccount;
use std::path::Path;
mod access;
pub use access::access;

pub fn account(root: &Path, record: &serde_json::Value) -> Result<ExecutionAccount> {
    let owner = record["owner_sid"].as_str().ok_or_else(|| anyhow::anyhow!("online.owner"))?;
    let path = root.join("private/online-account.json");
    aura_windows_platform::verify_execution_installation_acl(&path, owner, true)?;
    let bytes = std::fs::read(&path)?;
    ensure!(bytes.len() <= 65536, "online.record_limit");
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    ensure!(
        value["schema"] == "execution-online-account-v1"
            && value["install_id"] == record["install_id"]
            && value["owner_sid"] == owner
            && value["state"] == "installed",
        "online.record_binding"
    );
    let sid = value["sid"].as_str().ok_or_else(|| anyhow::anyhow!("online.sid"))?;
    let name = value["name"].as_str().ok_or_else(|| anyhow::anyhow!("online.name"))?;
    let id = record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("online.install_id"))?;
    // Reuse the native installation UUID validator before deriving a name.
    aura_windows_platform::ExecutionDevice::Null.sid(id)?;
    ensure!(
        name == format!("AuraOn_{}", &id.replace('-', "")[..12])
            && sid != owner
            && value["sid"] != record["offline_sid"],
        "online.identity_mixed"
    );
    let credential = root.join("private/online-credential.dpapi");
    aura_windows_platform::verify_execution_installation_acl(&credential, owner, true)?;
    let hash = file_binding::capture(&credential, true)?
        .0
        .sha256
        .ok_or_else(|| anyhow::anyhow!("online.credential_hash"))?;
    ensure!(
        value["credential_sha256"].as_str().is_some_and(|v| v.eq_ignore_ascii_case(&hash.hex)),
        "online.credential_binding"
    );
    Ok(ExecutionAccount { name: name.into(), sid: sid.into(), credential_sha256: hash })
}
