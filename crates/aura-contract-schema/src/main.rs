use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let mut root = env::current_dir().context("failed to resolve the current directory")?;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--root" => {
                root = PathBuf::from(args.next().context("--root requires a path")?);
            }
            "--help" | "-h" => {
                println!("Usage: aura-contract-schema [--root <workspace-root>]");
                return Ok(());
            }
            _ => bail!("unknown argument: {argument}"),
        }
    }

    for path in aura_contract_schema::write_rust_generated_artifacts(&root)? {
        println!("generated {}", path.display());
    }
    Ok(())
}
