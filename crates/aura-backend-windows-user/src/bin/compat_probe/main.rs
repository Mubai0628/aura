//! Local, line-oriented adoption harness. No IPC route or product selection exposes it.
mod requests;
use anyhow::{Result, ensure};
use aura_backend_windows_user::compat::{CompatBackend, CompatJobs};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead, Write},
    path::PathBuf,
};

fn write_result(value: &Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value)?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let mut lines = std::io::stdin().lock().lines();
    let first =
        lines.next().ok_or_else(|| anyhow::anyhow!("local harness configuration required"))??;
    let config: Value = serde_json::from_str(&first)?;
    let payload = PathBuf::from(
        config["payload"].as_str().ok_or_else(|| anyhow::anyhow!("managed payload required"))?,
    );
    let manifest = PathBuf::from(
        config["manifest"].as_str().ok_or_else(|| anyhow::anyhow!("managed manifest required"))?,
    );
    // Windows' hidden `=C:`-style entries of a process started from a shell are not variables.
    let mut trusted = std::env::vars_os()
        .filter(|(name, _)| !name.to_string_lossy().starts_with('='))
        .map(|(name, value)| (name.to_string_lossy().to_ascii_uppercase(), value))
        .collect::<BTreeMap<_, _>>();
    if let Some(overrides) = config["environment_overrides"].as_object() {
        for (name, value) in overrides {
            ensure!(!name.contains(['=', '\0']), "invalid trusted fixture environment name");
            trusted.insert(
                name.to_ascii_uppercase(),
                OsString::from(
                    value
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("fixture environment value required"))?,
                ),
            );
        }
    }
    let backend = CompatBackend::with_environment(
        &payload,
        &manifest,
        trusted.into_iter().map(|(name, value)| (OsString::from(name), value)).collect(),
    )?;
    write_result(
        &json!({"ready":true,"image_verification_ms":backend.image_verification().as_secs_f64()*1000.0,"powershell":backend.powershell_path(),"cmd":backend.cmd_path(),"removed_environment_names":backend.removed_environment_names()}),
    )?;
    let mut jobs = CompatJobs::default();
    for line in lines {
        let request: Value = serde_json::from_str(&line?)?;
        if request["action"] == "exit" {
            break;
        }
        let result = match requests::handle(&backend, &mut jobs, &request) {
            Ok(result) => result,
            Err(error) => requests::failure(&error),
        };
        write_result(&result)?;
    }
    Ok(())
}
