//! `cargo xtask sweep-target [--apply] [--idle-days <n> | --idle-hours <n>] [--unused]`: build
//! outputs cargo will not use again.
//!
//! Cargo deletes no output it no longer needs. A unit built with other features, another profile
//! or older sources stays beside the current one, and rustc keeps an incremental folder for every
//! unit it compiled. This lists, and with `--apply` deletes:
//! - units whose recorded sources are missing or newer than their outputs (cargo rebuilds those
//!   before using them, so deleting them costs no build time);
//! - incremental sessions older than the newest one of the same unit;
//! - incremental folders and test programs not written within the idle period (default 3 days): a
//!   test program is relinked when it is needed again, and a unit without its incremental folder
//!   is compiled once in full;
//! - with `--unused`, units that neither the last test build nor the last Clippy run (nor xtask's
//!   own build) used, last written before the earlier of those two runs. After a toolchain or
//!   profile change every unit of the old configuration is such a unit. The development and E2E
//!   builds are configurations of their own, so their units from before those runs are listed too
//!   and rebuilt when next needed.
//!
//! Programs cargo also keeps under `target/debug` itself (the ones `pnpm dev` and the E2E tests
//! start) are never listed. `--apply` refuses while another cargo or rustc runs.
mod dependency_info;
mod incremental;
mod outputs;
mod processes;

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail, ensure};

const USAGE: &str =
    "usage: cargo xtask sweep-target [--apply] [--idle-days <n> | --idle-hours <n>] [--unused]";

pub(crate) fn run(root: &Path, arguments: Vec<String>) -> Result<()> {
    let mut apply = false;
    let mut unused = false;
    let mut idle_hours = 3 * 24u64;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--apply" => apply = true,
            "--unused" => unused = true,
            "--idle-days" | "--idle-hours" => {
                let count: u64 = arguments
                    .next()
                    .context(USAGE)?
                    .parse()
                    .with_context(|| format!("{argument} needs a whole number\n{USAGE}"))?;
                idle_hours = if argument == "--idle-days" { count * 24 } else { count };
            }
            other => bail!("unknown argument {other}\n{USAGE}"),
        }
    }
    let debug = crate::rust_tests::target_folder(root).join("debug");
    ensure!(debug.is_dir(), "{} does not exist", debug.display());
    if apply {
        let others = processes::other_builds()?;
        ensure!(
            others.is_empty(),
            "cargo or rustc is running ({}); sweep after it ends",
            others.join(", ")
        );
    }
    let idle_before = SystemTime::now() - Duration::from_secs(idle_hours * 60 * 60);
    let mut plan = Vec::new();
    plan.extend(outputs::stale_units(root, &debug)?);
    plan.extend(incremental::superseded_and_idle(&debug, idle_before)?);
    plan.extend(outputs::idle_test_programs(&debug, idle_before)?);
    if unused {
        // xtask's own build selects other features than the test build; it was just built, so
        // cargo only reports its units.
        let xtask = crate::rust_tests::cargo_messages(root, &["build", "-p", "xtask"])?;
        let used = crate::rust_tests::used_units(root, &xtask).context(
            "no record of the units the test build and Clippy use yet; run `cargo xtask test` \
             and `cargo xtask clippy` first",
        )?;
        plan.extend(outputs::unused_units(&debug, used.recorded, &used.units)?);
    }
    plan.sort_by(|left, right| left.path.cmp(&right.path));
    plan.dedup_by(|left, right| left.path == right.path);
    report(root, &plan, apply)
}

/// One file or folder to delete and why.
#[derive(Debug)]
pub(super) struct Removal {
    pub(super) path: PathBuf,
    pub(super) bytes: u64,
    pub(super) reason: &'static str,
}

fn report(root: &Path, plan: &[Removal], apply: bool) -> Result<()> {
    let mut reasons: Vec<&'static str> = plan.iter().map(|removal| removal.reason).collect();
    reasons.sort_unstable();
    reasons.dedup();
    for reason in &reasons {
        let items: Vec<&Removal> =
            plan.iter().filter(|removal| removal.reason == *reason).collect();
        let bytes: u64 = items.iter().map(|removal| removal.bytes).sum();
        println!("{reason}: {} items, {:.1} GB", items.len(), gb(bytes));
    }
    let total: u64 = plan.iter().map(|removal| removal.bytes).sum();
    println!("total: {} items, {:.1} GB", plan.len(), gb(total));
    if !apply {
        println!("nothing deleted; `cargo xtask sweep-target --apply` deletes these");
        return Ok(());
    }
    let before = crate::rust_tests::free_gb(root)?;
    let mut failed = 0usize;
    for removal in plan {
        let result = if removal.path.is_dir() {
            std::fs::remove_dir_all(&removal.path)
        } else {
            std::fs::remove_file(&removal.path)
        };
        if let Err(error) = result {
            failed += 1;
            eprintln!("not deleted: {}: {error}", removal.path.display());
        }
    }
    let after = crate::rust_tests::free_gb(root)?;
    println!(
        "deleted {} of {} items; build drive free {before:.1} -> {after:.1} GB",
        plan.len() - failed,
        plan.len()
    );
    Ok(())
}

fn gb(bytes: u64) -> f64 {
    bytes as f64 / (1u64 << 30) as f64
}
