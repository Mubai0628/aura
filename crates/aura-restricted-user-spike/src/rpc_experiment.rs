//! Historical plan and read-only inspection; broad LRPC execution is retired.
use anyhow::{Result, bail};
use aura_windows_platform::{inspect_spike_rpc_guard, spike_rpc_guard_plan};
use std::path::Path;

pub fn run(action: &str, root: &Path) -> Result<()> {
    // Reject before loading any host state, even if an old command is reused.
    if action == "run" {
        bail!("Broad LRPC experiment retired; D0/D1 evidence required before a narrow candidate");
    }
    let spec = crate::wfp::load_spec(root)?;
    match action {
        "plan" => println!("{}", serde_json::to_string_pretty(&spike_rpc_guard_plan(&spec)?)?),
        "inspect" => {
            println!("{}", serde_json::to_string_pretty(&inspect_spike_rpc_guard(&spec)?)?)
        }
        _ => bail!("retired RPC experiment supports historical plan and read-only inspect only"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn retired_run_rejects_before_host_access() {
        let error = super::run("run", std::path::Path::new("nonexistent-retired-fixture"))
            .expect_err("retired execution must be rejected");
        assert!(error.to_string().contains("Broad LRPC experiment retired"));
    }
}
