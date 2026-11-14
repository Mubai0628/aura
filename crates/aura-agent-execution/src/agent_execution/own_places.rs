//! The places outside its workspace that a conversation works in as its own. A sub-agent's
//! temporary folder (its commands' TEMP and TMP) is its own. A main agent's sub-agents' worktrees
//! are its own. A sub-agent's commands delete without asking in its temporary folder, and, when it
//! has a worktree, in that worktree: what was there is committed on its branch, and the folder goes
//! when the sub-agent ends.
use super::*;
use aura_agent_team::{WorktreeState, store as team_store, worktree as rows};
use aura_windows_platform::{NativePathInfo, inspect_existing_path};
use std::path::{Path, PathBuf};

/// A conversation's own places, found when one of its calls is resolved.
#[derive(Clone, Debug, Default)]
pub(super) struct OwnPlaces {
    /// Folders outside the workspace that are its own.
    pub folders: Vec<PathBuf>,
    /// Where its commands delete without asking.
    pub deletable: Vec<PathBuf>,
    /// The TEMP and TMP of its commands, when it has its own.
    pub temp: Option<PathBuf>,
    /// What the user lets a sub-agent's commands do without asking.
    pub allowed: aura_agent_team::Allowances,
}

impl OwnPlaces {
    /// How its commands are read: its workspace and its own places.
    pub(super) fn area(&self, workspace_root: &Path) -> super::shell_risks::Area {
        let mut roots = vec![workspace_root.to_path_buf()];
        roots.extend(self.folders.iter().cloned());
        super::shell_risks::Area {
            roots,
            deletable: self.deletable.clone(),
            temp: self.temp.clone(),
        }
    }
}

impl AgentExecution {
    /// Reads the journal: only from inside a bounded section, never while holding its lock.
    pub(super) fn own_places(
        &self,
        session: SessionId,
        workspace: WorkspaceId,
    ) -> anyhow::Result<OwnPlaces> {
        let (sub_agent, worktrees) = {
            let journal = self.journal.lock()?;
            let db = journal.database.connection();
            match team_store::member_by_child(db, session)? {
                Some(member) => (Some(member.details.allowed), Vec::new()),
                None => (None, rows::worktrees_of_parent(db, session)?),
            }
        };
        if let Some(mut allowed) = sub_agent {
            let temp = crate::command_execution::process_policy::temp_directory(session);
            let mut deletable = vec![temp.clone()];
            let worktree = self.persistence.worktree_parent(workspace).is_some();
            if worktree {
                deletable.push(self.workspace.resolver(workspace)?.canonical_root().to_path_buf());
            }
            // Deleting without asking stays in a worktree, whose files its branch keeps.
            allowed.deletions &= worktree;
            return Ok(OwnPlaces {
                folders: vec![temp.clone()],
                deletable,
                temp: Some(temp),
                allowed,
            });
        }
        let folders = worktrees
            .into_iter()
            .filter(|worktree| worktree.state == WorktreeState::Active)
            // A copy whose folder cannot be named now is no place of the conversation's.
            .filter_map(|worktree| self.workspace.resolver(worktree.workspace_id).ok())
            .map(|resolver| resolver.canonical_root().to_path_buf())
            .collect();
        Ok(OwnPlaces { folders, ..OwnPlaces::default() })
    }

    /// The external capabilities a call holds: its live grants, its usable Skills' folders, and
    /// its conversation's own folders.
    pub(in crate::agent_execution) fn call_authority(
        &self,
        call: &CallContext,
    ) -> Vec<ResolvedCapability> {
        let mut covered = self.external_authority(&call.operation);
        for capability in own_authority(&call.operation, &call.own_folders, call.snapshot.mode) {
            if !covered.contains(&capability) {
                covered.push(capability);
            }
        }
        covered
    }
}

/// The external capabilities of `operation` that its conversation's own `folders` cover: it reads
/// them, and where `mode` writes the workspace it changes them as it does the workspace.
pub(super) fn own_authority(
    operation: &SecuredOperation,
    folders: &[PathBuf],
    mode: PermissionMode,
) -> Vec<ResolvedCapability> {
    if folders.is_empty() {
        return Vec::new();
    }
    let content = &operation.content;
    let owned = content.resources.iter().any(|resource| match resource {
        ResourceDescriptor::ExternalPath { canonical_path, .. } => {
            in_own_folder(folders, Path::new(&canonical_path.display))
        }
        _ => false,
    });
    if !owned {
        return Vec::new();
    }
    content
        .capabilities
        .iter()
        .copied()
        .filter(|capability| match capability {
            ResolvedCapability::ExternalFileRead => true,
            ResolvedCapability::ExternalFileWrite
            | ResolvedCapability::ExternalFileCreate
            | ResolvedCapability::ExternalFileDelete => mode != PermissionMode::ReadOnly,
            _ => false,
        })
        .collect()
}

/// Whether `path`, or the folder a path still to be made goes in, lies in one of `folders`; both
/// sides are compared as the objects they are, so a link out of a folder is not in it.
pub(super) fn in_own_folder(folders: &[PathBuf], path: &Path) -> bool {
    let target: Option<NativePathInfo> = match inspect_existing_path(path, true) {
        Ok(info) => Some(info),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            path.parent().and_then(|parent| inspect_existing_path(parent, true).ok())
        }
        Err(_) => None,
    };
    let Some(target) = target else { return false };
    folders.iter().any(|folder| {
        inspect_existing_path(folder, true).is_ok_and(|root| {
            root.is_directory && super::file_targets::native_scope::within(&root, &target)
        })
    })
}
