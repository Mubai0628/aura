//! Installation-only read grants to the fixed shared package. Each existing
//! object receives a non-inheriting read ACE; private account secrets are never
//! in this scope. Restore precedes legacy runtime Upgrade/Remove checks.
use super::*;
use aura_windows_platform::ExecutionFileSecurity;
use aura_windows_user_contract::ExecutionFileBinding;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    binding: ExecutionFileBinding,
    before: String,
    after: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: String,
    installation_id: String,
    account_sid: String,
    state: String,
    entries: Vec<Entry>,
}
fn save(path: &Path, value: &Journal) -> Result<()> {
    let stage = path.with_extension("next");
    if stage.exists() {
        let (_, retained) = file_binding::capture(&stage, false)?;
        ensure!(!retained.info().is_directory, "online.stage_directory");
        drop(retained);
        std::fs::remove_file(&stage)?;
    }
    let mut output = std::fs::OpenOptions::new().create_new(true).write(true).open(&stage)?;
    output.write_all(&serde_json::to_vec(value)?)?;
    output.sync_all()?;
    drop(output);
    aura_windows_platform::atomic_replace_file(&stage, path)?;
    Ok(())
}
fn push_existing(paths: &mut Vec<PathBuf>, path: PathBuf) -> Result<()> {
    match aura_windows_platform::inspect_existing_path(&path, false) {
        Ok(info) => {
            ensure!(
                !info.is_reparse_point && (info.is_directory || info.number_of_links == 1),
                "online.runtime_alias"
            );
            paths.push(path);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = vec![root.to_path_buf(), root.join("record.json")];
    for relative in
        ["execution.json", "aura-command-runner.exe", "aura-core.exe", "execution-scratch"]
    {
        push_existing(&mut paths, root.join(relative))?;
    }
    let runtime = root.join("runtime");
    match aura_windows_platform::inspect_existing_path(&runtime, false) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(info) => {
            ensure!(!info.is_reparse_point && info.is_directory, "online.runtime_alias");
            let mut pending = vec![runtime];
            while let Some(path) = pending.pop() {
                ensure!(paths.len() < 4096, "online.runtime_entry_limit");
                let info = aura_windows_platform::inspect_existing_path(&path, false)?;
                ensure!(
                    !info.is_reparse_point && (info.is_directory || info.number_of_links == 1),
                    "online.runtime_alias"
                );
                if info.is_directory {
                    for child in std::fs::read_dir(&path)? {
                        pending.push(child?.path());
                    }
                }
                paths.push(path);
            }
        }
    }
    Ok(paths)
}
pub fn access(root: &Path, restore: bool) -> Result<serde_json::Value> {
    ensure!(aura_windows_platform::current_process_is_elevated()?, "online.install_elevation");
    let (record, root_handle) = crate::resources::record(root)?;
    // The record read pins the install root without FILE_SHARE_DELETE. A later
    // WRITE_DAC reopen of that directory needs DELETE and would hit os error 32.
    drop(root_handle);
    let owner = record["owner_sid"].as_str().ok_or_else(|| anyhow::anyhow!("online.owner"))?;
    let path = root.join("private/online-read-access.json");
    aura_windows_platform::verify_execution_installation_acl(&root.join("private"), owner, true)?;
    let mut journal = if path.exists() {
        let (binding, _retained) = file_binding::capture(&path, true)?;
        ensure!(std::fs::metadata(&path)?.len() <= 8 * 1024 * 1024, "online.journal_size");
        let bytes = std::fs::read(&path)?;
        ensure!(binding.sha256 == Some(ContentHash::sha256(&bytes)), "online.journal_changed");
        let journal: Journal = serde_json::from_slice(&bytes)?;
        ensure!(
            journal.schema == "execution-online-read-v1"
                && journal.installation_id == record["install_id"]
                && journal.entries.len() <= 4096,
            "online.journal_binding"
        );
        Some(journal)
    } else {
        None
    };
    if restore && journal.is_none() {
        return Ok(serde_json::json!({"restored":true,"objects":0}));
    }
    if !restore && journal.as_ref().is_none_or(|v| v.state == "restored") {
        let account = super::account(root, &record)?;
        let mut entries = Vec::new();
        for target in paths(root)? {
            let (binding, held) = file_binding::capture(&target, false)?;
            drop(held);
            let file = ExecutionFileSecurity::open(&target, true)?;
            let before = file.capture()?;
            ensure!(!before.contains(&account.sid), "online.unowned_read_ace");
            let after = file.plan(&[(&account.sid, 0x1200a9)])?;
            entries.push(Entry { binding, before, after });
        }
        let next = Journal {
            schema: "execution-online-read-v1".into(),
            installation_id: record["install_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("online.id"))?
                .into(),
            account_sid: account.sid,
            state: "prepared".into(),
            entries,
        };
        save(&path, &next)?;
        journal = Some(next);
    }
    let mut journal = journal.ok_or_else(|| anyhow::anyhow!("online.journal_missing"))?;
    if journal.state == "restored" && restore {
        return Ok(serde_json::json!({"restored":true,"objects":journal.entries.len()}));
    }
    let account = super::account(root, &record)?;
    ensure!(journal.account_sid == account.sid, "online.read_identity_changed");
    ensure!(matches!(journal.state.as_str(), "prepared" | "applied"), "online.journal_state");
    let root_info = aura_windows_platform::inspect_existing_path(root, false)?;
    let expected = paths(root)?
        .into_iter()
        .map(|v| std::fs::canonicalize(v).map_err(Into::into))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    ensure!(journal.entries.len() == expected.len(), "online.scope_inventory_changed");
    let mut seen = std::collections::BTreeSet::new();
    for entry in &journal.entries {
        ensure!(
            !entry.before.contains(&journal.account_sid)
                && ExecutionFileSecurity::read_only_plan(&entry.before, &journal.account_sid)?
                    == entry.after,
            "online.read_transform_changed"
        );
        let retained = file_binding::verify(&entry.binding)?;
        ensure!(
            aura_windows_platform::final_path_within(&root_info, retained.info())
                && expected.contains(&std::fs::canonicalize(&entry.binding.path)?)
                && seen.insert(entry.binding.path.clone()),
            "online.scope_changed"
        );
        drop(retained);
        let object = ExecutionFileSecurity::open(Path::new(&entry.binding.path), true)?;
        let current = object.capture()?;
        ensure!(
            ExecutionFileSecurity::sddl_equivalent(&current, &entry.before)
                || ExecutionFileSecurity::sddl_equivalent(&current, &entry.after),
            "online.acl_drift"
        );
        let target = if restore { &entry.before } else { &entry.after };
        if !ExecutionFileSecurity::sddl_equivalent(&current, target) {
            object.apply_exact(&current, target)?;
        }
    }
    journal.state = if restore { "restored" } else { "applied" }.into();
    save(&path, &journal)?;
    Ok(
        serde_json::json!({"restored":restore,"objects":journal.entries.len(),"private_access":false,"account_sid":journal.account_sid}),
    )
}

#[cfg(test)]
mod tests {
    use super::paths;
    use std::fs;

    fn isolated_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "aura-online-paths-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
            label
        ));
        fs::create_dir_all(root.join("execution-scratch")).expect("scratch");
        fs::create_dir_all(root.join("runtime")).expect("runtime");
        fs::write(root.join("record.json"), b"{}").expect("record");
        fs::write(root.join("execution.json"), b"{}").expect("manifest");
        fs::write(root.join("aura-command-runner.exe"), b"runner").expect("runner");
        root
    }

    #[test]
    fn paths_skips_missing_install_root_core() {
        let root = isolated_root("missing-core");
        let listed = paths(&root).expect("paths");
        assert!(listed.contains(&root));
        assert!(listed.contains(&root.join("record.json")));
        assert!(listed.contains(&root.join("execution.json")));
        assert!(listed.contains(&root.join("aura-command-runner.exe")));
        assert!(listed.contains(&root.join("execution-scratch")));
        assert!(listed.contains(&root.join("runtime")));
        assert!(
            !listed.iter().any(|path| path.file_name().is_some_and(|name| name == "aura-core.exe"))
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn paths_includes_existing_install_root_core() {
        let root = isolated_root("present-core");
        fs::write(root.join("aura-core.exe"), b"core").expect("core");
        let listed = paths(&root).expect("paths");
        assert!(listed.contains(&root.join("aura-core.exe")));
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
