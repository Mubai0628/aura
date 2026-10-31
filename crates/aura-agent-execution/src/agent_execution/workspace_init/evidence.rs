//! The checks that need the workspace: evidence files, executed commands, link targets,
//! command directories and secrets. Every problem is collected; nothing is written here.
use super::super::*;
use aura_windows_platform::NativeFileAccess;
use aura_windows_user_contract::{ShellCallState, ShellQueryInput};
use aura_workspace::SecurePathResolver;
use aura_workspace_domain::{
    AGENTS_PATH, ExecutedEvidence, FsExpectedState, FsReadTextRequest, GUIDES_DIRECTORY, InitPlan,
    InitShell, InitViolation, TextReadRange, WorkspaceFileTarget, WorkspaceInitCompleteInput,
    guide_path,
};
use std::collections::BTreeSet;

pub(super) fn verify(
    service: &AgentExecution,
    id: &CoreInvocationIdentityV1,
    snapshot: &PermissionSnapshot,
    resolver: &SecurePathResolver,
    plan: &InitPlan,
) -> anyhow::Result<Vec<InitViolation>> {
    let mut violations = Vec::new();
    let mut refuse = |field: &str, rule: &'static str, detail: String| {
        violations.push(InitViolation { field: field.to_owned(), rule, detail });
    };
    let guides = format!("{GUIDES_DIRECTORY}/");
    for evidence in &plan.repository_evidence {
        if evidence.path == AGENTS_PATH || evidence.path.starts_with(&guides) {
            refuse(
                &evidence.field,
                "evidence",
                format!(
                    "{} is what /init writes; cite the build file, script or documentation that defines the command",
                    evidence.path
                ),
            );
            continue;
        }
        match read_text(service, snapshot, &evidence.path) {
            Some(text) if text.contains(&evidence.command) => {}
            Some(_) => refuse(
                &evidence.field,
                "evidence",
                format!(
                    "{} does not contain `{}` exactly as written; quote it as the file has it, or run it and cite that call",
                    evidence.path, evidence.command
                ),
            ),
            None => refuse(
                &evidence.field,
                "evidence",
                format!("{} is not a readable text file in this workspace", evidence.path),
            ),
        }
    }
    if !plan.executed_evidence.is_empty() {
        let store = service.journal.lock()?;
        for evidence in &plan.executed_evidence {
            if let Err(detail) = executed(store.database.connection(), id, evidence) {
                refuse(&evidence.field, "evidence", detail);
            }
        }
    }
    for directory in &plan.directories {
        if !existing(resolver, directory).is_some_and(|is_directory| is_directory) {
            refuse(
                "commands.entries",
                "working_directory",
                format!("{directory} is not a directory in this workspace"),
            );
        }
    }
    let written = plan.files.iter().map(|file| file.path.as_str()).collect::<BTreeSet<_>>();
    for link in &plan.links {
        if !written.contains(link.target.as_str()) && existing(resolver, &link.target).is_none() {
            refuse(
                &link.source,
                "link",
                format!("{} does not exist and is not written by this call", link.target),
            );
        }
    }
    for file in &plan.files {
        if let Some(kind) = aura_secret_scanner::detect_text(&file.content) {
            refuse(
                &file.path,
                "secret",
                format!(
                    "the text contains what looks like a {kind}; never put credentials in these files"
                ),
            );
        }
    }
    Ok(violations)
}

