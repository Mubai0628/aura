//! Owned installation management only. No user command execution, DNS probes,
//! RPC experiments, process payloads, or test acceptance shortcuts.
//! UNWIRED: not in the release layout (scripts/release/layout.mjs); only the development setup
//! builds it.
use anyhow::{Result, bail};
use std::path::Path;
mod acceptance;
#[cfg(test)]
mod acceptance_tests;
mod maintenance;
mod manifest;
mod online;
mod resources;

fn main() -> Result<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let value = match arguments.as_slice() {
        [verb, root] if matches!(verb.as_str(), "online-read-apply" | "online-read-restore") => {
            online::access(Path::new(root), verb == "online-read-restore")?
        }
        [verb] if verb == "maintenance-paths" => {
            serde_json::json!({"service":aura_windows_platform::execution_maintenance::SERVICE_NAME,"program":aura_windows_platform::execution_maintenance::program_directory()?,"data":aura_windows_platform::execution_maintenance::data_directory()?})
        }
        [verb, root] if verb == "maintenance-config" => {
            serde_json::to_value(maintenance::config(Path::new(root))?)?
        }
        [verb, root] if verb == "maintenance-state" => maintenance::state(Path::new(root))?,
        [verb] if verb == "maintenance-remove" => {
            serde_json::to_value(aura_execution_maintainer::remove_installed()?)?
        }
        [verb, root, binding] if verb == "migrate-pre-generation-receipt" => {
            maintenance::migrate_pre_generation(Path::new(root), Path::new(binding))?
        }
        [verb, path] if verb == "maintenance-verify-path" => {
            aura_windows_platform::execution_maintenance::verify_protected_path(Path::new(path))?;
            serde_json::json!({"protected":true})
        }
        [verb, expected_inventory_sha256, package, home, pro] if verb == "aggregate" => {
            acceptance::aggregate(
                expected_inventory_sha256,
                Path::new(package),
                Path::new(home),
                Path::new(pro),
            )?
        }
        [verb, expected_inventory_sha256, package, home, pro, lineage] if verb == "aggregate" => {
            acceptance::aggregate_with_lineage(
                expected_inventory_sha256,
                Path::new(package),
                Path::new(home),
                Path::new(pro),
                Some(Path::new(lineage)),
            )?
        }
        [verb, path] if verb == "fingerprint" => {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            serde_json::to_value(aura_domain_common::jcs::canonical_sha256(&value)?)?
        }
        [verb, path] if verb == "bind-file" => serde_json::to_value(
            aura_backend_windows_user::file_binding::capture(Path::new(path), true)?.0,
        )?,
        [verb, root, core] if verb == "manifest" => {
            serde_json::to_value(manifest::build(Path::new(root), Path::new(core))?)?
        }
        [verb, root] if verb == "inspect" => {
            let installed =
                aura_backend_windows_user::installation::Installation::open(Path::new(root))?;
            serde_json::json!({"schema":"windows-user-installation-inspection-v1","manifest":installed.manifest,
                "fingerprint":installed.fingerprint,"r06_complete":false,"scope":"resources and file identities only"})
        }
        [verb, root] if verb == "system-cmd-manifest" => serde_json::to_value(
            aura_backend_windows_user::installation::Installation::recapture_system_cmd(
                Path::new(root),
            )?,
        )?,
        [verb, root] if verb == "system-cmd-conversion" => serde_json::to_value(
            aura_backend_windows_user::installation::Installation::prepare_system_cmd(Path::new(
                root,
            ))?,
        )?,
        [verb, root, expected] if verb == "apply-system-cmd" => serde_json::to_value(
            aura_backend_windows_user::installation::Installation::apply_system_cmd(
                Path::new(root),
                expected,
            )?,
        )?,
        [verb, root, conversion] if verb == "system-cmd-verify" => {
            let bytes = std::fs::read(conversion)?;
            anyhow::ensure!(bytes.len() <= 1024 * 1024, "runner.system_cmd_verify_size");
            let recorded: aura_windows_user_contract::SystemCmdConversion =
                serde_json::from_slice(&bytes)?;
            serde_json::to_value(
                aura_backend_windows_user::installation::Installation::verify_recorded_system_cmd_conversion(
                    Path::new(root),
                    recorded,
                )?,
            )?
        }
        [verb, root] if verb == "repair-leases" => manifest::repair_leases(Path::new(root))?,
        [verb, id] if matches!(verb.as_str(), "null-plan" | "ksec-plan" | "cng-plan") => {
            resources::device_plan(verb.trim_end_matches("-plan"), id)?
        }
        [verb, operation, root] if matches!(verb.as_str(), "null" | "ksec" | "cng") => {
            resources::device(verb, operation, Path::new(root))?
        }
        [verb, id, sid, policy] if verb == "wfp-plan" => resources::network_plan(id, sid, policy)?,
        [verb, operation, root] if verb == "wfp" => resources::network(operation, Path::new(root))?,
        _ => bail!(
            "use aggregate <expected-inventory-sha256> <package.json> <home-report.json> \
             <pro-report.json> [lineage.json] | bind-file | manifest | inspect | system-cmd-manifest | \
             system-cmd-conversion | apply-system-cmd | system-cmd-verify | \
             migrate-pre-generation-receipt | repair-leases | reviewed \
             installation resources"
        ),
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
