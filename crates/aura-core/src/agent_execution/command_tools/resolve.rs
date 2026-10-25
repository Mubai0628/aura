//! Planning: a save or removal is checked against the rules and the workspace before the
//! user is asked, so the approval card shows exactly what will be written or removed.
use super::super::*;
use super::{BoundFile, DeleteAction, SaveAction, folder};
use aura_agent_domain::{
    CommandExpectation, CommandSource, CommandsDeleteInput, CommandsSaveInput, builtin_commands,
    valid_command_name,
};
use aura_commands::{LoadedCommand, MANIFEST_FILE, TEMPLATE_FILE, WORKSPACE_COMMANDS_DIRECTORY};
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use aura_workspace_domain::{
    FsExpectedState, FsReadTextRequest, TextReadRange, WorkspaceFileTarget,
};

/// A request that broke rules; every problem goes back to the model at once.
#[derive(Debug)]
struct CommandRefused(Vec<String>);
impl std::fmt::Display for CommandRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "commands.refused: {} problem(s); nothing was written and the user was not asked. Fix them and call again.",
            self.0.len()
        )?;
        for problem in &self.0 {
            write!(f, "\n- {problem}")?;
        }
        Ok(())
    }
}
impl std::error::Error for CommandRefused {}

/// The message the model sees when planning refuses a command tool call.
pub(in crate::agent_execution) fn admission_feedback(error: &anyhow::Error) -> Option<String> {
    let text = error.to_string();
    text.starts_with("commands.").then_some(text)
}

