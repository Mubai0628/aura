//! A new file's approved target names the deepest folder that existed when it was approved, and the
//! path still to be created below it. Folders that appear along that path afterwards (another write
//! of the same response creating them, typically) anchor the target deeper without changing what
//! the call does. That is not a change of what was approved, as long as each folder on the way is an
//! ordinary folder at its own place inside the workspace, not a link, and the file is still absent.
use super::*;
use aura_execution_contract::OperationContent;
use aura_security_contract::{FileConsentTarget, ResourceDescriptor};

impl AgentExecution {
    /// Whether `fresh` differs from the approved `frozen` only by folders created since along new
    /// files' paths.
    pub(in crate::agent_execution) fn folders_created_since(
        &self,
        frozen: &OperationContent,
        fresh: &OperationContent,
    ) -> anyhow::Result<bool> {
        if frozen.resources.len() != fresh.resources.len() {
            return Ok(false);
        }
        let mut adopted = fresh.clone();
        let mut moved = false;
        for (was, now) in frozen.resources.iter().zip(adopted.resources.iter_mut()) {
            if was == now {
                continue;
            }
            let (
                ResourceDescriptor::WorkspacePath {
                    workspace_id,
                    canonical_relative_path,
                    file_target: Some(before),
                },
                ResourceDescriptor::WorkspacePath {
                    workspace_id: now_workspace,
                    canonical_relative_path: now_path,
                    file_target: Some(after),
                },
            ) = (was, &*now)
            else {
                return Ok(false);
            };
            if workspace_id != now_workspace
                || canonical_relative_path != now_path
                || !anchored_deeper(before, after)
                || !self.only_ordinary_folders_to(*workspace_id, canonical_relative_path)?
            {
                return Ok(false);
            }
            *now = was.clone();
            moved = true;
        }
        // A download's target, checked again, names the deeper folder as well.
        if moved && download_anchored_deeper(&frozen.action, &adopted.action) {
            adopted.action = frozen.action.clone();
        }
        Ok(moved && adopted == *frozen)
    }

    /// Every folder on the way to `path` that exists is an ordinary folder inside the workspace,
    /// not a link, and `path` itself does not exist.
    pub(in crate::agent_execution) fn only_ordinary_folders_to(
        &self,
        workspace: WorkspaceId,
        path: &WorkspacePath,
    ) -> anyhow::Result<bool> {
        let resolver = self.workspace.resolver(workspace)?;
        Ok(only_ordinary_folders(resolver.canonical_root(), path)?)
    }
}

/// [`AgentExecution::only_ordinary_folders_to`] below `root`.
fn only_ordinary_folders(root: &std::path::Path, path: &WorkspacePath) -> std::io::Result<bool> {
    let components = path.as_str().split('/').collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        // Not following links: a link on the way is itself what is looked at, and refused.
        match aura_windows_platform::inspect_existing_path(&current, false) {
            Ok(info) if index + 1 < components.len() => {
                if !info.is_directory || info.is_reparse_point {
                    return Ok(false);
                }
            }
            // The file itself exists now.
            Ok(_) => return Ok(false),
            // Nothing below a missing folder exists either.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

/// The same download, its target anchored at a folder of its way made since.
fn download_anchored_deeper(frozen: &serde_json::Value, fresh: &serde_json::Value) -> bool {
    let (Some(before), Some(after)) = (frozen.get("download_target"), fresh.get("download_target"))
    else {
        return false;
    };
    let mut rest = fresh.clone();
    rest["download_target"] = before.clone();
    let decode = |value: &serde_json::Value| {
        serde_json::from_value::<aura_fs_tools::download::DownloadTarget>(value.clone()).ok()
    };
    rest == *frozen
        && decode(before)
            .zip(decode(after))
            .is_some_and(|(before, after)| after.anchored_deeper_than(&before))
}

/// The same new entry, anchored at a deeper folder of its own path.
fn anchored_deeper(before: &FileConsentTarget, after: &FileConsentTarget) -> bool {
    before.canonical_path == after.canonical_path
        && before.recursive == after.recursive
        && before.protection == after.protection
        && before.identity_key.starts_with("entry:")
        && after.identity_key.starts_with("entry:")
        && after.ancestors.len() > before.ancestors.len()
        && after.ancestors.starts_with(&before.ancestors)
}

#[cfg(test)]
mod tests {
    use super::{download_anchored_deeper, only_ordinary_folders};
    use aura_domain_common::WorkspacePath;

    fn path(value: &str) -> WorkspacePath {
        WorkspacePath::parse(value).expect("a workspace path")
    }

    #[test]
    fn a_download_whose_folders_were_made_since_is_the_same_download() {
        let target = |anchor: &str, missing: &[&str]| {
            serde_json::json!({
                "url": "https://example.com/pkg.zip",
                "path": ".cache/s00/pkg.zip",
                "download_target": {
                    "path": "C:\\w\\.cache\\s00\\pkg.zip",
                    "parent_identity": {"volume_serial_number": "1", "file_id": anchor},
                    "identity": null,
                    "blake3": null,
                    "workspace_path": ".cache/s00/pkg.zip",
                    "missing_folders": missing,
                }
            })
        };
        let checked = target("root", &[".cache", "s00"]);
        assert!(download_anchored_deeper(&checked, &target("cache", &["s00"])));
        assert!(download_anchored_deeper(&checked, &target("s00", &[])));
        assert!(!download_anchored_deeper(&target("s00", &[]), &checked), "never shallower");
        let mut elsewhere = target("s00", &[]);
        elsewhere["download_target"]["path"] = "C:\\other\\pkg.zip".into();
        assert!(!download_anchored_deeper(&checked, &elsewhere));
        let mut other_url = target("s00", &[]);
        other_url["url"] = "https://example.com/other.zip".into();
        assert!(!download_anchored_deeper(&checked, &other_url));
    }

    #[test]
    fn only_ordinary_folders_may_appear_on_the_way_to_a_new_file() {
        let root = tempfile::tempdir().expect("workspace");
        let elsewhere = tempfile::tempdir().expect("outside");
        let target = path("game/server/api.mjs");
        assert!(only_ordinary_folders(root.path(), &target).expect("nothing yet"));
        std::fs::create_dir_all(root.path().join("game/server")).expect("folders");
        assert!(only_ordinary_folders(root.path(), &target).expect("ordinary folders"));
        std::fs::write(root.path().join("game/server/api.mjs"), "x").expect("the file");
        assert!(!only_ordinary_folders(root.path(), &target).expect("read"), "the file exists");
        junction::create(elsewhere.path(), root.path().join("linked")).expect("a junction");
        assert!(!only_ordinary_folders(root.path(), &path("linked/api.mjs")).expect("read"));
        std::fs::write(root.path().join("plain"), "x").expect("a file");
        assert!(!only_ordinary_folders(root.path(), &path("plain/api.mjs")).expect("read"));
    }
}
