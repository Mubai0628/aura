use anyhow::{Result, bail, ensure};
use aura_windows_platform::{
    capture_file_dacl, current_user_token_identity, grant_empty_execution_write_root,
    inspect_existing_path, restore_file_dacl,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn mechanism() -> Result<Value> {
    run(None, false, &[], None)
}

/// Current owner identity only: useful for startup diagnosis, never a dedicated
/// identity/network/host acceptance result. Commands remain fixed tool probes.
pub fn diagnostics(review: &Path) -> Result<Value> {
    let plan: Value = serde_json::from_slice(&fs::read(review)?)?;
    let (owner, restricted) = current_user_token_identity()?;
    ensure!(
        !restricted && plan["owner_sid"] == owner && plan["schema"] == "r06-review-v0",
        "diagnostics requires review owner identity"
    );
    let id =
        plan["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installation ID missing"))?;
    for device in aura_windows_platform::ExecutionDevice::EXECUTION {
        ensure!(device.inspect(id)?, "installed {} capability required", device.name());
    }
    let tools =
        serde_json::from_value::<Vec<crate::shell_probes::ToolImage>>(plan["tools"].clone())?;
    let root = plan["root"].as_str().ok_or_else(|| anyhow::anyhow!("review root missing"))?;
    aura_backend_windows_user::runtime_package::verify(Path::new(root), &plan)?;
    run(None, false, &tools, Some(id))
}

pub fn host(root: &Path) -> Result<Value> {
    let record: Value = serde_json::from_slice(&fs::read(root.join("record.json"))?)?;
    let (sid, restricted) = current_user_token_identity()?;
    ensure!(
        !restricted && record["offline_sid"] == sid && record["state"] == "installed",
        "dedicated installed Offline identity required"
    );
    let executable = std::env::current_exe()?;
    let executable_hash = hash(&fs::read(&executable)?);
    ensure!(
        executable.canonicalize()? == root.join("runner.exe").canonicalize()?,
        "wrong Runner location"
    );
    ensure!(
        record["runner_sha256"].as_str().is_some_and(|h| h.eq_ignore_ascii_case(&executable_hash)),
        "Runner changed"
    );
    let tools =
        serde_json::from_value::<Vec<crate::shell_probes::ToolImage>>(record["tools"].clone())?;
    aura_backend_windows_user::runtime_package::verify(root, &record)?;
    let id =
        record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installation ID missing"))?;
    for device in aura_windows_platform::ExecutionDevice::EXECUTION {
        let capability = &record[format!("{}_capability", device.name())];
        ensure!(
            capability["sid"] == device.sid(id)?
                && capability["installed"] == true
                && device.inspect(id)?,
            "{} capability requires reviewed Setup/Repair",
            device.name()
        );
    }
    let namespace = &record["namespace_capability"];
    ensure!(
        namespace["sid"] == aura_windows_platform::execution_namespace_sid(id)?
            && namespace["offline_sid"] == sid
            && namespace["scope"]["session"]
                == aura_windows_platform::current_windows_session_id()?
            && aura_windows_platform::execution_namespace_access()["required"]["opened"] == true,
        "namespace capability absent or session changed; RepairRequired"
    );
    run(Some(&root.join("scratch")), true, &tools, Some(id))
}

