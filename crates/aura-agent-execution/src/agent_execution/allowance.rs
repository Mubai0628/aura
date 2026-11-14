//! What a sub-agent's question may offer beyond one answer: the user lets its commands delete in
//! its own folders from then on, as the card's 本工作区允许 says. The command is read again as it
//! was planned, so the card offers only what the next command of the same kind would be spared.
use super::*;
use aura_agent_team::{Allowances, store as team_store};
use std::path::Path;

impl AgentExecution {
    /// Whether answering the question about `operation` may also let its sub-agent's later
    /// commands delete without asking: a sub-agent with a worktree, a PowerShell or Cmd command
    /// that asks only because it deletes, and deletions the allowance covers.
    pub fn offers_deletion_allowance(&self, operation: &SecuredOperation) -> anyhow::Result<bool> {
        let content = &operation.content;
        if !matches!(content.source, SecuritySourceRef::BuiltInTool { .. })
            || !matches!(content.action_kind.as_str(), "shell.powershell" | "shell.cmd")
            || content.risks != [OperationRiskFlag::PermanentFileDeletion]
            || content.capabilities.iter().any(|capability| {
                matches!(
                    capability,
                    ResolvedCapability::DownloadArtifact
                        | ResolvedCapability::DownloadWindowsExecutable
                )
            })
        {
            return Ok(false);
        }
        let (Some(session), Some(workspace)) =
            (content.identity.session_id, content.identity.workspace_id)
        else {
            return Ok(false);
        };
        let sub_agent = {
            let journal = self.journal.lock()?;
            team_store::member_by_child(journal.database.connection(), session)?.is_some()
        };
        if !sub_agent || self.persistence.worktree_parent(workspace).is_none() {
            return Ok(false);
        }
        let Ok(command) = serde_json::from_value::<aura_windows_user_contract::ManagedShellCommand>(
            content.action.clone(),
        ) else {
            return Ok(false);
        };
        let own = self.own_places(session, workspace)?;
        let root = self.workspace.resolver(workspace)?.canonical_root().to_path_buf();
        let found = super::shell_risks::of_script(
            &command.script,
            Some(Path::new(&command.working_directory)),
            &own.area(&root),
        );
        let deletions = Allowances { deletions: true, ..own.allowed };
        Ok(found.deletion_asks(&own.allowed) && !found.deletion_asks(&deletions))
    }

    /// The question about `operation` as the main agent is shown it, when it may answer first: a
    /// sub-agent with a worktree whose built-in call only deletes or writes in its own worktree
    /// and temporary folder, as far as the call shows; no network, no other risk.
    pub fn reviewable(&self, operation: &SecuredOperation) -> anyhow::Result<Option<Reviewable>> {
        let content = &operation.content;
        let (Some(session), Some(workspace)) =
            (content.identity.session_id, content.identity.workspace_id)
        else {
            return Ok(None);
        };
        let own_workspace = |resource: &ResourceDescriptor| match resource {
            ResourceDescriptor::WorkspaceRoot { workspace_id }
            | ResourceDescriptor::WorkspacePath { workspace_id, .. } => *workspace_id == workspace,
            _ => false,
        };
        if !matches!(content.source, SecuritySourceRef::BuiltInTool { .. })
            || content.network != OperationNetwork::None
            || !content.risks.iter().all(|risk| *risk == OperationRiskFlag::PermanentFileDeletion)
            || !content.resources.iter().all(own_workspace)
            || !content.capabilities.iter().all(|capability| {
                matches!(
                    capability,
                    ResolvedCapability::ProcessExecute
                        | ResolvedCapability::WorkspaceFileRead
                        | ResolvedCapability::WorkspaceFileWrite
                        | ResolvedCapability::WorkspaceFileCreate
                        | ResolvedCapability::WorkspaceFileDelete
                        | ResolvedCapability::WorkspaceFileMove
                )
            })
        {
            return Ok(None);
        }
        let sub_agent = {
            let journal = self.journal.lock()?;
            team_store::member_by_child(journal.database.connection(), session)?.is_some()
        };
        if !sub_agent || self.persistence.worktree_parent(workspace).is_none() {
            return Ok(None);
        }
        let deletes = content.risks.contains(&OperationRiskFlag::PermanentFileDeletion)
            || content.action_kind == "fs.delete";
        let request = match content.action_kind.as_str() {
            "shell.powershell" | "shell.cmd" => {
                let Ok(command) = serde_json::from_value::<
                    aura_windows_user_contract::ManagedShellCommand,
                >(content.action.clone()) else {
                    return Ok(None);
                };
                let own = self.own_places(session, workspace)?;
                let root = self.workspace.resolver(workspace)?.canonical_root().to_path_buf();
                let found = super::shell_risks::of_script(
                    &command.script,
                    Some(Path::new(&command.working_directory)),
                    &own.area(&root),
                );
                // A place outside its own, written or deleted, stays the user's to decide.
                if !found.outside.is_empty() || found.deletes && !found.deletes_here {
                    return Ok(None);
                }
                command.script
            }
            kind if kind.starts_with("fs.") => {
                let Some(path) = content.action.get("path").and_then(serde_json::Value::as_str)
                else {
                    return Ok(None);
                };
                format!("{kind} {path}")
            }
            _ => return Ok(None),
        };
        Ok(Some(Reviewable { request, deletes }))
    }
}

/// What the main agent is shown of a sub-agent's question it may answer first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reviewable {
    /// What the call does: the command, or the file tool and the file.
    pub request: String,
    /// It deletes files.
    pub deletes: bool,
}
