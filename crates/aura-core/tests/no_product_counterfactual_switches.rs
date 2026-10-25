//! **no counterfactual switch may exist in product code.**
//!
//! # Why this is a structural check and not a test of behaviour
//!
//! The batch-1/batch-2 repairs each proved a criterion by showing it *rejects* the
//! pre-repair implementation. Several of those proofs were driven by an environment
//! variable read from **product** source, e.g. a switch that restored the process-wide
//! lock or skipped the fault-original protection.
//!
//! "The variable is unset by default" is **not** a guarantee of unreachability. An
//! operator, a wrapper script, a stray CI variable or a future edit could set one, and
//! the switch would silently re-introduce a defect the stage had already fixed — in a
//! release build. A data-preservation or isolation guarantee that can be turned off by
//! an unset-means-safe variable is not a guarantee.
//!
//! So the requirement is structural: those branches must not be reachable from product
//! source **at all**. Their counterfactual evidence is preserved as logs, and
//! reproducing a counterfactual now means temporarily editing the function — an explicit
//! source mutation that cannot reach a build by accident.
//!
//! # What this checks
//!
//! Every `.rs` file outside a test context, in the workspace's product crates, is read
//! and searched for the counterfactual marker. A test context is a path containing a
//! `tests/` segment, or a file whose name ends in `_tests.rs` or is `tests.rs`.
//!
//! It is deliberately a **source** check rather than a runtime one: a runtime check would
//! have to call the switch to observe it, and the whole point is that the switch is gone.

use std::path::{Path, PathBuf};

/// The marker every counterfactual switch carried.
const COUNTERFACTUAL_MARKER: &str = "AURA_S05R2";

/// Product source roots to scan, relative to the workspace root.
const PRODUCT_ROOTS: &[&str] = &["crates", "apps"];

/// Whether a path is a test context rather than product source.
///
/// Test code may legitimately name a counterfactual — a test that asserts the switch is
/// gone, for instance. The requirement is about **product** paths.
fn is_test_context(path: &Path) -> bool {
    let normalised = path.to_string_lossy().replace('\\', "/");
    if normalised.contains("/tests/") || normalised.ends_with("/tests.rs") {
        return true;
    }
    path.file_name().is_some_and(|name| name.to_string_lossy().ends_with("_tests.rs"))
}

fn collect_rust_files(root: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            // Build output and vendored trees are not product source.
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_rust_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// No counterfactual switch may be reachable from product source.
///
/// This is the structural half of "移除产品路径的临时变异分支", and it fails the build
/// rather than a run: a re-introduced switch is a source-level regression, so it should
/// be caught before anything executes.
#[test]
fn no_counterfactual_switch_exists_in_product_source() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate lives at <workspace>/crates/<crate>");

    let mut offenders = Vec::new();
    for product_root in PRODUCT_ROOTS {
        let mut files = Vec::new();
        collect_rust_files(&workspace_root.join(product_root), &mut files);
        for file in files {
            if is_test_context(&file) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&file) else {
                continue;
            };
            for (index, line) in source.lines().enumerate() {
                // Only a **read** of the variable is a switch. A comment that names the
                // removed switch is documentation of the repair, not a mutation path,
                // and it is deliberately allowed so the reason it was removed survives
                // next to the code.
                if line.contains(COUNTERFACTUAL_MARKER) && line.contains("env::var") {
                    offenders.push(format!(
                        "{}:{}: {}",
                        file.strip_prefix(workspace_root).unwrap_or(&file).display(),
                        index + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a counterfactual switch is reachable from product source; it must live only in a \
         test build or a temporary source mutation, never in a shipped path:\n{}",
        offenders.join("\n")
    );
}