impl AgentExecution {
    pub(in crate::agent_execution) fn resolve_command_tool(
        &self,
        handler: H,
        action: &mut serde_json::Value,
        capabilities: &[ResolvedCapability],
        batch: &ToolBatchRequestV1,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<Vec<ResourceDescriptor>> {
        match handler {
            // The reads go through `aura-commands` on `.aura/commands` only; they take no
            // workspace file claim, so they never wait on or exclude a writer.
            H::CommandsList | H::CommandsDescribe => {
                let id = &batch.invocation_identity;
                Ok(vec![ResourceDescriptor::AgentTask {
                    session_id: id.session_id,
                    turn_task_id: id.turn_task_id,
                    agent_run_id: id.agent_run_id,
                }])
            }
            H::CommandsSave => {
                let input: CommandsSaveInput = serde_json::from_value(action.clone())?;
                let bound = self.plan_save(input, snapshot)?;
                let paths = bound.files.iter().map(|file| file.path.clone()).collect::<Vec<_>>();
                *action = serde_json::to_value(bound)?;
                self.command_targets(&paths, capabilities, batch, snapshot)
            }
            H::CommandsDelete => {
                let input: CommandsDeleteInput = serde_json::from_value(action.clone())?;
                let bound = self.plan_delete(input, snapshot)?;
                let target = bound.folder.clone();
                *action = serde_json::to_value(bound)?;
                self.command_targets(&[target], capabilities, batch, snapshot)
            }
            _ => anyhow::bail!("commands.unknown_tool"),
        }
    }

    fn current_command(
        &self,
        snapshot: &PermissionSnapshot,
        name: &str,
    ) -> anyhow::Result<Option<LoadedCommand>> {
        let resolver = self.workspace.resolver(snapshot.workspace_id)?;
        Ok(aura_commands::scan_within(
            resolver.canonical_root(),
            WORKSPACE_COMMANDS_DIRECTORY,
            CommandSource::Workspace,
        )
        .into_iter()
        .find(|command| command.name == name))
    }

    fn plan_save(
        &self,
        input: CommandsSaveInput,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<SaveAction> {
        let mut problems = Vec::new();
        let name_ok = valid_command_name(&input.name);
        if !name_ok {
            problems
                .push("name: use lowercase letters, digits and -, starting with a letter".into());
        }
        if builtin_commands().iter().any(|command| command.name == input.name) {
            problems
                .push(format!("name: /{} is a built-in command; choose another name", input.name));
        }
        let current = if name_ok { self.current_command(snapshot, &input.name)? } else { None };
        let version = current
            .as_ref()
            .and_then(|command| command.manifest.as_deref())
            .and_then(|manifest| aura_commands::parse_manifest(manifest, &input.name).ok())
            .map_or(1, |manifest| manifest.version.saturating_add(1));
        let manifest = aura_commands::render_manifest(
            &input.name,
            input.title.as_deref(),
            &input.description,
            input.argument_hint.as_deref(),
            version,
        )
        .map_err(|issue| problems.push(format!("command.toml: {issue}")))
        .ok();
        if let Err(issue) = aura_commands::check_template(&input.template) {
            problems.push(format!("template: {issue}"));
        }
        match (&input.expected, &current) {
            (CommandExpectation::Absent, Some(_)) => problems.push(format!(
                "expected: /{} already exists; read it with commands.describe and pass its revision",
                input.name
            )),
            (CommandExpectation::Revision { .. }, None) if name_ok => problems.push(format!(
                "expected: /{} does not exist; use {{kind: absent}}",
                input.name
            )),
            (CommandExpectation::Revision { revision }, Some(command))
                if command.revision.as_deref() != Some(revision.as_str()) =>
            {
                problems.push(format!("expected: /{} changed since you read it; read it again", input.name))
            }
            _ => {}
        }
        let Some(manifest) = manifest.filter(|_| problems.is_empty()) else {
            return Err(CommandRefused(problems).into());
        };
        let directory = folder(&input.name);
        let mut files = Vec::new();
        for (file, content) in
            [(MANIFEST_FILE, manifest), (TEMPLATE_FILE, input.template.replace("\r\n", "\n"))]
        {
            let path = format!("{directory}/{file}");
            let (previous, expected) = self.command_file_state(snapshot, &path)?;
            files.push(BoundFile { path, content, previous, expected });
        }
        Ok(SaveAction { created: current.is_none(), files, input })
    }

    fn plan_delete(
        &self,
        input: CommandsDeleteInput,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<DeleteAction> {
        let current = if valid_command_name(&input.name) {
            self.current_command(snapshot, &input.name)?
        } else {
            None
        };
        let Some(current) = current else {
            return Err(CommandRefused(vec![format!(
                "name: /{} is not a command of this workspace",
                input.name
            )])
            .into());
        };
        if current.revision.as_deref() != Some(input.revision.as_str()) {
            return Err(CommandRefused(vec![format!(
                "revision: /{} changed since you read it, or it is a linked folder Aura does not change",
                input.name
            )])
            .into());
        }
        Ok(DeleteAction {
            folder: folder(&input.name),
            manifest: current.manifest,
            template: current.template,
            input,
        })
    }

    /// The text a file holds now, as the session sees it, and the state a write must expect.
    fn command_file_state(
        &self,
        snapshot: &PermissionSnapshot,
        path: &str,
    ) -> anyhow::Result<(Option<String>, FsExpectedState)> {
        match self.fs.read_text(FsReadTextRequest {
            workspace_id: snapshot.workspace_id,
            session_id: snapshot.session_id,
            target: WorkspaceFileTarget { path: WorkspacePath::parse(path)? },
            range: TextReadRange::Full,
            explicit_encoding: None,
        }) {
            Ok(read) => {
                Ok((Some(read.text), FsExpectedState::FileHash { hash: read.logical_hash }))
            }
            Err(aura_fs_tools::FsToolError::Workspace(
                aura_workspace::WorkspaceError::NotFound { .. },
            )) => Ok((None, FsExpectedState::Absent)),
            Err(_) => anyhow::bail!("commands.unreadable: {path} cannot be read as text"),
        }
    }

    fn command_targets(
        &self,
        paths: &[String],
        capabilities: &[ResolvedCapability],
        batch: &ToolBatchRequestV1,
        snapshot: &PermissionSnapshot,
    ) -> anyhow::Result<Vec<ResourceDescriptor>> {
        let mut resources = Vec::new();
        for path in paths {
            let mut target = serde_json::json!({ "path": path });
            let mut needed = capabilities.to_vec();
            let refused = || {
                anyhow::anyhow!(
                    "commands.protected_target: {path} is not an ordinary workspace file and cannot be changed here."
                )
            };
            let resolved = self
                .resolve_file_target(H::FileWrite, &mut target, &mut needed, batch, snapshot)
                .map_err(|_| refused())?;
            anyhow::ensure!(
                needed == capabilities
                    && matches!(resolved.as_slice(), [ResourceDescriptor::WorkspacePath { .. }]),
                "commands.protected_target: {path} is not an ordinary workspace file and cannot be changed here."
            );
            resources.extend(resolved);
        }
        Ok(resources)
    }
}
