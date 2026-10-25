//! Fixed pre-contract DNS/service-path experiment. It cannot execute a caller
//! script, register a backend or change an installed Runner/host policy.
use anyhow::{Result, bail, ensure};
use aura_windows_platform::{
    capture_file_dacl, current_user_token_identity, grant_empty_execution_write_root,
    inspect_existing_path, restore_file_dacl,
};
use serde_json::{Value, json};
use std::{fs, net::IpAddr, path::Path, time::Duration};

pub fn run(root: &Path, peer: &str, run_id: &str) -> Result<Value> {
    // The optional IPv6 scope is a local interface identifier, not code.
    let address = peer.split('%').collect::<Vec<_>>();
    ensure!(address.len() <= 2 && address[0].parse::<IpAddr>().is_ok(), "literal peer IP required");
    ensure!(address.len() == 1 || address[1].parse::<u32>().is_ok(), "invalid peer interface");
    ensure!(run_id.len() == 32 && run_id.bytes().all(|c| c.is_ascii_hexdigit()), "fixture ID");
    let record: Value = serde_json::from_slice(&fs::read(root.join("record.json"))?)?;
    let (owner, restricted) = current_user_token_identity()?;
    ensure!(
        !restricted && record["offline_sid"] == owner && record["state"] == "installed",
        "dedicated installed identity required"
    );
    aura_backend_windows_user::runtime_package::verify(root, &record)?;
    let id = record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("install ID"))?;
    let runtime = root.join("runtime");
    let attempt =
        tempfile::Builder::new().prefix("r06-network-").tempdir_in(root.join("scratch"))?;
    let allowed = attempt.path().join("allowed");
    fs::create_dir(&allowed)?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(anyhow::Error::msg)?;
    let sid = format!(
        "S-1-5-21-{}",
        random
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four bytes"))
                .max(1)
                .to_string())
            .collect::<Vec<_>>()
            .join("-")
    );
    let before = capture_file_dacl(&allowed)?;
    let identity = inspect_existing_path(&allowed, false)?.identity;
    fs::write(
        attempt.path().join("acl-journal.json"),
        serde_json::to_vec(&json!({"path":allowed,"before":before,"sid":sid,"pending":"grant"}))?,
    )?;
    if let Err(error) = grant_empty_execution_write_root(&allowed, &sid) {
        bail!("network fixture grant failed: {error}; preserve {}", attempt.keep().display());
    }
    let outcome = (|| -> Result<Value> {
        let applied = capture_file_dacl(&allowed)?;
        fs::write(
            attempt.path().join("acl-journal.json"),
            serde_json::to_vec(
                &json!({"path":allowed,"before":before,"applied":applied,"sid":sid}),
            )?,
        )?;
        let observation = (|| -> Result<Value> {
            fs::write(
                allowed.join("dns-probe-native.ps1"),
                include_str!("../../../scripts/r06/dns-probe-native.ps1"),
            )?;
            fs::write(
                allowed.join("network-experiment-probe.ps1"),
                include_str!("../../../scripts/r06/network-experiment-probe.ps1"),
            )?;
            let script =
                format!("& ./network-experiment-probe.ps1 -Peer '{peer}' -RunId '{run_id}'");
            let args = aura_backend_windows_user::environment::powershell_command(&script);
            let mut process = crate::monitor::launch(
                &sid,
                &runtime.join("pwsh.exe"),
                &args,
                attempt.path(),
                Some(id),
                Some(&runtime),
            )?;
            crate::monitor::collect_until(&mut process, false, 16_384, Duration::from_secs(20))
        })();
        if inspect_existing_path(&allowed, false)?.identity != identity
            || capture_file_dacl(&allowed)? != applied
        {
            bail!("network fixture ACL drift");
        }
        if let Err(error) = restore_file_dacl(&allowed, &before) {
            bail!("network fixture ACL restore failed: {error}");
        }
        let after = capture_file_dacl(&allowed)?;
        if !crate::probe::restored_dacl_matches(&before, &after) {
            bail!("network fixture ACL restore drift");
        }
        Ok(
            json!({"schema":"r06-dns-service-experiment-v2","run_id":run_id,"peer":peer,"dedicated_identity":owner,"write_restricted_child":true,"observation":observation?,"acl_restored":true,"scope":"fixed direct UDP and native DNS comparison; not production Backend or external isolation acceptance","r06_complete":false}),
        )
    })();
    match outcome {
        Ok(value) => Ok(value),
        Err(error) => bail!("{error}; preserve network fixture {}", attempt.keep().display()),
    }
}
