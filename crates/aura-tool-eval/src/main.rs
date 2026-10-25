use anyhow::{Result, bail, ensure};
use aura_tool_eval::{
    budget::{Ledger, Reconciliation},
    owner::{OwnerSource, OwnerTarget},
    runner::{RunConfig, run},
    snapshots::current_requests,
};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("verify-owner") => {
            read_target(args.get(1))?;
            println!("OwnerTarget::verify_current passed; no secret read or provider request.");
        }
        Some("inspect-owner") => {
            let source: OwnerSource = serde_json::from_slice(&std::fs::read(
                args.get(1).ok_or_else(|| anyhow::anyhow!("owner source metadata required"))?,
            )?)?;
            println!("{}", serde_json::to_string_pretty(&source.read()?)?);
        }
        Some("reconcile") => {
            let ledger_path =
                Path::new(args.get(1).ok_or_else(|| anyhow::anyhow!("ledger path required"))?);
            ensure!(
                args.get(2).map(String::as_str) == Some("--reconciliation-file"),
                "eval.reconciliation_file_required"
            );
            let note: Reconciliation = serde_json::from_slice(&std::fs::read(
                args.get(3).ok_or_else(|| anyhow::anyhow!("reconciliation file required"))?,
            )?)?;
            let ledger = Ledger::open_for_reconciliation(ledger_path)?;
            ledger.reconcile(&note)?;
            println!(
                "Reconciliation recorded; explicit recovery completed. Task stops and original limits are retained."
            );
        }
        Some("snapshots") => {
            let path = Path::new(
                args.get(1).ok_or_else(|| anyhow::anyhow!("snapshot destination required"))?,
            );
            std::fs::create_dir_all(path)?;
            for (name, body) in current_requests()? {
                std::fs::write(
                    path.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(&body)?,
                )?;
            }
        }
        Some("replay") | Some("real") => {
            let config: RunConfig = serde_json::from_slice(&std::fs::read(
                args.get(1).ok_or_else(|| anyhow::anyhow!("config required"))?,
            )?)?;
            let real = if args[0] == "real" {
                ensure!(
                    args.get(2).map(String::as_str) == Some("--owner-approved-estimate"),
                    "eval.owner_estimate_confirmation_required"
                );
                Some(read_target(args.get(3))?)
            } else {
                None
            };
            println!("{}", serde_json::to_string_pretty(&run(config, real).await?)?);
        }
        Some("probe") => {
            let root = Path::new(
                args.get(1).ok_or_else(|| anyhow::anyhow!("isolated report directory required"))?,
            );
            ensure!(
                args.get(2).map(String::as_str) == Some("--owner-approved-estimate"),
                "eval.owner_estimate_confirmation_required"
            );
            let target = read_target(args.get(3))?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &aura_tool_eval::probe::run_probe(root, target).await?
                )?
            );
        }
        _ => bail!(
            "aura-tool-eval snapshots <dir> | inspect-owner <source.json> | replay <config> | real <config> --owner-approved-estimate <target.json> | probe <isolated-dir> --owner-approved-estimate <target.json> | reconcile <ledger.db> --reconciliation-file <note.json>"
        ),
    }
    Ok(())
}

fn read_target(path: Option<&String>) -> Result<OwnerTarget> {
    let target: OwnerTarget = serde_json::from_slice(&std::fs::read(
        path.ok_or_else(|| anyhow::anyhow!("read-only owner target metadata required"))?,
    )?)?;
    target.verify_current()?;
    Ok(target)
}
