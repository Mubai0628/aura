use anyhow::{Result, ensure};
use aura_backend_windows_user::file_binding;
use aura_domain_common::{Revision, ToolWireVersionV1};
use aura_windows_user_contract::{ASSURANCE_VERSION, ExecutionAccount, InstallationManifest};
use std::path::Path;

pub fn build(root: &Path, core: &Path) -> Result<InstallationManifest> {
    let (record, _root) = super::resources::record(root)?;
    ensure!(
        record["state"] == "installed" && record["wfp_policy"] == "block_all",
        "installer.resources_not_committed"
    );
    let required = |name: &str| {
        record[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("installer.missing_{name}"))
    };
    let cipher = file_binding::capture(&root.join("private/credential.dpapi"), true)?
        .0
        .sha256
        .ok_or_else(|| anyhow::anyhow!("installer.credential_hash_missing"))?;
    ensure!(
        record["credential_sha256"].as_str().is_some_and(|v| v.eq_ignore_ascii_case(&cipher.hex)),
        "installer.credential_record_changed"
    );
    aura_backend_windows_user::runtime_package::verify(root, &record)?;
    let resource_record_hash = file_binding::capture(&root.join("record.json"), true)?
        .0
        .sha256
        .ok_or_else(|| anyhow::anyhow!("installer.record_hash_missing"))?;
    let maintenance_path =
        aura_windows_platform::execution_maintenance::data_directory()?.join("config.json");
    aura_windows_platform::execution_maintenance::verify_protected_path(&maintenance_path)?;
    let maintenance_config_sha256 = file_binding::capture(&maintenance_path, true)?
        .0
        .sha256
        .ok_or_else(|| anyhow::anyhow!("installer.maintenance_hash_missing"))?;
    Ok(InstallationManifest {
        installation_version: aura_windows_user_contract::INSTALLATION_VERSION.into(),
        schema_version: ToolWireVersionV1,
        assurance_version: ASSURANCE_VERSION.into(),
        revision: Revision::INITIAL,
        installation_id: required("install_id")?,
        owner_sid: required("owner_sid")?,
        resource_record_hash,
        maintenance_config_sha256,
        core_image: file_binding::capture(core, true)?.0,
        runner_image: file_binding::capture(&root.join("aura-command-runner.exe"), true)?.0,
        powershell_image: file_binding::capture(&root.join("runtime/pwsh.exe"), true)?.0,
        cmd_image: file_binding::capture(
            &aura_windows_platform::windows_directory()?.join("System32/cmd.exe"),
            true,
        )?
        .0,
        offline: ExecutionAccount {
            name: required("account")?,
            sid: required("offline_sid")?,
            credential_sha256: cipher,
        },
        online: Some(crate::online::account(root, &record)?),
    })
}

pub fn repair_leases(root: &Path) -> Result<serde_json::Value> {
    let (record, _root) = super::resources::record(root)?;
    let owner =
        record["owner_sid"].as_str().ok_or_else(|| anyhow::anyhow!("installer.owner_missing"))?;
    let installation =
        record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installer.id_missing"))?;
    aura_backend_windows_user::lease_recovery::recover_for_setup(root, installation, owner)
}
