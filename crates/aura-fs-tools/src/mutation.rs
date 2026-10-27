mod catalog;
#[cfg(test)]
use catalog::changeset_catalog_changes;
mod publication;
use publication::write_atomic;
mod copy_receipt;
use copy_receipt::{OwnMember, file_result, hash_held};
mod single;

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
};

use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, CorrelationId, DeferredMutationId, FileIdentity,
    MutationJournalId, SessionId, UtcTimestamp, WorkspaceId, WorkspacePath,
};
use aura_persistence::{DeferredMutationRecord, MutationAuditRecord, MutationOwnershipExecutionId};
use aura_windows_platform::{
    HandleBoundReplaceContent, HandlePublishError, NativeFileAccess, NativeFileIdentity,
    VerifiedFile, final_path_within, handle_bound_create_absent, handle_bound_move_existing,
    handle_bound_replace_with_backup_checked, held_file_identity, inspect_existing_path,
    open_held_for_move, open_verified_publish_parent,
};
// The platform keeps its own digest enumeration so it holds no dependency on the workspace's
// contract types. This module carries a caller's algorithm from the domain type onto it, and the
// alias keeps the two apart at every use site rather than only at the conversion.
use aura_windows_platform::ContentHashAlgorithm as PlatformHashAlgorithm;
use aura_workspace::{
    CatalogEntry, ResourceFacts, SecurePathResolver, WorkspaceError, decode_text, encode_text,
};
use aura_workspace_domain::{
    CommitRequest, ExecutionId, FileResource, FsApplyChangesetRequest, FsApplyChangesetResult,
    FsApplyPatchRequest, FsChangesetOperation, FsChangesetStepResult, FsCopyRequest,
    FsCreateDirectoryRequest, FsDeleteRequest, FsExpectedState, FsMoveRequest,
    FsMutationDisposition, FsMutationResult, FsWriteFileRequest, LineEnding, ResourceAccess,
    ResourceCommit, ResourceRequest, TextEncoding, WorkspaceFileTarget,
};
mod patch;
#[cfg(test)]
mod patch_tests;
use patch::prepare_patch_bytes;
pub(crate) use patch::{parse_patch, patched_bytes, prepare_diff, refuse_create_or_delete};
use serde_json::json;

use crate::error::io_error;
use crate::{FsToolError, FsToolService};

/// The real resources one changeset operation names, from the node types the resolver verified.
///
/// **The complete real resource set of one operation, each at its own access.**
///
/// # Why the scope is not the DTO's flags
///
/// A `recursive` flag is a caller's *intent*, not a fact about the tree. The confirmed rule is that
/// a directory operation covers the affected subtree "based on the real node type and read/write
/// scope": a recursive delete of something that is really a file still names only that file, and a
/// non-recursive delete of a directory still names the directory entry — while a move names both
/// ends regardless of what the caller believes is there.
///
/// # Why the access is per resource
///
/// The pre-fix form returned a `Vec<FileResource>` and the caller wrapped the whole thing in
/// `CommitRequest::many(resources, Write)`. That turned a **copy's source** — which this module's
/// own comment called a shared read — into a write claim, so two copies of one file serialised
/// against each other and a copy appeared to write its own source. Access is a property of what the
/// caller does to *one* resource, so it is stated with that resource.
///
/// # What each operation really covers
///
/// * a write or a patch names the object *and* the naming slot, because an existing-file change
///   protects both the bytes and the entry that will name the replacement;
/// * a create names the slot only — there is no object yet;
/// * a copy names the destination's slot and object as writes, and the **source as a read** at its
///   real node type: a directory source carries its whole subtree, because copying a directory
///   really reads every member and a writer anywhere inside it must be ordered;
/// * a move names **both ends**: each end's object and slot, and — when that end is really a
///   directory — that end's subtree, because moving a directory moves everything inside it;
/// * a delete names the object when something is really there, the slot always, and the subtree
///   **only when the target is really a directory** and the caller asked for it. A `recursive=true`
///   delete of something that is really a file is therefore a legal file delete and is not widened
///   to a tree that is not there — which is the case the pre-fix `directory_subtree` call rejected
///   outright.
///
/// A target that does not resolve yields no resource for the missing half rather than an invented
/// one: the resolver's own answer is the only thing that may create a resource fact.
fn changeset_resources(
    facts: &ResourceFacts,
    operation: &FsChangesetOperation,
) -> Result<Vec<ResourceRequest>, FsToolError> {
    let mut resources = Vec::new();
    match operation {
        FsChangesetOperation::WriteFile { target, .. }
        | FsChangesetOperation::ApplyPatch { target, .. } => {
            push_object_and_slot(facts, &target.path, &mut resources)?;
        }
        FsChangesetOperation::CreateDirectory { target } => {
            resources.push(write(facts.slot(&target.path)?));
        }
        FsChangesetOperation::Delete { target, recursive, .. } => {
            push_object_and_slot(facts, &target.path, &mut resources)?;
            // The subtree scope is taken only when the target really is a directory *and* the
            // caller asked for the tree. A `recursive` flag alone can neither widen the set to a
            // tree that is not there nor make a legal recursive file delete illegal.
            if *recursive && object_is_directory(facts, &target.path)? {
                resources.push(subtree_write(facts.directory_subtree(&target.path)?));
            }
        }
        FsChangesetOperation::Copy { source, destination, .. } => {
            push_object_and_slot(facts, &destination.path, &mut resources)?;
            // The source is **read**. A directory source additionally carries its whole subtree,
            // because the copy really reads every member under it.
            if let Some(object) = facts.object(&source.path)? {
                let is_directory = object_is_directory(facts, &source.path)?;
                resources.push(read(object));
                if is_directory {
                    resources.push(read(facts.directory_subtree(&source.path)?));
                }
            }
        }
        FsChangesetOperation::Move { source, destination, .. } => {
            push_move_end(facts, &source.path, &mut resources)?;
            push_move_end(facts, &destination.path, &mut resources)?;
        }
    }
    // An operation that resolved to the same resource twice — a move whose ends already name one
    // object — contributes it once, so the set describes distinct resources. The *widest* access
    // wins, so deduplication can never weaken a claim.
    let mut deduped: Vec<ResourceRequest> = Vec::with_capacity(resources.len());
    for request in resources {
        match deduped.iter_mut().find(|existing| existing.resource.same_resource(&request.resource))
        {
            Some(existing) => {
                if request.access > existing.access {
                    existing.access = request.access;
                }
            }
            None => deduped.push(request),
        }
    }
    Ok(deduped)
}

fn write(resource: FileResource) -> ResourceRequest {
    ResourceRequest::new(resource, ResourceAccess::Write)
}

fn read(resource: FileResource) -> ResourceRequest {
    ResourceRequest::new(resource, ResourceAccess::Read)
}

fn subtree_write(resource: FileResource) -> ResourceRequest {
    ResourceRequest::new(resource, ResourceAccess::SubtreeWrite)
}

/// Whether the target really is a directory, from the resolver's own answer.
///
/// Used instead of the caller's `recursive` flag so a scope is never taken for a tree that is not
/// there, and so a recursive delete of something that is really a file stays a file delete. An
/// absent target answers "not a directory": there is no node type to widen to.
fn object_is_directory(facts: &ResourceFacts, path: &WorkspacePath) -> Result<bool, FsToolError> {
    Ok(facts.is_directory(path)?)
}

/// One end of a move: its object and naming slot, plus its subtree when it really is a directory.
///
/// Both ends are covered because a move removes from one place and creates in another, and a
/// directory end moves everything inside it.
fn push_move_end(
    facts: &ResourceFacts,
    path: &WorkspacePath,
    resources: &mut Vec<ResourceRequest>,
) -> Result<(), FsToolError> {
    push_object_and_slot(facts, path, resources)?;
    if object_is_directory(facts, path)? {
        resources.push(subtree_write(facts.directory_subtree(path)?));
    }
    Ok(())
}

/// Adds the object at `path` (when one is really there) and the naming slot, both as writes.
///
/// Both, because "replacing a file" and "the entry that will name the replacement" are two real
/// things: a create-vs-delete-recreate race is a slot conflict, and a write-vs-write on existing
/// bytes is an object conflict. A caller that protected only one of them would let the other through.
fn push_object_and_slot(
    facts: &ResourceFacts,
    path: &WorkspacePath,
    resources: &mut Vec<ResourceRequest>,
) -> Result<(), FsToolError> {
    if let Some(object) = facts.object(path)? {
        resources.push(write(object));
    }
    resources.push(write(facts.slot(path)?));
    Ok(())
}

impl FsToolService {
    /// Writes Artifact bytes through the same Workspace transaction, editor
    /// deferral, Mutation Ownership, Journal, compensation, and Catalog path
    /// used by fs.* mutations. This is deliberately not a raw path writer.
    pub fn export_artifact_bytes(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        target: WorkspacePath,
        bytes: Vec<u8>,
        expected: FsExpectedState,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        let resolver = self.resolver(workspace_id)?;
        match self.buffers().active_editor(&resolver, &target) {
            Ok(Some(_owner)) => {
                let deferred_mutation_id = DeferredMutationId::new();
                self.persistence.record_deferred_mutation(DeferredMutationRecord {
                    deferred_mutation_id,
                    workspace_id,
                    session_id,
                    path: target.clone(),
                    expected_hash: expected_hash(&expected),
                    reason: "editor_activity_requires_artifact_export_reread".to_owned(),
                    created_at: UtcTimestamp::now(),
                })?;
                let revision = self.persistence.workspace(workspace_id)?.revision;
                self.workspace.notify_projection_change()?;
                return Ok(FsMutationResult {
                    disposition: FsMutationDisposition::DeferredByEditorActivity,
                    workspace_revision: revision,
                    affected_paths: vec![target],
                    deferred_mutation_id: Some(deferred_mutation_id),
                    requires_reread: true,
                    catalog_refresh_pending: false,
                    external: None,
                });
            }
            Ok(None) | Err(WorkspaceError::NotFound { .. }) => {}
            Err(error) => return Err(error.into()),
        }
        // An unrelated mutation must not save another session's unsaved work. This used to call
        // `flush_non_active_dirty_buffers`, which wrote every other session's dirty buffer to disk
        // as a side effect of exporting an artifact - committing content its owner had not decided
        // to commit, and racing this very target. The unsaved content is now reported as a fact
        // and left alone; only this caller's own target is written below.
        let export_operations = [FsChangesetOperation::WriteFile {
            target: WorkspaceFileTarget { path: target.clone() },
            content: String::new(),
            expected: expected.clone(),
            encoding: None,
            line_ending: None,
        }];
        if let Some(uncommitted) = self.defer_for_uncommitted_editors(
            workspace_id,
            session_id,
            &export_operations,
            &target,
        )? {
            let revision = self.persistence.workspace(workspace_id)?.revision;
            self.workspace.notify_projection_change()?;
            return Ok(FsMutationResult {
                disposition: FsMutationDisposition::DeferredByEditorActivity,
                workspace_revision: revision,
                affected_paths: vec![target],
                deferred_mutation_id: Some(uncommitted),
                requires_reread: true,
                catalog_refresh_pending: false,
                external: None,
            });
        }
        // One execution id for this call. Two concurrent exports of one session are two
        // executions with two rows, so neither one's release ends the other's ownership.
        let ownership = MutationOwnershipExecutionId::new();
        self.persistence.acquire_mutation_ownership_for_execution(
            workspace_id,
            session_id,
            ownership.clone(),
        )?;
        let result = (|| {
            let catalog = self.catalog(workspace_id)?;
            ensure_parent(&resolver, &target, &HashSet::new())?;
            let state = load_state(&resolver, &target)?;
            validate_expected(&target, &state, &expected)?;
            let after_hash = ContentHash::blake3(&bytes);
            let prepared = vec![PreparedOperation::Write {
                target: target.clone(),
                bytes,
                expected,
                existed: state.exists(),
                audits: vec![audit_seed(
                    "artifact_export",
                    target.clone(),
                    state.hash(),
                    Some(after_hash),
                    Some(&catalog),
                )],
            }];
            self.execute_prepared(
                &resolver,
                PreparedChangesetExecution {
                    workspace_id,
                    session_id,
                    prepared,
                    affected_paths: vec![target],
                    operation_name: "artifact.export",
                    correlation_id,
                },
            )
            .map(|result| result.mutation)
        })();
        let released = self.persistence.release_mutation_ownership_for_execution(
            workspace_id,
            session_id,
            ownership,
        );
        match (result, released) {
            (Ok(value), Ok(true)) => Ok(value),
            (Ok(_), Ok(false)) => Err(FsToolError::InvalidRequest(
                "workspace mutation ownership disappeared before release".to_owned(),
            )),
            (Ok(_), Err(error)) => Err(error.into()),
            (Err(error), _) => Err(error),
        }
    }

    pub fn create_directory(
        &self,
        request: FsCreateDirectoryRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        let result = self.apply_changeset(
            FsApplyChangesetRequest {
                workspace_id: request.workspace_id,
                session_id: request.session_id,
                operations: vec![FsChangesetOperation::CreateDirectory { target: request.target }],
            },
            correlation_id,
        )?;
        Ok(result.mutation)
    }

    pub fn copy(
        &self,
        request: FsCopyRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        let result = self.apply_changeset(
            FsApplyChangesetRequest {
                workspace_id: request.workspace_id,
                session_id: request.session_id,
                operations: vec![FsChangesetOperation::Copy {
                    source: request.source,
                    destination: request.destination,
                    source_expected: request.source_expected,
                    destination_expected: request.destination_expected,
                }],
            },
            correlation_id,
        )?;
        Ok(result.mutation)
    }

    pub fn move_path(
        &self,
        request: FsMoveRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        let result = self.apply_changeset(
            FsApplyChangesetRequest {
                workspace_id: request.workspace_id,
                session_id: request.session_id,
                operations: vec![FsChangesetOperation::Move {
                    source: request.source,
                    destination: request.destination,
                    source_expected: request.source_expected,
                    destination_expected: request.destination_expected,
                }],
            },
            correlation_id,
        )?;
        Ok(result.mutation)
    }

