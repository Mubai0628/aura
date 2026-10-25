use std::path::Path;

use anyhow::Result;

pub(crate) fn check(root: &Path) -> Result<()> {
    super::run(root, "node", &["scripts/architecture/check-file-size.mjs"])?;
    super::run(
        root,
        "node",
        &[
            "--test",
            "--test-isolation=none",
            "scripts/architecture/graph.test.mjs",
            "scripts/architecture/file-size.test.mjs",
            "scripts/architecture/renderer.test.mjs",
            "scripts/architecture/contract-baseline.test.mjs",
            "scripts/architecture/contract-invariants.test.mjs",
        ],
    )?;
    super::run(root, "node", &["scripts/architecture/contract-baseline.mjs"])?;
    super::run(root, "node", &["scripts/architecture/check.mjs"])?;
    super::run(root, super::corepack_command(), &["pnpm", "lint"])
}

pub(crate) fn check_fast(root: &Path) -> Result<()> {
    check(root)?;
    super::run(root, "cargo", &["fmt", "--all", "--", "--check"])?;
    super::contracts::check()?;
    super::manifests::validate_manifests(root)?;
    // Run the existing package scripts sequentially, without pnpm's recursive output pipes.
    for package in ["packages/contracts", "apps/desktop"] {
        super::run(&root.join(package), super::corepack_command(), &["pnpm", "typecheck"])?;
    }
    Ok(())
}
