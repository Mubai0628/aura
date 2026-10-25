//! Helper-only entry: scripts bind these values to the owned installed account.
use anyhow::{Result, ensure};
use aura_windows_platform::{AccountNetworkPolicy, AccountNetworkSpec, manage_account_network};
use serde_json::Value;
use std::path::Path;

pub fn plan(id: &str, sid: &str, policy: &str) -> Result<()> {
    let policy = match policy {
        "block_all" => AccountNetworkPolicy::BlockAll,
        "allow_loopback" => AccountNetworkPolicy::AllowLoopback,
        _ => anyhow::bail!("unknown network policy"),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&manage_account_network(
            &AccountNetworkSpec {
                install_id: id.into(),
                offline_sid: sid.into(),
                policy,
                expected_sublayer_weight: None
            },
            "plan"
        )?)?
    );
    Ok(())
}

pub fn run(operation: &str, root: &Path) -> Result<()> {
    let report = manage_account_network(&load_spec(root)?, operation)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub(crate) fn load_spec(root: &Path) -> Result<AccountNetworkSpec> {
    let base = std::env::var("ProgramData")?;
    let record: Value = serde_json::from_slice(&std::fs::read(root.join("record.json"))?)?;
    let id = record["install_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("installation UUID missing"))?;
    ensure!(
        root.canonicalize()? == Path::new(&base).join("Aura/R06-Spike").join(id).canonicalize()?,
        "not an R06 resource root"
    );
    let sid =
        record["offline_sid"].as_str().ok_or_else(|| anyhow::anyhow!("Offline SID missing"))?;
    let policy = match record["wfp_policy"].as_str() {
        Some("block_all") => AccountNetworkPolicy::BlockAll,
        Some("allow_loopback") => AccountNetworkPolicy::AllowLoopback,
        _ => anyhow::bail!("reviewed WFP policy missing"),
    };
    let expected_sublayer_weight =
        record["wfp"]["sublayer_weight"].as_u64().map(u16::try_from).transpose()?;
    Ok(AccountNetworkSpec {
        install_id: id.into(),
        offline_sid: sid.into(),
        policy,
        expected_sublayer_weight,
    })
}
