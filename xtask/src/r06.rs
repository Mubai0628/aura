use anyhow::{Result, ensure};
use std::path::Path;

// Engineering checks only: this command cannot close the host acceptance gate.
pub(crate) fn check(root: &Path) -> Result<()> {
    super::ensure_build_limits("check-r06-spike")?;
    ensure!(
        root.join("scripts/r06/check.ps1").is_file(),
        "check-r06-spike runs the host-preparation scripts, which only the development repository carries"
    );
    super::architecture::check(root)?;
    super::run(root, "pwsh", &["-NoProfile", "-File", "scripts/r06/check.ps1"])?;
    super::run(root, "cargo", &["test", "-p", "aura-restricted-user-spike", "--offline"])?;
    // Real stream/Job regressions of the shared launcher; never enable ignored
    // P04/AppContainer setup tests or turn a StandardUser pass into R06 proof.
    super::run(
        root,
        "cargo",
        &[
            "test",
            "-p",
            "aura-windows-platform",
            "--lib",
            "--offline",
            "--features",
            "restricted-user-spike",
            "--",
            "sandbox::restricted_user::",
            "wfp_spike::",
            "host_preparation::device_spike::",
            "sandbox::tests::restricted_process_",
            "sandbox::tests::terminating_execution_job_",
        ],
    )?;
    super::run(
        root,
        "cargo",
        &[
            "clippy",
            "-p",
            "aura-restricted-user-spike",
            "-p",
            "aura-windows-platform",
            "-p",
            "xtask",
            "--all-targets",
            "--offline",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    println!(
        "R06 Spike engineering checks passed. Home/Pro, dedicated identity and Offline enforcement remain separate and REQUIRED; R06 is not complete."
    );
    Ok(())
}

pub(crate) fn mechanism(root: &Path) -> Result<()> {
    ensure!(
        std::env::var("CARGO_BUILD_JOBS").as_deref() == Ok(super::BUILD_JOBS),
        "R06 requires CARGO_BUILD_JOBS={}",
        super::BUILD_JOBS
    );
    super::run(
        root,
        "cargo",
        &["run", "-p", "aura-restricted-user-spike", "--offline", "--", "mechanism"],
    )
}
