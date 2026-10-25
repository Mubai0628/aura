//! Generated-contract determinism and the handwritten desktop projection boundary.
use std::fs;

use anyhow::{Context, Result, ensure};
use aura_contract_schema::RUST_GENERATED_PATHS;

use super::{
    TYPESCRIPT_CONTRACT_PATH, TYPESCRIPT_READINESS_VALIDATOR_PATH, TYPESCRIPT_ROUTES_PATH,
    corepack_command, generate_contracts, run, workspace_root,
};

pub(crate) fn check() -> Result<()> {
    let root = workspace_root();
    let temporary = tempfile::tempdir().context("failed to create contract check directory")?;
    generate_contracts(temporary.path())?;

    let paths = RUST_GENERATED_PATHS.iter().copied().chain([
        TYPESCRIPT_CONTRACT_PATH,
        TYPESCRIPT_ROUTES_PATH,
        TYPESCRIPT_READINESS_VALIDATOR_PATH,
        "apps/desktop/shared/generated/agent-validator.ts",
        "apps/desktop/shared/generated/git-validator.ts",
        "apps/desktop/shared/generated/package-validator.ts",
        "apps/desktop/shared/generated/memory-validator.ts",
        "apps/desktop/shared/generated/recovery-validator.ts",
        "apps/desktop/shared/generated/tool-credential-validator.ts",
    ]);
    let mut changed = Vec::new();

    for relative in paths {
        let checked_in = root.join(relative);
        let regenerated = temporary.path().join(relative);
        let expected = fs::read(&checked_in)
            .with_context(|| format!("missing generated file {}", checked_in.display()))?;
        let actual = fs::read(&regenerated).with_context(|| {
            format!("failed to read regenerated file {}", regenerated.display())
        })?;
        if expected != actual {
            changed.push(relative);
        }
    }

    ensure!(
        changed.is_empty(),
        "generated contracts are stale: {}. Run `cargo xtask generate-contracts`.",
        changed.join(", ")
    );
    println!("contract generation is deterministic and up to date.");
    // Full Rust samples must cross the handwritten desktop boundary as well.
    run(
        &root,
        corepack_command(),
        &["pnpm", "--filter", "@aura/desktop", "test:unit", "shared/projection-fixtures.test.ts"],
    )?;
    Ok(())
}