fn run(
    base: Option<&Path>,
    dedicated: bool,
    tools: &[crate::shell_probes::ToolImage],
    device_install_id: Option<&str>,
) -> Result<Value> {
    let root = if let Some(base) = base {
        tempfile::Builder::new().prefix("r06-attempt-").tempdir_in(base)?
    } else {
        tempfile::Builder::new().prefix("r06-mechanism-").tempdir()?
    };
    fs::write(root.path().join("r06-owned-fixture"), b"R06 disposable test only")?;
    let allowed = root.path().join("allowed");
    let denied = root.path().join("denied");
    fs::create_dir(&allowed)?;
    fs::create_dir(&denied)?;
    fs::write(denied.join("read.txt"), b"read compatibility")?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(anyhow::Error::msg)?;
    let ids = random
        .chunks_exact(4)
        .map(|v| u32::from_le_bytes(v.try_into().expect("four bytes")).max(1).to_string())
        .collect::<Vec<_>>();
    let sid = format!("S-1-5-21-{}", ids.join("-"));
    let executable = std::env::current_exe()?;
    let image = hash(&fs::read(&executable)?);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let control = crate::replay::AttemptControl {
        operation: "r06.fixed-probe".into(),
        attempt: 1,
        nonce: hash(&random),
        executable_sha256: image.clone(),
        expires_unix_ms: now + 30_000,
        cancellation_revision: 0,
    };
    let mut claims = crate::replay::Claims::default();
    claims.claim(&control, now, &image, 0)?;
    let before = capture_file_dacl(&allowed)?;
    let identity = inspect_existing_path(&allowed, false)?.identity;
    fs::write(
        root.path().join("acl-journal.json"),
        serde_json::to_vec(&json!({"path":allowed,"sid":sid,"before":before,"pending":"grant"}))?,
    )?;
    let applied = match grant_empty_execution_write_root(&allowed, &sid)
        .and_then(|()| capture_file_dacl(&allowed))
    {
        Ok(applied) => applied,
        Err(error) => {
            bail!("ACL grant interrupted: {error}; reconcile journal at {}", root.keep().display())
        }
    };
    // Local attempt journal is not a database, and contains no secrets.
    let journal = fs::write(
        root.path().join("acl-journal.json"),
        serde_json::to_vec(&json!({"path":allowed,"sid":sid,"before":before,"applied":applied}))?,
    );
    if let Err(error) = journal {
        bail!("ACL journal interrupted: {error}; reconcile root {}", root.keep().display());
    }
    let observed = (|| -> Result<Value> {
        // Add the alias after granting an EMPTY owned root. Propagating ACLs
        // through pre-existing hard links would change their outside target.
        fs::hard_link(denied.join("read.txt"), allowed.join("escape-link.txt"))?;
        let mut process = crate::monitor::launch(
            &sid,
            &executable,
            &args("access", root.path()),
            root.path(),
            device_install_id,
            None,
        )?;
        process.write_stdin(b"R06_STDIN_CANARY")?;
        let access = crate::monitor::collect(&mut process, false, 16_384)?;
        let payload = &access["payload"];
        let null_passed = device_install_id.is_none()
            || ["read", "write", "read_write"]
                .iter()
                .all(|key| payload["null_access"][key]["opened"] == true);
        let access_passed = null_passed
            && [
                "write",
                "rename",
                "delete",
                "read",
                "outside_denied",
                "hardlink_denied",
                "dacl_denied",
                "restricted",
                "stdin_verified",
                "secret_verified",
            ]
            .iter()
            .all(|key| payload[key] == true)
            && ["restricted", "allowed", "outside_denied"]
                .iter()
                .all(|key| payload["child"][key] == true);
        if !access_passed {
            return Ok(
                json!({"access":access,"passed":false,"remaining":"not_run_after_access_failure"}),
            );
        }
        let mut tree = crate::monitor::launch(
            &sid,
            &executable,
            &args("tree", root.path()),
            root.path(),
            device_install_id,
            None,
        )?;
        let cancellation = crate::monitor::collect(&mut tree, true, 16_384)?;
        let mut flood = crate::monitor::launch(
            &sid,
            &executable,
            &args("flood", root.path()),
            root.path(),
            device_install_id,
            None,
        )?;
        let output = crate::monitor::collect(&mut flood, false, 16_384)?;
        let passed =
            cancellation["termination"] == "cancelled" && output["termination"] == "output_limit";
        let runtime_startup = if !tools.is_empty() {
            let mut runtime = crate::monitor::launch(
                &sid,
                &executable,
                &args("runtime", root.path()),
                root.path(),
                device_install_id,
                None,
            )?;
            crate::monitor::collect(&mut runtime, false, 16_384)?
        } else {
            json!({"status":"not_requested"})
        };
        let shell_execution = if !tools.is_empty() {
            crate::shell_probes::run(tools, &sid, root.path(), device_install_id)
        } else {
            json!({"status":"not_run_requires_dedicated_identity"})
        };
        let passed = passed
            && (device_install_id.is_none()
                || crate::compatibility::ksec_boundary_passed(&runtime_startup["payload"]));
        let passed = passed
            && (!dedicated
                || crate::compatibility::core_namespace_boundary_passed(
                    &runtime_startup["payload"],
                ));
        let local_named_ipc =
            crate::compatibility::namespace_boundary_passed(&runtime_startup["payload"]);
        let network = if dedicated {
            crate::network::run(&sid, &executable, root.path(), device_install_id)?
        } else {
            json!({"status":"not_run_requires_dedicated_identity"})
        };
        Ok(
            json!({"access":access,"cancellation":cancellation,"output":output,"runtime_startup":runtime_startup,"optional_capabilities":{"local_named_ipc":local_named_ipc},"shell_execution":shell_execution,"network":network,"passed":passed}),
        )
    })();
    let cleanup = (|| -> Result<bool> {
        Ok(
            if inspect_existing_path(&allowed, false)?.identity == identity
                && capture_file_dacl(&allowed)? == applied
            {
                restore_file_dacl(&allowed, &before)?;
                restored_dacl_matches(&before, &capture_file_dacl(&allowed)?)
            } else {
                false
            },
        )
    })();
    let cleanup = match cleanup {
        Ok(value) => value,
        Err(error) => {
            bail!("ACL restoration failed: {error}; reconcile journal at {}", root.keep().display())
        }
    };
    let observed = observed.unwrap_or_else(|e| json!({"passed":false,"error":e.to_string()}));
    let mut result = json!({"schema":"r06-spike-v0","dedicated_offline_identity":dedicated,"mechanism_passed":observed["passed"]==true&&cleanup,"acl_restored":cleanup,"observed":observed,"parent_compatibility":crate::compatibility::parent(),"runner_sha256":image,"network_enforcement":"not_verified","home_pro_matrix":"not_verified","r06_complete":false});
    if !cleanup {
        result["recovery"] = json!({"reason":"temporary_acl_drift","preserved_root":root.keep()});
    }
    Ok(result)
}