    pub fn delete(
        &self,
        request: FsDeleteRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsMutationResult, FsToolError> {
        let result = self.apply_changeset(
            FsApplyChangesetRequest {
                workspace_id: request.workspace_id,
                session_id: request.session_id,
                operations: vec![FsChangesetOperation::Delete {
                    target: request.target,
                    expected: request.expected,
                    recursive: request.recursive,
                }],
            },
            correlation_id,
        )?;
        Ok(result.mutation)
    }

    pub fn apply_changeset(
        &self,
        request: FsApplyChangesetRequest,
        correlation_id: CorrelationId,
    ) -> Result<FsApplyChangesetResult, FsToolError> {
        if request.operations.is_empty() {
            return Err(FsToolError::InvalidRequest(
                "changeset must contain at least one operation".to_owned(),
            ));
        }
        if request.operations.len() > self.policy.max_changeset_operations {
            return Err(FsToolError::InvalidRequest(format!(
                "changeset exceeds {} operations",
                self.policy.max_changeset_operations
            )));
        }
        let resolver = self.resolver(request.workspace_id)?;
        if let Some(deferred) = self.defer_for_editor_activity(
            &resolver,
            request.workspace_id,
            request.session_id,
            &request.operations,
        )? {
            if self.workspace.notify_projection_change().is_err() {
                return Err(FsToolError::ProjectionFailed {
                    revision: deferred.workspace_revision,
                });
            }
            return Ok(FsApplyChangesetResult { mutation: deferred, steps: Vec::new() });
        }
        // An unrelated mutation must not save another session's unsaved work. The former
        // `flush_non_active_dirty_buffers` here wrote every other session's dirty buffer in this
        // workspace to disk, so a changeset touching one file silently committed every other
        // session's editor state. Each target is checked instead, and a conflicting buffer is
        // reported rather than saved.
        for path in mutation_paths(&request.operations) {
            if let Some(deferred_mutation_id) = self.defer_for_uncommitted_editors(
                request.workspace_id,
                request.session_id,
                &request.operations,
                &path,
            )? {
                let revision = self.persistence.workspace(request.workspace_id)?.revision;
                if self.workspace.notify_projection_change().is_err() {
                    return Err(FsToolError::ProjectionFailed { revision });
                }
                return Ok(FsApplyChangesetResult {
                    mutation: FsMutationResult {
                        disposition: FsMutationDisposition::DeferredByEditorActivity,
                        workspace_revision: revision,
                        affected_paths: vec![path],
                        deferred_mutation_id: Some(deferred_mutation_id),
                        requires_reread: true,
                        catalog_refresh_pending: false,
                        external: None,
                    },
                    steps: Vec::new(),
                });
            }
        }
        // The **complete resource set** is assembled from the operations' real targets and admitted
        // as one unit by the one shared owner. This is the entry every Main Fs and Agent Fs mutation
        // funnels through — `write_file`, `apply_patch`, `create_directory`, `copy`, `move_path` and
        // `delete` all build a one-operation changeset and call here — so wiring this once covers
        // every one of them, and adding a per-tool owner is exactly what the brief forbids.
        //
        // The set is taken **before** the journal, the staging area and the first side effect, so a
        // refusal costs nothing and no partial state exists to unwind. A directory operation takes
        // its real subtree scope from the node type the resolver verified, not from the DTO's
        // `recursive` flag alone, and both ends of a move are in the set.
        //
        // This synchronous carrier makes one complete-set attempt. Only the async application
        // owner may wait and retry a ResourceBusy refusal, before any journal or file effect.
        let admission = self.admit_complete_set(&request)?;
        let Some(admission) = admission else {
            return Ok(FsApplyChangesetResult {
                mutation: self.deferred_by_holder_result(
                    request.workspace_id,
                    request.session_id,
                    &request.operations,
                )?,
                steps: Vec::new(),
            });
        };
        let _activity = crate::observe_file_execution(admission.requests());
        #[cfg(all(feature = "acceptance-barrier", debug_assertions))]
        crate::file_acceptance::hold_commit(
            request.workspace_id,
            request.session_id,
            correlation_id,
        );
        // One execution id for this changeset. A second concurrent changeset of the same session
        // is a separate execution with its own row, so a release cannot end the other's ownership.
        let ownership = MutationOwnershipExecutionId::new();
        if let Err(error) = self.persistence.acquire_mutation_ownership_for_execution(
            request.workspace_id,
            request.session_id,
            ownership.clone(),
        ) {
            // No journal or file effect has started. A failed ownership registration must not
            // leave an unreachable commit permanently excluding the complete resource set.
            self.eligibility_owner().release_commit(admission);
            return Err(error.into());
        }
        let result = self.execute_changeset(
            &resolver,
            request.workspace_id,
            request.session_id,
            request.operations,
            correlation_id,
        );
        let released = self.persistence.release_mutation_ownership_for_execution(
            request.workspace_id,
            request.session_id,
            ownership,
        );
        // The real work is over — or has failed — so the commit ends. It is released on every path:
        // the commit protects the *write*, and a write that did not happen holds nothing.
        self.eligibility_owner().release_commit(admission);
        match (result, released) {
            (Ok(value), Ok(true)) => Ok(value),
            (Ok(_), Ok(false)) => Err(FsToolError::InvalidRequest(
                "workspace mutation ownership disappeared before release".to_owned(),
            )),
            (Ok(_), Err(error)) => Err(error.into()),
            (Err(error), _) => Err(error),
        }
    }

    /// Attempts the complete resource set once, without sleeping on a blocking worker.
    ///
    /// Returns `Ok(Some(commit))` when the whole set was taken and `Ok(None)` when the deadline
    /// is a long editor claim. A short execution conflict is ResourceBusy: no journal or effect
    /// has begun, so the async owner can retry the unchanged request on its original budget.
    ///
    /// # Why the set is admitted here and not per operation
    ///
    /// "One full admission" is the confirmed rule: a multi-target operation must take all of its
    /// resources together rather than holding some while it waits for the rest, because a caller
    /// that took the first target and then waited on the second would deadlock against a caller
    /// doing the mirror image. It is also why an unrelated target stays parallel: the set contains
    /// only what this changeset really names.
    ///
    /// The resources are derived from the resolver's verified chain, each at the access this
    /// operation really needs on it, so they are the same facts Main Fs, the Agent tools, Buffer and
    /// the artifact writer all compare — one physical file is one object whichever entry reached it.
    fn admit_complete_set(
        &self,
        request: &FsApplyChangesetRequest,
    ) -> Result<Option<ResourceCommit>, FsToolError> {
        let facts = self.resource_facts(request.workspace_id, request.session_id)?;
        let mut resources = Vec::new();
        for operation in &request.operations {
            resources.extend(changeset_resources(&facts, operation)?);
        }
        if resources.is_empty() {
            return Ok(None);
        }
        let execution = ExecutionId::next();
        match self.eligibility_owner().commit(
            &CommitRequest::per_resource(resources),
            request.session_id,
            execution,
            None,
        ) {
            Ok(commit) => Ok(Some(commit)),
            Err(refusal) if refusal.retryable => {
                if self.eligibility_owner().is_edit_refusal(&refusal) {
                    Ok(None)
                } else {
                    Err(FsToolError::ResourceBusy(Box::new(refusal)))
                }
            }
            Err(refusal) => Err(FsToolError::InvalidRequest(format!(
                "the resource set was refused by the file protection owner: {refusal:?}"
            ))),
        }
    }

    /// The result a caller gets when another execution holds a resource it needs.
    ///
    /// It is the same `DeferredByEditorActivity` disposition the editor-activity path already
    /// returns, and for the same reason: the caller is told to re-read rather than being allowed to
    /// write over work it does not own. The deferred record is what makes the refusal inspectable
    /// instead of a bare error.
    fn deferred_by_holder_result(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        operations: &[FsChangesetOperation],
    ) -> Result<FsMutationResult, FsToolError> {
        let paths = mutation_paths(operations);
        let Some(path) = paths.first().cloned() else {
            return Err(FsToolError::InvalidRequest("changeset names no target".to_owned()));
        };
        let deferred_mutation_id = DeferredMutationId::new();
        self.persistence.record_deferred_mutation(DeferredMutationRecord {
            deferred_mutation_id,
            workspace_id,
            // The **real** requesting session, not an invented one: the deferred record is a
            // foreign-keyed row naming who must re-read, and a fabricated session both fails that
            // key and misattributes the deferral.
            session_id,
            path: path.clone(),
            // The caller's **original** condition is preserved. Writing None here — which is
            // what the previous form did — threw away the only record of what this caller had
            // decided the file should be, so the deferral could not be resolved against the
            // condition it was actually made under.
            expected_hash: expected_hash_for_path(operations, &path),
            reason: "another execution holds a conflicting edit eligibility on this resource"
                .to_owned(),
            created_at: UtcTimestamp::now(),
        })?;
        let revision = self.persistence.workspace(workspace_id)?.revision;
        self.workspace.notify_projection_change()?;
        Ok(FsMutationResult {
            disposition: FsMutationDisposition::DeferredByEditorActivity,
            workspace_revision: revision,
            affected_paths: paths,
            deferred_mutation_id: Some(deferred_mutation_id),
            requires_reread: true,
            catalog_refresh_pending: false,
            external: None,
        })
    }

    fn defer_for_editor_activity(
        &self,
        resolver: &SecurePathResolver,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        operations: &[FsChangesetOperation],
    ) -> Result<Option<FsMutationResult>, FsToolError> {
        for path in mutation_paths(operations) {
            match self.buffers().active_editor(resolver, &path) {
                Ok(Some(_owner)) => {
                    let deferred_mutation_id = DeferredMutationId::new();
                    self.persistence.record_deferred_mutation(DeferredMutationRecord {
                        deferred_mutation_id,
                        workspace_id,
                        session_id,
                        path: path.clone(),
                        expected_hash: expected_hash_for_path(operations, &path),
                        reason: "editor_activity_requires_reread".to_owned(),
                        created_at: UtcTimestamp::now(),
                    })?;
                    let revision = self.persistence.workspace(workspace_id)?.revision;
                    return Ok(Some(FsMutationResult {
                        disposition: FsMutationDisposition::DeferredByEditorActivity,
                        workspace_revision: revision,
                        affected_paths: vec![path],
                        deferred_mutation_id: Some(deferred_mutation_id),
                        requires_reread: true,
                        catalog_refresh_pending: false,
                        external: None,
                    }));
                }
                Ok(None) | Err(WorkspaceError::NotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }

    /// Reports a conflicting edit on a target, **consulting the common real resource**.
    ///
    /// # Why this no longer scans one workspace's path text
    ///
    /// The previous form asked the Buffer for the uncommitted buffers *of one workspace* and
    /// compared their `WorkspacePath` as text. Two things were wrong with that, and both are the
    /// measured defects of this batch:
    ///
    /// * the same physical object reached through a second workspace has a different relative path
    ///   there, so the text comparison missed a real conflict on one file; and
    /// * a same-named file in an unrelated workspace compared equal, so an unrelated buffer could
    ///   defer a caller that had nothing to do with it.
    ///
    /// The conflict is therefore asked of the **one shared eligibility owner**, on the real
    /// resource derived from the resolver's verified chain. The owner answers for every entry, so
    /// this reports a Main Fs write against a Buffer's eligibility and a Buffer's eligibility
    /// against an Agent tool, with no per-entry view involved.
    ///
    /// The caller's **original `expected` is preserved** in the deferred record. Writing `None` — as
    /// the previous form did — discarded the condition the caller had decided under, so the
    /// deferral could not be resolved against the very thing it deferred.
    ///
    /// A buffer belonging to the *calling* session is not reported: that session is the one making
    /// the decision, and its own write path already handles its buffer. The owner's own rule is what
    /// enforces that, because a claim this execution holds is never reported against it.
    ///
    /// # Why this probe takes and immediately releases a commit
    ///
    /// The question is "is anybody else on this real resource right now", and the only way to ask it
    /// of the one owner is to try. The probe therefore takes a commit and **always releases it**,
    /// whatever the answer: a probe that kept its commit would itself become the conflict it was
    /// looking for, and the caller's own real admission — which runs immediately after — would then
    /// be refused by its own probe. That is not a hypothetical; it is what a first version of this
    /// did, and the caller silently reported `DeferredByEditorActivity` for its own write.
    fn defer_for_uncommitted_editors(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        operations: &[FsChangesetOperation],
        target: &WorkspacePath,
    ) -> Result<Option<DeferredMutationId>, FsToolError> {
        // The real resource, from the same derivation every other entry uses.
        let facts = self.resource_facts(workspace_id, session_id)?;
        let Some(object) = facts.object(target)? else {
            // Nothing is there to hold an eligibility on: a first creation has no object to
            // conflict with, and its slot is protected by the commit below.
            return Ok(None);
        };
        let execution = ExecutionId::next();
        let probe = self.eligibility_owner().commit(
            &CommitRequest::one(object, ResourceAccess::Write),
            session_id,
            execution,
            None,
        );
        let holder = match probe {
            Ok(commit) => {
                // Nobody else was there. The probe ends here; the caller's own admission takes the
                // real commit.
                self.eligibility_owner().release_commit(commit);
                return Ok(None);
            }
            Err(refusal) if refusal.retryable => {
                if !self.eligibility_owner().is_edit_refusal(&refusal) {
                    return Ok(None);
                }
                refusal
            }
            // A non-retryable refusal is not "somebody holds it", it is a refusal this caller
            // cannot resolve by re-reading: report it rather than mislabelling it a deferral.
            Err(refusal) => {
                return Err(FsToolError::InvalidRequest(format!(
                    "the file protection owner refused this target: {refusal:?}"
                )));
            }
        };
        let deferred_mutation_id = DeferredMutationId::new();
        self.persistence.record_deferred_mutation(DeferredMutationRecord {
            deferred_mutation_id,
            workspace_id,
            session_id,
            path: target.clone(),
            expected_hash: expected_hash_for_path(operations, target),
            reason: format!(
                "another execution holds a conflicting edit eligibility on this real resource \
                 (held by {:?})",
                holder.held_by
            ),
            created_at: UtcTimestamp::now(),
        })?;
        Ok(Some(deferred_mutation_id))
    }

    fn execute_changeset(
        &self,
        resolver: &SecurePathResolver,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        operations: Vec<FsChangesetOperation>,
        correlation_id: CorrelationId,
    ) -> Result<FsApplyChangesetResult, FsToolError> {
        let catalog = self.catalog(workspace_id)?;
        let prepared = prepare_changeset(resolver, &catalog, &operations)?;
        self.execute_prepared(
            resolver,
            PreparedChangesetExecution {
                workspace_id,
                session_id,
                prepared,
                affected_paths: mutation_paths(&operations),
                operation_name: "fs.apply_changeset",
                correlation_id,
            },
        )
    }

    fn execute_prepared(
        &self,
        resolver: &SecurePathResolver,
        execution: PreparedChangesetExecution,
    ) -> Result<FsApplyChangesetResult, FsToolError> {
        let PreparedChangesetExecution {
            workspace_id,
            session_id,
            prepared,
            affected_paths,
            operation_name,
            correlation_id,
        } = execution;
        let catalog = self.catalog(workspace_id)?;
        let journal_id = MutationJournalId::new();
        let operation_trace = prepared.iter().map(PreparedOperation::trace).collect::<Vec<_>>();
        self.persistence.begin_workspace_journal(
            journal_id,
            workspace_id,
            session_id,
            operation_name,
            json!({ "operations": operation_trace, "correlation_id": correlation_id }),
        )?;
        let mut staging = StagingArea::create(resolver, journal_id)?;
        // The journal is a **record of what happened**, never a gate on doing the right thing. Every
        // update below therefore routes through `Journalled`, which keeps the write failure and
        // carries on: aborting on a journal error is what made the pre-fix code mask the real file
        // failure, skip the compensation it still owed and leave the journal stuck at `applying`.
        let mut journal = Journalled::default();
        journal.record(self.persistence.update_workspace_journal(
            journal_id,
            "applying",
            json!({
                "operations": operation_trace,
                "staging_path": staging.relative_root,
                "applied_steps": 0,
            }),
        ));
        let mut applied = Vec::with_capacity(prepared.len());
        let mut steps = Vec::with_capacity(prepared.len());
        for (index, operation) in prepared.iter().enumerate() {
            match staging.apply(resolver, operation) {
                Ok(applied_operation) => {
                    applied.push(applied_operation);
                    steps.push(FsChangesetStepResult {
                        index: u32::try_from(index).unwrap_or(u32::MAX),
                        operation: operation.kind_name().to_owned(),
                        paths: operation.paths(),
                        after_hash: operation.written_hash(),
                    });
                    journal.record(self.persistence.update_workspace_journal(
                        journal_id,
                        "applying",
                        json!({
                            "operations": operation_trace,
                            "staging_path": staging.relative_root,
                            "applied_steps": applied.len(),
                        }),
                    ));
                }
                Err(error) => {
                    // The material this execution has registered so far is snapshotted **before**
                    // compensation runs, so a step that failed after producing recovery material
                    // still has that material named even though it never entered `applied`.
                    let registered_material = staging.registered.clone();
                    journal.record(self.persistence.update_workspace_journal(
                        journal_id,
                        "compensating",
                        json!({
                            "operations": operation_trace,
                            "staging_path": staging.relative_root,
                            "applied_steps": applied.len(),
                            "failed_step": index,
                            "failure": error.to_string(),
                        }),
                    ));
                    let compensate_result =
                        staging.compensate(resolver, &applied, &registered_material);
                    return match compensate_result {
                        Ok(count) => {
                            journal.record(self.persistence.update_workspace_journal(
                                journal_id,
                                "compensated",
                                json!({
                                    "operations": operation_trace,
                                    "failed_step": index,
                                    "applied_steps": applied.len(),
                                    "compensated_steps": count,
                                }),
                            ));
                            Err(FsToolError::ChangesetFailed {
                                failed_step: index,
                                applied_steps: applied.len(),
                                compensated_steps: count,
                                cause: error.to_string(),
                                journal_fault: journal.fault_text(),
                            })
                        }
                        Err(compensation_error) => {
                            journal.record(self.persistence.update_workspace_journal(
                                journal_id,
                                "reconciliation_required",
                                json!({
                                    "operations": operation_trace,
                                    "staging_path": staging.relative_root,
                                    "failed_step": index,
                                    "failure": error.to_string(),
                                    "compensation_failure": compensation_error.to_string(),
                                }),
                            ));
                            Err(FsToolError::ReconciliationRequired(format!(
                                "step {index}: {error}; compensation: {compensation_error}{}",
                                journal.suffix()
                            )))
                        }
                    };
                }
            }
        }
        if let Err(error) = staging.finish(resolver, &applied) {
            journal.record(self.persistence.update_workspace_journal(journal_id, "reconciliation_required", json!({
                "operations": operation_trace, "applied_steps": applied.len(), "cleanup_failure": error.to_string(),
            })));
            return Err(FsToolError::ReconciliationRequired(format!(
                "applied file operations could not settle their own recovery material: {error}{}",
                journal.suffix()
            )));
        }
        self.persistence.update_workspace_journal(
            journal_id,
            "completed",
            json!({
                "operations": operation_trace,
                "applied_steps": applied.len(),
            }),
        )?;

        // The disk commit already happened, so a failure to index is recorded as a
        // recoverable follow-up rather than reported as a failed write that
        // invites the model to replay it.
        //
        // Only the directories this changeset actually touched are re-read, so a
        // write no longer costs a full workspace rescan and a cursor in an
        // unrelated directory keeps its meaning.
        let catalog_refresh_pending = self
            .refresh_catalog_after_mutation(
                &catalog,
                resolver,
                &prepared,
                &affected_paths,
                operation_name,
                correlation_id,
            )
            .is_err();
        if catalog_refresh_pending {
            self.persistence.mark_catalog_stale(workspace_id)?;
        }
        for operation in &prepared {
            for audit in operation.audits(workspace_id, session_id, &catalog)? {
                self.persistence.audit_workspace_mutation(audit)?;
            }
        }
        let mut revision = self.persistence.complete_workspace_file_mutation(
            workspace_id,
            correlation_id,
            operation_name,
        )?;
        if !catalog_refresh_pending {
            revision = self.persistence.workspace(workspace_id)?.revision;
        }
        if self.workspace.notify_projection_change().is_err() {
            return Err(FsToolError::ProjectionFailed { revision });
        }
        Ok(FsApplyChangesetResult {
            mutation: FsMutationResult {
                disposition: FsMutationDisposition::Applied,
                workspace_revision: revision,
                affected_paths,
                deferred_mutation_id: None,
                requires_reread: false,
                catalog_refresh_pending,
                external: None,
            },
            steps,
        })
    }
}

fn mutation_paths(operations: &[FsChangesetOperation]) -> Vec<WorkspacePath> {
    let mut paths = operations
        .iter()
        .flat_map(|operation| match operation {
            FsChangesetOperation::WriteFile { target, .. }
            | FsChangesetOperation::ApplyPatch { target, .. }
            | FsChangesetOperation::CreateDirectory { target }
            | FsChangesetOperation::Delete { target, .. } => vec![target.path.clone()],
            FsChangesetOperation::Copy { source, destination, .. }
            | FsChangesetOperation::Move { source, destination, .. } => {
                vec![source.path.clone(), destination.path.clone()]
            }
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn expected_hash_for_path(
    operations: &[FsChangesetOperation],
    path: &WorkspacePath,
) -> Option<ContentHash> {
    operations.iter().find_map(|operation| match operation {
        FsChangesetOperation::WriteFile { target, expected, .. } if target.path == *path => {
            expected_hash(expected)
        }
        FsChangesetOperation::ApplyPatch { target, expected_hash, .. } if target.path == *path => {
            Some(expected_hash.clone())
        }
        FsChangesetOperation::Copy {
            source,
            destination,
            source_expected,
            destination_expected,
        }
        | FsChangesetOperation::Move {
            source,
            destination,
            source_expected,
            destination_expected,
        } => {
            if source.path == *path {
                expected_hash(source_expected)
            } else if destination.path == *path {
                expected_hash(destination_expected)
            } else {
                None
            }
        }
        FsChangesetOperation::Delete { target, expected, .. } if target.path == *path => {
            expected_hash(expected)
        }
        _ => None,
    })
}

fn expected_hash(expected: &FsExpectedState) -> Option<ContentHash> {
    match expected {
        FsExpectedState::FileHash { hash } => Some(hash.clone()),
        FsExpectedState::Absent | FsExpectedState::Directory => None,
    }
}

#[derive(Clone)]
pub(crate) enum ExistingState {
    Absent,
    File { bytes: Vec<u8>, hash: ContentHash },
    Directory,
}

impl ExistingState {
    pub(crate) fn hash(&self) -> Option<ContentHash> {
        match self {
            Self::File { hash, .. } => Some(hash.clone()),
            Self::Absent | Self::Directory => None,
        }
    }

    fn is_directory(&self) -> bool {
        matches!(self, Self::Directory)
    }

    fn exists(&self) -> bool {
        !matches!(self, Self::Absent)
    }

    /// The state of bytes that were read from an already-verified handle. External
    /// targets load their state this way, so both paths compare and encode with the
    /// same rules.
    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
        let hash = ContentHash::blake3(&bytes);
        Self::File { bytes, hash }
    }
}

struct AuditSeed {
    operation_kind: &'static str,
    path: WorkspacePath,
    before_hash: Option<ContentHash>,
    after_hash: Option<ContentHash>,
    before_entry: Option<CatalogEntry>,
}

struct PreparedChangesetExecution {
    workspace_id: WorkspaceId,
    session_id: SessionId,
    prepared: Vec<PreparedOperation>,
    affected_paths: Vec<WorkspacePath>,
    operation_name: &'static str,
    correlation_id: CorrelationId,
}

enum PreparedOperation {
    Write {
        target: WorkspacePath,
        bytes: Vec<u8>,
        expected: FsExpectedState,
        existed: bool,
        audits: Vec<AuditSeed>,
    },
    CreateDirectory {
        target: WorkspacePath,
        audits: Vec<AuditSeed>,
    },
    Copy {
        source: WorkspacePath,
        destination: WorkspacePath,
        source_expected: FsExpectedState,
        destination_expected: FsExpectedState,
        source_is_directory: bool,
        destination_existed: bool,
        audits: Vec<AuditSeed>,
    },
    Move {
        source: WorkspacePath,
        destination: WorkspacePath,
        source_expected: FsExpectedState,
        destination_expected: FsExpectedState,
        destination_existed: bool,
        audits: Vec<AuditSeed>,
    },
    Delete {
        target: WorkspacePath,
        expected: FsExpectedState,
        recursive: bool,
        audits: Vec<AuditSeed>,
    },
}

impl PreparedOperation {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Write { .. } => "write_file",
            Self::CreateDirectory { .. } => "create_directory",
            Self::Copy { .. } => "copy",
            Self::Move { .. } => "move",
            Self::Delete { .. } => "delete",
        }
    }

    fn paths(&self) -> Vec<WorkspacePath> {
        match self {
            Self::Write { target, .. }
            | Self::CreateDirectory { target, .. }
            | Self::Delete { target, .. } => vec![target.clone()],
            Self::Copy { source, destination, .. } | Self::Move { source, destination, .. } => {
                vec![source.clone(), destination.clone()]
            }
        }
    }

    fn trace(&self) -> serde_json::Value {
        json!({ "operation": self.kind_name(), "paths": self.paths() })
    }

    fn audit_seeds(&self) -> &[AuditSeed] {
        match self {
            Self::Write { audits, .. }
            | Self::CreateDirectory { audits, .. }
            | Self::Copy { audits, .. }
            | Self::Move { audits, .. }
            | Self::Delete { audits, .. } => audits,
        }
    }

    fn audits(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        catalog: &aura_workspace::PathCatalog,
    ) -> Result<Vec<MutationAuditRecord>, FsToolError> {
        self.audit_seeds()
            .iter()
            .map(|seed| {
                // A path below an ignored folder has no entry; the folder decides its flags.
                let (ignored, generated) =
                    match catalog.entry(&seed.path)?.or_else(|| seed.before_entry.clone()) {
                        Some(entry) => (entry.ignored, entry.generated),
                        None => catalog.flags_beyond_index(&seed.path)?,
                    };
                Ok(MutationAuditRecord {
                    workspace_id,
                    session_id,
                    operation_kind: seed.operation_kind.to_owned(),
                    path: seed.path.clone(),
                    ignored,
                    generated,
                    before_hash: seed.before_hash.clone(),
                    after_hash: seed.after_hash.clone(),
                })
            })
            .collect()
    }
}

fn prepare_changeset(
    resolver: &SecurePathResolver,
    catalog: &aura_workspace::PathCatalog,
    operations: &[FsChangesetOperation],
) -> Result<Vec<PreparedOperation>, FsToolError> {
    let mut prepared = Vec::with_capacity(operations.len());
    let mut mutation_targets = HashMap::<String, bool>::new();
    let mut planned_directories = HashSet::<WorkspacePath>::new();
    for operation in operations {
        match operation {
            FsChangesetOperation::WriteFile {
                target,
                content,
                expected,
                encoding,
                line_ending,
            } => {
                register_target(&mut mutation_targets, &target.path, false)?;
                ensure_parent(resolver, &target.path, &planned_directories)?;
                let state = load_state(resolver, &target.path)?;
                validate_expected(&target.path, &state, expected)?;
                let bytes =
                    prepare_write_bytes(&target.path, &state, content, *encoding, *line_ending)?;
                let after_hash = ContentHash::blake3(&bytes);
                prepared.push(PreparedOperation::Write {
                    target: target.path.clone(),
                    bytes,
                    expected: expected.clone(),
                    existed: state.exists(),
                    audits: vec![audit_seed(
                        "write_file",
                        target.path.clone(),
                        state.hash(),
                        Some(after_hash),
                        Some(catalog),
                    )],
                });
            }
            FsChangesetOperation::ApplyPatch { target, unified_diff, expected_hash } => {
                register_target(&mut mutation_targets, &target.path, false)?;
                ensure_parent(resolver, &target.path, &planned_directories)?;
                let state = load_state(resolver, &target.path)?;
                validate_expected(
                    &target.path,
                    &state,
                    &FsExpectedState::FileHash { hash: expected_hash.clone() },
                )?;
                let bytes = prepare_patch_bytes(&target.path, &state, unified_diff)?;
                let after_hash = ContentHash::blake3(&bytes);
                prepared.push(PreparedOperation::Write {
                    target: target.path.clone(),
                    bytes,
                    expected: FsExpectedState::FileHash { hash: expected_hash.clone() },
                    existed: true,
                    audits: vec![audit_seed(
                        "apply_patch",
                        target.path.clone(),
                        state.hash(),
                        Some(after_hash),
                        Some(catalog),
                    )],
                });
            }
            FsChangesetOperation::CreateDirectory { target } => {
                register_target(&mut mutation_targets, &target.path, true)?;
                ensure_parent(resolver, &target.path, &planned_directories)?;
                let state = load_state(resolver, &target.path)?;
                validate_expected(&target.path, &state, &FsExpectedState::Absent)?;
                planned_directories.insert(target.path.clone());
                prepared.push(PreparedOperation::CreateDirectory {
                    target: target.path.clone(),
                    audits: vec![audit_seed(
                        "create_directory",
                        target.path.clone(),
                        None,
                        None,
                        Some(catalog),
                    )],
                });
            }
            FsChangesetOperation::Copy {
                source,
                destination,
                source_expected,
                destination_expected,
            } => {
                reject_source_changed_by_prior_step(&mutation_targets, &source.path)?;
                register_target(&mut mutation_targets, &destination.path, false)?;
                ensure_parent(resolver, &destination.path, &planned_directories)?;
                let source_state = load_state(resolver, &source.path)?;
                validate_source_expected(&source.path, &source_state, source_expected)?;
                let destination_state = load_state(resolver, &destination.path)?;
                validate_expected(&destination.path, &destination_state, destination_expected)?;
                reject_directory_into_itself(
                    resolver,
                    &source.path,
                    &source_state,
                    &destination.path,
                    &planned_directories,
                )?;
                let after_hash = source_state.hash();
                prepared.push(PreparedOperation::Copy {
                    source: source.path.clone(),
                    destination: destination.path.clone(),
                    source_expected: source_expected.clone(),
                    destination_expected: destination_expected.clone(),
                    source_is_directory: source_state.is_directory(),
                    destination_existed: destination_state.exists(),
                    audits: vec![audit_seed(
                        "copy",
                        destination.path.clone(),
                        destination_state.hash(),
                        after_hash,
                        Some(catalog),
                    )],
                });
            }
            FsChangesetOperation::Move {
                source,
                destination,
                source_expected,
                destination_expected,
            } => {
                register_target(&mut mutation_targets, &source.path, false)?;
                register_target(&mut mutation_targets, &destination.path, false)?;
                ensure_parent(resolver, &destination.path, &planned_directories)?;
                let source_state = load_state(resolver, &source.path)?;
                validate_source_expected(&source.path, &source_state, source_expected)?;
                let destination_state = load_state(resolver, &destination.path)?;
                validate_expected(&destination.path, &destination_state, destination_expected)?;
                reject_directory_into_itself(
                    resolver,
                    &source.path,
                    &source_state,
                    &destination.path,
                    &planned_directories,
                )?;
                prepared.push(PreparedOperation::Move {
                    source: source.path.clone(),
                    destination: destination.path.clone(),
                    source_expected: source_expected.clone(),
                    destination_expected: destination_expected.clone(),
                    destination_existed: destination_state.exists(),
                    audits: vec![
                        audit_seed(
                            "move_source",
                            source.path.clone(),
                            source_state.hash(),
                            None,
                            Some(catalog),
                        ),
                        audit_seed(
                            "move_destination",
                            destination.path.clone(),
                            destination_state.hash(),
                            source_state.hash(),
                            Some(catalog),
                        ),
                    ],
                });
            }
            FsChangesetOperation::Delete { target, expected, recursive } => {
                register_target(&mut mutation_targets, &target.path, false)?;
                let state = load_state(resolver, &target.path)?;
                validate_expected(&target.path, &state, expected)?;
                if state.is_directory()
                    && !recursive
                    && directory_not_empty(resolver, &target.path)?
                {
                    return Err(FsToolError::InvalidRequest(format!(
                        "directory {} is not empty; recursive=true is required",
                        target.path
                    )));
                }
                prepared.push(PreparedOperation::Delete {
                    target: target.path.clone(),
                    expected: expected.clone(),
                    recursive: *recursive,
                    audits: vec![audit_seed(
                        "delete",
                        target.path.clone(),
                        state.hash(),
                        None,
                        Some(catalog),
                    )],
                });
            }
        }
    }
    Ok(prepared)
}

fn register_target(
    targets: &mut HashMap<String, bool>,
    path: &WorkspacePath,
    directory_creation: bool,
) -> Result<(), FsToolError> {
    reject_root(path)?;
    let folded = path.as_str().to_lowercase();
    if targets.contains_key(&folded) {
        return Err(FsToolError::InvalidRequest(format!(
            "changeset mutates {} more than once",
            path
        )));
    }
    for (existing, existing_is_directory_creation) in targets.iter() {
        if is_path_ancestor(existing, &folded) && !*existing_is_directory_creation {
            return Err(FsToolError::InvalidRequest(format!(
                "changeset has overlapping mutation targets: {existing} and {path}"
            )));
        }
        if is_path_ancestor(&folded, existing) && !directory_creation {
            return Err(FsToolError::InvalidRequest(format!(
                "changeset has overlapping mutation targets: {path} and {existing}"
            )));
        }
    }
    targets.insert(folded, directory_creation);
    Ok(())
}

fn reject_source_changed_by_prior_step(
    targets: &HashMap<String, bool>,
    source: &WorkspacePath,
) -> Result<(), FsToolError> {
    let folded = source.as_str().to_lowercase();
    if targets.keys().any(|target| target == &folded || is_path_ancestor(target, &folded)) {
        return Err(FsToolError::InvalidRequest(format!(
            "changeset reads {source} after an earlier step mutates it or an ancestor"
        )));
    }
    Ok(())
}

fn is_path_ancestor(parent: &str, child: &str) -> bool {
    child.len() > parent.len()
        && child.starts_with(parent)
        && child.as_bytes().get(parent.len()) == Some(&b'/')
}

fn reject_root(path: &WorkspacePath) -> Result<(), FsToolError> {
    if path.is_root() {
        return Err(FsToolError::InvalidRequest(
            "workspace root cannot be a mutation target".to_owned(),
        ));
    }
    Ok(())
}

fn parent_path(path: &WorkspacePath) -> Result<WorkspacePath, FsToolError> {
    let parent = path.as_str().rsplit_once('/').map_or(WorkspacePath::ROOT, |(parent, _)| parent);
    WorkspacePath::parse(parent)
        .map_err(|error| FsToolError::InvalidRequest(format!("invalid parent path: {error}")))
}

fn ensure_parent(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
    planned_directories: &HashSet<WorkspacePath>,
) -> Result<(), FsToolError> {
    let parent = parent_path(path)?;
    if planned_directories.contains(&parent) {
        return Ok(());
    }
    resolver.validate_parent(path)?;
    Ok(())
}

fn load_state(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
) -> Result<ExistingState, FsToolError> {
    match resolver.open_existing(path, NativeFileAccess::Read) {
        Ok(mut verified) => {
            if verified.file.info().is_directory {
                return Ok(ExistingState::Directory);
            }
            let mut bytes = Vec::new();
            verified.file.file_mut().read_to_end(&mut bytes).map_err(|source| {
                io_error("read verified mutation target", &verified.file.info().final_path, source)
            })?;
            let hash = ContentHash::blake3(&bytes);
            Ok(ExistingState::File { bytes, hash })
        }
        Err(WorkspaceError::NotFound { .. }) => Ok(ExistingState::Absent),
        Err(error) => Err(error.into()),
    }
}

/// The one expected-state comparison. External targets reuse it so an
/// `expected_state` conflict means the same thing wherever the file lives.
pub(crate) fn validate_expected(
    path: &WorkspacePath,
    state: &ExistingState,
    expected: &FsExpectedState,
) -> Result<(), FsToolError> {
    match (expected, state) {
        (FsExpectedState::Absent, ExistingState::Absent)
        | (FsExpectedState::Directory, ExistingState::Directory) => Ok(()),
        (FsExpectedState::Absent, _) => Err(FsToolError::ExpectedAbsent { path: path.clone() }),
        (FsExpectedState::Directory, _) => {
            Err(FsToolError::ExpectedDirectory { path: path.clone() })
        }
        (
            FsExpectedState::FileHash { hash: expected },
            ExistingState::File { hash: actual, .. },
        ) if expected == actual => Ok(()),
        (
            FsExpectedState::FileHash { hash: expected },
            ExistingState::File { hash: actual, .. },
        ) => Err(FsToolError::ExpectedHash {
            path: path.clone(),
            expected: expected.clone(),
            actual: actual.clone(),
        }),
        (FsExpectedState::FileHash { .. }, ExistingState::Absent | ExistingState::Directory) => {
            Err(FsToolError::InvalidRequest(format!("{} is not the expected file", path)))
        }
    }
}

fn validate_source_expected(
    path: &WorkspacePath,
    state: &ExistingState,
    expected: &FsExpectedState,
) -> Result<(), FsToolError> {
    if matches!(expected, FsExpectedState::Absent) {
        return Err(FsToolError::InvalidRequest(format!(
            "source {} cannot use expected_state=absent",
            path
        )));
    }
    validate_expected(path, state, expected)
}

/// The one encoding decision: an existing file keeps its detected encoding and
/// line ending unless the caller overrides them, and a new file must state both.
/// External targets reuse it so a written file is encoded the same way wherever it
/// lives.
pub(crate) fn write_encoding(
    path: &WorkspacePath,
    state: &ExistingState,
    encoding: Option<TextEncoding>,
    line_ending: Option<LineEnding>,
) -> Result<(TextEncoding, LineEnding), FsToolError> {
    match state {
        ExistingState::File { bytes, .. } => {
            let existing = decode_text(bytes)?;
            Ok((encoding.unwrap_or(existing.encoding), line_ending.unwrap_or(existing.line_ending)))
        }
        ExistingState::Absent => Ok((
            encoding.ok_or_else(|| {
                FsToolError::InvalidRequest(format!(
                    "new file {} requires an explicit encoding",
                    path
                ))
            })?,
            line_ending.ok_or_else(|| {
                FsToolError::InvalidRequest(format!(
                    "new file {} requires an explicit line ending",
                    path
                ))
            })?,
        )),
        ExistingState::Directory => {
            Err(FsToolError::InvalidRequest(format!("{} is a directory", path)))
        }
    }
}

pub(crate) fn prepare_write_bytes(
    path: &WorkspacePath,
    state: &ExistingState,
    content: &str,
    encoding: Option<TextEncoding>,
    line_ending: Option<LineEnding>,
) -> Result<Vec<u8>, FsToolError> {
    let (encoding, line_ending) = write_encoding(path, state, encoding, line_ending)?;
    encode_text(content, encoding, line_ending).map_err(Into::into)
}

fn directory_not_empty(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
) -> Result<bool, FsToolError> {
    let resolved = resolver.resolve_existing(path)?;
    let mut entries = fs::read_dir(&resolved.info.final_path).map_err(|source| {
        io_error("enumerate directory before delete", &resolved.info.final_path, source)
    })?;
    Ok(entries
        .next()
        .transpose()
        .map_err(|source| {
            io_error("read directory entry before delete", &resolved.info.final_path, source)
        })?
        .is_some())
}

fn reject_directory_into_itself(
    resolver: &SecurePathResolver,
    source: &WorkspacePath,
    source_state: &ExistingState,
    destination: &WorkspacePath,
    planned_directories: &HashSet<WorkspacePath>,
) -> Result<(), FsToolError> {
    if !source_state.is_directory() {
        return Ok(());
    }
    if is_path_ancestor(&source.as_str().to_lowercase(), &destination.as_str().to_lowercase()) {
        return Err(FsToolError::InvalidRequest(
            "directory cannot be copied or moved into itself".to_owned(),
        ));
    }
    let parent = parent_path(destination)?;
    if planned_directories.contains(&parent) {
        return Ok(());
    }
    let source_info = resolver.resolve_existing(source)?.info;
    let parent_info = resolver.open_existing(&parent, NativeFileAccess::Metadata)?.file;
    if final_path_within(&source_info, parent_info.info()) {
        return Err(FsToolError::InvalidRequest(
            "directory destination resolves inside its source".to_owned(),
        ));
    }
    Ok(())
}

fn audit_seed(
    operation_kind: &'static str,
    path: WorkspacePath,
    before_hash: Option<ContentHash>,
    after_hash: Option<ContentHash>,
    catalog: Option<&aura_workspace::PathCatalog>,
) -> AuditSeed {
    let before_entry = catalog.and_then(|value| value.entry(&path).ok().flatten());
    AuditSeed { operation_kind, path, before_hash, after_hash, before_entry }
}

/// The journal writes one changeset attempted, and the ones that failed.
///
/// # Why the journal cannot gate the work
///
/// The pre-fix code wrote each journal transition with `?`, so a real SQLite refusal of the
/// `compensating` update **aborted the function** at that line. The consequences were measured on
/// the real runtime: the caller received only the journal error, the created directory stayed on
/// disk, no compensation ran, and the journal was left at `applying`. The primary cause — the file
/// failure that had already happened — was never reported, and work the execution still owed was
/// skipped because a *record* of the work could not be written.
///
/// So every transition is recorded here instead. The first failure is kept and appended to whatever
/// the caller is finally told, the operation continues to the end of its effect responsibility, and
/// the returned error distinguishes the primary failure from the journal fault rather than letting
/// either hide the other.
#[derive(Default)]
struct Journalled {
    failures: Vec<String>,
}

impl Journalled {
    /// Keeps one transition outcome. An `Err` is kept; an `Ok` is the ordinary case.
    fn record(&mut self, outcome: Result<(), aura_persistence::PersistenceError>) {
        if let Err(error) = outcome {
            self.failures.push(error.to_string());
        }
    }

    /// The additive text for an error that already carries its own cause.
    ///
    /// Empty when every write landed, so an ordinary failure's message is unchanged and a real
    /// journal fault is visible as an added fact rather than as a replacement for the cause.
    fn suffix(&self) -> String {
        if self.failures.is_empty() {
            return String::new();
        }
        format!(
            "; {} journal write(s) were refused, so the change journal does not record this \
             operation: {}",
            self.failures.len(),
            self.failures.join(" | ")
        )
    }

    /// The same fact, named for the field that carries it on [`FsToolError::ChangesetFailed`].
    fn fault_text(&self) -> String {
        self.suffix()
    }
}

#[derive(Clone)]
struct StagedBackup {
    original: WorkspacePath,
    staged: WorkspacePath,
}

/// One staged backup and the real identity of the object it holds.
///
/// The identity is recorded at the moment of staging, because that is the last instant the object
/// the caller decided about is known: after the move the destination name is empty, and a later
/// re-read could only observe whatever a competitor put there.
#[derive(Clone)]
struct StagedObject {
    backup: StagedBackup,
    identity: Option<NativeFileIdentity>,
    condition: HandleCondition,
    members: Option<Vec<OwnMember>>,
}

/// What one operation really published, in the form a compensation can prove ownership with.
///
/// A path alone is not proof of ownership: the path says where something is, never whose it is. So
/// each applied operation records what this execution really produced, and the compensation
/// compares that against what is there when it runs:
///
/// * a **file** result carries the real `FileIdentity` **and** the digest of the bytes this execution wrote. The
///   identity alone is not enough: Windows keeps the file id across an in-place rewrite, so a
///   third party that rewrote the same object in place would still "be" this execution's file by
///   identity. The digest records the same content condition the publication itself was made under.
/// * a **directory** result carries the identity this execution observed for the directory it
///   created, when it could establish one. A directory has no content condition, so its emptiness
///   is the second fact; both are required.
#[derive(Clone, Eq, PartialEq)]
enum PublishedResult {
    File { identity: FileIdentity, hash: ContentHash },
    Directory { identity: Option<FileIdentity> },
}

/// What one applied operation really produced, so its compensation can prove what it may remove.
enum AppliedOperation {
    Write {
        target: WorkspacePath,
        backup: Option<StagedObject>,
        published: PublishedResult,
    },
    CreateDirectory {
        target: WorkspacePath,
        published: PublishedResult,
    },
    Copy {
        destination: WorkspacePath,
        backup: Option<StagedObject>,
        published: PublishedResult,
        /// The directory members this copy really created under its own destination.
        ///
        /// A copy of a directory tree produces members, and a compensation that removed the whole
        /// tree would sweep away anything a third party added alongside them. Keeping the members
        /// this execution made — and only those — is what lets a partial rollback remove its own
        /// material while a third-party member stops it. `None` means the copy produced a file
        /// result, whose identity and bytes are the whole proof.
        own_members: Option<Vec<OwnMember>>,
    },
    Move {
        source: WorkspacePath,
        destination: WorkspacePath,
        destination_backup: Option<StagedObject>,
        /// What this move really placed at `destination`.
        ///
        /// The compensation consumes **this**, not the destination path: the previous form ignored
        /// the recorded result and moved whatever was at `destination` back to `source`, which is
        /// how a competitor's object was relocated. With the recorded result the compensation can
        /// prove the object it is about to move back is the one this execution put there.
        published: PublishedResult,
        /// The source's own result before the move, so a restore can prove what it is restoring.
        source_before: Option<FileIdentity>,
        own_members: Option<Vec<OwnMember>>,
    },
    Delete {
        backup: StagedObject,
    },
}

/// Opens one object so the caller can hold it across the staging move.
///
/// # Why the hold has to exclude writers
///
/// A rename is a **path** operation: it carries whatever object the name currently refers to. So the
/// only way a caller's content decision can still be true at the instant of the move is for the
/// object to be unable to change between them — and on Windows that means holding a handle that
/// denies share-write and share-delete. With that handle alive, an external writer's in-place
/// rewrite, delete-and-recreate swap or racing create all fail at their own `CreateFileW` instead of
/// winning a window this code cannot observe.
///
/// The handle is closed immediately after the move, so the exclusion spans exactly the verified
/// condition and the move it protects.
fn open_held_for_staging(absolute: &Path) -> Result<VerifiedFile, FsToolError> {
    open_held_for_move(absolute).map_err(|source| {
        // A refusal here is a **real** condition — another writer already holds the object — and is
        // reported as such rather than being read as "the object is absent".
        io_error("hold workspace object for the staged move", absolute, source)
    })
}

/// The domain identity of whatever object `path` currently names, if one is there.
///
/// Used where a compensation needs to record what an operation started from, read through the
/// resolver's verified handle rather than from a planner's earlier observation.
fn source_state_identity(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
) -> Option<FileIdentity> {
    let resolved = resolver.resolve_existing(path).ok()?;
    Some(domain_identity_of(&resolved.info.identity))
}

/// Named, default-off interference points inside the staging area's own operations.
///
/// # Why these exist and what they are not
///
/// Several of this batch's required facts are about what a **compensation** does when the world
/// changed while the operation was between its steps: a third party adding a member to a directory
/// this execution just created, or replacing this execution's result. A test cannot arrange that
/// from outside without racing the operation, and re-implementing the operation would prove nothing
/// about the product.
///
/// So a test installs an action that runs against the **real path**, inside the real staging area,
/// after the real side effect and before the result is recorded. The action cannot change the
/// operation's outcome — the side effect already happened and the production code continues exactly
/// as it would — so every assertion afterwards is about how the production compensation really
/// behaved.
///
/// Nothing is armed unless a caller arms it: with no action installed the production path pays one
/// relaxed atomic load and returns.
pub mod staging_fault {
    use std::{
        path::{Path, PathBuf},
        sync::{
            Mutex, MutexGuard,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
    };

    /// One step of a staging operation a test may interfere with.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Stage {
        /// A directory has really been created under its destination name.
        DirectoryCreated,
        /// A file result has really been published under its destination name.
        FilePublished,
        /// The held object has moved; its kernel identity and digest have been recorded.
        ObjectMoved,
        /// **After the caller's original condition has been verified and before the real move.**
        ///
        /// This is the exact window the batch requires to be closed: the object is still on disk
        /// under its own name, its identity and bytes have just been checked against the caller's
        /// `expected`, and the staging rename has not happened yet. An action installed here runs
        /// on the **real object** at the real instant, so what it does to the file is what an
        /// independent external writer could really do to it — an in-place rewrite that keeps the
        /// file id, or a swap that replaces the object outright.
        ///
        /// The pre-fix code released its read handle at `verify_staged_condition` and then renamed
        /// by path, so an action here would always win. Reaching this point and observing a refusal
        /// is therefore the measurement that the window is closed, and the same action must be
        /// *unable* to make a correct-condition control fail.
        BeforeStagedMove,
        /// The compensation verified its held result and is about to delete that same object.
        BeforeCompensationDelete,
        /// A directory copy's original tree failed to be removed **partially**, leaving both the
        /// remaining original material and the complete backup at the staging path.
        ///
        /// This is the case that decides whether the current failing step's recovery material
        /// survives. It can only be arranged inside the operation, because it depends on which
        /// member of the original tree the removal stopped at.
        PartialOriginalRemoval,
    }

    impl Stage {
        /// The stable name a test arms this stage by.
        #[must_use]
        pub const fn name(self) -> &'static str {
            match self {
                Self::DirectoryCreated => "directory_created",
                Self::FilePublished => "file_published",
                Self::ObjectMoved => "object_moved",
                Self::BeforeStagedMove => "before_staged_move",
                Self::BeforeCompensationDelete => "before_compensation_delete",
                Self::PartialOriginalRemoval => "partial_original_removal",
            }
        }
    }

    type Action = std::sync::Arc<dyn Fn(&Path) + Send + Sync>;

    struct Registration {
        id: u64,
        stage: &'static str,
        only: Option<PathBuf>,
        action: Action,
        /// Whether this registration also asks a directory staging step to take the
        /// **copy-then-remove** route, which production otherwise reaches only when the watcher
        /// holds the tree open. See [`forced_directory_copy`].
        force_copy: bool,
    }

    fn installed() -> &'static Mutex<Vec<Registration>> {
        static ACTIONS: std::sync::OnceLock<Mutex<Vec<Registration>>> = std::sync::OnceLock::new();
        ACTIONS.get_or_init(|| Mutex::new(Vec::new()))
    }

    static ANY: AtomicBool = AtomicBool::new(false);
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn lock() -> MutexGuard<'static, Vec<Registration>> {
        installed().lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Installs `action` at `stage`, optionally limited to one path.
    pub fn install(stage: Stage, only: Option<PathBuf>, action: Action) -> StagingFaultGuard {
        install_inner(stage, only, action, false)
    }

    /// Installs an action that **also** asks the directory staging step to copy-then-remove.
    ///
    /// Used for the partial-removal failure exit, which production otherwise reaches only when the
    /// watcher happens to hold the tree open. The copy, the material registration, the removal and
    /// the error are all still the production ones.
    pub fn install_forcing_copy(
        stage: Stage,
        only: Option<PathBuf>,
        action: Action,
    ) -> StagingFaultGuard {
        install_inner(stage, only, action, true)
    }

    fn install_inner(
        stage: Stage,
        only: Option<PathBuf>,
        action: Action,
        force_copy: bool,
    ) -> StagingFaultGuard {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut registrations = lock();
        registrations.push(Registration { id, stage: stage.name(), only, action, force_copy });
        ANY.store(true, Ordering::Relaxed);
        StagingFaultGuard { id }
    }

    /// Disarms only its own registration when it drops.
    pub struct StagingFaultGuard {
        id: u64,
    }

    impl Drop for StagingFaultGuard {
        fn drop(&mut self) {
            let mut registrations = lock();
            registrations.retain(|registration| registration.id != self.id);
            ANY.store(!registrations.is_empty(), Ordering::Relaxed);
        }
    }

    /// Returns whether any installed action asked this staging call to take the
    /// **copy-then-remove** route for a directory instead of the ordinary rename.
    ///
    /// # Why this is a decision and not only an interference
    ///
    /// The copy-then-remove route exists because a directory the workspace watcher holds open cannot
    /// be renamed at all. That makes it reachable in production only when the watcher happens to hold
    /// the tree, so a test could not exercise its **failure exit** — and a failure exit that no test
    /// can reach is not coverage. An action installed here says "the rename will be refused", which is
    /// exactly the condition the production branch keys on, so the real branch, its real material
    /// registration and its real partial-removal error all run.
    ///
    /// Nothing else changes: the copy, the registration, the removal and the error are the production
    /// ones, and with no action installed this returns `false` after one relaxed atomic load.
    pub(crate) fn forced_directory_copy(stage: Stage, path: &Path) -> bool {
        if !ANY.load(Ordering::Relaxed) {
            return false;
        }
        let actions = lock()
            .iter()
            .filter(|registration| {
                registration.stage == stage.name()
                    && registration.force_copy
                    && registration.only.as_ref().is_none_or(|expected| expected == path)
            })
            .count();
        actions > 0
    }

    /// Runs any action installed for `stage` at `path`. A no-op while none is installed.
    pub(crate) fn reached(stage: Stage, path: &Path) {
        if !ANY.load(Ordering::Relaxed) {
            return;
        }
        // The actions are cloned out under the lock and run after it is released, so an action that
        // touches the registry cannot deadlock against this call.
        let actions = lock()
            .iter()
            .filter(|registration| {
                registration.stage == stage.name()
                    && registration.only.as_ref().is_none_or(|expected| expected == path)
            })
            .map(|registration| std::sync::Arc::clone(&registration.action))
            .collect::<Vec<_>>();
        for action in actions {
            action(path);
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::{Arc, atomic::AtomicUsize};

        use super::*;

        #[test]
        fn dropping_one_registration_keeps_another_target_armed_on_the_worker() {
            let root = tempfile::tempdir().expect("isolated hook targets");
            let first_path = root.path().join("first");
            let second_path = root.path().join("second");
            let first_calls = Arc::new(AtomicUsize::new(0));
            let second_calls = Arc::new(AtomicUsize::new(0));
            let first_action = Arc::clone(&first_calls);
            let second_action = Arc::clone(&second_calls);
            let first = install(
                Stage::FilePublished,
                Some(first_path.clone()),
                Arc::new(move |_| {
                    first_action.fetch_add(1, Ordering::Relaxed);
                }),
            );
            let second = install(
                Stage::FilePublished,
                Some(second_path.clone()),
                Arc::new(move |_| {
                    second_action.fetch_add(1, Ordering::Relaxed);
                }),
            );
            drop(first);
            let worker_target = second_path.clone();
            std::thread::spawn(move || reached(Stage::FilePublished, &worker_target))
                .join()
                .expect("worker hook");
            reached(Stage::FilePublished, &first_path);
            assert_eq!(first_calls.load(Ordering::Relaxed), 0);
            assert_eq!(second_calls.load(Ordering::Relaxed), 1);
            drop(second);
            reached(Stage::FilePublished, &second_path);
            assert_eq!(second_calls.load(Ordering::Relaxed), 1);
        }
    }
}

/// The domain identity of a native identity, so a compensation compares like with like.
pub(crate) fn domain_identity_of(
    identity: &aura_windows_platform::NativeFileIdentity,
) -> FileIdentity {
    FileIdentity {
        volume_serial_number: identity.volume_serial_number.to_string(),
        file_id: identity.file_id.iter().map(|byte| format!("{byte:02x}")).collect(),
    }
}

struct StagingArea {
    relative_root: WorkspacePath,
    root_result: PublishedResult,
    next_backup: usize,
    /// The caller's **original** condition for the object currently about to be staged.
    ///
    /// Set immediately before each [`StagingArea::stage_existing`] call and cleared after it, so the
    /// condition re-verified at the move boundary is the one the caller really stated for *this*
    /// object. It is never refreshed and never re-derived from a later read.
    staged_condition: Option<HandleCondition>,
    /// Every piece of recovery material this execution has produced, registered **before** the
    /// action that produced it could fail.
    ///
    /// This is what makes "do not delete the current failing step's material" enforceable rather
    /// than hopeful. A directory copy stages the original tree aside; if the subsequent partial
    /// removal fails, the step never enters `applied`, and only this registry still knows that the
    /// complete backup exists and must be kept.
    registered: Vec<StagedBackup>,
}

impl StagingArea {
    fn create(
        resolver: &SecurePathResolver,
        journal_id: MutationJournalId,
    ) -> Result<Self, FsToolError> {
        let relative_root = WorkspacePath::parse(format!(".aura-transaction-{journal_id}"))
            .map_err(|error| FsToolError::InvalidRequest(error.to_string()))?;
        let parent = resolver.validate_parent(&relative_root)?;
        let created =
            aura_windows_platform::handle_bound_create_directory(&parent.parent, &parent.leaf_name)
                .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
        let root_result = PublishedResult::Directory {
            identity: Some(domain_identity_of(&created.info().identity)),
        };
        drop(created);
        if let Err(error) = resolver.verify_after_mutation(&relative_root) {
            let primary = FsToolError::from(error);
            return match Self::remove_own_result(resolver, &relative_root, &root_result) {
                Ok(()) => Err(primary),
                Err(cleanup) => Err(FsToolError::ReconciliationRequired(format!(
                    "staging root verification failed ({primary}); its own cleanup also failed ({cleanup})"
                ))),
            };
        }
        Ok(Self {
            relative_root,
            root_result,
            next_backup: 0,
            staged_condition: None,
            registered: Vec::new(),
        })
    }

    /// Stages `original` aside, re-verifying `condition` on the held object at the move boundary.
    ///
    /// `condition` is the caller's own condition for this exact object, captured before the staging
    /// step while the object was still at its own name. Passing it here — rather than letting the
    /// staging step look up whatever is on disk now — is what keeps the comparison the caller's
    /// decision instead of a fresh observation.
    fn stage_conditioned(
        &mut self,
        resolver: &SecurePathResolver,
        original: &WorkspacePath,
        condition: Option<HandleCondition>,
    ) -> Result<StagedObject, FsToolError> {
        self.staged_condition = condition;
        let staged = self.stage_existing(resolver, original);
        // The condition belongs to one staging action only. Clearing it here means a later action
        // can never re-verify a previous step's condition against a different object.
        self.staged_condition = None;
        staged
    }

    fn apply(
        &mut self,
        resolver: &SecurePathResolver,
        operation: &PreparedOperation,
    ) -> Result<AppliedOperation, FsToolError> {
        match operation {
            PreparedOperation::Write { target, bytes, expected, existed, .. } => {
                let state = load_state(resolver, target)?;
                validate_expected(target, &state, expected)?;
                if *existed != state.exists() {
                    return Err(FsToolError::InvalidRequest(format!(
                        "{} changed after changeset preflight",
                        target
                    )));
                }
                // Consume the caller's **original byte condition** here, at the last instant it can
                // be read: while the object it was decided about is still on disk under this name.
                // Once the staging step below moves that object aside the destination holds
                // nothing, so no later exclusive handle can re-verify bytes that are no longer
                // there — which is exactly why this check cannot be deferred to the publication.
                //
                // The algorithm is the caller's own and the expected value is never refreshed.
                verify_staged_condition(resolver, target, expected)?;
                // Capture the identity the same decision was made against, at the same moment.
                let pre_stage_condition =
                    handle_condition_for(target, &resolver.absolute_path(target), expected)?;
                let backup = if state.exists() {
                    // The caller's own condition travels into the staging step, so the object that
                    // is moved aside is re-verified **on the handle** at the move boundary — after
                    // the check above returned and before the rename. That is the window the pre-fix
                    // code left open by releasing its read handle and then moving by path.
                    Some(self.stage_conditioned(
                        resolver,
                        target,
                        Some(pre_stage_condition.clone()),
                    )?)
                } else {
                    None
                };
                // When a backup was staged the publication really is a creation over an empty name,
                // so the action is "fail if it exists"; the byte condition was consumed just above.
                // When nothing was staged the destination is still there and the full condition —
                // identity **and** bytes — reaches the exclusive handle unchanged.
                let condition = match (&backup, &pre_stage_condition) {
                    (Some(staged), _) => staged_condition(expected, staged.identity.clone())?,
                    (None, _) => pre_stage_condition.clone(),
                };
                let published = match write_atomic(
                    resolver,
                    target,
                    bytes,
                    &condition,
                    pre_stage_condition.replace_identity(),
                ) {
                    Ok(published) => published,
                    Err(error) => {
                        return Err(self.failed_operation(resolver, backup.as_ref(), error));
                    }
                };
                Ok(AppliedOperation::Write { target: target.clone(), backup, published })
            }
            PreparedOperation::CreateDirectory { target, .. } => {
                validate_expected(
                    target,
                    &load_state(resolver, target)?,
                    &FsExpectedState::Absent,
                )?;
                let parent = resolver.validate_parent(target)?;
                let absolute = parent.parent_absolute_path.join(&parent.leaf_name);
                let created = aura_windows_platform::handle_bound_create_directory(
                    &parent.parent,
                    &parent.leaf_name,
                )
                .map_err(|error| {
                    FsToolError::ReconciliationRequired(format!("create directory: {error}"))
                })?;
                let published = PublishedResult::Directory {
                    identity: Some(domain_identity_of(&created.info().identity)),
                };
                // Capture the kernel creation receipt before the name can be reopened or replaced.
                drop(created);
                staging_fault::reached(staging_fault::Stage::DirectoryCreated, &absolute);
                if let Err(error) = resolver.verify_after_mutation(target) {
                    let cleanup = Self::remove_own_result(resolver, target, &published);
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "directory publication verification failed ({error}); own-result cleanup: {cleanup:?}"
                    )));
                }
                Ok(AppliedOperation::CreateDirectory { target: target.clone(), published })
            }
            PreparedOperation::Copy {
                source,
                destination,
                source_expected,
                destination_expected,
                source_is_directory,
                destination_existed,
                ..
            } => {
                let source_state = load_state(resolver, source)?;
                validate_source_expected(source, &source_state, source_expected)?;
                if source_state.is_directory() != *source_is_directory {
                    return Err(FsToolError::InvalidRequest(format!(
                        "{} changed node kind after preflight",
                        source
                    )));
                }
                let destination_state = load_state(resolver, destination)?;
                validate_expected(destination, &destination_state, destination_expected)?;
                if destination_state.exists() != *destination_existed {
                    return Err(FsToolError::InvalidRequest(format!(
                        "{} changed after changeset preflight",
                        destination
                    )));
                }
                // As with a write: the caller's **original byte condition** is consumed while the
                // destination object it was decided about is still on disk, before staging moves it
                // aside and leaves a name no later handle can verify bytes against.
                verify_staged_condition(resolver, destination, destination_expected)?;
                // Capture the identity the same decision was made against, at the same moment.
                let pre_stage_condition = handle_condition_for(
                    destination,
                    &resolver.absolute_path(destination),
                    destination_expected,
                )?;
                let backup = if destination_state.exists() {
                    Some(self.stage_conditioned(
                        resolver,
                        destination,
                        Some(pre_stage_condition.clone()),
                    )?)
                } else {
                    None
                };
                let condition = match (&backup, &pre_stage_condition) {
                    (Some(staged), _) => {
                        staged_condition(destination_expected, staged.identity.clone())?
                    }
                    (None, condition) => condition.clone(),
                };
                let source_condition =
                    handle_condition_for(source, &resolver.absolute_path(source), source_expected)?;
                let (published, own_members) = match copy_logical(
                    resolver,
                    source,
                    destination,
                    *source_is_directory,
                    &condition,
                    &source_condition,
                    pre_stage_condition.replace_identity(),
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        return Err(self.failed_operation(resolver, backup.as_ref(), error));
                    }
                };
                Ok(AppliedOperation::Copy {
                    destination: destination.clone(),
                    backup,
                    published,
                    own_members,
                })
            }
            PreparedOperation::Move {
                source,
                destination,
                source_expected,
                destination_expected,
                destination_existed,
                ..
            } => {
                let source_state = load_state(resolver, source)?;
                validate_source_expected(source, &source_state, source_expected)?;
                let destination_state = load_state(resolver, destination)?;
                validate_expected(destination, &destination_state, destination_expected)?;
                if destination_state.exists() != *destination_existed {
                    return Err(FsToolError::InvalidRequest(format!(
                        "{} changed after changeset preflight",
                        destination
                    )));
                }
                // The source's own identity **before** the move, read from the resolved handle, so
                // a restore can say what it is putting back rather than assuming the planner's view
                // still holds.
                let source_before = source_state_identity(resolver, source);
                // The **source's own original condition** is consumed here, while the object it was
                // decided about is still on disk under its own name, and it travels into the real
                // move so the object that is carried is re-verified on the held handle at the move
                // boundary. It is never refreshed: the identity below is read from the same verified
                // handle, and the algorithm and digest are the caller's own.
                verify_staged_condition(resolver, source, source_expected)?;
                let source_condition =
                    handle_condition_for(source, &resolver.absolute_path(source), source_expected)?;
                // The **destination's** original condition: a destination that exists was decided
                // about by the caller too, so it is verified under the same rule before it is staged,
                // and the condition travels into the staging step rather than being looked up again.
                let destination_backup = if destination_state.exists() {
                    verify_staged_condition(resolver, destination, destination_expected)?;
                    let pre_stage_condition = handle_condition_for(
                        destination,
                        &resolver.absolute_path(destination),
                        destination_expected,
                    )?;
                    Some(self.stage_conditioned(
                        resolver,
                        destination,
                        Some(pre_stage_condition),
                    )?)
                } else {
                    None
                };
                let moved =
                    match move_logical(resolver, source, destination, &source_condition, None) {
                        Ok(moved) => moved,
                        Err(error) => {
                            // **No reverse blind move.** The previous form moved `destination` back to
                            // `source` unconditionally, which relocates whatever happens to be at the
                            // destination — a competitor's object included — and is not this execution's
                            // to move. Only the staging this execution really performed is undone,
                            // through the recorded backup, which proves the object it restores.
                            return Err(self.failed_operation(
                                resolver,
                                destination_backup.as_ref(),
                                error,
                            ));
                        }
                    };
                // The result at the destination is bound to the object the held handle carried, not
                // to a fresh inspection of the name: a competitor that took the destination name
                // between the move and this read must not be recorded as this execution's result,
                // because compensation would then remove theirs.
                let published = match moved.hash {
                    Some(hash) => PublishedResult::File {
                        identity: domain_identity_of(&moved.identity),
                        hash,
                    },
                    None => PublishedResult::Directory {
                        identity: Some(domain_identity_of(&moved.identity)),
                    },
                };
                Ok(AppliedOperation::Move {
                    source: source.clone(),
                    destination: destination.clone(),
                    destination_backup,
                    published,
                    source_before,
                    own_members: moved.members,
                })
            }
            PreparedOperation::Delete { target, expected, recursive, .. } => {
                let state = load_state(resolver, target)?;
                validate_expected(target, &state, expected)?;
                if state.is_directory() && !recursive && directory_not_empty(resolver, target)? {
                    return Err(FsToolError::InvalidRequest(format!(
                        "directory {} became non-empty after preflight",
                        target
                    )));
                }
                // The caller's **original condition** for the object about to be deleted travels into
                // the staging step, which re-verifies identity and bytes on the held handle at the
                // move boundary. Passing the caller's own condition — rather than letting staging
                // look up whatever is on disk now — is what keeps the comparison the caller's
                // decision. A recursive directory delete has no file identity to carry, so its
                // condition is the node-kind fact the caller already stated.
                let delete_condition = match (state.is_directory(), expected) {
                    (true, _) => None,
                    (false, expected) => Some(handle_condition_for(
                        target,
                        &resolver.absolute_path(target),
                        expected,
                    )?),
                };
                let backup = self.stage_conditioned(resolver, target, delete_condition)?;
                Ok(AppliedOperation::Delete { backup })
            }
        }
    }

    fn stage_existing(
        &mut self,
        resolver: &SecurePathResolver,
        original: &WorkspacePath,
    ) -> Result<StagedObject, FsToolError> {
        let info = resolver.resolve_existing(original)?;
        let is_directory = info.info.is_directory;
        let staged =
            WorkspacePath::parse(format!("{}/{:08}", self.relative_root, self.next_backup))
                .map_err(|error| FsToolError::InvalidRequest(error.to_string()))?;
        self.next_backup = self.next_backup.saturating_add(1);
        let original_absolute = resolver.absolute_path(original);
        let staged_absolute = resolver.absolute_path(&staged);
        if !is_directory {
            // The caller's original identity and bytes are consumed on the held object. This
            // DELETE-capable handle denies other writers and renames until the native move ends.
            let mut held = open_held_for_staging(&original_absolute)?;
            if let Some(condition) = self.staged_condition.as_ref() {
                condition.verify_held_file(original, held.file_mut())?;
            }
            let verified_identity = held_file_identity(held.file()).map_err(|source| {
                io_error("inspect held staged object", &original_absolute, source)
            })?;
            let parent_path = staged_absolute.parent().ok_or_else(|| {
                FsToolError::InvalidRequest("staging destination has no parent".into())
            })?;
            let parent = open_verified_publish_parent(parent_path)
                .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
            let leaf = staged_absolute.file_name().ok_or_else(|| {
                FsToolError::InvalidRequest("staging destination has no leaf".into())
            })?;
            let original_hash = hash_held(held.file_mut())?;
            staging_fault::reached(staging_fault::Stage::BeforeStagedMove, &original_absolute);
            // The effect names the held object, not a second inspection of the source path.
            handle_bound_move_existing(&held, &parent, leaf)
                .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
            // Register recovery material before any later verification can fail.
            self.registered
                .push(StagedBackup { original: original.clone(), staged: staged.clone() });
            drop(held);
            // The object really arrived under the staged name, and it is still the one verified.
            //
            // **No path rename-back.** The pre-fix form renamed `staged` back to `original` by path,
            // which relocates whatever object the *original* name holds at the instant it runs: a
            // competitor that created that name while this execution was moving would have the
            // staging slot overwritten by, or would itself be moved into, a place this execution
            // never decided about. The move already happened, the material is registered, and the
            // recovery owner is the only party that may settle it — so the failure is reported with
            // both locations named and the material left exactly where it is.
            if let Err(error) = resolver.verify_after_mutation(&staged) {
                return Err(FsToolError::ReconciliationRequired(format!(
                    "staging {original} moved the object to {staged} but that name is not a valid \
                     workspace path ({error}); the material is kept there and no rename-back was \
                     attempted"
                )));
            }
            let staged_info = inspect_existing_path(&staged_absolute, false).map_err(|source| {
                io_error("inspect staged workspace object", &staged_absolute, source)
            })?;
            if staged_info.identity != verified_identity {
                return Err(FsToolError::ReconciliationRequired(format!(
                    "staging {original} moved an object that is not the one this operation verified; \
                     the verified object is at {} and the recovered material is named for the \
                     recovery owner",
                    staged
                )));
            }
            return Ok(StagedObject {
                backup: StagedBackup { original: original.clone(), staged },
                identity: Some(verified_identity.clone()),
                condition: self.staged_condition.clone().unwrap_or(HandleCondition::File {
                    identity: verified_identity,
                    algorithm: PlatformHashAlgorithm::Blake3,
                    hex: original_hash.hex,
                }),
                members: None,
            });
        }
        let original_members = copy_receipt::snapshot_members(resolver, original)?;
        let parent = open_verified_publish_parent(staged_absolute.parent().ok_or_else(|| {
            FsToolError::InvalidRequest("staging destination has no parent".into())
        })?)
        .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
        let leaf = staged_absolute
            .file_name()
            .ok_or_else(|| FsToolError::InvalidRequest("staging destination has no leaf".into()))?;
        let forced_copy = staging_fault::forced_directory_copy(
            staging_fault::Stage::PartialOriginalRemoval,
            &original_absolute,
        );
        let renamed = if forced_copy {
            false
        } else {
            match open_held_for_staging(&original_absolute) {
                Ok(held) => {
                    if held.info().identity != info.info.identity || !held.info().is_directory {
                        return Err(FsToolError::ReconciliationRequired(
                            "directory changed before staging".into(),
                        ));
                    }
                    staging_fault::reached(
                        staging_fault::Stage::BeforeStagedMove,
                        &original_absolute,
                    );
                    copy_receipt::verify_members(resolver, original, &original_members)?;
                    match handle_bound_move_existing(&held, &parent, leaf) {
                        Ok(()) => true,
                        Err(HandlePublishError::NativeRename { source, .. })
                            if source.kind() == std::io::ErrorKind::PermissionDenied
                                || source.raw_os_error() == Some(32) =>
                        {
                            false
                        }
                        Err(error) => {
                            return Err(FsToolError::ReconciliationRequired(error.to_string()));
                        }
                    }
                }
                Err(FsToolError::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::PermissionDenied
                        || source.raw_os_error() == Some(32) =>
                {
                    false
                }
                Err(error) => return Err(error),
            }
        };
        let (identity, members) = if renamed {
            self.registered
                .push(StagedBackup { original: original.clone(), staged: staged.clone() });
            let mut members = original_members;
            copy_receipt::rebase(&mut members, original, &staged)?;
            (info.info.identity.clone(), members)
        } else {
            // A watched root may refuse rename. The fallback records each actual creation,
            // registers the complete backup first, and removes only the original snapshot.
            // It never sweeps a late child or a replacement with recursive path deletion.
            let source =
                aura_windows_platform::open_held_for_read(&original_absolute).map_err(|error| {
                    io_error("hold directory before staging copy", &original_absolute, error)
                })?;
            if source.info().identity != info.info.identity || !source.info().is_directory {
                return Err(FsToolError::ReconciliationRequired(
                    "directory changed before staging copy".into(),
                ));
            }
            copy_receipt::verify_members(resolver, original, &original_members)?;
            let copied = copy_directory_verified(resolver, &original_absolute, &staged_absolute)?;
            let PublishedResult::Directory { identity: Some(copied_identity) } = &copied.result
            else {
                return Err(FsToolError::ReconciliationRequired(
                    "staging copy has no directory receipt".into(),
                ));
            };
            self.registered
                .push(StagedBackup { original: original.clone(), staged: staged.clone() });
            copy_receipt::verify_members(resolver, original, &original_members)?;
            drop(source);
            staging_fault::reached(
                staging_fault::Stage::PartialOriginalRemoval,
                &original_absolute,
            );
            let original_result = PublishedResult::Directory {
                identity: Some(domain_identity_of(&info.info.identity)),
            };
            if let Err(error) =
                self.remove_own_copy(resolver, original, &original_result, Some(&original_members))
            {
                return Err(FsToolError::ReconciliationRequired(format!(
                    "staging {original} by copy failed while removing the original ({error}); the complete backup is kept at {staged}"
                )));
            }
            (aura_workspace::native_identity(copied_identity)?, copied.members)
        };
        // The same rule for the directory branch: the move (or the copy-plus-remove) already
        // happened and its material is registered, so a verification failure here is material for
        // the recovery owner and never a second path rename. A rename-back would carry whatever
        // object the original name holds by then, which is not this execution's to move.
        if let Err(error) = resolver.verify_after_mutation(&staged) {
            return Err(FsToolError::ReconciliationRequired(format!(
                "staging {original} moved the tree to {staged} but that name is not a valid \
                 workspace path ({error}); the complete material is kept there and no rename-back \
                 was attempted"
            )));
        }
        Ok(StagedObject {
            backup: StagedBackup { original: original.clone(), staged },
            identity: Some(identity.clone()),
            condition: HandleCondition::IdentityOnly { identity },
            members: Some(members),
        })
    }

    fn restore_backup(
        &self,
        resolver: &SecurePathResolver,
        backup: &StagedObject,
    ) -> Result<(), FsToolError> {
        if load_state(resolver, &backup.backup.original)?.exists() {
            return Err(FsToolError::ReconciliationRequired(format!(
                "cannot restore {}; target is occupied",
                backup.backup.original
            )));
        }
        let staged = resolver.absolute_path(&backup.backup.staged);
        let original_validated = resolver.validate_parent(&backup.backup.original)?;
        // The material is moved **through a held handle on the staged object**, with the identity
        // recorded when it was staged travelling in as the condition. A path `fs::rename` would
        // carry whatever object the staged name refers to at that instant, so a competitor that
        // replaced the staging slot would have *their* object restored to the original name — the
        // restore would silently put back something this execution never had.
        backup.identity.as_ref().ok_or_else(|| {
            FsToolError::ReconciliationRequired(format!(
                "cannot restore {}; the staged material has no recorded identity to move back",
                backup.backup.original
            ))
        })?;
        let condition = backup.condition.clone();
        let staged_absolute = staged.clone();
        let mut held = open_held_for_staging(&staged_absolute)?;
        condition.verify_held_file(&backup.backup.staged, held.file_mut())?;
        if let Some(members) = &backup.members {
            copy_receipt::verify_members(resolver, &backup.backup.staged, members)?;
        }
        let parent = open_verified_publish_parent(&original_validated.parent_absolute_path)
            .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
        // `fail if it exists`: the occupancy check above is repeated at the real move boundary, so a
        // competitor that took the original name in the meantime is refused rather than replaced.
        handle_bound_move_existing(&held, &parent, &original_validated.leaf_name)
            .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
        drop(held);
        // The object really arrived at the original name. A verification failure here is material
        // for the recovery owner: it is never "fixed" by a second path rename.
        if let Err(error) = resolver.verify_after_mutation(&backup.backup.original) {
            return Err(FsToolError::ReconciliationRequired(format!(
                "restore moved the staged material to {} but that name is not a valid workspace \
                 path ({error}); the object is kept there and no rename-back was attempted",
                backup.backup.original
            )));
        }
        Ok(())
    }

    /// Removes this execution's own result from `target`, refusing to remove anything else.
    ///
    /// # Why the path is not enough
    ///
    /// The previous form called `remove_logical_if_present` and then, on the recursive branch,
    /// `remove_any` → `remove_dir_all`. A path only says *where* something is; it says nothing
    /// about *whose* it is. So a compensation that ran after a third party replaced the object, or
    /// added members to a directory this execution created, deleted the competitor's content — and
    /// reported a successful rollback while doing it.
    ///
    /// # The three defects this closes, in the order they bite
    ///
    /// * **Any inspect error read as "absent".** The old form treated every failure as "there is
    ///   nothing there", so a sharing violation or a permission error looked like a clean removal
    ///   and the caller was told the rollback succeeded. Only an explicit `NotFound` is absence; a
    ///   real error is reported and nothing is touched.
    /// * **A directory with no identity check.** An empty directory is still *an object*, and one a
    ///   third party created is theirs. The check is therefore the same as for a file — the
    ///   directory must be the one this execution published. An execution that created a directory
    ///   has no file identity for it, so it records the stronger fact it does have: the directory
    ///   must be the one it made, established from the handle it held when it made it.
    /// * **In-place rewrite keeps the identity.** Comparing the identity alone let a file that a
    ///   third party rewrote **in place** be deleted, because Windows keeps the file id. The
    ///   removal therefore also compares the bytes against the result this execution published —
    ///   the same content condition the publication itself used.
    ///
    /// # Why the check and the delete are one handle action
    ///
    /// Checking the identity and then deleting by path is a fresh race window: between the two, a
    /// third party can replace the object and the delete removes theirs. The object is therefore
    /// opened, verified **through the handle**, and removed while that handle is still the one the
    /// check was made against.
    fn remove_own_result(
        resolver: &SecurePathResolver,
        target: &WorkspacePath,
        published: &PublishedResult,
    ) -> Result<(), FsToolError> {
        let absolute = resolver.absolute_path(target);
        // The object is **opened once, held, and verified through that handle**, and the removal is
        // performed on the same handle. Checking a path and then deleting the path is two decisions
        // about a name, and a name says nothing about whose object is beneath it: a third party that
        // replaced the object in between would have theirs deleted. This handle carries DELETE
        // access and denies competing writes and renames, so the window cannot be entered at all.
        let info = match inspect_existing_path(&absolute, false) {
            Ok(info) => info,
            // Only an explicit "it is not there" is absence. Every other failure is a real
            // condition the caller has to see, and nothing is deleted on it.
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(io_error("inspect compensation target", &absolute, source));
            }
        };
        match published {
            PublishedResult::File { identity, hash } => {
                if info.is_directory {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate {}; a directory is there where this execution published \
                         a file",
                        target
                    )));
                }
                let mut held = open_held_for_staging(&absolute)?;
                // Identity, compared against the handle — the object this removal is about.
                let held_identity = held_file_identity(held.file()).map_err(|source| {
                    io_error("inspect held compensation target", &absolute, source)
                })?;
                if domain_identity_of(&held_identity) != *identity {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate {}; the object there is not the one this execution \
                         published",
                        target
                    )));
                }
                // A same-identity in-place rewrite is the case identity comparison cannot see, so
                // the bytes are compared too, **over the same held handle** the removal uses. A file
                // that no longer holds the result this execution wrote belongs to whoever wrote it.
                if hash_held(held.file_mut())? != *hash {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate {target}; its content changed after this execution published it"
                    )));
                }
                // The removal is the **same handle's** action: an object that is not the one just
                // verified cannot be what is deleted, because nothing else can hold this name while
                // this handle is open.
                staging_fault::reached(staging_fault::Stage::BeforeCompensationDelete, &absolute);
                aura_windows_platform::handle_bound_delete(held.file()).map_err(|error| {
                    FsToolError::ReconciliationRequired(format!(
                        "cannot remove this execution's own result at {target}: {error}"
                    ))
                })?;
                Ok(())
            }
            PublishedResult::Directory { identity } => {
                if !info.is_directory {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate {}; a file is there where this execution published a \
                         directory",
                        target
                    )));
                }
                // The empty third-party directory the previous form deleted is refused here: the
                // directory must be the one this execution made, and a directory that is not
                // carries no proof it is this execution's to remove.
                let held = open_held_for_staging(&absolute)?;
                let held_identity = held_file_identity(held.file()).map_err(|source| {
                    io_error("inspect held compensation directory", &absolute, source)
                })?;
                if !held.info().is_directory
                    || identity.as_ref() != Some(&domain_identity_of(&held_identity))
                {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate {}; the directory there is not the one this execution \
                         created",
                        target
                    )));
                }
                // Even this execution's own directory may only go while it is empty: a member is
                // somebody else's object, and sweeping it would be the delete-the-competitor
                // failure this whole change exists to prevent.
                let mut members = fs::read_dir(&absolute).map_err(|source| {
                    io_error("enumerate compensation directory", &absolute, source)
                })?;
                if members.next().is_some() {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate {}; the directory now holds members this execution did \
                         not create",
                        target
                    )));
                }
                drop(members);
                staging_fault::reached(staging_fault::Stage::BeforeCompensationDelete, &absolute);
                aura_windows_platform::handle_bound_delete(held.file()).map_err(|error| {
                    FsToolError::ReconciliationRequired(format!(
                        "cannot remove this execution's own empty directory at {target}: {error}"
                    ))
                })?;
                Ok(())
            }
        }
    }

    /// Removes exactly the material this execution registered as its own, and **never** anything it
    /// did not register.
    ///
    /// # Why this is not `remove_any(staging_root)` any more
    ///
    /// The pre-fix compensation ended with a blanket `remove_any(&self.absolute_root)`, which
    /// deletes the whole staging directory. That is only sound while every piece of material in it
    /// belongs to an `applied` step this call is undoing. The case that broke it is the **currently
    /// failing** step: a directory copy whose original tree was only partially removed returns
    /// `ReconciliationRequired` from staging, so the step never enters `applied` — and the outer
    /// `compensate(applied)` then swept the staging root, deleting the **only complete backup** of
    /// the original tree while recording `compensated`.
    ///
    /// So the registered material is tracked explicitly. A staging action that produced recovery
    /// material registers it *before* it can fail; compensation removes the staging directory only
    /// when nothing registered survives, and a surviving registration is reported as material that
    /// must be kept rather than silently deleted.
    fn compensate(
        &mut self,
        resolver: &SecurePathResolver,
        applied: &[AppliedOperation],
        registered: &[StagedBackup],
    ) -> Result<usize, FsToolError> {
        let mut count = 0;
        for operation in applied.iter().rev() {
            match operation {
                AppliedOperation::Write { target, backup, published } => {
                    Self::remove_own_result(resolver, target, published)?;
                    if let Some(backup) = backup {
                        self.restore_backup(resolver, backup)?;
                    }
                }
                AppliedOperation::Copy { destination, backup, published, own_members } => {
                    // A copy's own tree is removed by its **own members**, not by a blanket
                    // recursive delete: a member a third party added alongside them is not in the
                    // recorded set and therefore stops the rollback instead of being swept away.
                    self.remove_own_copy(resolver, destination, published, own_members.as_deref())?;
                    if let Some(backup) = backup {
                        self.restore_backup(resolver, backup)?;
                    }
                }
                AppliedOperation::CreateDirectory { target, published } => {
                    // A directory this execution created is proven by the identity it observed at
                    // creation time, and additionally must still be empty: an empty third-party
                    // directory is still a third-party object.
                    Self::remove_own_result(resolver, target, published)?;
                }
                AppliedOperation::Move {
                    source,
                    destination,
                    destination_backup,
                    published,
                    source_before,
                    own_members,
                } => {
                    // The compensation consumes **its own recorded result**: the object at
                    // `destination` is moved back only when it is really the one this move placed
                    // there. The previous form moved whatever was at the destination back to the
                    // source, which relocated a competitor's object.
                    self.undo_move(
                        resolver,
                        source,
                        destination,
                        published,
                        source_before.as_ref(),
                        own_members.as_deref(),
                    )?;
                    if let Some(backup) = destination_backup {
                        self.restore_backup(resolver, backup)?;
                    }
                }
                AppliedOperation::Delete { backup } => {
                    self.restore_backup(resolver, backup)?;
                }
            }
            count += 1;
        }
        // **Unregistered recovery material is never deleted.**
        //
        // A registration exists for material a step produced *before* it could be known to have
        // succeeded. Once every applied step has been undone, any registration still present is
        // material for a step that did **not** complete — exactly the current failing step's
        // complete backup. It is kept, and the caller is told, because deleting it would destroy
        // the only copy of the original tree.
        let survivors = registered
            .iter()
            .filter(|backup| resolver.absolute_path(&backup.staged).exists())
            .map(|backup| backup.staged.to_string())
            .collect::<Vec<_>>();
        if !survivors.is_empty() {
            return Err(FsToolError::ReconciliationRequired(format!(
                "compensation of {} applied step(s) kept {} registered recovery item(s) belonging \
                 to a step that did not complete: {}",
                count,
                survivors.len(),
                survivors.join(", ")
            )));
        }
        // Nothing registered survives, so the staging directory holds only this execution's own
        // internal bookkeeping and can go.
        Self::remove_own_result(resolver, &self.relative_root, &self.root_result)?;
        Ok(count)
    }

    /// Undoes one move, proving the object it moves back is the one this execution placed.
    ///
    /// The source is re-created only when the destination really holds this execution's result: its
    /// identity must match what the move published. A destination that no longer holds that object
    /// is somebody else's, and moving it would both lose their object and put the wrong thing back
    /// at the source.
    fn undo_move(
        &self,
        resolver: &SecurePathResolver,
        source: &WorkspacePath,
        destination: &WorkspacePath,
        published: &PublishedResult,
        source_before: Option<&FileIdentity>,
        own_members: Option<&[OwnMember]>,
    ) -> Result<(), FsToolError> {
        let destination_absolute = resolver.absolute_path(destination);
        match published {
            PublishedResult::File { identity, .. }
            | PublishedResult::Directory { identity: Some(identity) } => {
                let current = match inspect_existing_path(&destination_absolute, false) {
                    Ok(info) => info,
                    Err(source_error) if source_error.kind() == std::io::ErrorKind::NotFound => {
                        // The moved object is already gone. There is nothing to move back, and the
                        // source's own restoration is the caller's next step.
                        return Ok(());
                    }
                    Err(source_error) => {
                        return Err(io_error(
                            "inspect move compensation destination",
                            &destination_absolute,
                            source_error,
                        ));
                    }
                };
                if domain_identity_of(&current.identity) != *identity {
                    return Err(FsToolError::ReconciliationRequired(format!(
                        "cannot compensate the move of {source} to {destination}; the object at the \
                         destination is not the one this execution placed there"
                    )));
                }
            }
            // The move recorded no result identity, so it cannot prove what is at the destination.
            // Refusing keeps a competitor's object where it is.
            PublishedResult::Directory { identity: None } => {
                return Err(FsToolError::ReconciliationRequired(format!(
                    "cannot compensate the move of {source} to {destination}; this execution did \
                     not record what it placed there"
                )));
            }
        }
        let source_absolute = resolver.absolute_path(source);
        if source_absolute.exists() {
            // Restoring over an occupant would delete whoever put something back at the source
            // while this execution was rolling back.
            return Err(FsToolError::ReconciliationRequired(format!(
                "cannot compensate the move of {source} to {destination}; the source is occupied \
                 again"
            )));
        }
        let _ = source_before;
        // The undo move is performed through a **held handle on the destination**, and the
        // condition it carries is the identity this execution recorded for the object it placed —
        // so the object that travels back is the one this move put there, verified on the same
        // handle that performs the move. Moving the destination back by path would relocate
        // whatever name is there at that instant.
        let destination_condition = match published {
            PublishedResult::File { identity, hash } => HandleCondition::File {
                algorithm: match hash.algorithm {
                    ContentHashAlgorithm::Blake3 => PlatformHashAlgorithm::Blake3,
                    ContentHashAlgorithm::Sha256 => PlatformHashAlgorithm::Sha256,
                },
                hex: hash.hex.clone(),
                identity: aura_workspace::native_identity(identity).map_err(|error| {
                    FsToolError::ReconciliationRequired(format!(
                        "cannot compensate the move of {source} to {destination}; the recorded \
                         result identity is not a native identity ({error})"
                    ))
                })?,
            },
            PublishedResult::Directory { identity: Some(identity) } => {
                HandleCondition::IdentityOnly {
                    identity: aura_workspace::native_identity(identity)?,
                }
            }
            _ => {
                return Err(FsToolError::ReconciliationRequired(format!(
                    "cannot compensate the move of {source} to {destination}; this execution did \
                     not record a file result it could move back"
                )));
            }
        };
        move_logical(resolver, destination, source, &destination_condition, own_members)?;
        Ok(())
    }

    /// Removes exactly the members a directory copy created, then the directory itself.
    ///
    /// # Why "always refuse a non-empty directory" is not a completed rollback
    ///
    /// A copy of a tree produces members, so refusing to touch any non-empty destination would
    /// leave the whole copy behind and report a failed rollback for an operation that is entirely
    /// this execution's to undo. Conversely a blanket recursive delete would sweep away a member a
    /// third party added. So the recorded **own members** are removed individually, deepest first,
    /// and any member that is not in that set stops the rollback and is reported.
    fn remove_own_copy(
        &self,
        resolver: &SecurePathResolver,
        destination: &WorkspacePath,
        published: &PublishedResult,
        own_members: Option<&[OwnMember]>,
    ) -> Result<(), FsToolError> {
        let Some(own_members) = own_members else {
            // Not a directory copy: the file rules apply unchanged.
            return Self::remove_own_result(resolver, destination, published);
        };
        let destination_absolute = resolver.absolute_path(destination);
        let root = match open_held_for_staging(&destination_absolute) {
            Ok(root) => root,
            Err(FsToolError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if !root.info().is_directory {
            return Err(FsToolError::ReconciliationRequired(format!(
                "cannot compensate the copy into {destination}; a file is there where this \
                 execution copied a directory"
            )));
        }
        if !matches!(published, PublishedResult::Directory { identity: Some(identity) }
            if domain_identity_of(&root.info().identity) == *identity)
        {
            return Err(FsToolError::ReconciliationRequired(format!(
                "cannot compensate the copy into {destination}; the directory there is not the one \
                 this execution created"
            )));
        }
        let mut members = own_members.to_vec();
        members.sort_by_key(|member| std::cmp::Reverse(member.path.as_str().split('/').count()));
        let mut failures = Vec::new();
        for member in &members {
            if let Err(error) = Self::remove_own_result(resolver, &member.path, &member.result) {
                failures.push(error.to_string());
            }
        }
        let empty = fs::read_dir(&destination_absolute)
            .map(|mut entries| entries.next().is_none())
            .map_err(|error| {
                io_error("inspect copied directory before cleanup", &destination_absolute, error)
            })?;
        if empty {
            staging_fault::reached(
                staging_fault::Stage::BeforeCompensationDelete,
                &destination_absolute,
            );
            if let Err(error) = aura_windows_platform::handle_bound_delete(root.file()) {
                failures.push(error.to_string());
            }
        } else {
            failures.push(format!("{destination} retains changed or third-party members"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(FsToolError::ReconciliationRequired(format!(
                "copy compensation kept material it could not prove its own: {}",
                failures.join("; ")
            )))
        }
    }

    fn failed_operation(
        &self,
        resolver: &SecurePathResolver,
        backup: Option<&StagedObject>,
        primary: FsToolError,
    ) -> FsToolError {
        if let Some(backup) = backup
            && let Err(secondary) = self.restore_backup(resolver, backup)
        {
            return FsToolError::ReconciliationRequired(format!(
                "operation failed ({primary}); restoring its own backup also failed ({secondary})"
            ));
        }
        primary
    }

    fn finish(
        self,
        resolver: &SecurePathResolver,
        applied: &[AppliedOperation],
    ) -> Result<(), FsToolError> {
        // Settlement may delete only the exact backups this execution staged. A late
        // rewrite/member under the private transaction directory is not ours to sweep.
        for operation in applied {
            let backup = match operation {
                AppliedOperation::Write { backup, .. } | AppliedOperation::Copy { backup, .. } => {
                    backup.as_ref()
                }
                AppliedOperation::Move { destination_backup, .. } => destination_backup.as_ref(),
                AppliedOperation::Delete { backup } => Some(backup),
                AppliedOperation::CreateDirectory { .. } => None,
            };
            if let Some(backup) = backup {
                let path = &backup.backup.staged;
                if let Some(members) = &backup.members {
                    let identity = backup.identity.as_ref().map(domain_identity_of);
                    self.remove_own_copy(
                        resolver,
                        path,
                        &PublishedResult::Directory { identity },
                        Some(members),
                    )?;
                } else {
                    let absolute = resolver.absolute_path(path);
                    let mut held = open_held_for_staging(&absolute)?;
                    backup.condition.verify_held_file(path, held.file_mut())?;
                    aura_windows_platform::handle_bound_delete(held.file())
                        .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
                }
            }
        }
        Self::remove_own_result(resolver, &self.relative_root, &self.root_result)
    }
}

/// What the caller requires the destination to be, expressed for the platform's exclusive handle.
///
/// Both halves of a `File` condition are established at the **same instant**, from the same object,
/// before anything is moved: the identity that object had, and the bytes the caller decided it
/// should still have. Keeping them together is what makes the pair meaningful — an identity from one
/// moment and a hash from another would describe an object that never existed.
#[derive(Clone)]
enum HandleCondition {
    /// The destination must not exist. The final action is "fail if it exists".
    Absent,
    /// The destination must be this exact object and still hold exactly these bytes.
    File { identity: NativeFileIdentity, algorithm: PlatformHashAlgorithm, hex: String },
    /// The destination must be this exact object, with no byte condition.
    ///
    /// This is the **compensation and undo-move** condition, and it exists because those callers
    /// really did not decide about bytes: an undo move restores the object this execution placed,
    /// and the caller's original content condition belonged to the forward operation, which already
    /// consumed it at its own boundary. Inventing a digest here — or reusing a stale one — would be
    /// a condition nobody stated, and comparing a made-up digest could only ever be wrong. The
    /// identity is the fact that is genuinely known, and the held handle it is verified on is what
    /// makes it binding: the object cannot be swapped between the comparison and the move.
    IdentityOnly { identity: NativeFileIdentity },
}

impl HandleCondition {
    /// The identity a replacement requires. Only a `File` condition has one: the `Absent` creation
    /// path does not go through the replace entry at all, because "fail if it exists" cannot be
    /// expressed as a replacement.
    fn replace_identity(&self) -> Option<&NativeFileIdentity> {
        match self {
            Self::Absent => None,
            Self::File { identity, .. } | Self::IdentityOnly { identity } => Some(identity),
        }
    }

    /// The content condition for a **replacement**.
    ///
    /// # Why `Unchecked` is unreachable on this path
    ///
    /// The platform's `Unchecked` variant exists for callers whose decision is not about the old
    /// bytes. That is precisely the case the brief forbids here: a replace-an-existing-file
    /// operation *did* decide about the old bytes, and an `Unchecked` replacement would reopen the
    /// window between whatever check the caller made and the exclusive open — the window an
    /// independent in-place rewrite slips through.
    ///
    /// So the replace branch only ever sees a `File` condition, and this returns the caller's own
    /// algorithm and hex. The `Absent` arm is unreachable through the replace path **by
    /// construction**: `write_atomic` dispatches `Absent` to `handle_bound_create_absent`, which
    /// takes no content condition at all. [`Self::carries_byte_condition`] is asserted at that
    /// dispatch, so a broken routing is a loud failure rather than a silent unchecked publication.
    fn content(&self) -> HandleBoundReplaceContent<'_> {
        match self {
            Self::Absent => HandleBoundReplaceContent::Unchecked,
            Self::File { algorithm, hex, .. } => {
                HandleBoundReplaceContent::Hex { algorithm: *algorithm, expected: hex }
            }
            // An identity-only condition never reaches the replace entry: `write_atomic` asserts
            // [`Self::carries_byte_condition`] before entering it. Returning `Unchecked` here is
            // therefore unreachable through that path, and the assertion is what keeps it so.
            Self::IdentityOnly { .. } => HandleBoundReplaceContent::Unchecked,
        }
    }

    /// Whether this condition really carries the caller's byte condition.
    ///
    /// `write_atomic` asserts this before it enters the replace branch. It is the structural guard
    /// that keeps `Unchecked` off the replace path: without it, a future edit that routed an
    /// `Absent` into the replace branch would silently drop the caller's condition, which is the
    /// exact defect this batch repairs.
    #[must_use]
    const fn carries_byte_condition(&self) -> bool {
        matches!(self, Self::File { .. })
    }
    /// Re-verifies the caller's condition **through a held handle**, at the move boundary.
    ///
    /// # Why this is the unbypassable position
    ///
    /// Everything the caller decided — the object's identity and its original bytes — is compared
    /// here, while this call holds the object itself with no sharing, and the move that follows
    /// happens before the handle is released. There is no window between the comparison and the
    /// move for a competing writer:
    ///
    /// * an **in-place rewrite** keeps the file id but changes the bytes, and is caught by the byte
    ///   comparison over the held handle;
    /// * a **swap** (delete and recreate, or a rename over the name) changes the file id, and is
    ///   caught because the handle this was verified against names the old object;
    /// * a **race-create** cannot happen: this call fails to open the object at all if the name was
    ///   taken away, which is itself a refusal.
    ///
    /// The expected value is the caller's own and the algorithm is the caller's own; neither is
    /// refreshed, and a SHA-256 condition is never reinterpreted as BLAKE3.
    fn verify_held_file(
        &self,
        target: &WorkspacePath,
        held: &mut std::fs::File,
    ) -> Result<(), FsToolError> {
        let (identity, byte_condition) = match self {
            Self::Absent => {
                // An `Absent` condition has nothing to compare: its whole statement is "the name
                // must be free", which the exclusive open itself already enforced.
                return Ok(());
            }
            Self::IdentityOnly { identity } => (identity, None),
            Self::File { identity, algorithm, hex } => (identity, Some((*algorithm, hex))),
        };
        let actual_identity = held_file_identity(held).map_err(|source| {
            io_error("inspect held staged object", Path::new(target.as_str()), source)
        })?;
        if actual_identity != *identity {
            return Err(FsToolError::InvalidRequest(format!("{target} is not the expected file")));
        }
        let Some((algorithm, hex)) = byte_condition else {
            // Identity-only: the object is the one this caller named, and the held handle is what
            // makes that binding. No byte condition was stated, so none is invented.
            return Ok(());
        };
        use std::io::Seek;
        held.rewind().map_err(|source| {
            io_error("rewind held staged object", Path::new(target.as_str()), source)
        })?;
        let mut bytes = Vec::new();
        held.read_to_end(&mut bytes).map_err(|source| {
            io_error("read held staged object", Path::new(target.as_str()), source)
        })?;
        let actual = match algorithm {
            PlatformHashAlgorithm::Blake3 => ContentHash::blake3(&bytes),
            PlatformHashAlgorithm::Sha256 => ContentHash::sha256(&bytes),
        };
        if actual.hex != *hex {
            return Err(FsToolError::ExpectedHash {
                path: target.clone(),
                expected: ContentHash { algorithm: actual.algorithm, hex: hex.clone() },
                actual,
            });
        }
        Ok(())
    }
}

/// Translates the caller's expected state into the platform's condition, reading the identity the
/// caller's own decision was made against.
///
/// This runs **before** the changeset stages the existing object aside, while the object the caller
/// decided about is still at the destination name. The algorithm is carried through unchanged: a
/// SHA-256 hex from a caller that decided under SHA-256 is never compared against a BLAKE3 digest,
/// and the expected hash is never replaced with whatever the file holds now.
fn handle_condition_for(
    target: &WorkspacePath,
    destination: &Path,
    expected: &FsExpectedState,
) -> Result<HandleCondition, FsToolError> {
    match expected {
        FsExpectedState::Absent => {
            if destination.exists() {
                return Err(FsToolError::ExpectedAbsent { path: target.clone() });
            }
            Ok(HandleCondition::Absent)
        }
        FsExpectedState::FileHash { hash } => {
            let info = inspect_existing_path(destination, true)
                .map_err(|source| io_error("inspect mutation target", destination, source))?;
            if info.is_directory {
                return Err(FsToolError::InvalidRequest(format!(
                    "{} is not the expected file",
                    target
                )));
            }
            Ok(HandleCondition::File {
                identity: info.identity,
                algorithm: match hash.algorithm {
                    ContentHashAlgorithm::Blake3 => PlatformHashAlgorithm::Blake3,
                    ContentHashAlgorithm::Sha256 => PlatformHashAlgorithm::Sha256,
                },
                hex: hash.hex.clone(),
            })
        }
        FsExpectedState::Directory => {
            let info = inspect_existing_path(destination, false)
                .map_err(|source| io_error("inspect expected directory", destination, source))?;
            if !info.is_directory {
                return Err(FsToolError::InvalidRequest(format!(
                    "{target} is not the expected directory"
                )));
            }
            Ok(HandleCondition::IdentityOnly { identity: info.identity })
        }
    }
}

/// The publication action for a destination that is **already** staged aside.
///
/// # What this is and is not
///
/// The staging step moved the object the caller decided about to the staging area, so the
/// destination name is really empty and the publication is a **creation**. That is why the returned
/// action is [`HandleCondition::Absent`]: the final action must be "fail if it exists", because a
/// competitor that created the name while this execution was staging must make the creation fail
/// rather than be silently replaced.
///
/// # Where the caller's byte condition is enforced instead
///
/// This function does **not** verify the caller's `expected` bytes, and it must not pretend to. The
/// consumption of the byte condition happens in [`verify_staged_condition`], which runs at the
/// staging instant while the object is still on disk under its own name — the last moment at which
/// the bytes the caller decided about can be read at all. Once the object is in the staging area the
/// destination holds nothing, so there is no destination content left for a handle to re-verify.
///
/// The `staged_identity` is therefore a **bookkeeping assertion**, not evidence about bytes: it
/// requires that a caller who decided about an existing file really did stage one, so a broken
/// staging step is a failure here rather than a silent create over a file the caller believed it was
/// replacing.
fn staged_condition(
    expected: &FsExpectedState,
    staged_identity: Option<NativeFileIdentity>,
) -> Result<HandleCondition, FsToolError> {
    match expected {
        FsExpectedState::Absent => Ok(HandleCondition::Absent),
        FsExpectedState::FileHash { .. } => {
            staged_identity.ok_or_else(|| {
                FsToolError::InvalidRequest(
                    "the expected file was staged aside without an identity".to_owned(),
                )
            })?;
            Ok(HandleCondition::Absent)
        }
        FsExpectedState::Directory => Ok(HandleCondition::Absent),
    }
}

/// Re-verifies the caller's **original** byte condition against the object about to be staged.
///
/// # Why this is a separate step and why it runs here
///
/// A replace-an-existing-file operation stages the old object aside and then publishes the new one
/// into an empty name. That means the exclusive publication handle is opened on a destination that
/// **holds nothing**, so it cannot re-verify the old bytes — there are none to verify. The caller's
/// `expected` must therefore be consumed at the only instant it can be: immediately before the
/// object is moved, while it is still on disk under its own name and this execution holds the
/// verified information about it.
///
/// This is the condition the decision was actually taken against. It is deliberately **not**
/// refreshed to whatever the file holds now, and the algorithm is the caller's: a SHA-256 digest is
/// recomputed as SHA-256 and never reinterpreted as BLAKE3, because comparing one function's output
/// against another's could only ever be wrong.
///
/// A destination that no longer holds the expected bytes is refused here, before anything is moved,
/// so the caller's own object is never displaced by a decision made about different content.
///
/// # Errors
/// Returns [`FsToolError::ExpectedHash`] naming the expected and actual digests when the bytes are
/// not the ones the caller named.
fn verify_staged_condition(
    resolver: &SecurePathResolver,
    target: &WorkspacePath,
    expected: &FsExpectedState,
) -> Result<(), FsToolError> {
    let FsExpectedState::FileHash { hash } = expected else {
        // `Absent` has nothing to verify against, and `Directory` is rejected by the caller's own
        // node-kind check before this point.
        return Ok(());
    };
    let resolved = resolver.resolve_existing(target)?;
    if resolved.info.is_directory {
        return Err(FsToolError::InvalidRequest(format!("{target} is not the expected file")));
    }
    let mut file = resolver.open_existing(target, NativeFileAccess::Read)?.file;
    let mut bytes = Vec::new();
    file.file_mut().read_to_end(&mut bytes).map_err(|source| {
        io_error("read expected mutation target", &resolved.info.final_path, source)
    })?;
    let actual = match hash.algorithm {
        ContentHashAlgorithm::Blake3 => ContentHash::blake3(&bytes),
        ContentHashAlgorithm::Sha256 => ContentHash::sha256(&bytes),
    };
    if actual != *hash {
        return Err(FsToolError::ExpectedHash {
            path: target.clone(),
            expected: hash.clone(),
            actual,
        });
    }
    Ok(())
}

/// Maps a real publication failure onto the caller's error, keeping the stage facts.
fn publish_error(target: &WorkspacePath, error: HandlePublishError) -> FsToolError {
    FsToolError::InvalidRequest(format!("atomic conditional write failed for {target}: {error}"))
}

fn copy_logical(
    resolver: &SecurePathResolver,
    source: &WorkspacePath,
    destination: &WorkspacePath,
    source_is_directory: bool,
    condition: &HandleCondition,
    source_condition: &HandleCondition,
    previous_identity: Option<&NativeFileIdentity>,
) -> Result<(PublishedResult, Option<Vec<OwnMember>>), FsToolError> {
    if source_is_directory {
        let mut source_hold = aura_windows_platform::open_held_for_read(
            &resolver.absolute_path(source),
        )
        .map_err(|error| {
            io_error("hold directory copy source", resolver.absolute_path(source), error)
        })?;
        source_condition.verify_held_file(source, source_hold.file_mut())?;
        let receipt = copy_directory_verified(
            resolver,
            &resolver.absolute_path(source),
            &resolver.absolute_path(destination),
        )?;
        resolver
            .verify_after_mutation(destination)
            .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
        return Ok((receipt.result, Some(receipt.members)));
    }
    let absolute = resolver.absolute_path(source);
    let mut held = aura_windows_platform::open_held_for_read(&absolute)
        .map_err(|error| io_error("hold copy source", &absolute, error))?;
    source_condition.verify_held_file(source, held.file_mut())?;
    use std::io::Seek;
    held.file_mut().rewind().map_err(|error| io_error("rewind copy source", &absolute, error))?;
    let mut bytes = Vec::new();
    held.file_mut()
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read held copy source", &absolute, error))?;
    Ok((write_atomic(resolver, destination, &bytes, condition, previous_identity)?, None))
}

fn copy_directory_verified(
    resolver: &SecurePathResolver,
    source: &Path,
    destination: &Path,
) -> Result<copy_receipt::DirectoryCopy, FsToolError> {
    copy_receipt::copy_directory(resolver, source, destination)
}

/// What one move really did, so the caller can compensate with the same object facts.
struct MovedObject {
    /// The identity of the object that arrived at the destination, read from the held handle the
    /// move was performed through — never from a later inspection of the destination path.
    identity: NativeFileIdentity,
    hash: Option<ContentHash>,
    members: Option<Vec<OwnMember>>,
}

/// Moves one object **through the handle the caller's condition was verified on**.
///
/// # Why this is not `fs::rename(source_path, destination_path)`
///
/// A path rename carries whatever object the source *name* refers to at the instant the kernel
/// resolves it. Two things follow, and both are the last window this batch closes:
///
/// * the caller's original `condition` — the object's identity **and** the bytes it decided about —
///   is compared against one object and the move can then carry a *different* one, because nothing
///   between the comparison and the rename holds the source still; and
/// * the failure exit was a **path rename-back**, which relocates whatever is at the destination
///   name when it runs. A competitor that took the name in between would have *its* object moved
///   back to the source, and the source name would then hold something this execution never
///   touched.
///
/// So the source is opened with [`open_held_for_move`] — a READ+DELETE handle that denies competing
/// writes and renames — the caller's condition is re-verified **through that handle**, and the real
/// move is performed handle-relative with `fail-if-exists`, so a competitor that created the
/// destination name while this execution was verifying makes the move fail rather than be silently
/// replaced.
///
/// # What is deliberately not done on failure
///
/// There is no rename-back. The native move either happened — in which case the destination holds
/// this execution's object and the ordinary recorded-result compensation owns getting it back — or
/// it did not, in which case the source is still at its own name and nothing needs undoing. A
/// verification failure *after* a successful move is reported as material the recovery owner must
/// settle, with the object's location named, rather than being "fixed" by a second path rename.
fn move_logical(
    resolver: &SecurePathResolver,
    source: &WorkspacePath,
    destination: &WorkspacePath,
    condition: &HandleCondition,
    expected_members: Option<&[OwnMember]>,
) -> Result<MovedObject, FsToolError> {
    let source_absolute = resolver.absolute_path(source);
    let parent_validated = resolver.validate_parent(destination)?;
    // The caller's own condition is consumed on the held object, before anything moves. This is the
    // last instant at which the object it was decided about is still reachable under its own name.
    let mut held = open_held_for_staging(&source_absolute)?;
    condition.verify_held_file(source, held.file_mut())?;
    let moved_identity = held_file_identity(held.file())
        .map_err(|error| io_error("inspect held move source", &source_absolute, error))?;
    let parent = open_verified_publish_parent(&parent_validated.parent_absolute_path)
        .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
    // The same named interference point the staging move uses, reached at the same real instant:
    // the caller's condition has just been verified, the source is held with no sharing, and the
    // native move has not happened yet. An action installed here therefore runs on the real object
    // at the real name — exactly what an independent external writer could do in that window — and
    // cannot change this call's own decision, because the verification above has already happened
    // through the handle that still denies that writer.
    let mut members = if held.info().is_directory {
        Some(copy_receipt::snapshot_members(resolver, source)?)
    } else {
        None
    };
    if let Some(expected) = expected_members {
        copy_receipt::verify_members(resolver, source, expected)?;
    }
    let moved_hash =
        if held.info().is_directory { None } else { Some(hash_held(held.file_mut())?) };
    staging_fault::reached(staging_fault::Stage::BeforeStagedMove, &source_absolute);
    if let Some(expected) = expected_members {
        copy_receipt::verify_members(resolver, source, expected)?;
    }
    // `fail if it exists`: a competitor that created the destination name wins the refusal rather
    // than being replaced by this move; a program that only briefly blocks the rename is waited out.
    crate::held_rename::move_held(&held, &parent, &parent_validated.leaf_name)
        .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
    drop(held);
    staging_fault::reached(staging_fault::Stage::ObjectMoved, &resolver.absolute_path(destination));
    // The effect really happened. From here the reported facts are the ones a recovery owner needs:
    // where the object is, and which object it is. A verification failure is material to settle,
    // never a reason to rename something back by path.
    if let Err(error) = resolver.verify_after_mutation(destination) {
        return Err(FsToolError::ReconciliationRequired(format!(
            "move placed the object at {destination} but the destination is not a valid workspace \
             path ({error}); the object is the one verified for {source} and no rename-back was \
             attempted"
        )));
    }
    if load_state(resolver, source)?.exists() {
        return Err(FsToolError::ReconciliationRequired(format!(
            "move left both source {} and destination {}",
            source, destination
        )));
    }
    if let Some(members) = &mut members {
        copy_receipt::rebase(members, source, destination)?;
    }
    Ok(MovedObject { identity: moved_identity, hash: moved_hash, members })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_workspace::CatalogChange;

    /// The paths a changeset declares are exactly what the uncommitted-editor check walks, so a
    /// move's source and destination are both considered and a no-op change set walks nothing.
    #[test]
    fn a_changeset_walks_every_declared_target_including_both_ends_of_a_move() {
        let source = FsChangesetOperation::Move {
            source: aura_workspace_domain::WorkspaceFileTarget {
                path: WorkspacePath::parse("alpha/one.txt").expect("path"),
            },
            destination: aura_workspace_domain::WorkspaceFileTarget {
                path: WorkspacePath::parse("beta/two.txt").expect("path"),
            },
            source_expected: FsExpectedState::Absent,
            destination_expected: FsExpectedState::Absent,
        };
        assert_eq!(
            mutation_paths(&[source]),
            vec![
                WorkspacePath::parse("alpha/one.txt").expect("path"),
                WorkspacePath::parse("beta/two.txt").expect("path"),
            ],
            "both ends of a move are real targets and must both be checked"
        );

        let write = FsChangesetOperation::WriteFile {
            target: aura_workspace_domain::WorkspaceFileTarget {
                path: WorkspacePath::parse("notes.txt").expect("path"),
            },
            content: "x".to_owned(),
            expected: FsExpectedState::Absent,
            encoding: None,
            line_ending: None,
        };
        assert_eq!(
            mutation_paths(&[write]),
            vec![WorkspacePath::parse("notes.txt").expect("path")]
        );
    }
    fn paths(entries: &[(WorkspacePath, CatalogChange)]) -> Vec<(String, CatalogChange)> {
        entries.iter().map(|(path, change)| (path.as_str().to_owned(), *change)).collect()
    }

    /// The scope a committed changeset describes decides how much of the
    /// workspace is re-read, so it is asserted directly rather than inferred from
    /// the absence of a rebuild log.
    #[test]
    fn a_committed_changeset_names_the_paths_it_touched_and_their_listing() {
        // One file inside a directory: that file, plus the directory's own
        // listing so a create or delete beside it is still noticed.
        let nested =
            changeset_catalog_changes(&[], &[WorkspacePath::parse("alpha/one.txt").expect("path")]);
        assert_eq!(
            paths(&nested),
            vec![
                ("alpha".to_owned(), CatalogChange::DirectoryListed),
                ("alpha/one.txt".to_owned(), CatalogChange::Changed),
            ],
            "a nested write must not describe any other directory or subtree"
        );

        // A file directly in the root is exactly one path: its parent is the
        // workspace itself, which is never an entry.
        let root =
            changeset_catalog_changes(&[], &[WorkspacePath::parse("one.txt").expect("path")]);
        assert_eq!(paths(&root), vec![("one.txt".to_owned(), CatalogChange::Changed)]);

        // A move names both ends, and both of their directories.
        let moved = changeset_catalog_changes(
            &[PreparedOperation::Move {
                source: WorkspacePath::parse("alpha/one.txt").expect("path"),
                destination: WorkspacePath::parse("beta/two.txt").expect("path"),
                source_expected: FsExpectedState::Absent,
                destination_expected: FsExpectedState::Absent,
                destination_existed: false,
                audits: Vec::new(),
            }],
            &[
                WorkspacePath::parse("alpha/one.txt").expect("path"),
                WorkspacePath::parse("beta/two.txt").expect("path"),
            ],
        );
        assert_eq!(
            paths(&moved),
            vec![
                ("alpha".to_owned(), CatalogChange::DirectoryListed),
                // A move leaves its source behind, so the source is a removal: a
                // "changed" description would only re-list the directory that held it
                // and leave a moved directory's own entries in the index.
                ("alpha/one.txt".to_owned(), CatalogChange::Removed),
                ("beta".to_owned(), CatalogChange::DirectoryListed),
                ("beta/two.txt".to_owned(), CatalogChange::Changed),
            ]
        );

        // A deletion is the case that made this necessary: the whole subtree has to
        // leave the index, which only a removal describes.
        let deleted = changeset_catalog_changes(
            &[PreparedOperation::Delete {
                target: WorkspacePath::parse("gone").expect("path"),
                expected: FsExpectedState::Directory,
                recursive: true,
                audits: Vec::new(),
            }],
            &[WorkspacePath::parse("gone").expect("path")],
        );
        assert_eq!(paths(&deleted), vec![("gone".to_owned(), CatalogChange::Removed)]);
    }
    /// **The required external-writer counterexample for the replace path.**
    ///
    /// The brief's exit fact is: an independent external writer rewrites the file **in place** after
    /// the pre-staging hash check, with `FileIdentity` unchanged, and the final publication must
    /// still reject the caller's old `expected`.
    ///
    /// # Why this is the case that was broken
    ///
    /// A replace-an-existing-file operation stages the old object aside and then publishes into an
    /// empty name, so the exclusive publication handle never sees the old bytes. A first version of
    /// this code therefore built the publication condition from the staged identity alone and
    /// **discarded** the caller's algorithm and hash — the `(Some(staged), _)` arm threw away the
    /// value that carried them. The identity cannot catch this case at all: Windows keeps the file
    /// id across an in-place rewrite, which the assertions below measure directly.
    ///
    /// # What this test does
    ///
    /// It performs the exact sequence the operation performs, against a real temporary directory:
    ///
    /// 1. the caller states its condition as the hash of the bytes it decided about;
    /// 2. an **independent writer** — plain `std::fs::write`, no Aura code, exactly what an external
    ///    process does — rewrites the object in place, leaving the identity alone;
    /// 3. the operation's own verification step runs.
    ///
    /// The operation must refuse, and the external writer's bytes must survive: a refusal that has
    /// already overwritten is not a refusal.
    #[test]
    fn the_original_byte_condition_rejects_an_in_place_rewrite_before_staging() {
        let root = tempfile::tempdir().expect("root");
        let absolute = root.path().join("note.txt");
        std::fs::write(&absolute, b"the original bytes").expect("seed");
        let workspace_id = WorkspaceId::new();
        let resolver = test_resolver_for(root.path(), workspace_id);
        let target = WorkspacePath::parse("note.txt").expect("path");

        // The caller decided about these bytes.
        let expected =
            FsExpectedState::FileHash { hash: ContentHash::blake3(b"the original bytes") };

        // An independent writer rewrites the object **in place**. This is deliberately not an Aura
        // call: it is what another process does, and it is the case an identity check cannot see.
        let identity_before = resolver.resolve_existing(&target).expect("resolve").info.identity;
        std::fs::write(&absolute, b"the outside writer won").expect("in-place rewrite");
        let identity_after = resolver.resolve_existing(&target).expect("resolve").info.identity;
        assert_eq!(
            identity_before, identity_after,
            "an in-place rewrite keeps the FileIdentity, which is why identity alone cannot decide"
        );

        let refusal = verify_staged_condition(&resolver, &target, &expected)
            .expect_err("the publication must reject the caller's old expected bytes");
        assert!(
            matches!(refusal, FsToolError::ExpectedHash { .. }),
            "the refusal must name the real expected/actual hash: {refusal:?}"
        );
        assert_eq!(
            std::fs::read(&absolute).expect("destination"),
            b"the outside writer won",
            "the external writer's bytes must survive a refused operation"
        );

        // The control: with the condition the file really holds, the same step admits, so the test
        // is measuring the condition and not a blanket refusal.
        let current =
            FsExpectedState::FileHash { hash: ContentHash::blake3(b"the outside writer won") };
        verify_staged_condition(&resolver, &target, &current).expect("the current bytes admit");
    }

    /// The same counterexample on the algorithm the caller used.
    ///
    /// A SHA-256 condition must be recomputed as SHA-256. Feeding that hex to a BLAKE3 comparison
    /// would compare two different functions' output, which can only ever produce a false answer —
    /// so a caller that decided under SHA-256 is checked under SHA-256, and its condition still
    /// rejects an in-place rewrite.
    #[test]
    fn the_original_algorithm_is_the_one_the_condition_is_checked_under() {
        let root = tempfile::tempdir().expect("root");
        let absolute = root.path().join("note.txt");
        std::fs::write(&absolute, b"sha256 condition").expect("seed");
        let resolver = test_resolver_for(root.path(), WorkspaceId::new());
        let target = WorkspacePath::parse("note.txt").expect("path");

        let sha = ContentHash::sha256(b"sha256 condition");
        assert_eq!(sha.algorithm, ContentHashAlgorithm::Sha256);
        verify_staged_condition(
            &resolver,
            &target,
            &FsExpectedState::FileHash { hash: sha.clone() },
        )
        .expect("the caller's own algorithm admits its own bytes");

        std::fs::write(&absolute, b"rewritten elsewhere").expect("in-place rewrite");
        let refusal =
            verify_staged_condition(&resolver, &target, &FsExpectedState::FileHash { hash: sha })
                .expect_err("the old SHA-256 condition must reject the new bytes");
        assert!(
            matches!(refusal, FsToolError::ExpectedHash { ref expected, .. }
                if expected.algorithm == ContentHashAlgorithm::Sha256),
            "the refusal keeps the caller's algorithm rather than reinterpreting the digest: {refusal:?}"
        );
    }

    /// A test resolver built the way the runtime builds one.
    fn test_resolver_for(root: &std::path::Path, workspace_id: WorkspaceId) -> SecurePathResolver {
        use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};

        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        SecurePathResolver::from_workspace(&WorkspaceSummary {
            workspace_id,
            display_name: "mutation test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(aura_workspace::domain_identity(&info.identity)),
            state: WorkspaceState::Online,
            revision: aura_domain_common::Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: aura_domain_common::Revision::INITIAL,
        })
        .expect("resolver")
    }
    /// The replace path really reaches the exclusive handle with the caller's condition.
    ///
    /// # Why this is measured through the platform rather than inferred
    ///
    /// The previous round's defect was not that `HandleCondition` lacked fields — it had them — but
    /// that the `(Some(staged), _)` arm **threw the value away** and rebuilt the condition from the
    /// staged identity alone. Reading the source cannot distinguish "the value is carried" from
    /// "the value is computed and then discarded", which is exactly why that defect survived a
    /// source review.
    ///
    /// So this drives the real publication against a real file and observes the **platform's own
    /// condition seam**, which reports what the checked replace was actually given at the instant it
    /// consumed it. The observed condition must be the caller's **original** bytes, never a
    /// refreshed hash of whatever is there now.
    #[test]
    fn the_replace_reaches_the_exclusive_handle_with_the_callers_original_condition() {
        let root = tempfile::tempdir().expect("root");
        let absolute = root.path().join("note.txt");
        std::fs::write(&absolute, b"the bytes the caller decided about").expect("seed");
        let resolver = test_resolver_for(root.path(), WorkspaceId::new());
        let target = WorkspacePath::parse("note.txt").expect("path");

        let observed: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&observed);
        let _hook = aura_windows_platform::publication_fault::observe_replace_condition(
            &absolute,
            Box::new(move |algorithm, expected_hex| {
                sink.lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push((algorithm.to_owned(), expected_hex.to_owned()));
            }),
        );

        let expected_hash = ContentHash::blake3(b"the bytes the caller decided about");
        let expected = FsExpectedState::FileHash { hash: expected_hash.clone() };
        let condition = handle_condition_for(&target, &absolute, &expected)
            .expect("the caller's condition translates");
        assert!(
            condition.carries_byte_condition(),
            "the replace path must carry bytes, not an identity-only condition"
        );

        write_atomic(
            &resolver,
            &target,
            b"the replacement bytes",
            &condition,
            condition.replace_identity(),
        )
        .expect("the conditional replacement publishes");

        let observed = observed.lock().unwrap_or_else(|error| error.into_inner()).clone();
        assert_eq!(
            observed.len(),
            1,
            "the checked publication must have consumed exactly one condition: {observed:?}"
        );
        assert_eq!(observed[0].0, "blake3", "the caller's algorithm reaches the handle");
        assert_eq!(
            observed[0].1, expected_hash.hex,
            "the handle must receive the caller's ORIGINAL expected bytes, not a refreshed hash"
        );
        assert_eq!(
            std::fs::read(&absolute).expect("destination"),
            b"the replacement bytes",
            "the replacement really published"
        );
    }
}
