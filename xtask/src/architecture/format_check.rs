//! The workspace's Rust format check, run as several `cargo fmt -p ... -- --check` commands.
//!
//! `cargo fmt --all` hands the root file of every target to one rustfmt command line, each as a
//! verbatim path (`\\?\C:\...`). With 500-odd targets that line passes Windows' limit of 32,767
//! characters, and the check stops before it reads a file ("The filename or extension is too
//! long", os error 206). So the packages are split into groups whose root files fit the budget
//! below, measured with this checkout's own paths: a longer checkout path gets smaller groups.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// Characters the file arguments of one rustfmt command line may take. Windows allows 32,767 for
/// the whole line; the rest is room for the program and its options.
const ARGUMENT_BUDGET: usize = 24_000;
/// What a root file costs beyond its path: the verbatim prefix `\\?\`, a separating space and the
/// quotes a path with a space in it needs.
const PER_FILE: usize = 7;

/// Checks the formatting of every workspace package.
pub(crate) fn check(root: &Path) -> Result<()> {
    let output = crate::process::command("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("failed to start cargo metadata")?;
    ensure!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Value =
        serde_json::from_slice(&output.stdout).context("cargo metadata printed no JSON")?;
    for group in groups(&packages(&metadata)?, ARGUMENT_BUDGET) {
        let mut args = vec!["fmt"];
        for name in &group {
            args.extend(["-p", name.as_str()]);
        }
        args.extend(["--", "--check"]);
        crate::run(root, "cargo", &args)?;
    }
    Ok(())
}

/// Each workspace member, in metadata order, with the characters its targets' root files take on
/// a rustfmt command line.
fn packages(metadata: &Value) -> Result<Vec<(String, usize)>> {
    let members = metadata["workspace_members"]
        .as_array()
        .context("cargo metadata lists no workspace members")?;
    let packages = metadata["packages"].as_array().context("cargo metadata lists no packages")?;
    let mut weighed = Vec::new();
    for package in packages {
        if !members.contains(&package["id"]) {
            continue;
        }
        let name = package["name"].as_str().context("a package without a name")?;
        let targets = package["targets"].as_array().context("a package without targets")?;
        let mut weight = 0;
        for target in targets {
            let path = target["src_path"].as_str().context("a target without a root file")?;
            weight += path.len() + PER_FILE;
        }
        weighed.push((name.to_owned(), weight));
    }
    Ok(weighed)
}

/// The packages in order, split into consecutive groups that each stay within `budget`. A package
/// heavier than the budget on its own is a group by itself: `cargo fmt -p` cannot split it.
fn groups(packages: &[(String, usize)], budget: usize) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut taken = 0;
    for (name, weight) in packages {
        match groups.last_mut() {
            Some(group) if taken + weight <= budget => {
                group.push(name.clone());
                taken += weight;
            }
            _ => {
                groups.push(vec![name.clone()]);
                taken = *weight;
            }
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::{groups, packages};
    use serde_json::json;

    fn named(weights: &[usize]) -> Vec<(String, usize)> {
        weights.iter().enumerate().map(|(index, weight)| (format!("p{index}"), *weight)).collect()
    }

    #[test]
    fn every_package_is_checked_once_and_in_order() {
        let packages = named(&[5, 9, 3, 7, 1, 6]);
        let checked = groups(&packages, 10).concat();
        let expected: Vec<String> = packages.iter().map(|(name, _)| name.clone()).collect();
        assert_eq!(checked, expected);
    }

    #[test]
    fn no_group_takes_more_than_the_budget() {
        let packages = named(&[5, 9, 3, 7, 1, 6]);
        for group in groups(&packages, 10) {
            let weight: usize = group
                .iter()
                .map(|name| packages.iter().find(|(known, _)| known == name).expect("known").1)
                .sum();
            assert!(weight <= 10, "{group:?} takes {weight}");
        }
    }

    #[test]
    fn a_package_heavier_than_the_budget_is_checked_on_its_own() {
        let checked = groups(&named(&[4, 25, 4]), 10);
        assert_eq!(
            checked,
            vec![vec!["p0".to_owned()], vec!["p1".to_owned()], vec!["p2".to_owned()]]
        );
    }

    #[test]
    fn a_package_weighs_its_targets_root_files_and_only_members_count() {
        let lib = r"C:\w\a\src\lib.rs";
        let test = r"C:\w\a\tests\flow.rs";
        let other = r"C:\w\b\src\main.rs";
        let metadata = json!({
            "workspace_members": ["a 0.1.0 (path+file:///C:/w/a)", "b 0.1.0 (path+file:///C:/w/b)"],
            "packages": [
                { "id": "a 0.1.0 (path+file:///C:/w/a)", "name": "a",
                  "targets": [{ "src_path": lib }, { "src_path": test }] },
                { "id": "outside 1.0.0 (registry+https://example.invalid)", "name": "outside",
                  "targets": [{ "src_path": r"C:\registry\outside\src\lib.rs" }] },
                { "id": "b 0.1.0 (path+file:///C:/w/b)", "name": "b",
                  "targets": [{ "src_path": other }] }
            ]
        });
        assert_eq!(
            packages(&metadata).expect("packages"),
            vec![
                ("a".to_owned(), lib.len() + test.len() + 2 * super::PER_FILE),
                ("b".to_owned(), other.len() + super::PER_FILE),
            ]
        );
    }
}
