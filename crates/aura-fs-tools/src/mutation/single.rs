//! One-file writes and patches, each a changeset of one operation. The model's own calls also
//! get the hash of the bytes they wrote, the expected state of its next change of the file.
//! A file the model creates in a folder that does not exist yet gets that folder: the missing
//! folders go first in the same changeset, each checked like any other target in the workspace.
use super::*;
use aura_workspace_domain::{AgentFileMutationResult, FsApplyChangesetResult};

impl FsToolService {
    pub fn write_file(
        &self,
        request: FsWriteFileRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        Ok(self.apply_changeset(write_changeset(request), correlation_id)?.mutation)
    }

    pub fn apply_patch(
        &self,
        request: FsApplyPatchRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        Ok(self.apply_changeset(patch_changeset(request), correlation_id)?.mutation)
    }

    /// `fs.write`: the mutation, and the file's logical_hash after it.
    pub fn write_file_for_agent(
        &self,
        request: FsWriteFileRequest,
        correlation_id: CorrelationId,
    ) -> Result<AgentFileMutationResult, FsToolError> {
        let folders = if request.expected == FsExpectedState::Absent {
            self.missing_folders(request.workspace_id, &request.target.path)?
        } else {
            Vec::new()
        };
        let mut changeset = write_changeset(request);
        changeset.operations.splice(
            0..0,
            folders.into_iter().map(|path| FsChangesetOperation::CreateDirectory {
                target: WorkspaceFileTarget { path },
            }),
        );
        Ok(for_agent(self.apply_changeset(changeset, correlation_id)?))
    }

    /// The folders above `path` that do not exist, outermost first; none when its parent exists.
    fn missing_folders(
        &self,
        workspace_id: WorkspaceId,
        path: &WorkspacePath,
    ) -> Result<Vec<WorkspacePath>, FsToolError> {
        let resolver = self.resolver(workspace_id)?;
        let mut missing = Vec::new();
        let mut current = path.as_str();
        while let Some((parent, _)) = current.rsplit_once('/') {
            let folder = WorkspacePath::parse(parent).map_err(|error| {
                FsToolError::InvalidRequest(format!("invalid parent path: {error}"))
            })?;
            match resolver.open_existing(&folder, NativeFileAccess::Metadata) {
                Err(WorkspaceError::NotFound { .. }) => missing.push(folder),
                _ => break,
            }
            current = parent;
        }
        missing.reverse();
        Ok(missing)
    }

    /// `fs.patch`: the mutation, and the file's logical_hash after it.
    pub fn apply_patch_for_agent(
        &self,
        request: FsApplyPatchRequest,
        correlation_id: CorrelationId,
    ) -> Result<AgentFileMutationResult, FsToolError> {
        Ok(for_agent(self.apply_changeset(patch_changeset(request), correlation_id)?))
    }
}

impl PreparedOperation {
    /// The hash of the bytes a write or patch step writes: the file's logical_hash after it.
    pub(super) fn written_hash(&self) -> Option<ContentHash> {
        match self {
            Self::Write { audits, .. } => audits.first().and_then(|seed| seed.after_hash.clone()),
            _ => None,
        }
    }
}

fn write_changeset(request: FsWriteFileRequest) -> FsApplyChangesetRequest {
    FsApplyChangesetRequest {
        workspace_id: request.workspace_id,
        session_id: request.session_id,
        operations: vec![FsChangesetOperation::WriteFile {
            target: request.target,
            content: request.content,
            expected: request.expected,
            encoding: request.encoding,
            line_ending: request.line_ending,
        }],
    }
}

fn patch_changeset(request: FsApplyPatchRequest) -> FsApplyChangesetRequest {
    FsApplyChangesetRequest {
        workspace_id: request.workspace_id,
        session_id: request.session_id,
        operations: vec![FsChangesetOperation::ApplyPatch {
            target: request.target,
            unified_diff: request.unified_diff,
            expected_hash: request.expected_hash,
        }],
    }
}

/// A deferred write has no step, and so no hash: the model reads the file again. The file's step
/// is the last one; the folders created for it come before.
fn for_agent(applied: FsApplyChangesetResult) -> AgentFileMutationResult {
    let hash = applied.steps.last().and_then(|step| step.after_hash.clone());
    AgentFileMutationResult::new(applied.mutation, hash)
}