// R06's own scope, deliberately **not** the W7 drive-root exception: this probe
// restores a temporary directory through the named API, which may re-establish
// `SE_DACL_AUTO_INHERITED` *while* restoring the exact ACEs. The accepted transitions
// here are therefore "identical" or "identical ACEs, marker added", and the ACE
// sequence is compared in file order because order decides access. Sharing a helper
// with the W7 rule would widen that rule's approval scope, so this stays local.
//
// The pre-restoration comparison against `applied` above remains byte-for-byte, and any
// other difference — a reordered ACE, a protection change, a changed mask — is a real
// difference and not "restored".
pub(crate) fn restored_dacl_matches(before: &str, after: &str) -> bool {
    if before == after {
        return true;
    }
    if !aura_windows_platform::ordered_ace_sequences_match(before, after).unwrap_or(false) {
        return false;
    }
    let control = |value: &str| {
        value
            .strip_prefix("D:")
            .map(|body| {
                let end = body.find('(').unwrap_or(body.len());
                body[..end].to_owned()
            })
            .ok_or(())
    };
    let (Ok(before_control), Ok(after_control)) = (control(before), control(after)) else {
        return false;
    };
    if before_control.contains("AR") || after_control.contains("AR") {
        return false;
    }
    // Only the marker may appear; every other control letter has to be identical.
    before_control == after_control
        || (before_control == after_control.replace("AI", "")
            && !before_control.contains("AI")
            && after_control.contains("AI"))
}

fn args(kind: &str, root: &Path) -> Vec<String> {
    vec!["payload".into(), kind.into(), root.display().to_string()]
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod restoration_tests {
    use super::restored_dacl_matches;

    #[test]
    fn only_os_auto_inherited_metadata_may_change_after_restoration() {
        let acl = "(A;OICIID;FA;;;SY)(A;OICIID;0x1200a9;;;BA)";
        assert!(restored_dacl_matches(&format!("D:{acl}"), &format!("D:AI{acl}")));
        assert!(restored_dacl_matches(&format!("D:P{acl}"), &format!("D:PAI{acl}")));
        assert!(!restored_dacl_matches(&format!("D:{acl}"), &format!("D:PAI{acl}")));
        assert!(!restored_dacl_matches(&format!("D:P{acl}"), &format!("D:AI{acl}")));
        assert!(!restored_dacl_matches(&format!("D:AI{acl}"), &format!("D:{acl}")));
        assert!(!restored_dacl_matches(
            &format!("D:{acl}"),
            &format!("D:AI{}", acl.replace("0x1200a9", "FA"))
        ));
        assert!(!restored_dacl_matches(&format!("D:{acl}"), &format!("D:AI{acl}(A;;FA;;;WD)")));
    }
}
