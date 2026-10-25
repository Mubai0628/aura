//! Execution: the approved files go through the file service as one changeset — missing
//! guide folders, then the guides, then the root file — each with its expected state, so a
//! file changed since it was read is never overwritten and a failure leaves nothing written.
use super::super::*;
use super::InitAction;
use aura_windows_platform::NativeFileAccess;
use aura_workspace_domain::{
    AGENTS_PATH, FsApplyChangesetRequest, FsChangesetOperation, FsExpectedState, GUIDES_DIRECTORY,
    InitWrittenFile, LineEnding, TextEncoding, WorkspaceFileTarget, WorkspaceInitCompleteResult,
    guide_path,
};
use std::collections::BTreeMap;

pub(in crate::agent_execution) fn write(
    service: &AgentExecution,
    call: &CallContext,
    correlation: CorrelationId,
) -> anyhow::Result<serde_json::Value> {
    let action: InitAction = serde_json::from_value(call.operation.content.action.clone())?;
    let resolver = service.workspace.resolver(call.snapshot.workspace_id)?;
    let mut expected = action
        .input
        .guides
        .iter()
        .map(|guide| (guide_path(guide.name.trim()), guide.expected.clone()))
        .collect::<BTreeMap<_, _>>();
    expected.insert(AGENTS_PATH.to_owned(), action.input.expected.clone());
    let mut operations = Vec::new();
    if !action.input.guides.is_empty() {
        for directory in ["docs", GUIDES_DIRECTORY] {
            let path = WorkspacePath::parse(directory)?;
            match resolver.open_existing(&path, NativeFileAccess::Metadata) {
                Ok(opened) => anyhow::ensure!(
                    opened.file.info().is_directory,
                    "workspace_init.target_conflict: {directory} exists and is not a directory"
                ),
                Err(_) => operations.push(FsChangesetOperation::CreateDirectory {
                    target: WorkspaceFileTarget { path },
                }),
            }
        }
    }
    let mut files = Vec::new();
    // The bound files, in their planned order: guides first, the root file last.
    for file in action.files {
        let state = expected
            .get(&file.path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("workspace_init.unplanned_file: {}", file.path))?;
        let creating = state == FsExpectedState::Absent;
        let path = WorkspacePath::parse(&file.path)?;
        files.push(InitWrittenFile { path: path.clone(), created: creating });
        operations.push(FsChangesetOperation::WriteFile {
            target: WorkspaceFileTarget { path },
            content: file.content,
            expected: state,
            // A new file is UTF-8 with LF; a replaced file keeps its own convention.
            encoding: creating.then_some(TextEncoding::Utf8),
            line_ending: creating.then_some(LineEnding::Lf),
        });
    }
    let result = service.fs.apply_changeset(
        FsApplyChangesetRequest {
            workspace_id: call.snapshot.workspace_id,
            session_id: call.snapshot.session_id,
            operations,
        },
        correlation,
    )?;
    Ok(serde_json::to_value(WorkspaceInitCompleteResult {
        disposition: result.mutation.disposition,
        files,
    })?)
}