/// The text each planned file replaces, after checking it against the state the model
/// read: a write that would conflict is refused before anyone is asked to approve it.
pub(super) fn targets(
    service: &AgentExecution,
    snapshot: &PermissionSnapshot,
    input: &WorkspaceInitCompleteInput,
    plan: &InitPlan,
) -> anyhow::Result<Result<Vec<Option<String>>, Vec<InitViolation>>> {
    let mut violations = Vec::new();
    let mut previous = Vec::new();
    for file in &plan.files {
        let (field, expected) = if file.path == AGENTS_PATH {
            ("expected".to_owned(), &input.expected)
        } else {
            let (index, guide) = input
                .guides
                .iter()
                .enumerate()
                .find(|(_, guide)| guide_path(guide.name.trim()) == file.path)
                .ok_or_else(|| anyhow::anyhow!("workspace_init.unplanned_file: {}", file.path))?;
            (format!("guides[{index}].expected"), &guide.expected)
        };
        // A path the rules already refused (an invalid guide name) has no state to check.
        let Ok(path) = WorkspacePath::parse(&file.path) else {
            previous.push(None);
            continue;
        };
        let current = match service.fs.read_text(FsReadTextRequest {
            workspace_id: snapshot.workspace_id,
            session_id: snapshot.session_id,
            target: WorkspaceFileTarget { path },
            range: TextReadRange::Full,
            explicit_encoding: None,
        }) {
            Ok(read) => Some(read),
            Err(aura_fs_tools::FsToolError::Workspace(
                aura_workspace::WorkspaceError::NotFound { .. },
            )) => None,
            Err(_) => {
                violations.push(violation(
                    &field,
                    format!(
                        "{} exists but cannot be read as text; /init cannot replace it",
                        file.path
                    ),
                ));
                previous.push(None);
                continue;
            }
        };
        let detail = match (expected, &current) {
            (FsExpectedState::Absent, None) => None,
            (FsExpectedState::Absent, Some(_)) => {
                Some(format!("{} already exists; read it and pass its logical_hash", file.path))
            }
            (FsExpectedState::FileHash { .. }, None) => {
                Some(format!("{} does not exist; use {{expected_kind: absent}}", file.path))
            }
            (FsExpectedState::FileHash { hash }, Some(read)) if *hash != read.logical_hash => {
                Some(format!("{} changed since you read it; read it again", file.path))
            }
            (FsExpectedState::FileHash { .. }, Some(_)) => None,
            (FsExpectedState::Directory, _) => {
                Some(format!("{} must be a file, not a directory", file.path))
            }
        };
        if let Some(detail) = detail {
            violations.push(violation(&field, detail));
        }
        previous.push(current.map(|read| read.text.replace("\r\n", "\n")));
    }
    Ok(if violations.is_empty() { Ok(previous) } else { Err(violations) })
}

fn violation(field: &str, detail: String) -> InitViolation {
    InitViolation { field: field.to_owned(), rule: "expected", detail }
}

/// The session's logical view of a text file, with LF line endings.
fn read_text(
    service: &AgentExecution,
    snapshot: &PermissionSnapshot,
    path: &str,
) -> Option<String> {
    let path = WorkspacePath::parse(path).ok()?;
    let read = service
        .fs
        .read_text(FsReadTextRequest {
            workspace_id: snapshot.workspace_id,
            session_id: snapshot.session_id,
            target: WorkspaceFileTarget { path },
            range: TextReadRange::Full,
            explicit_encoding: None,
        })
        .ok()?;
    Some(read.text.replace("\r\n", "\n"))
}

/// `Some(is_directory)` for an existing workspace entry.
fn existing(resolver: &SecurePathResolver, path: &str) -> Option<bool> {
    let path = WorkspacePath::parse(path).ok()?;
    let opened = resolver.open_existing(&path, NativeFileAccess::Metadata).ok()?;
    Some(opened.file.info().is_directory)
}

/// A shell call of this very task that finished with exit code 0, in the documented shell
/// and directory, whose script contains the command exactly as written.
fn executed(
    db: &aura_persistence_core::Connection,
    id: &CoreInvocationIdentityV1,
    evidence: &ExecutedEvidence,
) -> Result<(), String> {
    let call = evidence.tool_call_id;
    let not_here =
        || format!("{call} is not a shell.powershell or shell.cmd call of this /init task");
    let operation =
        aura_persistence_v2::execution::shell::operation_for_call(db, id.session_id, call)
            .ok()
            .flatten()
            .filter(|operation| operation.content.identity.turn_task_id == Some(id.turn_task_id))
            .ok_or_else(not_here)?;
    let view = aura_persistence_v2::execution::shell::query(
        db,
        id.session_id,
        &ShellQueryInput { tool_call_id: call, stdout_offset: 0, stderr_offset: 0, max_bytes: 1 },
    )
    .map_err(|_| not_here())?;
    if view.state != ShellCallState::Completed || view.exit_code != Some(0) {
        return Err(format!(
            "{call} did not finish with exit code 0; fix the command and run it again, or leave it out"
        ));
    }
    if !view.original_input.script.replace("\r\n", "\n").contains(&evidence.command) {
        return Err(format!(
            "the script of {call} does not contain `{}` exactly as written",
            evidence.command
        ));
    }
    let action = operation.content.action_kind.as_str();
    let shell = match evidence.shell {
        InitShell::PowerShell => action == "shell.powershell",
        InitShell::Cmd => action == "shell.cmd",
        InitShell::Any => true,
    };
    if !shell {
        return Err(format!("{call} ran in a different shell than the one documented"));
    }
    if !directory(&view.original_input.working_directory).eq_ignore_ascii_case(&evidence.directory)
    {
        return Err(format!(
            "{call} ran in {}, not in the documented directory {}",
            directory(&view.original_input.working_directory),
            evidence.directory
        ));
    }
    Ok(())
}

fn directory(relative: &str) -> String {
    let parts = relative.split(['/', '\\']).filter(|part| !part.is_empty() && *part != ".");
    let joined = parts.collect::<Vec<_>>().join("/");
    if joined.is_empty() { ".".to_owned() } else { joined }
}
