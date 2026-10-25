use std::sync::{Arc, Mutex};

use aura_domain_common::{SessionId, WorkspaceId, WorkspacePath};
use aura_persistence::PersistenceCoordinator;
use aura_workspace::{
    BufferStore, PathCatalog, ResourceFacts, SecurePathResolver, WorkspaceRuntime,
};
use aura_workspace_domain::{EligibilityOwner, FileTargetRef, WorkspaceFileTarget};

use crate::search::StageDiagnosticsSnapshot;
use crate::{FsToolError, FsToolPolicy};

pub struct FsToolService {
    pub(crate) persistence: Arc<PersistenceCoordinator>,
    pub(crate) policy: FsToolPolicy,
    pub(crate) workspace: Arc<WorkspaceRuntime>,
    /// Stage measurements from the most recent search. Diagnostics only: never
    /// projected into a wire result or a model request.
    last_search_diagnostics: Mutex<Option<StageDiagnosticsSnapshot>>,
    /// Directory iterators and object identities for an external find that has
    /// not finished. The model-visible cursor is not a disk fact: a later page
    /// resumes the iterator this service still holds and re-checks the object.
    pub(crate) find_queries: Mutex<crate::external::FindQueryTable>,
}

impl FsToolService {
    /// The configured upper bound for waiting on another short file commit. Async entry owners
    /// consume it once, within their original request budget; retries must not restart it.
    #[must_use]
    pub fn mutation_wait_budget(&self) -> std::time::Duration {
        self.policy.mutation_wait
    }

    pub fn new(
        workspace: Arc<WorkspaceRuntime>,
        persistence: Arc<PersistenceCoordinator>,
        policy: FsToolPolicy,
    ) -> Result<Arc<Self>, FsToolError> {
        policy.validate().map_err(|message| FsToolError::InvalidRequest(message.to_owned()))?;
        Ok(Arc::new(Self {
            persistence,
            policy,
            workspace,
            last_search_diagnostics: Mutex::new(None),
            find_queries: Mutex::new(crate::external::FindQueryTable::default()),
        }))
    }

    /// The **one** eligibility owner every file entry of this process coordinates through.
    ///
    /// It is read from the workspace runtime rather than created here, so the handle this service
    /// gets is the same instance the `BufferStore` holds and the same one P04's external writer
    /// gets. That is the same-instance property the batch is required to prove, and it is why this
    /// is a delegation and not a field: a service with its own owner would protect nothing while
    /// looking exactly like this one.
    #[must_use]
    pub(crate) fn eligibility_owner(&self) -> Arc<EligibilityOwner> {
        self.workspace.eligibility_owner()
    }

    /// The real resources one target names, derived from the resolver the runtime verified.
    ///
    /// A caller asks for the shape it really touches — the object, the entry, or a directory and
    /// everything inside it — so a directory mutation covers its subtree by real node type and
    /// read/write scope rather than by a DTO's `recursive` flag alone.
    ///
    /// # Errors
    /// Returns the real resolution error, or [`FsToolError::ScopeUnavailable`] when the target is
    /// not a workspace path.
    pub(crate) fn resource_facts(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
    ) -> Result<ResourceFacts, FsToolError> {
        self.workspace.resource_facts(workspace_id, session_id).map_err(Into::into)
    }

    /// Stage measurements for the most recent search on this service. Used by
    /// benchmarks and tests to attribute cost; not part of the product API.
    pub fn last_search_diagnostics(&self) -> Option<StageDiagnosticsSnapshot> {
        *self.last_search_diagnostics.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn record_search_diagnostics(&self, snapshot: StageDiagnosticsSnapshot) {
        *self.last_search_diagnostics.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(snapshot);
    }

    pub(crate) fn resolver(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Arc<SecurePathResolver>, FsToolError> {
        self.workspace.resolver(workspace_id).map_err(Into::into)
    }

    pub fn require_workspace_target(
        target: FileTargetRef,
    ) -> Result<WorkspaceFileTarget, FsToolError> {
        match target {
            FileTargetRef::Workspace { path } => Ok(WorkspaceFileTarget { path }),
            FileTargetRef::SessionFile { .. }
            | FileTargetRef::ExternalPath { .. }
            | FileTargetRef::ExternalGrant { .. } => Err(FsToolError::ScopeUnavailable),
        }
    }

    pub(crate) fn catalog(
        &self,
        workspace_id: WorkspaceId,
    ) -> Result<Arc<PathCatalog>, FsToolError> {
        self.workspace.catalog(workspace_id).map_err(Into::into)
    }

    pub(crate) fn buffers(&self) -> Arc<BufferStore> {
        self.workspace.buffers()
    }

    pub(crate) fn record_path_use(
        &self,
        workspace_id: WorkspaceId,
        path: &WorkspacePath,
    ) -> Result<(), FsToolError> {
        self.catalog(workspace_id)?.record_use(path)?;
        Ok(())
    }
}
