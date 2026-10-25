//! What `fs.write` and `fs.patch` return to
//! the model - the mutation, and the file's logical_hash after it.
use crate::{FsExternalWriteTarget, FsMutationDisposition, FsMutationResult};
use aura_domain_common::{ContentHash, DeferredMutationId, Revision, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgentFileMutationResult {
    pub disposition: FsMutationDisposition,
    pub workspace_revision: Revision,
    pub affected_paths: Vec<WorkspacePath>,
    pub deferred_mutation_id: Option<DeferredMutationId>,
    pub requires_reread: bool,
    pub catalog_refresh_pending: bool,
    #[serde(default)]
    pub external: Option<FsExternalWriteTarget>,
    /// The file's logical_hash after this change: `expected` for the next fs.write of the same
    /// file, `expected_hash` for its next fs.patch. Absent when nothing was written yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_hash: Option<ContentHash>,
}

impl AgentFileMutationResult {
    /// A mutation and the hash of the bytes it wrote; an external write names its own.
    #[must_use]
    pub fn new(result: FsMutationResult, logical_hash: Option<ContentHash>) -> Self {
        let logical_hash = logical_hash
            .or_else(|| result.external.as_ref().map(|target| target.logical_hash.clone()));
        Self {
            disposition: result.disposition,
            workspace_revision: result.workspace_revision,
            affected_paths: result.affected_paths,
            deferred_mutation_id: result.deferred_mutation_id,
            requires_reread: result.requires_reread,
            catalog_refresh_pending: result.catalog_refresh_pending,
            external: result.external,
            logical_hash,
        }
    }
}
