//! Execution of the four command tools on the file path. A save or removal publishes only
//! the files its Operation bound, through `apply_changeset`, so it is all or nothing.
use super::super::*;
use super::{DeleteAction, SaveAction, folder};
use aura_agent_domain::{
    CommandDefinitionView, CommandSource, CommandSummary, CommandsDeleteResult,
    CommandsDescribeInput, CommandsListResult, CommandsSaveResult, builtin_commands,
};
use aura_commands::WORKSPACE_COMMANDS_DIRECTORY;
use aura_windows_platform::NativeFileAccess;
use aura_workspace_domain::{
    FsApplyChangesetRequest, FsChangesetOperation, FsExpectedState, FsMutationDisposition,
    LineEnding, TextEncoding, WorkspaceFileTarget,
};

pub(in crate::agent_execution) fn run(
    service: &AgentExecution,
    call: &CallContext,
    correlation: CorrelationId,
) -> anyhow::Result<serde_json::Value> {
    let content = &call.operation.content;
    let resolver = service.workspace.resolver(call.snapshot.workspace_id)?;
    let workspace_commands = || {
        aura_commands::scan_within(
            resolver.canonical_root(),
            WORKSPACE_COMMANDS_DIRECTORY,
            CommandSource::Workspace,
        )
    };
    Ok(match content.action_kind.as_str() {
        "commands.list" => {
            let mut commands = builtin_commands()
                .into_iter()
                .map(|command| CommandSummary {
                    name: command.name,
                    title: command.title,
                    description: command.description,
                    kind: command.kind,
                    workspace: false,
                    issue: None,
                    revision: None,
                })
                .collect::<Vec<_>>();
            commands.extend(workspace_commands().into_iter().map(|command| {
                let entry = command.entry(true, None);
                CommandSummary {
                    name: entry.name,
                    title: entry.title,
                    description: entry.description,
                    kind: entry.kind,
                    workspace: true,
                    issue: entry.issue,
                    revision: entry.revision,
                }
            }));
            serde_json::to_value(CommandsListResult { commands })?
        }
        "commands.describe" => {
            let input: CommandsDescribeInput = serde_json::from_value(content.action.clone())?;
            let command = workspace_commands()
                .into_iter()
                .find(|command| command.name == input.name)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "commands.not_found: /{} is not a command of this workspace",
                        input.name
                    )
                })?;
            serde_json::to_value(CommandDefinitionView {
                path: WorkspacePath::parse(folder(&command.name))?,
                name: command.name,
                manifest: command.manifest,
                template: command.template,
                revision: command.revision,
                issue: command.issue,
            })?
        }
        "commands.save" => {
            let action: SaveAction = serde_json::from_value(content.action.clone())?;
            save(service, call, &resolver, action, correlation)?
        }
        "commands.delete" => {
            let action: DeleteAction = serde_json::from_value(content.action.clone())?;
            let path = WorkspacePath::parse(&action.folder)?;
            applied(service.fs.apply_changeset(
                FsApplyChangesetRequest {
                    workspace_id: call.snapshot.workspace_id,
                    session_id: call.snapshot.session_id,
                    operations: vec![FsChangesetOperation::Delete {
                        target: WorkspaceFileTarget { path: path.clone() },
                        expected: FsExpectedState::Directory,
                        recursive: true,
                    }],
                },
                correlation,
            )?)?;
            serde_json::to_value(CommandsDeleteResult { name: action.input.name, removed: path })?
        }
        other => anyhow::bail!("commands.unknown_tool: {other}"),
    })
}

fn save(
    service: &AgentExecution,
    call: &CallContext,
    resolver: &aura_workspace::SecurePathResolver,
    action: SaveAction,
    correlation: CorrelationId,
) -> anyhow::Result<serde_json::Value> {
    let mut operations = Vec::new();
    let mut directory = String::new();
    for part in folder(&action.input.name).split('/') {
        directory =
            if directory.is_empty() { part.to_owned() } else { format!("{directory}/{part}") };
        let path = WorkspacePath::parse(&directory)?;
        match resolver.open_existing(&path, NativeFileAccess::Metadata) {
            Ok(opened) => anyhow::ensure!(
                opened.file.info().is_directory,
                "commands.target_conflict: {directory} exists and is not a folder"
            ),
            Err(_) => operations.push(FsChangesetOperation::CreateDirectory {
                target: WorkspaceFileTarget { path },
            }),
        }
    }
    let mut files = Vec::new();
    for file in action.files {
        let creating = file.expected == FsExpectedState::Absent;
        let path = WorkspacePath::parse(&file.path)?;
        files.push(path.clone());
        operations.push(FsChangesetOperation::WriteFile {
            target: WorkspaceFileTarget { path },
            content: file.content,
            expected: file.expected,
            encoding: creating.then_some(TextEncoding::Utf8),
            line_ending: creating.then_some(LineEnding::Lf),
        });
    }
    applied(service.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: call.snapshot.workspace_id,
            session_id: call.snapshot.session_id,
            operations,
        },
        correlation,
    )?)?;
    Ok(serde_json::to_value(CommandsSaveResult {
        name: action.input.name,
        created: action.created,
        files,
    })?)
}

fn applied(result: aura_workspace_domain::FsApplyChangesetResult) -> anyhow::Result<()> {
    anyhow::ensure!(
        result.mutation.disposition == FsMutationDisposition::Applied,
        "commands.deferred: a command file is open with unsaved changes in an editor; nothing was changed. Try again after it is saved."
    );
    Ok(())
}
