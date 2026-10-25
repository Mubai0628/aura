//! One-file writes and patches, each a changeset of one operation. The model's own calls also
//! get the hash of the bytes they wrote, the expected state of its next change of the file.
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
        Ok(for_agent(self.apply_changeset(write_changeset(request), correlation_id)?))
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

/// A deferred write has no step, and so no hash: the model reads the file again.
fn for_agent(applied: FsApplyChangesetResult) -> AgentFileMutationResult {
    let hash = applied.steps.first().and_then(|step| step.after_hash.clone());
    AgentFileMutationResult::new(applied.mutation, hash)
}
