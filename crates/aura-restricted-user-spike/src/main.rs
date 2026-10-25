//! Disposable mechanism prototype. No Core route, SQL, formal V2 contract
//! or production Backend registration. Host acceptance remains a separate gate.
use anyhow::{Result, bail, ensure};
use std::{env, path::PathBuf};

mod compatibility;
mod device_capability;
mod egress_probe;

mod fixture_probe;
mod host_evidence;

mod monitor;
mod network;
mod network_experiment;
mod payload;
mod probe;
mod replay;
mod rpc_experiment;
mod shell_boundaries;
mod shell_probes;
mod shell_scenarios;
mod wfp;

fn main() -> Result<()> {
    aura_windows_platform::suppress_execution_error_dialogs();
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [mode, root, image, digest, run, canary] if mode == "egress-fixture" => {
            println!(
                "{}",
                serde_json::to_string(&fixture_probe::run(
                    &PathBuf::from(root),
                    &PathBuf::from(image),
                    digest,
                    run,
                    canary
                )?)?
            );
        }
        [mode, root, scenario, peer, canary] if mode == "egress-probe" => {
            println!(
                "{}",
                serde_json::to_string(&egress_probe::run(
                    &PathBuf::from(root),
                    scenario,
                    peer,
                    canary
                )?)?
            );
        }
        [mode, action, root] if mode == "rpc-experiment" => {
            rpc_experiment::run(action, &PathBuf::from(root))?;
        }
        [mode, root, peer, run_id] if mode == "dns-experiment" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&network_experiment::run(
                    &PathBuf::from(root),
                    peer,
                    run_id
                )?)?
            );
        }
        [mode, review] if mode == "diagnostics" => {
            let report = probe::diagnostics(&PathBuf::from(review))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            ensure!(report["mechanism_passed"] == true, "diagnostic mechanism failed");
        }
        [mode, id] if mode == "null-plan" || mode == "cng-plan" || mode == "ksec-plan" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&device_capability::plan(
                    if mode == "null-plan" {
                        aura_windows_platform::ExecutionDevice::Null
                    } else if mode == "cng-plan" {
                        aura_windows_platform::ExecutionDevice::Cng
                    } else {
                        aura_windows_platform::ExecutionDevice::Ksec
                    },
                    id
                )?)?
            )
        }
        [mode, operation, root] if mode == "null" || mode == "cng" || mode == "ksec" => println!(
            "{}",
            serde_json::to_string_pretty(&device_capability::run(
                if mode == "null" {
                    aura_windows_platform::ExecutionDevice::Null
                } else if mode == "cng" {
                    aura_windows_platform::ExecutionDevice::Cng
                } else {
                    aura_windows_platform::ExecutionDevice::Ksec
                },
                operation,
                &PathBuf::from(root)
            )?)?
        ),
        [mode, id, sid, policy] if mode == "wfp-plan" => wfp::plan(id, sid, policy)?,
        [mode, operation, root] if mode == "wfp" => wfp::run(operation, &PathBuf::from(root))?,
        [mode] if mode == "host-evidence" => {
            // Read-only: probes the host and prints the delta. Never applies it.
            println!("{}", serde_json::to_string_pretty(&host_evidence::report())?);
        }
        [mode] if mode == "mechanism" => {
            let report = probe::mechanism()?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            ensure!(report["mechanism_passed"] == true, "R06 mechanism not passed");
        }
        [mode, kind, root] if mode == "payload" => payload::run(kind, &PathBuf::from(root))?,
        [mode, root] if mode == "host" => {
            let report = probe::host(&PathBuf::from(root))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            ensure!(report["mechanism_passed"] == true, "R06 host mechanism not passed");
        }
        _ => bail!("use mechanism | host <prepared-root>; payload is internal fixed diagnostics"),
    }
    Ok(())
}
