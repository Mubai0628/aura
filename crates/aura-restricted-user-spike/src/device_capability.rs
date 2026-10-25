//! Fixed device helper; mutations consume the durable owned installation journal.
use anyhow::{Result, ensure};
use aura_windows_platform::ExecutionDevice;
use serde_json::{Value, json};
use std::{fs, path::Path};

pub fn plan(device: ExecutionDevice, id: &str) -> Result<Value> {
    let before = device.capture_dacl()?;
    let after = device.plan(id, &before, true)?;
    let removed = device.plan(id, &before, false)?;
    Ok(
        json!({"schema":"r06-device-v1","device":device.name(),"access_mask":device.access_mask(),"sid":device.sid(id)?,"ready":before==after,"before":before,"after":after,"removed":removed}),
    )
}

pub fn run(device: ExecutionDevice, operation: &str, root: &Path) -> Result<Value> {
    let record: Value = serde_json::from_slice(&fs::read(root.join("record.json"))?)?;
    let id =
        record["install_id"].as_str().ok_or_else(|| anyhow::anyhow!("installation ID missing"))?;
    let sid = device.sid(id)?;
    ensure!(
        root.canonicalize()?
            == Path::new(&std::env::var("ProgramData")?)
                .join("Aura/R06-Spike")
                .join(id)
                .canonicalize()?,
        "not an R06 root"
    );
    if operation == "inspect" {
        return plan(device, id);
    }
    let install = match operation {
        "install" if device != ExecutionDevice::Cng => true,
        "remove" => false,
        _ => anyhow::bail!("unknown or retired device operation"),
    };
    let pending = format!("{}-{}", operation, device.name());
    let state = if install { "upgrading" } else { "removing" };
    let change = &record[format!("{}_transition", device.name())];
    ensure!(
        (record["state"] == state
            || (matches!(device, ExecutionDevice::Cng | ExecutionDevice::Ksec)
                && !install
                && record["state"] == format!("removing-{}", device.name())))
            && record["pending"] == pending
            && change["operation"] == operation
            && change["sid"] == sid,
        "device operation is not journaled"
    );
    let before = change["before"].as_str().ok_or_else(|| anyhow::anyhow!("before missing"))?;
    let after = change["after"].as_str().ok_or_else(|| anyhow::anyhow!("after missing"))?;
    device.apply(id, before, after, install)?;
    plan(device, id)
}
