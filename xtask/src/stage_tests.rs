//! The stage test commands (`test-permission`, `test-sandbox`, `test-network`, `test-tool`). Each
//! runs its selection from the one Rust test build, so none of them builds a copy of its own.
use std::path::Path;

use anyhow::{Result, ensure};

use crate::rust_tests::Session;

pub(crate) fn permission(root: &Path) -> Result<()> {
    let mut session = Session::start(root)?;
    session.run(&[
        "-p",
        "aura-permission-domain",
        "-p",
        "aura-permission-engine",
        "-p",
        "aura-approval",
    ])?;
    session.run(&["-p", "aura-persistence", "--tests", "--", "p04_security::"])?;
    session.run(&["-p", "aura-core", "-p", "aura-security-service", "--", "p04"])?;
    session.finish()?;
    println!("P04 permission and approval tests passed.");
    Ok(())
}

pub(crate) fn sandbox(root: &Path) -> Result<()> {
    let mut session = Session::start(root)?;
    session.run(&[
        "-p",
        "aura-sandbox",
        "-p",
        "aura-sandbox-host",
        "-p",
        "aura-elevated-helper",
        "-p",
        "aura-windows-platform",
    ])?;
    session.finish()?;
    println!(
        "P04 sandbox and elevation automated tests passed. Real AppContainer lowbox/loopback evidence is collected separately."
    );
    Ok(())
}

pub(crate) fn network(root: &Path) -> Result<()> {
    let mut session = Session::start(root)?;
    session.run(&["-p", "aura-network-control", "-p", "aura-egress-proxy"])?;
    session.finish()?;
    println!("P04 network control and egress proxy tests passed.");
    Ok(())
}

/// `test-tool <namespace-or-filter>`. The package program tests run the source-built fixtures
/// from `target/debug`; the test build makes them and every session checks for them.
pub(crate) fn tool(root: &Path, target: &str) -> Result<()> {
    ensure!(
        !target.is_empty() && !target.starts_with('-'),
        "test-tool target must be a namespace or Rust test filter"
    );
    let mut session = Session::start(root)?;
    match target {
        "conformance" => {
            session.run(&[
                "-p",
                "aura-tool-domain",
                "-p",
                "aura-tool-package",
                "-p",
                "aura-tool-fixtures",
            ])?;
            session.run(&[
                "-p",
                "aura-package-service",
                "--lib",
                "--",
                "package_program::tests",
            ])?;
        }
        // Packages are imported, verified and installed through `packages.*`.
        "package" => {
            session.run(&["-p", "aura-tool-package"])?;
            session.run(&[
                "-p",
                "aura-persistence-v2",
                "--tests",
                "--",
                "package_install::",
                "package_content::",
                "package_schema_migration::",
            ])?;
            session.run(&[
                "-p",
                "aura-package-service",
                "--lib",
                "--",
                "package_program::",
                "package_services::",
            ])?;
            session.run(&["-p", "aura-core", "--lib", "--", "l04_packages::"])?;
        }
        "registry" => {
            session.run(&[
                "-p",
                "aura-tool-contract",
                "-p",
                "aura-tool-domain",
                "-p",
                "aura-tool-registry",
                "-p",
                "aura-tool-projection",
            ])?;
            session.run(&["-p", "aura-core", "--lib", "--", "l04_package_registry"])?;
        }
        "scheduler" => {
            session.run(&["-p", "aura-tool-scheduler"])?;
            session.run(&["-p", "aura-persistence", "--tests", "--", "p07_tool_execution::"])?;
        }
        "runtime" => {
            session.run(&[
                "-p",
                "aura-tool-runtime",
                "-p",
                "aura-sandbox",
                "-p",
                "aura-sandbox-host",
                "-p",
                "aura-windows-platform",
            ])?;
            session.run(&["-p", "aura-core", "--lib", "--", "w7_host_adapter"])?;
            session.run(&[
                "-p",
                "aura-package-service",
                "--lib",
                "--",
                "package_program::tests",
            ])?;
        }
        filter => {
            session.run(&[
                "-p",
                "aura-tool-contract",
                "-p",
                "aura-artifact-contract",
                "-p",
                "aura-tool-domain",
                "-p",
                "aura-tool-registry",
                "-p",
                "aura-tool-scheduler",
                "-p",
                "aura-tool-runtime",
                "-p",
                "aura-tool-package",
                "-p",
                "aura-tool-projection",
                "-p",
                "aura-tool-fixtures",
                "--",
                filter,
            ])?;
        }
    }
    session.finish()?;
    println!("P07 Tool test target '{target}' passed.");
    Ok(())
}
